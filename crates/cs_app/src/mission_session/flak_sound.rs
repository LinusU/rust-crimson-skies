//! The flak sounds (Rally #1886, FLAK-C4): the shot's `snd_enemyflak` and the
//! burst's `snd_missile_flak`, both named by the `wep_27` record and played
//! through the one-shot path the weapon shots use.
//!
//! The chain is read, never assumed (finding
//! `docs/findings/2026-10-22-flak-c4-flak-sounds.md`, provenance `static`):
//!
//! 1. `weapons.zrd` record `wep_27`: the `FIRE` row's `SOUND` names the shot's
//!    group, the `IMPACT` row `default` (index [`BURST_IMPACT_ROW`], the row a
//!    rocket burst starts, FLAK-C1) names the burst's;
//! 2. `sounds.zrd` maps each group to one archive member and carries the flags
//!    and the `RANGE` pair (kept, not applied: the world's designed
//!    attenuation places the cue);
//! 3. the member is fed from the launch sound's container, the same labelled
//!    choice [`LAUNCH_SOUND_CONTAINER`] records.
//!
//! The pick `0x5ad100` makes is `index = (rand() * count) >> 15` over the row's
//! sound list ([`pick_index`]); both flak groups map to a single member, so
//! the pick is index 0 whatever `rand` answers. The player's `snd_missile_flak`
//! `IMPACT` rows and whether the row's own sound handle plays beside the pick
//! are not settled and stay in the finding.

use std::fmt;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;
use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioDraft, AudioLevel, AudioPlayback, PlaybackMode,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_sim::audio_events::{AudioEventId, OneShotOutcome, spatialize};
use cs_types::Tick;
use cs_types::content::{ContentId, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use super::content::SoundArchive;
use super::flak_round::{BURST_IMPACT_ROW, FLAK_RECORD, OrdnanceBurst};
use super::gun_sound::{
    CLAIM_DECODED, CLAIM_MIX, READER_ARCHIVE, SOUNDS_MEMBER, WEAPONS_MEMBER, first_text,
    record_headed_by, sound_member, value_of,
};
use super::launch_sound::{LAUNCH_SOUND_CONTAINER, member_content_id, reader_member};
use crate::audio::{AudioSession, AudioSpatial};
use crate::weapon_effects::SpatialPlacement;

/// The audio producer serial of the flak cues (0 is the loops, 1 the weapon
/// shots and radio, 2 the music).
pub const FLAK_SOUND_PRODUCER: u32 = 3;

/// Claim: the one-shot mode is the absence of `LOOPED` in the group's flags.
const CLAIM_ONE_SHOT: &str = "flak-c4.one-shot-mode";

/// A voice read from the documents: group, member, `RANGE` pair.
pub type VoiceRow = (String, String, Option<[f64; 2]>);

/// What a flak cue is for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlakSoundKind {
    /// The turret's shot: the `FIRE` row's sound, at the muzzle.
    Shot,
    /// The burst: the `IMPACT` row `default`'s sound, at the burst point.
    Burst,
}

/// One measured flak sound.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakVoice {
    /// The sound group the weapon row names, e.g. `snd_enemyflak`.
    pub group: String,
    /// The archive member `sounds.zrd` maps it to.
    pub member: String,
    /// The group's `RANGE` pair (metres), as declared.
    pub range_m: Option<[f64; 2]>,
    /// The stable content id of the member in [`LAUNCH_SOUND_CONTAINER`].
    pub content: ContentId,
}

/// The measured flak sounds and what the composition needs to deliver them.
#[derive(Clone, Debug)]
pub struct FlakSound {
    /// The shot's sound.
    pub shot: FlakVoice,
    /// The burst's sound.
    pub burst: FlakVoice,
    /// The archive the members are fed from.
    pub container: String,
    /// The installation's digest of that archive.
    pub container_sha256: ContentHash,
    /// The archive on the host, inside the read-only installation.
    pub host: PathBuf,
    /// The archive's byte length.
    pub bytes: u64,
}

/// Why the flak sounds could not be measured or played.
#[derive(Debug, PartialEq)]
pub enum FlakSoundError {
    /// A production reader refused.
    Read(String),
    /// The data does not name the sounds the way the measurement requires.
    Unmeasured(String),
    /// The load delivered no spec for the member.
    UnknownAsset(ContentId),
    /// The spec cannot form a one-shot, or the sequences ran out.
    Refused(String),
}

impl fmt::Display for FlakSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the flak sound's source refuses: {reason}"),
            Self::Unmeasured(reason) => write!(f, "the flak sound is not measured: {reason}"),
            Self::UnknownAsset(id) => write!(f, "the load delivered no audio spec for {id}"),
            Self::Refused(reason) => write!(f, "the flak cue is refused: {reason}"),
        }
    }
}

impl std::error::Error for FlakSoundError {}

/// The index `0x5ad100` picks from a row's list of `count` sounds when `rand`
/// answers `rand` (`0..=32767`): `(rand * count) >> 15`.
#[must_use]
pub const fn pick_index(count: u32, rand: u32) -> u32 {
    (rand * count) >> 15
}

fn sound_of_row(row: &ZrdValue) -> Option<&str> {
    first_text(value_of(row, "SOUND")?)
}

fn voice_from_documents(
    group: &str,
    sounds: &ZrdValue,
    what: &str,
) -> Result<(String, Option<[f64; 2]>, bool), FlakSoundError> {
    let unmeasured = |reason: String| FlakSoundError::Unmeasured(reason);
    let entry = record_headed_by(sounds, group)
        .ok_or_else(|| unmeasured(format!("sounds.zrd declares no group {group} ({what})")))?;
    let member = entry
        .get(1)
        .and_then(ZrdValue::as_text)
        .ok_or_else(|| unmeasured(format!("sounds.zrd maps {group} to no single member")))?;
    let looped = entry.iter().any(|flag| flag.as_text() == Some("LOOPED"));
    let range = entry.iter().enumerate().find_map(|(at, flag)| {
        if flag.as_text() != Some("RANGE") {
            return None;
        }
        let pair = entry.get(at + 1)?.as_list()?;
        let number = |v: &ZrdValue| match v {
            ZrdValue::Float(n) => Some(f64::from(*n)),
            ZrdValue::Int(n) => Some(f64::from(*n)),
            _ => None,
        };
        Some([number(pair.first()?)?, number(pair.get(1)?)?])
    });
    Ok((member.to_owned(), range, looped))
}

/// The two group names and members from decoded `weapons.zrd` / `sounds.zrd`.
///
/// # Errors
///
/// [`FlakSoundError::Unmeasured`] when the record, a row, a group is absent or
/// a group is `LOOPED`.
pub fn voices_from_documents(
    weapons: &ZrdValue,
    sounds: &ZrdValue,
) -> Result<[VoiceRow; 2], FlakSoundError> {
    let unmeasured = |reason: String| FlakSoundError::Unmeasured(reason);
    let ballistics = value_of(weapons, "BALLISTICS")
        .ok_or_else(|| unmeasured("weapons.zrd has no BALLISTICS section".to_owned()))?;
    let record = value_of(ballistics, FLAK_RECORD)
        .ok_or_else(|| unmeasured(format!("BALLISTICS has no record {FLAK_RECORD}")))?;
    let fire = value_of(record, "FIRE")
        .ok_or_else(|| unmeasured(format!("{FLAK_RECORD} has no FIRE row")))?;
    let impact = value_of(record, "IMPACT")
        .ok_or_else(|| unmeasured(format!("{FLAK_RECORD} has no IMPACT section")))?;
    // Row `BURST_IMPACT_ROW` is the section's first: `default`.
    debug_assert_eq!(BURST_IMPACT_ROW, 0);
    let rows = impact
        .as_list()
        .ok_or_else(|| unmeasured("IMPACT is not a list".to_owned()))?;
    if rows.first().and_then(ZrdValue::as_text) != Some("default") {
        return Err(unmeasured("IMPACT's first row is not `default`".to_owned()));
    }
    let burst_row = rows
        .get(1)
        .ok_or_else(|| unmeasured("IMPACT row default has no body".to_owned()))?;
    let mut out = Vec::new();
    for (row, what) in [(fire, "FIRE"), (burst_row, "IMPACT default")] {
        let group = sound_of_row(row)
            .ok_or_else(|| unmeasured(format!("{FLAK_RECORD} {what} names no SOUND")))?;
        let (member, range, looped) = voice_from_documents(group, sounds, what)?;
        if looped {
            return Err(unmeasured(format!(
                "group {group} is LOOPED, not a one-shot"
            )));
        }
        out.push((group.to_owned(), member, range));
    }
    let burst = out.pop().expect("two rows");
    let shot = out.pop().expect("two rows");
    Ok([shot, burst])
}

/// Measures the flak sounds from the installation at `install_root`.
///
/// # Errors
///
/// [`FlakSoundError`] when a reader refuses, the chain is incomplete, a member
/// is not in the launch sound's container or the container is not among
/// `archives`.
pub fn measure_flak_sound(
    install_root: &Path,
    archives: &[SoundArchive],
) -> Result<FlakSound, FlakSoundError> {
    let read = |member: &str| -> Result<ZrdValue, FlakSoundError> {
        let bytes =
            reader_member(install_root, READER_ARCHIVE, member).map_err(FlakSoundError::Read)?;
        decode_zrd(&bytes).map_err(|error| FlakSoundError::Read(format!("{member}: {error}")))
    };
    let [shot, burst] = voices_from_documents(&read(WEAPONS_MEMBER)?, &read(SOUNDS_MEMBER)?)?;
    let voice = |(group, member, range_m): VoiceRow| {
        sound_member(install_root, LAUNCH_SOUND_CONTAINER, &member)
            .map_err(FlakSoundError::Read)?;
        let content = member_content_id(LAUNCH_SOUND_CONTAINER, &member)
            .map_err(|e| FlakSoundError::Read(e.to_string()))?;
        Ok::<_, FlakSoundError>(FlakVoice {
            group,
            member,
            range_m,
            content,
        })
    };
    let archive = archives
        .iter()
        .find(|archive| archive.key.eq_ignore_ascii_case(LAUNCH_SOUND_CONTAINER))
        .ok_or_else(|| {
            FlakSoundError::Unmeasured(format!(
                "{LAUNCH_SOUND_CONTAINER} is not among the mission's sound archives"
            ))
        })?;
    let container_sha256 =
        ContentHash::from_hex(&archive.sha256).map_err(|e| FlakSoundError::Read(e.to_string()))?;
    let host = install_root.join(LAUNCH_SOUND_CONTAINER);
    let bytes = std::fs::metadata(&host)
        .map_err(|e| FlakSoundError::Read(format!("cannot measure {}: {e}", host.display())))?
        .len();
    Ok(FlakSound {
        shot: voice(shot)?,
        burst: voice(burst)?,
        container: LAUNCH_SOUND_CONTAINER.to_owned(),
        container_sha256,
        host,
        bytes,
    })
}

impl FlakSound {
    /// The two members' declared catalog records: the one-shot mode is
    /// measured (`sounds.zrd` flags no `LOOPED`); the `Weapons` bus, the
    /// level and the container feed are designed.
    ///
    /// # Errors
    ///
    /// [`FlakSoundError::Read`] when a record part is refused.
    pub fn records(&self) -> Result<Vec<AudioAssetRecord>, FlakSoundError> {
        let refuse = |reason: String| FlakSoundError::Read(reason);
        let claim = |id: &str| ClaimId::new(id).map_err(|e| refuse(e.to_string()));
        let mut out = Vec::new();
        for voice in [&self.shot, &self.burst] {
            let designed = || -> Result<Provenance, FlakSoundError> {
                Ok(Provenance::designed(claim(CLAIM_MIX)?))
            };
            out.push(
                AudioAssetRecord::try_new(AudioDraft {
                    id: voice.content.clone(),
                    origin: Origin::Designed,
                    playback: AudioPlayback {
                        bus: Resolved::Known(Known::new(AudioBus::Weapons, designed()?)),
                        level: Resolved::Known(Known::new(
                            AudioLevel::try_new(1.0).map_err(|e| refuse(e.to_string()))?,
                            designed()?,
                        )),
                        mode: Resolved::Known(Known::new(
                            PlaybackMode::OneShot,
                            Provenance::new(
                                claim(CLAIM_ONE_SHOT)?,
                                ClaimStatus::ObservedTool,
                                None,
                            )
                            .map_err(|e| refuse(e.to_string()))?,
                        )),
                    },
                    decoded: Resolved::unknown(
                        claim(CLAIM_DECODED)?,
                        "decoded when the load populates the sample library",
                    )
                    .map_err(|e| refuse(e.to_string()))?,
                    provenance: designed()?,
                })
                .map_err(|e| refuse(e.to_string()))?,
            );
        }
        // The two groups may map to one member; the catalog holds it once.
        out.dedup_by(|a, b| a.id() == b.id());
        Ok(out)
    }

    /// The voice of `kind`.
    #[must_use]
    pub const fn voice(&self, kind: FlakSoundKind) -> &FlakVoice {
        match kind {
            FlakSoundKind::Shot => &self.shot,
            FlakSoundKind::Burst => &self.burst,
        }
    }
}

/// Resource: the flak producer's one-shot sequence counter of the installed
/// session (strictly increasing, like `WeaponShotSequences`).
#[derive(Resource, Clone, Debug, Default)]
pub struct FlakSoundSequences {
    next: u32,
}

/// One accepted flak cue.
#[derive(Clone, Debug, PartialEq)]
pub struct FlakSoundCue {
    /// Shot or burst.
    pub kind: FlakSoundKind,
    /// The accepted one-shot.
    pub event: cs_sim::audio_events::OneShotEvent,
    /// Where it sounds, metres.
    pub origin_m: [f64; 3],
    /// Its placement against the listener.
    pub spatial: SpatialPlacement,
}

/// Plays the `kind` cue of `sound` at `origin_m` on `tick` through the
/// session's router.
///
/// # Errors
///
/// [`FlakSoundError::UnknownAsset`] when the load delivered no spec for the
/// member, [`FlakSoundError::Refused`] for a spec that is no one-shot, an
/// exhausted sequence, an unplaceable position or a router refusal.
pub fn play_flak_sound(
    sound: &FlakSound,
    kind: FlakSoundKind,
    tick: Tick,
    origin_m: [f64; 3],
    audio: &mut AudioSession,
    sequences: &mut FlakSoundSequences,
    spatial: Option<&AudioSpatial>,
) -> Result<FlakSoundCue, FlakSoundError> {
    let voice = sound.voice(kind);
    let spec = audio
        .spec(&voice.content)
        .ok_or_else(|| FlakSoundError::UnknownAsset(voice.content.clone()))?;
    let sequence = sequences.next;
    sequences.next = sequence
        .checked_add(1)
        .ok_or_else(|| FlakSoundError::Refused("the sequences are exhausted".to_owned()))?;
    let event = spec
        .one_shot_event(AudioEventId {
            session: audio.router.session(),
            tick,
            producer: FLAK_SOUND_PRODUCER,
            sequence,
        })
        .map_err(|error| FlakSoundError::Refused(error.to_string()))?;
    let placement = match spatial {
        None => SpatialPlacement::NoListener,
        Some(config) => match spatialize(&config.policy, &config.listener, origin_m) {
            Ok(mix) => SpatialPlacement::Placed(mix),
            Err(error) => return Err(FlakSoundError::Refused(error.to_string())),
        },
    };
    match audio.router.play_one_shot(&event) {
        OneShotOutcome::Accepted { .. } => Ok(FlakSoundCue {
            kind,
            event,
            origin_m,
            spatial: placement,
        }),
        other => Err(FlakSoundError::Refused(format!("{other:?}"))),
    }
}

/// The shot cue: the `FIRE` row's sound at the muzzle.
///
/// # Errors
///
/// As [`play_flak_sound`].
pub fn play_flak_shot(
    sound: &FlakSound,
    tick: Tick,
    muzzle_m: [f64; 3],
    audio: &mut AudioSession,
    sequences: &mut FlakSoundSequences,
    spatial: Option<&AudioSpatial>,
) -> Result<FlakSoundCue, FlakSoundError> {
    play_flak_sound(
        sound,
        FlakSoundKind::Shot,
        tick,
        muzzle_m,
        audio,
        sequences,
        spatial,
    )
}

/// The burst cue: the `IMPACT` row's sound at the burst point. Only a burst of
/// the `default` row sounds it ([`BURST_IMPACT_ROW`]).
///
/// # Errors
///
/// As [`play_flak_sound`]; [`FlakSoundError::Unmeasured`] for another row.
pub fn play_flak_burst(
    sound: &FlakSound,
    burst: &OrdnanceBurst,
    audio: &mut AudioSession,
    sequences: &mut FlakSoundSequences,
    spatial: Option<&AudioSpatial>,
) -> Result<FlakSoundCue, FlakSoundError> {
    if burst.impact_row != BURST_IMPACT_ROW {
        return Err(FlakSoundError::Unmeasured(format!(
            "IMPACT row {} is not the measured default row",
            burst.impact_row
        )));
    }
    play_flak_sound(
        sound,
        FlakSoundKind::Burst,
        burst.at,
        burst.position_m,
        audio,
        sequences,
        spatial,
    )
}

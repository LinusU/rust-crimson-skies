//! The player's bullet-hit ricochets (Rally #1865, PARITY6-M01-AUDIO-TIMELINE.05):
//! the 22,264, 16,192 and 18,216 B members of the 40.5-48.5 s burst.
//!
//! What is tied, and by which finding (provenance `static`, nothing
//! `verified_original`):
//!
//! * **Event.** The player aircraft's hit routine plays `player.zrd`
//!   `bullet_hit_sound` — the group `bullet_hit_sg` — two-dimensionally when a
//!   gun round lands on the player
//!   (`docs/findings/2026-10-22-m01-audio-burst-triggers.md`, `bullet_impact`).
//! * **Member.** `bullet_hit_sg` is a `DYNAMIC_WEIGHTS` group over
//!   `snd_ricochet1..4`; the engine draws one member per play with the
//!   selector of `docs/findings/2026-10-23-m01-audio-sfx-group-pick.md`
//!   ([`GroupPick`]): equal initial weights, a weighted draw from the C
//!   runtime `rand()`, the picked member's weight times the group's factor,
//!   then renormalised to 100.
//!
//! What stays unknown and is not wired: which of `bullet_hit_sg` and
//! `bullet_warning_sg` a hit plays (`[aircraft+0x91c]`), the `rand()` stream
//! position of the original (so the project seeds its own stream — a project
//! choice, not original behaviour), the explosion and ground-contact buffers
//! of the burst (their events are not tied to M01's flight), and whether the
//! original's hit cue is further gated.

use std::fmt;
use std::path::{Path, PathBuf};

use bevy::ecs::resource::Resource;
use bevy::prelude::{Res, ResMut};
use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioDraft, AudioLevel, AudioPlayback, PlaybackMode,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_sim::audio_events::{AudioEventId, OneShotOutcome};
use cs_types::Tick;
use cs_types::content::{ContentId, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use super::MissionStage;
use super::content::SoundArchive;
use super::gun_sound::{CLAIM_DECODED, CLAIM_MIX, READER_ARCHIVE, SOUNDS_MEMBER, sound_member};
use super::launch_sound::{LAUNCH_SOUND_CONTAINER, member_content_id, reader_member};
use super::player_victim::PlayerVictim;
use crate::audio::AudioSession;
use crate::physics::PhysicsTickLedger;
use crate::weapons::WeaponScheduleReport;

/// The group the player's hit routine plays (`player.zrd` `bullet_hit_sound`).
pub const BULLET_HIT_GROUP: &str = "bullet_hit_sg";

/// The audio producer serial of the hit cues (0 loops, 1 weapon shots and
/// radio, 2 music, 3 flak).
pub const HIT_SOUND_PRODUCER: u32 = 4;

/// The seed of the project's own `rand()` stream. The original seeds with
/// `time(NULL)` (or `-seed N`); a fixed seed is a project choice so a smoke run
/// repeats.
pub const HIT_SOUND_SEED: u32 = 1;

/// Claim: the one-shot mode is the absence of `LOOPED` in the member's flags.
const CLAIM_ONE_SHOT: &str = "parity6-audio-05.one-shot-mode";

/// `0x38000100` as read from the engine image: 1/32767.
const RAND_SCALE: f32 = 1.0 / 32767.0;
/// The slack the walk adds to the running sum: 0.001 of the total.
const WALK_SLACK: f32 = 0.001;
/// The floor an eligible non-picked member is raised to.
const WEIGHT_FLOOR: f32 = 0.001;
/// The sum the weights are renormalised to.
const WEIGHT_SUM: f32 = 100.0;

/// The MSVCRT `rand()` generator: `seed = seed * 214013 + 2531011`, the result
/// the bits 16..30 of the new seed (`0..=32767`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CRand {
    seed: u32,
}

impl CRand {
    /// A stream seeded like `srand(seed)`.
    #[must_use]
    pub const fn new(seed: u32) -> Self {
        Self { seed }
    }

    /// The next `rand()` answer, `0..=32767`.
    pub const fn next(&mut self) -> u32 {
        self.seed = self.seed.wrapping_mul(214_013).wrapping_add(2_531_011);
        (self.seed >> 16) & 0x7fff
    }
}

/// One member of a `DYNAMIC_WEIGHTS` group and its running weight.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupMember {
    /// The member's sound name, e.g. `snd_ricochet1`.
    pub name: String,
    /// Its current weight (percent of the next draw).
    pub weight: f32,
}

/// The selector of the engine's group voice (`0x59a440`), for a group whose
/// members all start equal.
#[derive(Clone, Debug, PartialEq)]
pub struct GroupPick {
    members: Vec<GroupMember>,
    factor: f32,
}

impl GroupPick {
    /// A group of `names` with the group's repeat `factor` (clamped to
    /// `0..=1`), every member at `100 / count`.
    ///
    /// # Errors
    ///
    /// A message for an empty group or a non-finite factor.
    pub fn new(names: Vec<String>, factor: f32) -> Result<Self, String> {
        if names.is_empty() {
            return Err("a sound group without members plays nothing".to_owned());
        }
        if !factor.is_finite() {
            return Err("the group's weight factor is not finite".to_owned());
        }
        let weight = WEIGHT_SUM / names.len() as f32;
        Ok(Self {
            members: names
                .into_iter()
                .map(|name| GroupMember { name, weight })
                .collect(),
            factor: factor.clamp(0.0, 1.0),
        })
    }

    /// The members with their current weights, in table order.
    #[must_use]
    pub fn members(&self) -> &[GroupMember] {
        &self.members
    }

    /// One play: the index of the member the selector picks, or `None` when
    /// the walk ends without a pick (a degenerate all-zero group).
    pub fn pick(&mut self, rand: &mut CRand) -> Option<usize> {
        if let [only] = self.members.as_slice() {
            return (only.weight > 0.0).then_some(0);
        }
        let total: f32 = self.members.iter().map(|m| m.weight).sum();
        let draw = rand.next() as f32 * RAND_SCALE * total;
        let mut running = 0.0f32;
        let mut picked = None;
        for (index, member) in self.members.iter().enumerate() {
            running += member.weight;
            if running + WALK_SLACK * total >= draw {
                picked = Some(index);
                break;
            }
        }
        let picked = picked?;
        self.members[picked].weight *= self.factor;
        for (index, member) in self.members.iter_mut().enumerate() {
            if index != picked && member.weight < WEIGHT_FLOOR {
                member.weight = WEIGHT_FLOOR;
            }
        }
        let sum: f32 = self.members.iter().map(|m| m.weight).sum();
        if sum > 0.0 {
            let scale = WEIGHT_SUM / sum;
            for member in &mut self.members {
                member.weight *= scale;
            }
        }
        Some(picked)
    }
}

/// One member of the measured group and where its sample comes from.
#[derive(Clone, Debug, PartialEq)]
pub struct HitVoice {
    /// The member's sound name, e.g. `snd_ricochet1`.
    pub name: String,
    /// The archive member `sounds.zrd` maps it to, e.g. `ricco4.wav`.
    pub member: String,
    /// The stable content id of the member in [`LAUNCH_SOUND_CONTAINER`].
    pub content: ContentId,
}

/// The measured `bullet_hit_sg` and what the composition needs to deliver it.
#[derive(Clone, Debug)]
pub struct HitSound {
    /// The members in table order.
    pub voices: Vec<HitVoice>,
    /// The group's weight factor.
    pub factor: f32,
    /// The archive the members are fed from.
    pub container: String,
    /// The installation's digest of that archive.
    pub container_sha256: ContentHash,
    /// The archive on the host, inside the read-only installation.
    pub host: PathBuf,
    /// The archive's byte length.
    pub bytes: u64,
}

/// Why the hit sound could not be measured or played.
#[derive(Debug, PartialEq)]
pub enum HitSoundError {
    /// A production reader refused.
    Read(String),
    /// The data does not name the group the way the measurement requires.
    Unmeasured(String),
    /// The load delivered no spec for the member.
    UnknownAsset(ContentId),
    /// The spec cannot form a one-shot, or the sequences ran out.
    Refused(String),
}

impl fmt::Display for HitSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the hit sound's source refuses: {reason}"),
            Self::Unmeasured(reason) => write!(f, "the hit sound is not measured: {reason}"),
            Self::UnknownAsset(id) => write!(f, "the load delivered no audio spec for {id}"),
            Self::Refused(reason) => write!(f, "the hit cue is refused: {reason}"),
        }
    }
}

impl std::error::Error for HitSoundError {}

/// The `.wav` of the single-sound record named `name`: a list whose first
/// element is `name` and whose second is a `.wav` text.
fn wav_of<'a>(node: &'a ZrdValue, name: &str) -> Option<&'a str> {
    let children = node.as_list()?;
    if children.first().and_then(ZrdValue::as_text) == Some(name)
        && let Some(file) = children.get(1).and_then(ZrdValue::as_text)
        && file.to_ascii_lowercase().ends_with(".wav")
    {
        return Some(file);
    }
    children.iter().find_map(|child| wav_of(child, name))
}

/// The `[group DYNAMIC_WEIGHTS factor member...]` record named `group`.
fn dynamic_record<'a>(node: &'a ZrdValue, group: &str) -> Option<&'a [ZrdValue]> {
    let children = node.as_list()?;
    if children.first().and_then(ZrdValue::as_text) == Some(group)
        && children.get(1).and_then(ZrdValue::as_text) == Some("DYNAMIC_WEIGHTS")
    {
        return Some(children);
    }
    children
        .iter()
        .find_map(|child| dynamic_record(child, group))
}

fn member_names(node: &ZrdValue, out: &mut Vec<String>) {
    match node {
        ZrdValue::List(children) => children.iter().for_each(|c| member_names(c, out)),
        ZrdValue::Text(text) if text.starts_with("snd_") => out.push(text.clone()),
        _ => {}
    }
}

/// The group's members (name, file) and factor from a decoded `sounds.zrd`.
///
/// # Errors
///
/// [`HitSoundError::Unmeasured`] when the group is absent, is not
/// `DYNAMIC_WEIGHTS`, spells anything but plain member lists (a `WEIGHT`,
/// `REPEAT`, ... keyword would change the selector's start state, which this
/// measurement does not model) or a member maps to no `.wav`.
pub fn voices_from_document(
    sounds: &ZrdValue,
) -> Result<(Vec<(String, String)>, f32), HitSoundError> {
    let unmeasured = HitSoundError::Unmeasured;
    let record = dynamic_record(sounds, BULLET_HIT_GROUP).ok_or_else(|| {
        unmeasured(format!(
            "sounds.zrd declares no DYNAMIC_WEIGHTS group {BULLET_HIT_GROUP}"
        ))
    })?;
    let factor = match record.get(2) {
        Some(ZrdValue::Float(value)) => *value,
        _ => return Err(unmeasured(format!("{BULLET_HIT_GROUP} states no factor"))),
    };
    let mut names = Vec::new();
    for member in &record[3..] {
        let list = member
            .as_list()
            .ok_or_else(|| unmeasured("a group member is not a list".to_owned()))?;
        if list.len() != 1 || list[0].as_text().is_none_or(|t| !t.starts_with("snd_")) {
            return Err(unmeasured(format!(
                "{BULLET_HIT_GROUP} spells more than a member name; the selector's start \
                 weights would not be equal"
            )));
        }
        member_names(member, &mut names);
    }
    let mut voices = Vec::new();
    for name in names {
        let file = wav_of(sounds, &name)
            .ok_or_else(|| unmeasured(format!("sounds.zrd maps {name} to no .wav member")))?;
        voices.push((name, file.to_owned()));
    }
    Ok((voices, factor))
}

/// Measures the group from the installation at `install_root`.
///
/// # Errors
///
/// [`HitSoundError`] when a reader refuses, the group is incomplete, a member
/// is not in the launch sound's container or the container is not among
/// `archives`.
pub fn measure_hit_sound(
    install_root: &Path,
    archives: &[SoundArchive],
) -> Result<HitSound, HitSoundError> {
    let bytes =
        reader_member(install_root, READER_ARCHIVE, SOUNDS_MEMBER).map_err(HitSoundError::Read)?;
    let sounds =
        decode_zrd(&bytes).map_err(|error| HitSoundError::Read(format!("sounds.zrd: {error}")))?;
    let (rows, factor) = voices_from_document(&sounds)?;
    let mut voices = Vec::new();
    for (name, member) in rows {
        sound_member(install_root, LAUNCH_SOUND_CONTAINER, &member).map_err(HitSoundError::Read)?;
        let content = member_content_id(LAUNCH_SOUND_CONTAINER, &member)
            .map_err(|e| HitSoundError::Read(e.to_string()))?;
        voices.push(HitVoice {
            name,
            member,
            content,
        });
    }
    let archive = archives
        .iter()
        .find(|archive| archive.key.eq_ignore_ascii_case(LAUNCH_SOUND_CONTAINER))
        .ok_or_else(|| {
            HitSoundError::Unmeasured(format!(
                "{LAUNCH_SOUND_CONTAINER} is not among the mission's sound archives"
            ))
        })?;
    let container_sha256 =
        ContentHash::from_hex(&archive.sha256).map_err(|e| HitSoundError::Read(e.to_string()))?;
    let host = install_root.join(LAUNCH_SOUND_CONTAINER);
    let bytes = std::fs::metadata(&host)
        .map_err(|e| HitSoundError::Read(format!("cannot measure {}: {e}", host.display())))?
        .len();
    Ok(HitSound {
        voices,
        factor,
        container: LAUNCH_SOUND_CONTAINER.to_owned(),
        container_sha256,
        host,
        bytes,
    })
}

impl HitSound {
    /// The members' declared catalog records: the one-shot mode is measured
    /// (`sounds.zrd` flags no `LOOPED`); the `Weapons` bus, the level and the
    /// container feed are designed.
    ///
    /// # Errors
    ///
    /// [`HitSoundError::Read`] when a record part is refused.
    pub fn records(&self) -> Result<Vec<AudioAssetRecord>, HitSoundError> {
        let refuse = HitSoundError::Read;
        let claim = |id: &str| ClaimId::new(id).map_err(|e| refuse(e.to_string()));
        let mut out = Vec::new();
        for voice in &self.voices {
            let designed = || -> Result<Provenance, HitSoundError> {
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
        out.dedup_by(|a, b| a.id() == b.id());
        Ok(out)
    }

    /// A fresh selector over this group.
    ///
    /// # Errors
    ///
    /// [`HitSoundError::Unmeasured`] for an empty group.
    pub fn picker(&self) -> Result<GroupPick, HitSoundError> {
        GroupPick::new(
            self.voices.iter().map(|v| v.name.clone()).collect(),
            self.factor,
        )
        .map_err(HitSoundError::Unmeasured)
    }
}

/// Resource: the hit producer's state for the installed session — the group's
/// live weights, the project's `rand()` stream, the strictly increasing
/// one-shot sequence and the report of what played.
#[derive(Resource, Clone, Debug)]
pub struct HitSoundState {
    picker: Option<GroupPick>,
    rand: CRand,
    next: u32,
    /// The members played so far, by sound name, in order.
    pub played: Vec<String>,
    /// Refusals, by name (a cue is never silently dropped).
    pub refusals: Vec<String>,
}

impl Default for HitSoundState {
    fn default() -> Self {
        Self {
            picker: None,
            rand: CRand::new(HIT_SOUND_SEED),
            next: 0,
            played: Vec::new(),
            refusals: Vec::new(),
        }
    }
}

/// Plays one hit cue on `tick`: one selector pick over the group, then the
/// picked member as a two-dimensional one-shot.
///
/// # Errors
///
/// [`HitSoundError::UnknownAsset`] when the load delivered no spec for the
/// picked member, [`HitSoundError::Refused`] for a spec that is no one-shot,
/// an exhausted sequence, a pick that ended silent or a router refusal.
pub fn play_hit_cue(
    sound: &HitSound,
    state: &mut HitSoundState,
    tick: Tick,
    audio: &mut AudioSession,
) -> Result<usize, HitSoundError> {
    if state.picker.is_none() {
        state.picker = Some(sound.picker()?);
    }
    let picker = state.picker.as_mut().expect("set above");
    let index = picker
        .pick(&mut state.rand)
        .ok_or_else(|| HitSoundError::Refused("the selector picked no member".to_owned()))?;
    let voice = &sound.voices[index];
    let spec = audio
        .spec(&voice.content)
        .ok_or_else(|| HitSoundError::UnknownAsset(voice.content.clone()))?;
    let sequence = state.next;
    state.next = sequence
        .checked_add(1)
        .ok_or_else(|| HitSoundError::Refused("the sequences are exhausted".to_owned()))?;
    let event = spec
        .one_shot_event(AudioEventId {
            session: audio.router.session(),
            tick,
            producer: HIT_SOUND_PRODUCER,
            sequence,
        })
        .map_err(|error| HitSoundError::Refused(error.to_string()))?;
    match audio.router.play_one_shot(&event) {
        OneShotOutcome::Accepted { .. } => {
            state.played.push(voice.name.clone());
            Ok(index)
        }
        other => Err(HitSoundError::Refused(format!("{other:?}"))),
    }
}

/// One fixed tick of the hit cues: every gun round the weapon step routed onto
/// the player this tick plays one pick of `bullet_hit_sg`.
///
/// Runs after the weapon step and before the damage schedule, on the
/// [`WeaponScheduleReport`] the step just wrote (like the debrief's tally). A
/// measured sound with no audio session is a named refusal, never silent.
pub fn cue_player_hits(
    stage: Option<Res<MissionStage>>,
    victim: Option<Res<PlayerVictim>>,
    report: Option<Res<WeaponScheduleReport>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    audio: Option<ResMut<AudioSession>>,
    state: Option<ResMut<HitSoundState>>,
) {
    let (Some(stage), Some(victim), Some(report), Some(ledger), Some(mut state)) =
        (stage, victim, report, ledger, state)
    else {
        return;
    };
    let Some(sound) = stage.hit_sound.as_ref() else {
        return;
    };
    let Some(stepped) = &report.stepped else {
        return;
    };
    let hits = stepped
        .routed
        .iter()
        .flat_map(|routed| routed.outcome.sweep.damage())
        .filter(|hit| hit.target == victim.actor && hit.attacker != Some(victim.actor))
        .count();
    if hits == 0 {
        return;
    }
    let Some(mut audio) = audio else {
        state.refusals.push(
            "the bullet hit cue has a measured sound but no audio session is installed".to_owned(),
        );
        return;
    };
    for _ in 0..hits {
        if let Err(error) = play_hit_cue(sound, &mut state, Tick(ledger.ticks), &mut audio) {
            state
                .refusals
                .push(format!("the bullet hit cue was refused: {error}"));
        }
    }
}

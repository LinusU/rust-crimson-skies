//! The M01 ambient loops (Rally #1864, PARITY6-M01-AUDIO-TIMELINE.04): the
//! player's `snd_planeshake` rattle and the `snd_waterfall` loop at the
//! `spew` nodes, measured from the original data.
//!
//! The chains are read, never assumed (finding
//! `docs/findings/2026-10-23-m01-audio-ambient-loops-wiring.md`, provenance
//! `static`):
//!
//! * rattle: `player.zrd` `rattle` names the group (`sound`) and
//!   `speed_range[0]`; `sounds.zrd` maps the group to a member and flags it
//!   `LOOPED`; the airframe record's chain supplies `mode` (the rattle
//!   consumer runs for the `jet` and `wingman` modes only) and `fd_speed`
//!   (read as the airframe maximum `[+0x668]`, a designed reading);
//! * waterfall: the world group's `waterfalls.zrd` animation holding
//!   `SOUND snd_waterfall AT_NODE <node>` carries `EXECUTION_BY_RANGE`; the
//!   node's composed world positions come from the world group's
//!   `gamez.zbd`; `sounds.zrd` supplies the member, `LOOPED` and `RANGE`.
//!
//! The 33,864 B loop has no member in either archive (#1861) and is not
//! wired; it is named in [`AmbientSound::plan`]'s refusals.

use std::fmt;
use std::path::Path;

use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioDraft, AudioLevel, AudioPlayback, PlaybackMode,
};
use cs_content::original_airframe::OriginalDocuments;
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_types::content::{ContentId, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus};

use super::gun_sound::{READER_ARCHIVE, SOUNDS_MEMBER, record_headed_by, sound_member, value_of};
use super::launch_sound::{
    LAUNCH_SOUND_CONTAINER, field_numbers, member_content_id, reader_member,
};
use crate::audio::{AmbientVoicePlan, EngineGroupLaw, RattleSpec, WaterfallSpec};

/// The member of the global reader archive holding the rattle section.
const PLAYER_MEMBER: &str = "player.zrd";
/// The world group's member holding the waterfall animation.
const WATERFALLS_MEMBER: &str = "waterfalls.zrd";
/// The vehicle modes whose update reaches the rattle consumer: `jet` is mode
/// 0 and `wingman` mode 4 (`vehicle.update_table`).
const RATTLE_MODES: [&str; 2] = ["jet", "wingman"];
/// The sound group of the waterfall loop.
const WATERFALL_GROUP: &str = "snd_waterfall";
/// Names the one unresolved loop in the plan's refusals.
const UNRESOLVED_LOOP: &str = "33864 B loop (11025 Hz): no member of either sound archive \
     decodes to this size (#1861); recorded unknown and not wired";

/// The bus the ambient records are declared on. Not `Environment`: an
/// `Environment` + `Loop` record is bound by `bind_spawned_emitters` as the
/// world's ambient emitter, which would start these members as an unplaced
/// loop of their own; these two voices are driven by
/// `crate::audio::drive_ambient_voices` instead. Both groups carry the `SFX`
/// tag in `sounds.zrd`; `Impacts` is the designed stand-in for it.
const AMBIENT_BUS: AudioBus = AudioBus::Impacts;

const CLAIM_MIX: &str = "parity6-m01-audio-timeline-04.ambient-mix";
const CLAIM_LOOPED: &str = "parity6-m01-audio-timeline-04.looped-mode";
const CLAIM_DECODED: &str = "parity6-m01-audio-timeline-04.decoded-shape";

/// Why the ambient sounds could not be measured.
#[derive(Debug, PartialEq, Eq)]
pub enum AmbientSoundError {
    /// A production reader refused.
    Read(String),
}

impl fmt::Display for AmbientSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the ambient sound's source refuses: {reason}"),
        }
    }
}

impl std::error::Error for AmbientSoundError {}

/// The measured ambient loops and the members the composition delivers.
#[derive(Clone, Debug, PartialEq)]
pub struct AmbientSound {
    /// The plan the audio plugin runs.
    pub plan: AmbientVoicePlan,
    /// The members to deliver from [`LAUNCH_SOUND_CONTAINER`].
    pub members: Vec<ContentId>,
}

fn number(value: &ZrdValue) -> Option<f64> {
    match value {
        ZrdValue::Int(n) => Some(f64::from(*n)),
        ZrdValue::Float(n) => Some(f64::from(*n)),
        _ => None,
    }
}

/// The numbers following `key` (a `key, [n, ...]` pair).
fn numbers_of(node: &ZrdValue, key: &str) -> Option<Vec<f64>> {
    Some(
        value_of(node, key)?
            .as_list()?
            .iter()
            .filter_map(number)
            .collect(),
    )
}

/// One group of `sounds.zrd`: member, `LOOPED`, `RANGE`, `VOLUME`.
struct Group {
    member: String,
    looped: bool,
    range: Option<(f64, f64)>,
    volume: Option<f64>,
}

fn group_of(sounds: &ZrdValue, group: &str) -> Result<Group, String> {
    let entry = record_headed_by(sounds, group)
        .ok_or_else(|| format!("{SOUNDS_MEMBER} declares no group {group}"))?;
    let member = entry
        .get(1)
        .and_then(ZrdValue::as_text)
        .ok_or_else(|| format!("{SOUNDS_MEMBER} maps {group} to no single member"))?
        .to_owned();
    let range = field_numbers(entry, "RANGE").and_then(|n| Some((*n.first()?, *n.get(1)?)));
    let volume = field_numbers(entry, "VOLUME").and_then(|n| n.first().copied());
    Ok(Group {
        member,
        looped: entry.iter().any(|flag| flag.as_text() == Some("LOOPED")),
        range,
        volume,
    })
}

fn rattle_from_documents(
    player: &ZrdValue,
    sounds: &ZrdValue,
    mode: Option<&str>,
    max_speed: Option<f64>,
) -> Result<(RattleSpec, String), String> {
    let mode = mode.ok_or("the airframe record chain carries no mode")?;
    if !RATTLE_MODES.contains(&mode) {
        return Err(format!(
            "the airframe mode {mode:?} is not stepped by the rattle consumer (jet, wingman)"
        ));
    }
    let max_speed = max_speed
        .filter(|speed| speed.is_finite() && *speed > 0.0)
        .ok_or("the airframe record chain carries no usable fd_speed")?;
    let rattle = value_of(player, "rattle").ok_or("player.zrd has no rattle section")?;
    let name = value_of(rattle, "sound")
        .and_then(super::gun_sound::first_text)
        .ok_or("player.zrd rattle names no sound")?;
    let speed_floor = *numbers_of(rattle, "speed_range")
        .ok_or("player.zrd rattle has no speed_range")?
        .first()
        .ok_or("player.zrd rattle speed_range is empty")?;
    let group = group_of(sounds, name)?;
    if !group.looped {
        return Err(format!(
            "{name} is not LOOPED: the held slot would play it once and keep a dead handle"
        ));
    }
    let spec = RattleSpec {
        group: name.to_owned(),
        member: member_content_id(LAUNCH_SOUND_CONTAINER, &group.member)
            .map_err(|e| e.to_string())?,
        // The group carries no VOLUME: the descriptor default was not read,
        // so the neutral factor is used (designed).
        volume: group.volume.unwrap_or(1.0),
        speed_floor,
        max_speed_m_s: max_speed,
    };
    Ok((spec, group.member))
}

/// The `ANIMATION_DEFINITION` that holds `SOUND <group> AT_NODE <node>`:
/// the node it names and its `EXECUTION_BY_RANGE` values.
fn waterfall_animation(waterfalls: &ZrdValue, group: &str) -> Option<(String, Vec<f64>)> {
    fn walk<'a>(node: &'a ZrdValue, group: &str, out: &mut Option<&'a ZrdValue>) {
        let Some(children) = node.as_list() else {
            return;
        };
        if children.first().and_then(ZrdValue::as_text) == Some("ANIMATION_DEFINITION")
            && contains_text(node, group)
        {
            *out = Some(node);
            return;
        }
        for child in children {
            walk(child, group, out);
        }
    }
    fn contains_text(node: &ZrdValue, text: &str) -> bool {
        match node {
            ZrdValue::Text(t) => t == text,
            ZrdValue::List(children) => children.iter().any(|c| contains_text(c, text)),
            _ => false,
        }
    }
    let mut found = None;
    walk(waterfalls, group, &mut found);
    let definition = found?;
    let sound = value_of(definition, "SOUND")?;
    let at_node = value_of(sound, "AT_NODE").and_then(super::gun_sound::first_text)?;
    let range = numbers_of(definition, "EXECUTION_BY_RANGE")?;
    Some((at_node.to_owned(), range))
}

/// The squared-distance window `0x51edb0` stores for `EXECUTION_BY_RANGE`: a
/// single value is `[0, value^2)`, two are `[min^2, max^2)`.
fn window_sq(range: &[f64]) -> Option<(f64, f64)> {
    match range {
        [max] if *max > 0.0 => Some((0.0, max * max)),
        [min, max] if *min >= 0.0 && max > min => Some((min * min, max * max)),
        _ => None,
    }
}

/// Measures the ambient loops of the mission whose world group is
/// `group_dir` (e.g. `zbd/c3`) for the player airframe record `airframe`.
///
/// # Errors
///
/// [`AmbientSoundError::Read`] when a document the chain needs cannot be
/// read. A voice the data cannot bind is a named refusal in the plan, not an
/// error.
pub fn measure_ambient_sound(
    install_root: &Path,
    airframe: &str,
    group_dir: &str,
) -> Result<AmbientSound, AmbientSoundError> {
    let read = |reason: String| AmbientSoundError::Read(reason);
    let decode = |archive: &str, member: &str| -> Result<ZrdValue, AmbientSoundError> {
        let bytes = reader_member(install_root, archive, member).map_err(read)?;
        decode_zrd(&bytes).map_err(|e| read(format!("{member}: {e}")))
    };
    let sounds = decode(READER_ARCHIVE, SOUNDS_MEMBER)?;
    let player = decode(READER_ARCHIVE, PLAYER_MEMBER)?;
    let documents = OriginalDocuments::read(install_root).map_err(|e| read(e.to_string()))?;
    let parameters = documents
        .airframe(airframe)
        .map_err(|e| read(format!("{airframe}: {e}")))?;
    let mode = parameters.mode.clone();
    let max_speed = parameters.value("fd_speed").map(|field| field.value);

    let mut plan = AmbientVoicePlan::default();
    let mut members = Vec::new();
    match rattle_from_documents(&player, &sounds, mode.as_deref(), max_speed) {
        Ok((spec, member)) => {
            sound_member(install_root, LAUNCH_SOUND_CONTAINER, &member).map_err(read)?;
            members.push(spec.member.clone());
            plan.rattle = Some(spec);
        }
        Err(reason) => plan.refusals.push(format!("rattle: {reason}")),
    }

    let waterfall = (|| -> Result<WaterfallSpec, String> {
        let archive = format!("{}/zrdr.zbd", mission_group_key(group_dir));
        let waterfalls = decode(&archive, WATERFALLS_MEMBER).map_err(|e| e.to_string())?;
        let (node_name, range) =
            waterfall_animation(&waterfalls, WATERFALL_GROUP).ok_or_else(|| {
                format!("{WATERFALLS_MEMBER} has no animation sounding {WATERFALL_GROUP}")
            })?;
        let window = window_sq(&range)
            .ok_or_else(|| format!("EXECUTION_BY_RANGE {range:?} is not a usable range"))?;
        let group = group_of(&sounds, WATERFALL_GROUP)?;
        if !group.looped {
            return Err(format!("{WATERFALL_GROUP} is not LOOPED"));
        }
        let (near, far) = group
            .range
            .ok_or_else(|| format!("{WATERFALL_GROUP} carries no RANGE"))?;
        let law = EngineGroupLaw::try_new(near, far, group.volume.unwrap_or(1.0))
            .map_err(|_| format!("{WATERFALL_GROUP} RANGE ({near}, {far}) is not a law"))?;
        let nodes: Vec<[f64; 3]> =
            super::super::mission_targets::world_node_positions(install_root, group_dir)?
                .into_iter()
                .filter(|(name, _)| *name == node_name)
                .map(|(_, position)| position)
                .collect();
        if nodes.is_empty() {
            return Err(format!(
                "the world container holds no node named {node_name:?}"
            ));
        }
        sound_member(install_root, LAUNCH_SOUND_CONTAINER, &group.member)?;
        Ok(WaterfallSpec {
            group: WATERFALL_GROUP.to_owned(),
            member: member_content_id(LAUNCH_SOUND_CONTAINER, &group.member)
                .map_err(|e| e.to_string())?,
            law,
            window_sq: window,
            nodes,
        })
    })();
    match waterfall {
        Ok(spec) => {
            members.push(spec.member.clone());
            plan.waterfall = Some(spec);
        }
        Err(reason) => plan.refusals.push(format!("waterfall: {reason}")),
    }
    plan.refusals.push(UNRESOLVED_LOOP.to_owned());
    members.dedup();
    Ok(AmbientSound { plan, members })
}

/// `zbd/c3` spelled as the reader archive's directory (`ZBD/C3`).
fn mission_group_key(group_dir: &str) -> String {
    let rest = group_dir.trim_start_matches("zbd/");
    format!("ZBD/{}", rest.to_ascii_uppercase())
}

impl AmbientSound {
    /// The members' declared catalog records: the loop mode is measured
    /// (`LOOPED`); the bus and the level are designed.
    ///
    /// # Errors
    ///
    /// [`AmbientSoundError::Read`] when a record part is refused.
    pub fn records(&self) -> Result<Vec<AudioAssetRecord>, AmbientSoundError> {
        let refuse = |reason: String| AmbientSoundError::Read(reason);
        let claim = |id: &str| ClaimId::new(id).map_err(|e| refuse(e.to_string()));
        let mut out = Vec::new();
        for id in &self.members {
            let designed = || -> Result<Provenance, AmbientSoundError> {
                Ok(Provenance::designed(claim(CLAIM_MIX)?))
            };
            out.push(
                AudioAssetRecord::try_new(AudioDraft {
                    id: id.clone(),
                    origin: Origin::Designed,
                    playback: AudioPlayback {
                        bus: Resolved::Known(Known::new(AMBIENT_BUS, designed()?)),
                        level: Resolved::Known(Known::new(
                            AudioLevel::try_new(1.0).map_err(|e| refuse(e.to_string()))?,
                            designed()?,
                        )),
                        mode: Resolved::Known(Known::new(
                            PlaybackMode::Loop,
                            Provenance::new(claim(CLAIM_LOOPED)?, ClaimStatus::ObservedTool, None)
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
        Ok(out)
    }
}

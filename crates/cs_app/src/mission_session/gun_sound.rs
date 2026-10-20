//! Which original sound a gun fires (M01-WINDOW-FIRE-GUN-SOUND, Rally #1546),
//! and which of those records the player's M01 guns carry
//! (M01-WINDOW-FIRE-GUN-LOADOUT, Rally #1628).
//!
//! The binding is a chain of record reads, never a filename guess:
//!
//! 1. the fresh-campaign pilot record's gun and ammunition slots — measured
//!    in `data/owner-static/engine-facts.toml` (`[flight_check_default]`,
//!    #1670) — answer `wep_50`, `wep_40` and `wep_30` through the engine's
//!    own `(gun, ammunition) -> weapon index` map (`[gun_loadout]`, #1628);
//!    the engine's current-gun selector resolves the first slot, so the
//!    delivered member is `wep_50`'s;
//! 2. a `BALLISTICS` record of `weapons.zrd` (`ZBD/zrdr.zbd`) names a sound
//!    group in its `LOOPED_SOUND_NAME` field (`wep_50` → `snd_50cal`);
//! 3. `sounds.zrd` maps the group to one archive member and carries its
//!    playback flags (`snd_50cal` → `50cal_gun.wav`, `3D LOOPED SFX`);
//! 4. the engine reads that field for every weapon record it loads (static
//!    evidence, `docs/findings/2026-10-18-m01-window-fire-gun-sound.md`).
//!
//! The member is fed from the launch sound's container — the same labelled
//! choice #1313 records, since which of `soundsl.zbd` / `soundsh.zbd` the
//! original opens is unmeasured — and declared on the `Weapons` bus in loop
//! mode. The bus and level are designed; the loop mode is measured, and the
//! record's provenance says so.

use std::fmt;
use std::path::{Path, PathBuf};

use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioDraft, AudioLevel, AudioPlayback, PlaybackMode,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_sound_archive, read_version_one_index};
use cs_types::content::{ContentId, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ClaimStatus, ContentHash};

use super::content::SoundArchive;
use super::launch_sound::{LAUNCH_SOUND_CONTAINER, member_content_id, reader_member};
use crate::engine_facts::{EngineFactsError, FlightCheckPlaneSlot, engine_facts};

/// The reader archive holding `weapons.zrd` and `sounds.zrd`.
pub(super) const READER_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member holding the ballistics records.
pub(super) const WEAPONS_MEMBER: &str = "weapons.zrd";
/// The member mapping sound groups to archive members.
const SOUNDS_MEMBER: &str = "sounds.zrd";
/// The weapon-record field naming the gun's sound group.
const SOUND_FIELD: &str = "LOOPED_SOUND_NAME";

/// The measured sound of one weapon record.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct WeaponSound {
    /// The weapon record, e.g. `wep_50`.
    pub weapon: String,
    /// The sound group its `LOOPED_SOUND_NAME` names.
    pub group: String,
    /// The archive member `sounds.zrd` maps the group to.
    pub member: String,
    /// Whether `sounds.zrd` flags the group `LOOPED`.
    pub looped: bool,
}

/// Why a weapon's sound could not be measured.
#[derive(Debug, PartialEq, Eq)]
pub enum WeaponSoundError {
    /// A production reader refused.
    Read(String),
    /// The record does not name the sound the way the measurement requires.
    Unmeasured(String),
}

impl fmt::Display for WeaponSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the weapon sound's source refuses: {reason}"),
            Self::Unmeasured(reason) => write!(f, "the weapon sound is not measured: {reason}"),
        }
    }
}

impl std::error::Error for WeaponSoundError {}

/// The value following `key` in the first list that holds it as a key.
pub(super) fn value_of<'a>(node: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
    let children = node.as_list()?;
    for (index, child) in children.iter().enumerate() {
        if child.as_text() == Some(key)
            && let Some(value) = children.get(index + 1)
        {
            return Some(value);
        }
    }
    children.iter().find_map(|child| value_of(child, key))
}

/// The first text in `node`, looking through single-element lists.
fn first_text(node: &ZrdValue) -> Option<&str> {
    match node {
        ZrdValue::Text(text) => Some(text),
        ZrdValue::List(children) => children.first().and_then(first_text),
        _ => None,
    }
}

/// The list whose first element is the text `name`.
fn record_headed_by<'a>(node: &'a ZrdValue, name: &str) -> Option<&'a [ZrdValue]> {
    let children = node.as_list()?;
    if children.first().and_then(ZrdValue::as_text) == Some(name) {
        return Some(children);
    }
    children
        .iter()
        .find_map(|child| record_headed_by(child, name))
}

/// Measures `weapon`'s sound from decoded `weapons.zrd` and `sounds.zrd`.
///
/// # Errors
///
/// [`WeaponSoundError::Unmeasured`] when the record, its field or the group's
/// member is absent.
pub fn weapon_sound_from_documents(
    weapons: &ZrdValue,
    sounds: &ZrdValue,
    weapon: &str,
) -> Result<WeaponSound, WeaponSoundError> {
    let unmeasured = |reason: String| WeaponSoundError::Unmeasured(reason);
    let ballistics = value_of(weapons, "BALLISTICS")
        .ok_or_else(|| unmeasured("weapons.zrd has no BALLISTICS section".to_owned()))?;
    let record = value_of(ballistics, weapon)
        .ok_or_else(|| unmeasured(format!("BALLISTICS has no record {weapon}")))?;
    let group = value_of(record, SOUND_FIELD)
        .and_then(first_text)
        .ok_or_else(|| unmeasured(format!("{weapon} declares no {SOUND_FIELD}")))?
        .to_owned();
    let entry = record_headed_by(sounds, &group)
        .ok_or_else(|| unmeasured(format!("sounds.zrd declares no group {group}")))?;
    let member = entry
        .get(1)
        .and_then(ZrdValue::as_text)
        .ok_or_else(|| unmeasured(format!("sounds.zrd maps {group} to no single member")))?
        .to_owned();
    let looped = entry.iter().any(|flag| flag.as_text() == Some("LOOPED"));
    Ok(WeaponSound {
        weapon: weapon.to_owned(),
        group,
        member,
        looped,
    })
}

/// Measures `weapon`'s sound from the installation at `install_root`.
///
/// # Errors
///
/// [`WeaponSoundError`] when a reader refuses or the chain is incomplete.
pub fn measure_weapon_sound(
    install_root: &Path,
    weapon: &str,
) -> Result<WeaponSound, WeaponSoundError> {
    let read = |member: &str| -> Result<ZrdValue, WeaponSoundError> {
        let bytes =
            reader_member(install_root, READER_ARCHIVE, member).map_err(WeaponSoundError::Read)?;
        decode_zrd(&bytes).map_err(|error| WeaponSoundError::Read(format!("{member}: {error}")))
    };
    weapon_sound_from_documents(&read(WEAPONS_MEMBER)?, &read(SOUNDS_MEMBER)?, weapon)
}

// --------------------------------------------------------- the gun loadout --

/// The member the fresh-campaign pilot's selected gun record resolves to:
/// `wep_50` → `snd_50cal` → `50cal_gun.wav`.
///
/// Spelled once so the mission's loadout can name the delivered id; the
/// measurement verifies this is exactly the member the record chain answers,
/// so a spelling that drifted from the data refuses rather than binds a
/// wrong file.
pub const MEASURED_GUN_MEMBER: &str = "50cal_gun.wav";

/// The claim ids the delivered record's provenance cites (#1628).
const CLAIM_MIX: &str = "m01-window-fire-gun-loadout.gun-loop-mix";
const CLAIM_LOOPED: &str = "m01-window-fire-gun-loadout.looped-mode";
const CLAIM_DECODED: &str = "m01-window-fire-gun-loadout.decoded-shape";

/// The content id [`measure_gun_sound`] delivers: [`MEASURED_GUN_MEMBER`]
/// inside the launch sound's container, derived through the same helper the
/// launch member's id is.
///
/// The mission's designed loadout names this as its guns' shot sound, so the
/// delivered member and the session's fire effect are one binding — the
/// spelling is fixed and the grammar never refuses it.
#[must_use]
pub fn measured_gun_sound_id() -> ContentId {
    member_content_id(LAUNCH_SOUND_CONTAINER, MEASURED_GUN_MEMBER)
        .expect("the measured gun member's id is valid")
}

/// The player's measured M01 gun sounds and everything the composition
/// needs to deliver the selected one (#1628).
#[derive(Clone, Debug)]
pub struct GunSound {
    /// Every gun slot's measured sound, in the pilot record's slot order —
    /// `wep_50`, `wep_40`, `wep_30` for the fresh-campaign Devastator.
    /// `weapons[0]` is the delivered one: the engine's current-gun selector
    /// resolves the first slot its resolver answers.
    pub weapons: Vec<WeaponSound>,
    /// The archive the delivered member is fed from — the launch sound's
    /// container, the same labelled choice that finding records.
    pub container: String,
    /// The installation's digest of that archive.
    pub container_sha256: ContentHash,
    /// The archive on the host, inside the read-only installation.
    pub host: PathBuf,
    /// The stable content id of the delivered member.
    pub content: ContentId,
    /// The archive's byte length.
    pub bytes: u64,
}

/// Why the player's gun sound could not be measured.
#[derive(Debug)]
pub enum GunSoundError {
    /// The committed engine facts could not be read.
    Facts(EngineFactsError),
    /// A weapon record's sound chain refused.
    Weapon(WeaponSoundError),
    /// A production reader refused.
    Read(String),
    /// The measured data does not satisfy what the delivery requires.
    Unmeasured(String),
}

impl fmt::Display for GunSoundError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Facts(error) => write!(f, "the engine facts refuse: {error}"),
            Self::Weapon(error) => write!(f, "{error}"),
            Self::Read(reason) => write!(f, "the gun sound's source refuses: {reason}"),
            Self::Unmeasured(reason) => write!(f, "the gun loadout is not measured: {reason}"),
        }
    }
}

impl std::error::Error for GunSoundError {}

/// Measures the player's M01 gun sounds from the installation at
/// `install_root`: the fresh-campaign pilot record's slots through the
/// committed (gun, ammunition) map, then each record's `LOOPED_SOUND_NAME`
/// chain through the production readers.
///
/// # Errors
///
/// [`GunSoundError`] when the facts, a reader or a record chain refuses,
/// the selected record's member is not `LOOPED` or is not
/// [`MEASURED_GUN_MEMBER`], or the launch sound's container is not among
/// `archives`.
pub fn measure_gun_sound(
    install_root: &Path,
    archives: &[SoundArchive],
) -> Result<GunSound, GunSoundError> {
    let facts = engine_facts().map_err(GunSoundError::Facts)?;
    let loadout = facts.gun_loadout();
    let default = facts
        .flight_check()
        .default_for(FlightCheckPlaneSlot::Pilot);
    let mut weapons = Vec::new();
    for (&gun, &ammo) in default.guns.iter().zip(default.ammo.iter()) {
        let Some(index) = loadout.weapon_index(gun, ammo) else {
            continue;
        };
        weapons.push(
            measure_weapon_sound(install_root, &loadout.record_name(index))
                .map_err(GunSoundError::Weapon)?,
        );
    }
    let Some(selected) = weapons.first() else {
        return Err(GunSoundError::Unmeasured(
            "the pilot record's gun slots resolve to no weapon record".to_owned(),
        ));
    };
    if !selected.looped {
        return Err(GunSoundError::Unmeasured(format!(
            "the selected record {}'s group {} is not flagged LOOPED",
            selected.weapon, selected.group
        )));
    }
    if selected.member != MEASURED_GUN_MEMBER {
        return Err(GunSoundError::Unmeasured(format!(
            "the selected record {} resolves to {:?}, not {MEASURED_GUN_MEMBER:?}",
            selected.weapon, selected.member
        )));
    }
    // The member is delivered from the launch sound's container — prove it
    // is really inside rather than spelling the name twice by hoping. The
    // container is a sound-family archive, which `reader_member` refuses by
    // family, so the sound reader answers membership here.
    sound_member(install_root, LAUNCH_SOUND_CONTAINER, &selected.member)
        .map_err(GunSoundError::Read)?;
    let content = member_content_id(LAUNCH_SOUND_CONTAINER, &selected.member)
        .map_err(|e| GunSoundError::Read(e.to_string()))?;
    let archive = archives
        .iter()
        .find(|archive| archive.key.eq_ignore_ascii_case(LAUNCH_SOUND_CONTAINER))
        .ok_or_else(|| {
            GunSoundError::Unmeasured(format!(
                "{LAUNCH_SOUND_CONTAINER} is not among the mission's sound archives"
            ))
        })?;
    let container_sha256 =
        ContentHash::from_hex(&archive.sha256).map_err(|e| GunSoundError::Read(e.to_string()))?;
    let host = install_root.join(LAUNCH_SOUND_CONTAINER);
    let bytes = std::fs::metadata(&host)
        .map_err(|e| GunSoundError::Read(format!("cannot measure {}: {e}", host.display())))?
        .len();
    Ok(GunSound {
        weapons,
        container: LAUNCH_SOUND_CONTAINER.to_owned(),
        container_sha256,
        host,
        content,
        bytes,
    })
}

impl GunSound {
    /// The delivered member's declared catalog record: its `LOOPED` mode is
    /// measured installation evidence (`ObservedTool`); the `Weapons` bus,
    /// the level and the container feed are designed, and the record says so.
    ///
    /// # Errors
    ///
    /// [`GunSoundError::Read`] when a record part is refused.
    pub fn record(&self) -> Result<AudioAssetRecord, GunSoundError> {
        let refuse = |reason: String| GunSoundError::Read(reason);
        let claim = |id: &str| ClaimId::new(id).map_err(|e| refuse(e.to_string()));
        let designed =
            || -> Result<Provenance, GunSoundError> { Ok(Provenance::designed(claim(CLAIM_MIX)?)) };
        let record = AudioAssetRecord::try_new(AudioDraft {
            id: self.content.clone(),
            origin: Origin::Designed,
            playback: AudioPlayback {
                bus: Resolved::Known(Known::new(AudioBus::Weapons, designed()?)),
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
        .map_err(|e| refuse(e.to_string()))?;
        Ok(record)
    }
}

/// One member of a sound-family archive (`ZBD/sounds*.zbd`), read through
/// the same production dispatch the loader runs.
///
/// [`reader_member`] only reads *reader* archives and refuses a sound
/// container by family; the delivered gun member lives in a sound archive,
/// so its membership proof goes through [`read_sound_archive`] instead.
fn sound_member(install_root: &Path, container: &str, member: &str) -> Result<Vec<u8>, String> {
    let bytes = std::fs::read(install_root.join(container))
        .map_err(|error| format!("cannot read {container}: {error}"))?;
    let spelling = cs_types::install::RelativePath::new(container).map_err(|e| e.to_string())?;
    let mut context = ParseContext::with_defaults("gun_sound");
    let decision =
        dispatch(ZbdProbe::new(container, &spelling, &bytes)).map_err(|e| e.to_string())?;
    let index =
        read_version_one_index(&mut context, decision, &bytes).map_err(|e| e.to_string())?;
    let table = index.member_table();
    let archive =
        read_sound_archive(&mut context, &table, index.data()).map_err(|e| e.to_string())?;
    archive
        .entries()
        .find(|entry| String::from_utf8_lossy(entry.name()).eq_ignore_ascii_case(member))
        .map(|entry| entry.content().to_vec())
        .ok_or_else(|| format!("{container} has no member {member}"))
}

/// The top-level keys of the vehicle record `name` (not its ancestors') whose
/// spelling suggests a sound, gun or cannon field.
///
/// Used to show the player airframe's record chain carries no gun sound
/// field; the keys are read from the document, not listed here.
#[must_use]
pub fn sound_like_keys(vehicle: &ZrdValue, name: &str) -> Vec<String> {
    let Some(ZrdValue::List(body)) = value_of(vehicle, name) else {
        return Vec::new();
    };
    body.chunks(2)
        .filter_map(|pair| pair.first().and_then(ZrdValue::as_text))
        .filter(|key| {
            let key = key.to_ascii_lowercase();
            ["sound", "snd", "gun", "cannon", "fire"]
                .iter()
                .any(|needle| key.contains(needle))
        })
        .map(str::to_owned)
        .collect()
}

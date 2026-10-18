//! Which original sound a gun fires (M01-WINDOW-FIRE-GUN-SOUND, Rally #1546).
//!
//! The binding is a chain of three record reads, never a filename guess:
//!
//! 1. a `BALLISTICS` record of `weapons.zrd` (`ZBD/zrdr.zbd`) names a sound
//!    group in its `LOOPED_SOUND_NAME` field (`wep_50` → `snd_50cal`);
//! 2. `sounds.zrd` maps the group to one archive member and carries its
//!    playback flags (`snd_50cal` → `50cal_gun.wav`, `3D LOOPED SFX`);
//! 3. the engine reads that field for every weapon record it loads (static
//!    evidence, `data/owner-static/engine-facts.toml` is not extended: the
//!    addresses are in `docs/findings/2026-10-18-m01-window-fire-gun-sound.md`).
//!
//! What this does **not** bind is the player's M01 gun to one of those
//! records: the mission's loadout is the project's designed one
//! (`weapons.rs`), and the original loadout the player starts with is not
//! measured. The designed shot-sound id therefore stays refused by name.

use std::fmt;
use std::path::Path;

use cs_content::stunts::{ZrdValue, decode_zrd};

use super::launch_sound::reader_member;

/// The reader archive holding `weapons.zrd` and `sounds.zrd`.
const READER_ARCHIVE: &str = "ZBD/zrdr.zbd";
/// The member holding the ballistics records.
const WEAPONS_MEMBER: &str = "weapons.zrd";
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
fn value_of<'a>(node: &'a ZrdValue, key: &str) -> Option<&'a ZrdValue> {
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

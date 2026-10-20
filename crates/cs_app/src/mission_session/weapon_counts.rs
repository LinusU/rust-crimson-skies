//! The starting counts the original's M01 HUD shows (PARITY2-M01-HUD-VALUES,
//! #1732).
//!
//! The owner's capture of the original at first control reads ROCKETS `BOOM`
//! **3** and GUNS `SLUG` **2000**. Both numbers are in `weapons.zrd`
//! (`ZBD/zrdr.zbd`), in the weapon records the fresh-campaign loadout selects:
//!
//! * the selected gun is `wep_50` (the first slot of the Flight Check's
//!   default record, see [`super::gun_sound`]) and its `AMMO_LIMIT` is 2000;
//! * the rocket kind the default record carries is id 1, whose short-name row
//!   reads `BOOM`; the record of that name in the player rocket block
//!   (`wep_05..=wep_15`, one record per rocket kind) is `wep_06`, and its
//!   `CLUSTER_SIZE` is 3 (`AMMO_LIMIT` is absent from rocket records).
//!
//! That the displayed number *is* `AMMO_LIMIT` / `CLUSTER_SIZE` is an
//! inference from the two equalities, not a measured read of the HUD code;
//! `wep_24` is a second `BOOM` record (`CLUSTER_SIZE` 7) the player's loadout
//! is not shown to use. Nothing here is `verified_original`.

use std::fmt;
use std::path::Path;

use cs_content::stunts::{ZrdValue, decode_zrd};

use super::gun_sound::{READER_ARCHIVE, WEAPONS_MEMBER, value_of};
use super::launch_sound::reader_member;

/// The weapon record the selected gun resolves to.
pub const SELECTED_GUN_RECORD: &str = "wep_50";
/// The weapon record of the default loadout's rocket kind (`BOOM`).
pub const SELECTED_ROCKET_RECORD: &str = "wep_06";

/// The counts the original's HUD shows at first control.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OriginalWeaponCounts {
    /// `AMMO_LIMIT` of [`SELECTED_GUN_RECORD`].
    pub gun_rounds: u64,
    /// `CLUSTER_SIZE` of [`SELECTED_ROCKET_RECORD`].
    pub rocket_stack: u64,
}

/// A count the data does not state.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum WeaponCountsError {
    /// The archive could not be read or decoded.
    Read(String),
    /// A record or its field is absent, or not a positive integer.
    Unmeasured(String),
}

impl fmt::Display for WeaponCountsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "weapon counts unreadable: {reason}"),
            Self::Unmeasured(reason) => write!(f, "weapon counts unmeasured: {reason}"),
        }
    }
}

impl std::error::Error for WeaponCountsError {}

fn positive_int(weapons: &ZrdValue, record: &str, field: &str) -> Result<u64, WeaponCountsError> {
    let unmeasured = |what: String| WeaponCountsError::Unmeasured(what);
    let record_node = value_of(weapons, record)
        .ok_or_else(|| unmeasured(format!("weapons.zrd has no record {record}")))?;
    let value = value_of(record_node, field)
        .ok_or_else(|| unmeasured(format!("{record} declares no {field}")))?;
    first_int(value)
        .map(u64::from)
        .filter(|n| *n > 0)
        .ok_or_else(|| unmeasured(format!("{record} {field} is not a positive integer")))
}

fn first_int(node: &ZrdValue) -> Option<u32> {
    match node {
        ZrdValue::Int(n) => Some(*n),
        ZrdValue::List(children) => children.first().and_then(first_int),
        _ => None,
    }
}

/// Reads the counts from a decoded `weapons.zrd`.
///
/// # Errors
///
/// [`WeaponCountsError::Unmeasured`] when a record or field is absent.
pub fn counts_from_document(weapons: &ZrdValue) -> Result<OriginalWeaponCounts, WeaponCountsError> {
    Ok(OriginalWeaponCounts {
        gun_rounds: positive_int(weapons, SELECTED_GUN_RECORD, "AMMO_LIMIT")?,
        rocket_stack: positive_int(weapons, SELECTED_ROCKET_RECORD, "CLUSTER_SIZE")?,
    })
}

/// Reads the counts from the installation's `weapons.zrd`.
///
/// # Errors
///
/// [`WeaponCountsError`] when the archive is unreadable or a count is absent.
pub fn measure_weapon_counts(
    install_root: &Path,
) -> Result<OriginalWeaponCounts, WeaponCountsError> {
    let bytes = reader_member(install_root, READER_ARCHIVE, WEAPONS_MEMBER)
        .map_err(WeaponCountsError::Read)?;
    let weapons = decode_zrd(&bytes)
        .map_err(|error| WeaponCountsError::Read(format!("{WEAPONS_MEMBER}: {error}")))?;
    counts_from_document(&weapons)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mission_session::cockpit_hud::current_gun_rounds;
    use crate::mission_session::open_mission_weapons_with;
    use crate::ui::hud::{ordnance_rows, weapon_gauge};
    use cs_sim::time::TickRate;
    use cs_types::net::SessionId;

    fn text(s: &str) -> ZrdValue {
        ZrdValue::Text(s.to_owned())
    }

    fn field(name: &str, value: u32) -> [ZrdValue; 2] {
        [text(name), ZrdValue::List(vec![ZrdValue::Int(value)])]
    }

    fn document(gun: u32, rocket: u32) -> ZrdValue {
        let mut gun_record = vec![text(SELECTED_GUN_RECORD)];
        gun_record.push(ZrdValue::List(field("AMMO_LIMIT", gun).to_vec()));
        let mut rocket_record = vec![text(SELECTED_ROCKET_RECORD)];
        rocket_record.push(ZrdValue::List(field("CLUSTER_SIZE", rocket).to_vec()));
        ZrdValue::List(vec![ZrdValue::List(
            gun_record.into_iter().chain(rocket_record).collect(),
        )])
    }

    #[test]
    fn accept_parity2_m01_hud_values_counts_come_from_the_records() {
        let counts = counts_from_document(&document(2000, 3)).expect("both fields present");
        assert_eq!(counts.gun_rounds, 2000);
        assert_eq!(counts.rocket_stack, 3);
        assert!(counts_from_document(&document(0, 3)).is_err());
        assert!(counts_from_document(&ZrdValue::List(Vec::new())).is_err());
    }

    #[test]
    fn accept_parity2_m01_hud_values_the_session_starts_with_the_stated_counts() {
        let session = SessionId::new(7).expect("nonzero");
        let counts = OriginalWeaponCounts {
            gun_rounds: 2000,
            rocket_stack: 3,
        };
        let sessions = open_mission_weapons_with(
            session,
            TickRate::new(60).expect("rate"),
            crate::scene::SceneGeneration(1),
            Some(counts),
        )
        .expect("the session opens");
        let actor = sessions.mission.player();
        let gauge = weapon_gauge(Some(&sessions.weapons), session, actor)
            .expect("same session")
            .expect("registered");
        assert_eq!(current_gun_rounds(&gauge), 2000);
        let rows = ordnance_rows(Some(&sessions.ordnance), session, actor)
            .expect("same session")
            .expect("registered");
        assert_eq!(rows.iter().map(|r| r.remaining_units).sum::<u64>(), 3);
    }

    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_parity2_m01_hud_values_retail_records_state_2000_and_3() {
        let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set");
        let counts = measure_weapon_counts(Path::new(&root)).expect("weapons.zrd states them");
        assert_eq!(counts.gun_rounds, 2000);
        assert_eq!(counts.rocket_stack, 3);
    }
}

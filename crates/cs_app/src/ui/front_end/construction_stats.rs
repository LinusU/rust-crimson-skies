//! The Plane Construction hub's statistics (task #2051): the four rating
//! lines (*TOP SPEED*, *ARMOR*, *AGILITY*, *OFFENSE*), the 20 bar-graph panes
//! they light and the blueprint of the airframe.
//!
//! Static readings of the owner-supplied engine image (`CS_ENGINE_IMAGE`);
//! nothing here is `verified_original`. The numbers below are facts read from
//! the image's tables, with the virtual address of each (the executable and
//! its disassembly are not committed). The script
//! `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` asks `E 2015(25, 0x20, ..)` for
//! the texts and `E 2015(25, 0x40, ..)` for the levels (handler `0x409685`,
//! level function `0x40faf0`, plane slot 25 being the working plane record at
//! `0x64cb78`).
//!
//! # The level function (`0x40faf0`, statistic 1 to 4)
//!
//! Every level is an integer 0 to 4 (a result of 5 or more is stored as 4).
//! With `rec` the plane record and `af` its airframe (`rec + 0x2c`):
//!
//! 1. **Top speed** (`0x40fb1c`): `trunc(SPEED[af] * ENGINE[rec + 0x30] - 1.0)
//!    / 85`; engine 6 (no engine) is level 0.
//! 2. **Armor** (`0x40fb95`): `(ARMOR[af] + rec[0x80] + rec[0x7c] + rec[0x78] +
//!    rec[0x74] - 1) / 73`.
//! 3. **Agility** (`0x40fc08`): `(AGILITY[af] - 1) / 4`, rounding toward zero.
//! 4. **Offense** (`0x40fc66`): `((rec[0x34] + rec[0x38]) * 480 + guns) / 2060`
//!    where `guns` is [`gun_total`] (`0x4055c0`).
//!
//! The text of statistic `k` is string `1018`, `1019`, `1020` or `1132` (each
//! ends in a space), a space (`0x61f428`) and string `501 + level`
//! (*Poor*, *Fair*, *Average*, *Good*, *Excellent*), after the face string
//! `93` (`94` when the call's last argument is non-zero; the script passes 0).
//! The script stores the four texts in the order speed, armor, agility,
//! offense, and lights bar `r` of a group when `r <= level`, so a level of 0
//! still lights one bar. Its bar groups, by statistic, are `PX_P_OS`,
//! `PX_P_SI`, `PX_P_AD`, `PX_P_OC`, and its text rows `PX_T_TOPSPEED`,
//! `PX_T_ARMOR`, `PX_T_AGILITY`, `PX_T_OFFENSE`.
//!
//! # What is *not* known, and so not claimed
//!
//! * **What the record's words mean in the game's terms.** The airframe and
//!   engine indices, the four armor words at `rec + 0x74 ..= 0x80` (the
//!   zones' stored values), the two hardpoint words, the gun kinds and the
//!   pair byte are the engine's fields; which content a draft's choice maps
//!   to is task #2052's feed. [`PlaneRecord`] is what the feed supplies.
//! * **The unit of the armor words** and **the engine multiplier's meaning**
//!   (the six factors are read, their name is not).
//! * **Airframe 11 and above, engine 7 and above, gun kinds above 5**: the
//!   tables have no row, so [`levels`] answers `None`.

/// The number of airframes the tables hold (`PX_0_Blueprint` to
/// `PX_10_Blueprint`).
pub const AIRFRAMES: usize = 11;
/// The engine index that means *no engine* (`cmp eax, 6` at `0x40fb1f`).
pub const NO_ENGINE: u32 = 6;
/// The gun kind that means *empty* (`cmp edx, 5` at `0x4055e1`).
pub const NO_GUN: u32 = 5;
/// The most bars a group lights (`cmp esi, 5` / `mov esi, 4`).
pub const MAX_LEVEL: u8 = 4;

/// The statistics in the engine's order.
pub const STATS: usize = 4;
/// The text rows by statistic.
pub const TEXT_ROWS: [&str; STATS] = [
    "PX_T_TOPSPEED",
    "PX_T_ARMOR",
    "PX_T_AGILITY",
    "PX_T_OFFENSE",
];
/// The bar-graph groups by statistic (the script's `ENA`, `GNA`, `FNA`,
/// `HNA`).
pub const BAR_GROUPS: [&str; STATS] = ["PX_P_OS", "PX_P_SI", "PX_P_AD", "PX_P_OC"];
/// The strings that head each statistic's text.
pub const HEAD_STRINGS: [u32; STATS] = [1018, 1019, 1020, 1132];
/// The first rating string (*Poor*); level `n` is `501 + n`.
pub const RATING_STRING: u32 = 501;
/// The face string of the texts (`93`).
pub const FACE_STRING: u32 = 93;
/// Bars per group.
pub const BARS: usize = 5;

/// Top speed per airframe: the first word of the 12-byte rows at `0x619da0`.
const SPEED: [i32; AIRFRAMES] = [200, 261, 126, 300, 240, 251, 207, 281, 215, 290, 201];
/// The engine factors, `f64` at `0x619e38` (engine 0 to 5).
const ENGINE: [f64; 6] = [0.9, 1.0, 1.1, 1.197, 1.33, 1.463];
/// The third word of the 44-byte airframe rows at `0x619bb8` (`0x619bc0`).
const ARMOR: [i32; AIRFRAMES] = [60, 95, 125, 80, 105, 100, 110, 90, 110, 85, 120];
/// The second word of the airframe rows (`0x619bbc`).
const AGILITY: [i32; AIRFRAMES] = [20, 12, -2, 19, 8, 10, 3, 16, 4, 17, 2];
/// The fifth word of the airframe rows (`0x619bc8`): the gun slots whose
/// bit is set read the second column of [`GUN`].
const GUN_SLOTS: [u8; AIRFRAMES] = [0, 8, 12, 0, 8, 0, 8, 0, 8, 0, 0];
/// The gun rows at `0x619e68`, 28 bytes each: the third and fourth words
/// (`+8`, `+0xc`) by kind 0 to 4.
const GUN: [[i32; 2]; 5] = [[280, 520], [380, 620], [480, 720], [580, 820], [680, 920]];

/// The fields of the working plane record the statistics read.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PlaneRecord {
    /// `rec + 0x2c`: the airframe, 0 to 10.
    pub airframe: u32,
    /// `rec + 0x30`: the engine, 0 to 5, or [`NO_ENGINE`].
    pub engine: u32,
    /// `rec + 0x74`, `0x78`, `0x7c`, `0x80`: the armor zones' words.
    pub armor: [i32; 4],
    /// `rec + 0x34`, `0x38`: the two hardpoint words.
    pub hardpoints: [i32; 2],
    /// `rec + 0x88 + 4 * slot`: the gun kinds, [`NO_GUN`] for an empty slot.
    pub guns: [u32; 4],
    /// `rec + 0x84`: bit `slot` set doubles that slot's gun (a pair).
    pub gun_pairs: u8,
}

/// What a level function could not answer.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatError {
    /// The airframe has no row in the tables.
    Airframe(u32),
    /// The engine has no factor.
    Engine(u32),
    /// A gun kind has no row.
    Gun(u32),
}

impl std::fmt::Display for StatError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Airframe(n) => write!(f, "airframe {n} has no row in the statistics tables"),
            Self::Engine(n) => write!(f, "engine {n} has no factor in the statistics tables"),
            Self::Gun(n) => write!(f, "gun kind {n} has no row in the statistics tables"),
        }
    }
}

impl std::error::Error for StatError {}

fn cap(level: i32) -> u8 {
    // `cmp esi, 5; jl keep; mov esi, 4`. The engine never goes below zero
    // with the tables' values; a negative result is kept at zero rather than
    // guessed at.
    level.clamp(0, i32::from(MAX_LEVEL)) as u8
}

fn airframe_row(record: &PlaneRecord) -> Result<usize, StatError> {
    let index = record.airframe as usize;
    if index < AIRFRAMES {
        Ok(index)
    } else {
        Err(StatError::Airframe(record.airframe))
    }
}

/// The offense's gun term (`0x4055c0` with its slot argument `-1`): the sum
/// over the four slots that hold a gun of the airframe's column for that slot
/// times 1, or 2 for a pair.
///
/// # Errors
///
/// [`StatError`] for an airframe or gun kind the tables do not cover.
pub fn gun_total(record: &PlaneRecord) -> Result<i32, StatError> {
    let af = airframe_row(record)?;
    let mut total = 0;
    for (slot, &kind) in record.guns.iter().enumerate() {
        if kind == NO_GUN {
            continue;
        }
        let row = GUN.get(kind as usize).ok_or(StatError::Gun(kind))?;
        let column = usize::from(GUN_SLOTS[af] & (1 << slot) != 0);
        let multiplier = 1 + i32::from(record.gun_pairs >> slot & 1);
        total += row[column] * multiplier;
    }
    Ok(total)
}

/// The four levels in the engine's order: speed, armor, agility, offense.
///
/// # Errors
///
/// [`StatError`] for an airframe, engine or gun kind the tables do not cover.
pub fn levels(record: &PlaneRecord) -> Result<[u8; STATS], StatError> {
    let af = airframe_row(record)?;
    let speed = if record.engine == NO_ENGINE {
        0
    } else {
        let factor = ENGINE
            .get(record.engine as usize)
            .ok_or(StatError::Engine(record.engine))?;
        // `fild`, `fmul`, `fsub 1.0`, `_ftol` (truncation), then `/ 85`.
        cap((f64::from(SPEED[af]) * factor - 1.0) as i32 / 85)
    };
    let armor = cap((ARMOR[af] + record.armor.iter().sum::<i32>() - 1) / 73);
    let agility = cap((AGILITY[af] - 1) / 4);
    let offense =
        cap(((record.hardpoints[0] + record.hardpoints[1]) * 480 + gun_total(record)?) / 2060);
    Ok([speed, armor, agility, offense])
}

/// The text of one statistic: its head string, a space and the rating
/// string. `head` and `rating` are the installation's strings with any
/// `[FONT]` tag already removed.
#[must_use]
pub fn caption(head: &str, rating: &str) -> String {
    format!("{head} {rating}")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record() -> PlaneRecord {
        PlaneRecord {
            airframe: 1,
            engine: 1,
            armor: [0, 0, 0, 0],
            hardpoints: [0, 0],
            guns: [NO_GUN; 4],
            gun_pairs: 0,
        }
    }

    #[test]
    fn engine_6_is_no_speed_and_levels_cap_at_four() {
        let none = PlaneRecord {
            engine: NO_ENGINE,
            ..record()
        };
        assert_eq!(levels(&none).expect("levels")[0], 0);
        let fast = PlaneRecord {
            airframe: 3,
            engine: 5,
            armor: [500, 500, 500, 500],
            ..record()
        };
        let got = levels(&fast).expect("levels");
        assert_eq!((got[0], got[1]), (4, 4));
    }

    #[test]
    fn a_gun_in_a_slot_the_airframe_marks_reads_the_second_column() {
        let plane = PlaneRecord {
            airframe: 1,
            guns: [NO_GUN, NO_GUN, NO_GUN, 0],
            ..record()
        };
        assert_eq!(gun_total(&plane), Ok(520));
        let pair = PlaneRecord {
            gun_pairs: 0b1000,
            ..plane
        };
        assert_eq!(gun_total(&pair), Ok(1040));
    }
}

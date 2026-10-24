//! The cabin's Change Memento screen: layout script `MomentoSelection`
//! (task #1957, follow-up of #1841).
//!
//! The screen is the layout section as written (background, glass, Accept,
//! Cancel and two arrows) plus the one picture the script builds at run time:
//! `MS_MEMENTO` shows `assets\graphics\scrapbook\<name>` where `<name>` comes
//! from native command 2150 (`MOMENTOSELECTION.SCRIPT`, findings
//! `docs/findings/2026-10-24-cabin-change-memento.md`):
//!
//! * `callback($$E$$, 2150, 0, -1, FX)` on creation: the pilot's current
//!   memento name;
//! * `… 0, 100, FX` (the right arrow) and `… 0, 101, FX` (the left arrow): the
//!   next and the previous name in the engine's table, skipping the ones the
//!   pilot has not unlocked;
//! * `… 1, FX` and `A 12` on Accept: the choice is stored and the cabin comes
//!   back; Cancel starts the cabin without calling 2150, so the choice stays.
//!
//! The table, its order and the skip rule are read from the decrypted engine
//! image (handler `0x40c7f4`, table `0x61af60`, 12-byte records); the test
//! `accept_parity5_cabin_change_memento_table_matches_the_engine_image`
//! re-reads those bytes. Nothing here is `verified_original`.

use std::collections::BTreeMap;

use super::shell::{MenuShell, ShellPane, ShellRect};

/// The layout section this screen is.
pub const MEMENTO_SECTION: &str = "MomentoSelection";
/// The pane the script builds the picture on (`ms_memento`).
pub const MEMENTO_PANE: &str = "MS_MEMENTO";
/// Where the `MS_MEMENTO` row puts the picture (`X,Y`).
pub const PICTURE_POSITION: (i32, i32) = (184, 48);
/// Accept (`ms_b_accept`): stores the shown picture.
pub const ACCEPT_BUTTON: &str = "MS_B_ACCEPT";
/// Cancel (`ms_b_cancel`): keeps the stored picture.
pub const CANCEL_BUTTON: &str = "MS_B_CANCEL";
/// The left arrow (`ms_b_arrowl`): the script's `GX = 101`, the previous one.
pub const ARROW_LEFT_BUTTON: &str = "MS_B_ARROWL";
/// The right arrow (`ms_b_arrowr`): the script's `GX = 100`, the next one.
pub const ARROW_RIGHT_BUTTON: &str = "MS_B_ARROWR";
/// The layout folder the pictures live under, in the engine's own spelling
/// (`Assets\Graphics\Scrapbook` at `0x61f354`).
pub const PICTURE_FOLDER: &str = "SCRAPBOOK/";
/// The memento a new pilot has: the owner's capture of a new pilot's cabin
/// shows this picture (`docs/findings/2026-10-24-pause-map-memento-photo.md`).
pub const DEFAULT_MEMENTO: &str = "MS_P_InitialPinup1.jpg";
/// The claim the table's rows are recorded under.
pub const MEMENTO_TABLE_CLAIM: &str = "OWNER-STATIC-2026-10-24-MEMENTO-TABLE";
/// The table's virtual address in the engine image.
pub const MEMENTO_TABLE_VA: u32 = 0x61_af60;
/// The base of the per-mission records the unlock test reads, at the
/// flags word (`0x64cca4`, record stride 168).
pub const MISSION_FLAGS_VA: u32 = 0x64_cca4;

/// What lets a picture be offered, the table's second and third words.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Gate {
    /// Record `-1`: offered when the pilot's own `MyMemento.jpg` exists under
    /// the installation's scrapbook folder.
    OwnFile,
    /// Record `0`: always offered.
    Always,
    /// Record `n`: offered when mission `n`'s flags word has bit 0 and `bit`
    /// set.
    Mission {
        /// The mission (the engine's record minus one is the index).
        mission: u32,
        /// The bit of the flags word.
        bit: u32,
    },
}

/// One row of the engine's table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MementoEntry {
    /// The picture's file name as the engine spells it.
    pub name: &'static str,
    /// What offers it.
    pub gate: Gate,
}

const fn always(name: &'static str) -> MementoEntry {
    MementoEntry {
        name,
        gate: Gate::Always,
    }
}

const fn after(name: &'static str, mission: u32, bit: u32) -> MementoEntry {
    MementoEntry {
        name,
        gate: Gate::Mission { mission, bit },
    }
}

/// The engine's table, in order (24 rows at `0x61af60`, terminated by a
/// pointer to an empty string).
pub const MEMENTOS: [MementoEntry; 24] = [
    MementoEntry {
        name: "MyMemento.jpg",
        gate: Gate::OwnFile,
    },
    always("MS_P_InitialPinup1.jpg"),
    always("MS_P_InitialPinup3.jpg"),
    always("MS_P_JustineBattleax.jpg"),
    always("MS_P_Mom.jpg"),
    always("MS_P_DoggiePhoto.jpg"),
    always("MS_P_Swan&NathaninCabin.jpg"),
    always("MS_P_ZacharyandPlane.jpg"),
    after("MS_P_02_01_hawaiianPrinces2.jpg", 2, 1),
    after("MS_P_05_01_TreasureGirl.jpg", 5, 1),
    after("MS_P_06_02_BSwanPortrait.jpg", 6, 2),
    after("MS_P_IllsaandSparks.jpg", 7, 0),
    after("MS_P_10_01_SoldierGirl.jpg", 10, 1),
    after("MS_P_11_01_LanaCooperGlam.jpg", 11, 1),
    after("MS_P_12_01_BettysScreentest2.jpg", 12, 1),
    after("MS_P_12_02_Charlie.jpg", 12, 2),
    after("MS_P_13_02_NathanGetsTrophy.jpg", 13, 2),
    after("MS_P_15_01_MyShoe.jpg", 15, 1),
    after("MS_P_16_01_Strip3.jpg", 16, 1),
    after("MS_P_17_04_Shangri-LaPinup.jpg", 17, 4),
    after("MS_P_19_01_SwanTheMorningAfter.jpg", 19, 1),
    after("MS_P_19_01_SwanWelcomeHome1.jpg", 19, 1),
    after("MS_P_24_01_SwanPinup.jpg", 24, 1),
    after("MS_P_24_01_theFinalKiss.jpg", 24, 1),
];

/// The layout art name of a picture: the scrapbook folder and the name.
#[must_use]
pub fn picture_art(name: &str) -> String {
    format!("{PICTURE_FOLDER}{name}")
}

/// The table index of `name`, compared case-insensitively. A name the table
/// does not hold gives `1`, as the engine's lookup (`0x410270`) does.
#[must_use]
pub fn index_of(name: &str) -> usize {
    MEMENTOS
        .iter()
        .position(|entry| entry.name.eq_ignore_ascii_case(name))
        .unwrap_or(1)
}

/// What a pilot has unlocked: the flags word of each mission's record
/// (`0x64cca4 + 168 * (mission - 1)`) and whether `MyMemento.jpg` exists.
///
/// What sets those bits when a mission is flown is not read from the engine,
/// so the start flow, which keeps no per-pilot progress, uses
/// [`Self::new_pilot`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MementoUnlocks {
    flags: BTreeMap<u32, u32>,
    own_file: bool,
}

impl MementoUnlocks {
    /// A new pilot: no mission flags, no `MyMemento.jpg`.
    #[must_use]
    pub fn new_pilot() -> Self {
        Self::default()
    }

    /// The same with mission `mission`'s flags word set.
    #[must_use]
    pub fn with_mission_flags(mut self, mission: u32, flags: u32) -> Self {
        self.flags.insert(mission, flags);
        self
    }

    /// The same with the pilot's own `MyMemento.jpg` present.
    #[must_use]
    pub fn with_own_file(mut self) -> Self {
        self.own_file = true;
        self
    }

    /// Whether the engine offers `entry`.
    #[must_use]
    pub fn offers(&self, entry: &MementoEntry) -> bool {
        match entry.gate {
            Gate::Always => true,
            Gate::OwnFile => self.own_file,
            Gate::Mission { mission, bit } => {
                let flags = self.flags.get(&mission).copied().unwrap_or(0);
                flags & 1 != 0 && bit < 32 && flags & (1 << bit) != 0
            }
        }
    }
}

/// Which arrow was pressed.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    /// The right arrow, `GX = 100`.
    Next,
    /// The left arrow, `GX = 101`.
    Previous,
}

/// The index the engine's 2150 returns after `current` for `direction`
/// (`0x40c7f4`): one step, wrapping at the table's end, repeated until a row
/// is offered. Stepping back from the first row goes to the last. When no row
/// is offered `current` stays.
#[must_use]
pub fn step(current: usize, direction: Direction, unlocks: &MementoUnlocks) -> usize {
    let count = MEMENTOS.len();
    let mut at = current.min(count - 1);
    for _ in 0..count {
        at = match direction {
            Direction::Next => (at + 1) % count,
            Direction::Previous => at.checked_sub(1).unwrap_or(count - 1),
        };
        if unlocks.offers(&MEMENTOS[at]) {
            return at;
        }
    }
    current
}

/// The screen while it is open.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MementoScreen {
    shown: usize,
}

impl MementoScreen {
    /// Opens on the pilot's current memento (`2150, 0, -1`).
    #[must_use]
    pub fn open(current: usize) -> Self {
        Self {
            shown: current.min(MEMENTOS.len() - 1),
        }
    }

    /// The table index shown.
    #[must_use]
    pub fn shown(&self) -> usize {
        self.shown
    }

    /// The name shown.
    #[must_use]
    pub fn name(&self) -> &'static str {
        MEMENTOS[self.shown].name
    }

    /// An arrow: the next or previous offered picture.
    pub fn arrow(&mut self, direction: Direction, unlocks: &MementoUnlocks) {
        self.shown = step(self.shown, direction, unlocks);
    }
}

/// Puts the shown picture on the `MS_MEMENTO` pane of the shell's section,
/// below the layout's own panes (its row has `Z` 1, under the background and
/// the glass). Returns whether the pane changed, `false` also when the
/// section or the picture's extent is missing.
pub fn show_picture(
    shell: &mut MenuShell,
    name: &str,
    extents: &BTreeMap<String, (u32, u32)>,
) -> bool {
    let art = picture_art(name);
    let Some(&(width, height)) = extents.get(&art.to_ascii_lowercase()) else {
        return false;
    };
    let Some(section) = shell.section_mut(MEMENTO_SECTION) else {
        return false;
    };
    let rect = ShellRect {
        x: PICTURE_POSITION.0,
        y: PICTURE_POSITION.1,
        width,
        height,
    };
    if let Some(pane) = section
        .panes
        .iter_mut()
        .find(|pane| pane.id.eq_ignore_ascii_case(MEMENTO_PANE))
    {
        if pane.art == art {
            return false;
        }
        pane.art = art;
        pane.rect = rect;
        return true;
    }
    section.panes.insert(
        0,
        ShellPane {
            id: MEMENTO_PANE.to_owned(),
            art,
            rect,
            frames: 1,
            frame: 0,
            is_region: false,
            color_keyed: false,
            transform: None,
        },
    );
    true
}

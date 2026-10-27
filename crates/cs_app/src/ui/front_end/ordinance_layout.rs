//! The Instant Action loadout screen, `OrdinanceLayout` (task #2062, follow-up
//! to #2045): the `[@OrdinanceLayout@]` section of `ASSETS/LAYOUT.CSV` with
//! `ASSETS/SCRIPTS/ORDINANCELAYOUT.SCRIPT` run over it, as the model a
//! presenter draws and a headless drive reads.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Script.** `gui_create` reads `@globals@XQ` (`DKA`, then clears it) and
//!   `@globals@ZQ` (`-1` the pilot's plane, `-2` the wingmate's; the flight
//!   check sets it, see `super::flow`), calls `E 2014(ZQ, -3, DKA)` and
//!   `E 2035(ZQ, DKA)`, then builds the background pane, the accept and
//!   cancel buttons, seven titles, the plane-info text, two plane diagrams,
//!   **four** gun-name texts (`OL_T_GUNNAME0..3`), **four** ammunition
//!   drop-downs (`OL_D_AMMO0..3`, list type 5, tagged `2027`), **eight**
//!   rocket drop-downs (`OL_D_ROCKETS0..7`, list type 12, tagged `2028`) and
//!   two scrolling descriptions (`OL_S_AMMODESC`, `OL_S_ROCKETDESC`).
//! * **Init.** For each gun position `E 2030(ZQ, slot, nameText, kind)`
//!   fills the name and answers a kind; kind `5` is read as *no gun*: the name
//!   is greyed (`0xff808080`) and the ammunition box deactivated. Otherwise
//!   the box's choice is the `kind` the call returned in its last argument.
//!   For each rocket box `E 2031(ZQ, slot, nameText)` answers a list index or
//!   `-1`; `-1` deactivates the box (choice 0), anything else is its choice.
//!   The ammunition description starts on the first active gun's choice and
//!   the rocket description on the first active rocket's, `4` for both when
//!   none is (`E 2032` / `E 2033` fill the description).
//! * **Messages.** `10015` (an entry chosen) stores it: `E 2036(ZQ, slot,
//!   choice)` for an ammunition box, `E 2037(ZQ, slot, choice)` for a rocket
//!   box, and shows its description; `10013` (rolled over) shows the rolled
//!   entry's description; `10001` (a box's init) shows its held choice's.
//!   The accept button (`10003`) calls `E 2034(ZQ, DKA)` and, when `DKA` is
//!   0, `callback($$A$$, 12, 0)`; the cancel button acts only when `DKA` is
//!   non-zero and then continues `@instantaction@`.
//! * **Engine.** Dispatcher `0x4093a0`, jump table `0x40f518`. `E 2027`
//!   (`0x409a5c`, the ammunition list text): `-1` asks the row count and
//!   answers 5, otherwise string `3360 + index`. `E 2028` (`0x409b72`, the
//!   rocket list text): `-1` answers the count of entries a table of twelve
//!   eight-byte records at `0x619efc` lets through (`0x40ff00`), otherwise
//!   the string `3395 + record` of the `n`-th entry that passes (`0x40ff50`).
//!   `E 2030` (`0x4099f2`) names the gun by string `3320 + kind`. `E 2031`
//!   (`0x409aec`) names the rocket by string `3395 + value` and answers `-1`
//!   for a box a plane lacks: boxes 0 to 3 against the count at `+0x34` of
//!   the plane's record, boxes 4 to 7 against `+0x38 + 4`.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The entries.** Which record of the plane a position reads, which
//!   twelve rockets pass the table `0x619efc` (it compares campaign state at
//!   `0x647b5c`, `0x647b68`, `0x647b6c`, `0x64b678`), and the ammunition
//!   kinds behind the five rows: the caller hands the panel its entries
//!   ([`OrdinanceFeed`]). Resolving feed: the Instant Action draft.
//! * **Which content id is an entry**, and the draft the stores `E 2036` /
//!   `E 2037` / `E 2034` write: the press is reported
//!   ([`OrdinancePress`]), nothing is applied here.
//! * **The description texts** (`E 2032`, `E 2033`; strings from `3350` and
//!   `3370`/`3380` with formats at `0x61f3d0`/`0x61f3d4`), **the plane-info
//!   text** (`E 2011`), **the diagrams' art choice** (`E 2010`) and the
//!   look of the drop-downs: not drawn.
//! * **What `XQ` means** beyond zero or non-zero.

use std::fmt;

use super::construction_slots::{SlotError, SlotText, rect_row, text_row};
use super::original::{OriginalLayout, Row};
use super::shell::ShellRect;
use super::widgets::{DropdownRow, dropdown_rows};

/// The layout section.
pub const SECTION: &str = "OrdinanceLayout";
/// Gun positions (`BA < 4`).
pub const AMMO_BOXES: usize = 4;
/// Rocket boxes (`BA < 8`).
pub const ROCKET_BOXES: usize = 8;
/// A gun kind the script reads as *no gun*.
pub const NO_GUN_KIND: i32 = 5;
/// The description both boxes start on when none is active.
pub const FALLBACK_DESCRIPTION: i32 = 4;
/// The colour a missing gun's name is greyed to.
pub const GREYED: u32 = 0xff80_8080;

/// Whose plane the screen edits (`@globals@ZQ`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinanceSide {
    /// `-1`.
    Pilot,
    /// `-2`.
    Wingmate,
}

impl OrdinanceSide {
    /// The script's `ZQ`.
    #[must_use]
    pub const fn zq(self) -> i32 {
        match self {
            Self::Pilot => -1,
            Self::Wingmate => -2,
        }
    }
}

/// Which list a position belongs to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum OrdinanceList {
    /// `OL_D_AMMO0..3`, tag `2027`.
    Ammo,
    /// `OL_D_ROCKETS0..7`, tag `2028`.
    Rocket,
}

/// A gun position as the feed gives it (`E 2030`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GunPosition {
    /// The kind the call answered; [`NO_GUN_KIND`] is no gun.
    pub kind: i32,
    /// The name line.
    pub name: String,
    /// The ammunition row it holds.
    pub choice: i32,
}

/// What the engine's natives hand the screen at `gui_init`.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OrdinanceFeed {
    /// The plane line (`E 2011`), if known.
    pub plane_info: Option<String>,
    /// The four gun positions.
    pub guns: Vec<GunPosition>,
    /// The ammunition rows (five in the layout).
    pub ammo_rows: Vec<String>,
    /// The rocket rows (`E 2028`; twelve in the layout).
    pub rocket_rows: Vec<String>,
    /// Each rocket box's held row, `None` where `E 2031` answered `-1`.
    pub rockets: Vec<Option<usize>>,
}

/// A button of the screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OrdinanceButton {
    /// The row id, upper-cased.
    pub id: String,
    /// The art file name.
    pub art: String,
    /// The top-left corner.
    pub x: i32,
    /// The top-left corner.
    pub y: i32,
}

/// What a press did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum OrdinancePress {
    /// Not the screen's.
    Nothing,
    /// A list opened or closed without a choice.
    Toggled,
    /// An ammunition entry was chosen (`E 2036`).
    Ammo {
        /// The gun position.
        slot: usize,
        /// The chosen row.
        choice: usize,
    },
    /// A rocket entry was chosen (`E 2037`).
    Rocket {
        /// The rocket box.
        slot: usize,
        /// The chosen row.
        choice: usize,
    },
    /// Accept (`E 2034`); `leave` is the script's `!DKA` (`callback A 12`).
    Accept {
        /// Whether the script also makes the `A 12` call.
        leave: bool,
    },
    /// Cancel, which the script honours only when `XQ` was non-zero.
    Cancel,
}

/// The screen.
#[derive(Clone, Debug)]
pub struct OrdinanceLayoutScreen {
    side: OrdinanceSide,
    entered: i32,
    texts: Vec<SlotText>,
    plane_info: SlotText,
    gun_names: Vec<SlotText>,
    ammo_boxes: Vec<DropdownRow>,
    rocket_boxes: Vec<DropdownRow>,
    ammo_desc: ShellRect,
    rocket_desc: ShellRect,
    accept: OrdinanceButton,
    cancel: OrdinanceButton,
    feed: OrdinanceFeed,
    ammo_choice: [i32; AMMO_BOXES],
    rocket_choice: [Option<usize>; ROCKET_BOXES],
    open: Option<(OrdinanceList, usize)>,
    rolled: Option<usize>,
    ammo_desc_entry: i32,
    rocket_desc_entry: i32,
    dirty: bool,
}

impl OrdinanceLayoutScreen {
    /// Reads the section's rows. `entered` is the script's `XQ`; `name`
    /// answers an `IDS_*` name with the installation's text.
    ///
    /// # Errors
    ///
    /// [`SlotError`] for a missing section or row, an unreadable column, or a
    /// list whose row count is not the script's (5 and 12).
    pub fn from_layout(
        layout: &OriginalLayout,
        side: OrdinanceSide,
        entered: i32,
        name: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, SlotError> {
        let section = layout
            .section(SECTION)
            .ok_or(SlotError::NoSection(SECTION))?;
        let mut texts = Vec::new();
        for id in [
            "OL_T_TITLE",
            "OL_T_AMMOTITLE",
            "OL_T_AMMOCAPTION",
            "OL_T_ROCKETTITLE",
            "OL_T_ROCKETCAPTION",
            "OL_T_AMMODESCTITLE",
            "OL_T_ROCKETDESCTITLE",
        ] {
            texts.push(text_row(section, id, Some(name))?);
        }
        let plane_info = text_row(section, "OL_T_PLANEINFO", None)?;
        let mut gun_names = Vec::new();
        for slot in 0..AMMO_BOXES {
            gun_names.push(text_row(section, &format!("OL_T_GUNNAME{slot}"), None)?);
        }
        let rows = dropdown_rows(layout, SECTION).map_err(SlotError::Dropdown)?;
        let pick = |id: String| {
            rows.iter()
                .find(|row| row.id.eq_ignore_ascii_case(&id))
                .cloned()
                .ok_or(SlotError::RowMissing(id))
        };
        let ammo_boxes = (0..AMMO_BOXES)
            .map(|slot| pick(format!("OL_D_AMMO{slot}")))
            .collect::<Result<Vec<_>, _>>()?;
        let rocket_boxes = (0..ROCKET_BOXES)
            .map(|slot| pick(format!("OL_D_ROCKETS{slot}")))
            .collect::<Result<Vec<_>, _>>()?;
        let button = |id: &str| {
            let row: &Row = section
                .buttons
                .iter()
                .find(|row| row.id.eq_ignore_ascii_case(id))
                .ok_or_else(|| SlotError::RowMissing(id.to_owned()))?;
            let at = |index: usize| {
                row.fields
                    .get(index)
                    .and_then(|field| field.trim().parse::<i32>().ok())
                    .ok_or_else(|| SlotError::Column { row: id.to_owned() })
            };
            Ok(OrdinanceButton {
                id: row.id.clone(),
                art: row.fields.first().cloned().unwrap_or_default(),
                x: at(1)?,
                y: at(2)?,
            })
        };
        Ok(Self {
            side,
            entered,
            texts,
            plane_info,
            gun_names,
            ammo_boxes,
            rocket_boxes,
            ammo_desc: rect_row(section, "OL_S_AMMODESC")?,
            rocket_desc: rect_row(section, "OL_S_ROCKETDESC")?,
            accept: button("OL_B_ACCEPT")?,
            cancel: button("OL_B_CANCEL")?,
            feed: OrdinanceFeed::default(),
            ammo_choice: [0; AMMO_BOXES],
            rocket_choice: [None; ROCKET_BOXES],
            open: None,
            rolled: None,
            ammo_desc_entry: FALLBACK_DESCRIPTION,
            rocket_desc_entry: FALLBACK_DESCRIPTION,
            dirty: true,
        })
    }

    /// Whose plane.
    #[must_use]
    pub const fn side(&self) -> OrdinanceSide {
        self.side
    }

    /// The script's `XQ`.
    #[must_use]
    pub const fn entered(&self) -> i32 {
        self.entered
    }

    /// Runs `gui_init` over the feed: names, greying, deactivation, choices
    /// and the descriptions' starting entries.
    pub fn init(&mut self, feed: OrdinanceFeed) {
        let mut first_ammo = None;
        for slot in 0..AMMO_BOXES {
            if let Some(gun) = feed.guns.get(slot) {
                self.ammo_choice[slot] = gun.choice;
                if gun.kind != NO_GUN_KIND && first_ammo.is_none() {
                    first_ammo = Some(gun.choice);
                }
            }
        }
        let mut first_rocket = None;
        for slot in 0..ROCKET_BOXES {
            let held = feed.rockets.get(slot).copied().flatten();
            self.rocket_choice[slot] = held;
            if held.is_some() && first_rocket.is_none() {
                first_rocket = held;
            }
        }
        self.ammo_desc_entry = first_ammo.unwrap_or(FALLBACK_DESCRIPTION);
        self.rocket_desc_entry = first_rocket.map_or(FALLBACK_DESCRIPTION, |c| c as i32);
        self.feed = feed;
        self.open = None;
        self.rolled = None;
        self.dirty = true;
    }

    /// Whether a gun position is live: the feed named a gun for it.
    #[must_use]
    pub fn ammo_active(&self, slot: usize) -> bool {
        self.feed
            .guns
            .get(slot)
            .is_some_and(|gun| gun.kind != NO_GUN_KIND)
    }

    /// Whether a rocket box is live (`E 2031` did not answer `-1`).
    #[must_use]
    pub fn rocket_active(&self, slot: usize) -> bool {
        self.rocket_choice.get(slot).copied().flatten().is_some()
    }

    /// The closed ammunition box.
    #[must_use]
    pub fn ammo_box(&self, slot: usize) -> Option<&DropdownRow> {
        self.ammo_boxes.get(slot)
    }

    /// The closed rocket box.
    #[must_use]
    pub fn rocket_box(&self, slot: usize) -> Option<&DropdownRow> {
        self.rocket_boxes.get(slot)
    }

    /// The ammunition description box (nothing is drawn in it).
    #[must_use]
    pub const fn ammo_description_rect(&self) -> ShellRect {
        self.ammo_desc
    }

    /// The rocket description box (nothing is drawn in it).
    #[must_use]
    pub const fn rocket_description_rect(&self) -> ShellRect {
        self.rocket_desc
    }

    /// The accept button.
    #[must_use]
    pub const fn accept_button(&self) -> &OrdinanceButton {
        &self.accept
    }

    /// The cancel button.
    #[must_use]
    pub const fn cancel_button(&self) -> &OrdinanceButton {
        &self.cancel
    }

    /// The entry the ammunition description shows.
    #[must_use]
    pub const fn ammo_description_entry(&self) -> i32 {
        self.ammo_desc_entry
    }

    /// The entry the rocket description shows.
    #[must_use]
    pub const fn rocket_description_entry(&self) -> i32 {
        self.rocket_desc_entry
    }

    /// The held ammunition row of a gun position.
    #[must_use]
    pub fn ammo_choice(&self, slot: usize) -> Option<i32> {
        self.ammo_choice.get(slot).copied()
    }

    /// The held row of a rocket box; `None` for an inactive box.
    #[must_use]
    pub fn rocket_choice(&self, slot: usize) -> Option<usize> {
        self.rocket_choice.get(slot).copied().flatten()
    }

    /// The open list.
    #[must_use]
    pub const fn open_list(&self) -> Option<(OrdinanceList, usize)> {
        self.open
    }

    /// The row the pointer is over in the open list.
    #[must_use]
    pub const fn rolled(&self) -> Option<usize> {
        self.rolled
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The rows of a list, as the feed gave them.
    #[must_use]
    pub fn row_texts(&self, list: OrdinanceList) -> &[String] {
        match list {
            OrdinanceList::Ammo => &self.feed.ammo_rows,
            OrdinanceList::Rocket => &self.feed.rocket_rows,
        }
    }

    fn box_of(&self, list: OrdinanceList, slot: usize) -> Option<&DropdownRow> {
        match list {
            OrdinanceList::Ammo => self.ammo_boxes.get(slot),
            OrdinanceList::Rocket => self.rocket_boxes.get(slot),
        }
    }

    /// The rect of an open list's `index`-th row: below the box.
    #[must_use]
    pub fn item_rect(&self, list: OrdinanceList, slot: usize, index: usize) -> Option<ShellRect> {
        let b = self.box_of(list, slot)?.rect;
        Some(ShellRect {
            x: b.x,
            y: b.y + ((index as u32 + 1) * b.height) as i32,
            width: b.width,
            height: b.height,
        })
    }

    fn row_at(&self, list: OrdinanceList, slot: usize, x: i32, y: i32) -> Option<usize> {
        (0..self
            .row_texts(list)
            .len()
            .min(self.displayed_rows(list, slot)))
            .find(|index| {
                self.item_rect(list, slot, *index)
                    .is_some_and(|r| r.contains(x, y))
            })
    }

    /// How many rows a box's list shows when open.
    #[must_use]
    pub fn displayed_rows(&self, list: OrdinanceList, slot: usize) -> usize {
        self.box_of(list, slot).map_or(0, |b| b.displayed as usize)
    }

    /// The pointer moved: while a list is open the row under it is rolled over
    /// (`10013`), and its description shown.
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let rolled = self.open.and_then(|(list, slot)| {
            let (x, y) = point?;
            self.row_at(list, slot, x, y)
        });
        if rolled != self.rolled {
            self.rolled = rolled;
            if let (Some((list, _)), Some(row)) = (self.open, rolled) {
                match list {
                    OrdinanceList::Ammo => self.ammo_desc_entry = row as i32,
                    OrdinanceList::Rocket => self.rocket_desc_entry = row as i32,
                }
            }
            self.dirty = true;
        }
    }

    /// A press at a logical point.
    pub fn press_at(&mut self, x: i32, y: i32) -> OrdinancePress {
        if let Some((list, slot)) = self.open {
            let picked = self.row_at(list, slot, x, y);
            let on_box = self
                .box_of(list, slot)
                .is_some_and(|b| b.rect.contains(x, y));
            self.open = None;
            self.rolled = None;
            self.dirty = true;
            return match picked {
                Some(row) => self.choose(list, slot, row),
                None if on_box => OrdinancePress::Toggled,
                None => OrdinancePress::Nothing,
            };
        }
        for slot in 0..AMMO_BOXES {
            if self.ammo_active(slot) && self.ammo_boxes[slot].rect.contains(x, y) {
                return self.open_box(OrdinanceList::Ammo, slot);
            }
        }
        for slot in 0..ROCKET_BOXES {
            if self.rocket_active(slot) && self.rocket_boxes[slot].rect.contains(x, y) {
                return self.open_box(OrdinanceList::Rocket, slot);
            }
        }
        OrdinancePress::Nothing
    }

    /// A press on a button, by the logical point and the art's size.
    pub fn press_button(&self, id: &str) -> OrdinancePress {
        if id.eq_ignore_ascii_case(&self.accept.id) {
            OrdinancePress::Accept {
                leave: self.entered == 0,
            }
        } else if id.eq_ignore_ascii_case(&self.cancel.id) && self.entered != 0 {
            OrdinancePress::Cancel
        } else {
            OrdinancePress::Nothing
        }
    }

    fn open_box(&mut self, list: OrdinanceList, slot: usize) -> OrdinancePress {
        self.open = Some((list, slot));
        self.dirty = true;
        OrdinancePress::Toggled
    }

    fn choose(&mut self, list: OrdinanceList, slot: usize, row: usize) -> OrdinancePress {
        match list {
            OrdinanceList::Ammo => {
                self.ammo_choice[slot] = row as i32;
                self.ammo_desc_entry = row as i32;
                OrdinancePress::Ammo { slot, choice: row }
            }
            OrdinanceList::Rocket => {
                self.rocket_choice[slot] = Some(row);
                self.rocket_desc_entry = row as i32;
                OrdinancePress::Rocket { slot, choice: row }
            }
        }
    }

    /// Closes the open list; whether one was open.
    pub fn close_list(&mut self) -> bool {
        let was = self.open.take().is_some();
        if was {
            self.rolled = None;
            self.dirty = true;
        }
        was
    }

    /// The texts to draw: the seven titles, the plane line when known, and a
    /// name line per gun position (greyed for no gun).
    #[must_use]
    pub fn texts(&self) -> Vec<SlotText> {
        let mut out = self.texts.clone();
        if let Some(info) = &self.feed.plane_info {
            out.push(SlotText {
                text: info.clone(),
                ..self.plane_info.clone()
            });
        }
        for (slot, line) in self.gun_names.iter().enumerate() {
            if let Some(gun) = self.feed.guns.get(slot) {
                out.push(SlotText {
                    text: gun.name.clone(),
                    color: if gun.kind == NO_GUN_KIND {
                        GREYED
                    } else {
                        line.color
                    },
                    ..line.clone()
                });
            }
        }
        out
    }

    /// The text of a held row, for the closed box.
    #[must_use]
    pub fn box_text(&self, list: OrdinanceList, slot: usize) -> Option<&str> {
        let row = match list {
            OrdinanceList::Ammo => usize::try_from(*self.ammo_choice.get(slot)?).ok()?,
            OrdinanceList::Rocket => self.rocket_choice(slot)?,
        };
        self.row_texts(list).get(row).map(String::as_str)
    }
}

impl fmt::Display for OrdinanceSide {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Pilot => "pilot",
            Self::Wingmate => "wingmate",
        })
    }
}

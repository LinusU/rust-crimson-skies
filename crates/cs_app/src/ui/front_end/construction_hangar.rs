//! The Plane Construction hub's Sell hangar and its four saved-plane picture
//! slots (task #2046, part of #2036): the `[@Hangar@]` section of
//! `ASSETS/LAYOUT.CSV` with the screen script `ASSETS/SCRIPTS/HANGAR.SCRIPT`
//! runs over it, as the model a presenter draws and a headless drive reads,
//! and the hub's `PX_P_PLANE0..3` picture panes.
//!
//! Nothing here renders. The page owns the saved-plane list, the selection and
//! the Sell rules; the planes themselves come from the caller ([`SavedPlane`]),
//! since the start flow keeps no per-pilot plane store yet.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! Engine image `CS_ENGINE_IMAGE`, dispatcher `0x4093a0`.
//!
//! * **Saved planes.** The engine keeps 25 records of 204 bytes (`0xcc`,
//!   `0x64b78c..0x64cb78`); a record whose first word is 0 is an empty slot,
//!   the record's name is at `+4` and its airframe at `+0x2c`. The working
//!   plane's record is the one after them (`0x64cb78`, slot 25). The hangar's
//!   drop-down shows 25 rows (`HA_D_PILOTPLANE`, `TotalDisplayed`).
//! * **`E 2018`** (handler `0x4097ea`) counts the occupied records
//!   (`0x40fd10`) and takes one off while the engine state `[0x64b340]` is 13
//!   or 17 (the construction screen, which keeps its plane in a record).
//!   `PLANECONSTRUCTION.SCRIPT` deactivates Sell below 3 and `HANGAR.SCRIPT`
//!   refuses to sell unless `2 < count`.
//! * **`E 2013(-1, i)`** (handler `0x4094eb`) selects list row `i`: the engine
//!   stores `slot_for_index(i)` (`0x4101d0`) in `[0x64b67c]`. **`E 2012`**
//!   (`0x4094ce`) answers `index_for_slot` (`0x410220`) of the selected slot.
//!   Both skip empty records; while constructing, the walk of `0x4101d0` stops
//!   at the selected record and `0x410220` does not count it. The mapping is
//!   transcribed as read ([`HangarPage::slot_for_index`]).
//! * **Sell** (`HA_B_SELLP`) runs `E 2018`; with a count of 3 or more it asks
//!   `E 2103` (handler `0x40a1c1`) for the box text: a record whose first word
//!   is 2 answers 0 with string 704 over the airframe's short name (string
//!   `3020 + airframe`) and the plane's name, and the script opens an OK-only
//!   box; any other record answers 1 with string 700 over the short name and
//!   the plane's value, and the script opens a yes/no box (type 4). An answer
//!   of 3 runs `E 2109` (handler `0x40a039`): the value (`0x405680`, rounded)
//!   is added to the cash `[0x64b788]` and the record's first word is cleared;
//!   then `E 2013(-1, 0)` selects row 0.
//! * **Done** (`HA_B_DONE`) runs `A 12`, selects row 0 for both players
//!   (`E 2013(-1, 0)`, `E 2013(-2, 1)`), ends the hangar and continues
//!   `planeconstruction.script`.
//! * **Texts.** `HA_T_PLANE` is `E 2011` (handler `0x409443`): string 207 over
//!   the plane's name and the airframe string `3000 + airframe`;
//!   `HA_T_VALUEP` is `E 2135` (handler `0x40a167`): string 1258 over the
//!   value.
//! * **The four picture slots.** `PLANECONSTRUCTION.SCRIPT` creates
//!   `px_p_plane0..3` and asks `E 2243(R, art, 1)` (handler `0x40d54d`) for
//!   each one's picture: the format at `0x61f370`,
//!   `Assets\Graphics\PX_Icon_%1!d!_%2!d!_%3!d!.tga`, over the airframe
//!   `[0x64cba4]`, the paint `[0x64cbb8]` (13 reads as 11) and `R`. With the
//!   last argument 0 it names `Assets\Graphics\PX_%1!d!_Blueprint.tga`
//!   (`0x61f3a0`) over the airframe. The four panes share one place and are
//!   drawn in the order of their `Z` column.
//!
//! # What is *not* known, and so not claimed
//!
//! * **Export** (`HA_B_EXPORTP`): `A 22`, `E 2105` (string 702 over the short
//!   name of a plane) and a message box. What `A 22` does is unread: the page
//!   answers [`HangarOutcome::Export`] with the box text and exports nothing.
//! * **The too-few notice** (`callback($$NB$$, 701, ..)`) is unread:
//!   [`HangarOutcome::TooFew`] carries no text.
//! * **The list rows' text** (the drop-down's callback) and **the statistics**
//!   (`E 2015`, `E 2019`, the plane-weapons text) are unread: no row label and
//!   no statistic is claimed.
//! * **What a record's first word means** besides 0 (empty) and 2 (not sold,
//!   string 704): unread.
//! * **The value** (`0x405680`) is the caller's figure.
//! * **The unit and the formatting** of the figures in strings 700 and 1258
//!   (`%d` is assumed).

use std::fmt;

use super::construction_hub::TabCaption;
use super::construction_slots::{SlotError, SlotText, text_row};
use super::original::{OriginalLayout, Section};
use super::previous_missions::split_tag;
use super::shell::ShellRect;
use super::start_flow::NotImplemented;
use super::widgets::{DropdownRow, WidgetError, dropdown_rows};

/// The layout section of the hangar.
pub const HANGAR_SECTION: &str = "Hangar";
/// The saved-plane records the engine keeps (`0x64b78c..0x64cb78` over `0xcc`).
pub const SAVED_SLOTS: usize = 25;
/// The record of the plane being built, one past the saved ones.
pub const WORKING_SLOT: usize = SAVED_SLOTS;
pub use super::construction_hub::SELL_MIN_PLANES;
/// The message box's answer for yes (`@globals@OR.VR == 3`).
pub const YES_ANSWER: u32 = 3;
/// A record whose first word is this is not sold (string 704 explains).
pub const KIND_NOT_SOLD: u32 = 2;

/// What Export stands on: the export itself (`A 22`) is unread, so the box
/// that announces it is the marked notice (task #2063).
pub const EXPORT_NOT_BUILT: NotImplemented = NotImplemented {
    section: HANGAR_SECTION,
    button: "HA_B_EXPORTP",
    what: "exporting a plane (A 22 unread)",
};

/// What Sell with too few saved planes stands on: the box text (`NB 701`) is
/// unread (task #2063).
pub const TOO_FEW_NOT_BUILT: NotImplemented = NotImplemented {
    section: HANGAR_SECTION,
    button: "HA_B_SELLP",
    what: "the too-few-planes box (NB 701 unread)",
};

/// `Done`, `Sell` and `Export` buttons.
pub const DONE_BUTTON: &str = "HA_B_DONE";
/// The sell button.
pub const SELL_BUTTON: &str = "HA_B_SELLP";
/// The export button.
pub const EXPORT_BUTTON: &str = "HA_B_EXPORTP";

const LIST_ROW: &str = "HA_D_PILOTPLANE";
const TITLE_ROW: &str = "HA_T_TITLE";
const PROMPT_ROW: &str = "HA_T_PROMPT";
const PLANE_ROW: &str = "HA_T_PLANE";
const VALUE_ROW: &str = "HA_T_VALUEP";

/// String: the plane-name line (`E 2011`).
pub const NAME_LINE_STRING: u32 = 207;
/// String: sell this plane? (`E 2103`, a plane that can be sold).
pub const SELL_PROMPT_STRING: u32 = 700;
/// String: the export box (`E 2105`).
pub const EXPORT_STRING: u32 = 702;
/// String: this plane cannot be sold (`E 2103`, first word 2).
pub const NOT_SOLD_STRING: u32 = 704;
/// String: the value line (`E 2135`).
pub const VALUE_LINE_STRING: u32 = 1258;
/// Strings `3000 + airframe` are the airframes' names.
pub const AIRFRAME_NAME_BASE: u32 = 3000;
/// Strings `3020 + airframe` are the airframes' short names.
pub const AIRFRAME_SHORT_BASE: u32 = 3020;

/// Why a page could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HangarError {
    /// The layout has no `[@Hangar@]` section.
    NoSection,
    /// A row the page needs is missing.
    RowMissing(String),
    /// A text row could not be read.
    Text(SlotError),
    /// The drop-down row could not be read.
    List(WidgetError),
}

impl fmt::Display for HangarError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no [@{HANGAR_SECTION}@] section"),
            Self::RowMissing(row) => write!(f, "the Hangar section has no {row} row"),
            Self::Text(error) => write!(f, "the Hangar section's text: {error}"),
            Self::List(error) => write!(f, "the Hangar section's list: {error}"),
        }
    }
}

impl std::error::Error for HangarError {}

/// One saved plane, as the caller hands it in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SavedPlane {
    /// The record's first word: 0 is an empty slot (hold `None` instead),
    /// [`KIND_NOT_SOLD`] a plane that is not sold.
    pub kind: u32,
    /// The plane's name (record `+4`).
    pub name: String,
    /// The airframe index (record `+0x2c`).
    pub airframe: u32,
    /// The plane's value (`0x405680`).
    pub value: u64,
}

/// What a press did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum HangarOutcome {
    /// Nothing happened.
    Nothing,
    /// Done: back to the hub.
    Done,
    /// Sell with a plane that can go: the yes/no box (type 4) is up with this
    /// text; [`HangarPage::answer_sell`] answers it.
    ConfirmSell {
        /// The box text (string 700).
        text: String,
    },
    /// Sell with a plane that is not sold: an OK-only box (type 1).
    NotSold {
        /// The box text (string 704).
        text: String,
    },
    /// Sell with fewer than [`SELL_MIN_PLANES`] saved planes: the box text
    /// (`NB 701`) is unread.
    TooFew,
    /// The sale ran.
    Sold {
        /// The record emptied.
        slot: usize,
        /// The value added to the cash.
        value: u64,
    },
    /// The yes/no box was answered no.
    Kept,
    /// Export: the box (string 702) is up; the export itself is unread.
    Export {
        /// The box text, when the strings supplied it.
        text: Option<String>,
    },
}

/// The Sell hangar page.
#[derive(Clone, Debug)]
pub struct HangarPage {
    title: SlotText,
    prompt: SlotText,
    plane_line: SlotText,
    value_line: SlotText,
    list: DropdownRow,
    captions: Vec<TabCaption>,
    strings: Strings,
    planes: Vec<Option<SavedPlane>>,
    constructing: bool,
    selected: usize,
    cash: u64,
    asking: Option<String>,
    list_open: bool,
    dirty: bool,
}

#[derive(Clone, Debug, Default)]
struct Strings {
    name_line: Option<String>,
    sell_prompt: Option<String>,
    not_sold: Option<String>,
    export: Option<String>,
    value_line: Option<String>,
    airframe_names: Vec<Option<String>>,
    airframe_short: Vec<Option<String>>,
}

/// How many airframes' strings are read (`3000..3010`, `3020..3030`).
const AIRFRAMES: u32 = 11;

impl HangarPage {
    /// Reads the page from the layout; `name` resolves a resource name (with
    /// its `[FONT]` tag) and `id` a string-table id.
    ///
    /// # Errors
    ///
    /// [`HangarError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        name: &dyn Fn(&str) -> Option<String>,
        id: &dyn Fn(u32) -> Option<String>,
    ) -> Result<Self, HangarError> {
        let section = layout
            .section(HANGAR_SECTION)
            .ok_or(HangarError::NoSection)?;
        let text = |row: &str, resolve: bool| {
            text_row(section, row, resolve.then_some(name)).map_err(HangarError::Text)
        };
        let list = dropdown_rows(layout, HANGAR_SECTION)
            .map_err(HangarError::List)?
            .into_iter()
            .find(|row| row.id.eq_ignore_ascii_case(LIST_ROW))
            .ok_or_else(|| HangarError::RowMissing(LIST_ROW.to_owned()))?;
        let plain = |string: u32| id(string).map(|raw| split_tag(&raw).1.to_owned());
        let strings = Strings {
            name_line: plain(NAME_LINE_STRING),
            sell_prompt: plain(SELL_PROMPT_STRING),
            not_sold: plain(NOT_SOLD_STRING),
            export: plain(EXPORT_STRING),
            value_line: plain(VALUE_LINE_STRING),
            airframe_names: (0..AIRFRAMES)
                .map(|n| plain(AIRFRAME_NAME_BASE + n))
                .collect(),
            airframe_short: (0..AIRFRAMES)
                .map(|n| plain(AIRFRAME_SHORT_BASE + n))
                .collect(),
        };
        Ok(Self {
            title: text(TITLE_ROW, true)?,
            prompt: text(PROMPT_ROW, true)?,
            plane_line: text(PLANE_ROW, false)?,
            value_line: text(VALUE_ROW, false)?,
            list,
            captions: read_captions(section, name),
            strings,
            planes: vec![None; SAVED_SLOTS],
            constructing: false,
            selected: 0,
            cash: 0,
            asking: None,
            list_open: false,
            dirty: true,
        })
    }

    /// Hands the page the pilot's saved planes (`planes[slot]`, at most
    /// [`SAVED_SLOTS`]; a shorter list leaves the rest empty), whether the
    /// construction screen is open (engine state 13 or 17: its plane then
    /// stands in the record `selected`), the selected record and the cash.
    pub fn set_planes(
        &mut self,
        planes: Vec<Option<SavedPlane>>,
        constructing: bool,
        selected: usize,
        cash: u64,
    ) {
        let mut planes = planes;
        planes.resize(SAVED_SLOTS, None);
        self.planes = planes;
        self.constructing = constructing;
        self.selected = selected.min(WORKING_SLOT);
        self.cash = cash;
        self.asking = None;
        self.list_open = false;
        self.dirty = true;
    }

    /// The Sell and Export buttons' captions with their four colours.
    #[must_use]
    pub fn captions(&self) -> &[TabCaption] {
        &self.captions
    }

    /// The drop-down's closed box (25 rows displayed).
    #[must_use]
    pub const fn list(&self) -> &DropdownRow {
        &self.list
    }

    /// The title text.
    #[must_use]
    pub const fn title(&self) -> &SlotText {
        &self.title
    }

    /// The prompt text (*sell or export*).
    #[must_use]
    pub const fn prompt(&self) -> &SlotText {
        &self.prompt
    }

    /// The cash on hand, as the sales left it.
    #[must_use]
    pub const fn cash(&self) -> u64 {
        self.cash
    }

    /// The selected record (`[0x64b67c]`).
    #[must_use]
    pub const fn selected_slot(&self) -> usize {
        self.selected
    }

    /// Whether a yes/no box is up.
    #[must_use]
    pub fn asking(&self) -> Option<&str> {
        self.asking.as_deref()
    }

    pub(super) fn mark_dirty(&mut self) {
        self.dirty = true;
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn occupied(&self, slot: usize) -> bool {
        self.planes.get(slot).is_some_and(Option::is_some)
    }

    /// `E 2018`: the occupied records, less one while constructing.
    #[must_use]
    pub fn saved_count(&self) -> u32 {
        let occupied = (0..SAVED_SLOTS).filter(|&slot| self.occupied(slot)).count() as u32;
        occupied - u32::from(self.constructing && occupied > 0)
    }

    /// Whether the hub's Sell is active (`E 2018` answers 3 or more).
    #[must_use]
    pub fn sell_enabled(&self) -> bool {
        self.saved_count() >= SELL_MIN_PLANES
    }

    /// `0x4101d0`: the record row `index` stands for. While constructing the
    /// walk stops at the selected record; running off the end answers the
    /// working plane's record, [`WORKING_SLOT`].
    #[must_use]
    pub fn slot_for_index(&self, index: usize) -> usize {
        let mut left = index;
        for slot in 0..SAVED_SLOTS {
            if !self.occupied(slot) {
                continue;
            }
            if self.constructing && slot == self.selected {
                return slot;
            }
            if left == 0 {
                return slot;
            }
            left -= 1;
        }
        WORKING_SLOT
    }

    /// `0x410220`: the row the record `slot` stands for (`E 2012`): the
    /// occupied records up to it, not counting the selected one while
    /// constructing, less one. `None` for the engine's -1.
    #[must_use]
    pub fn index_for_slot(&self, slot: usize) -> Option<usize> {
        let counted = (0..=slot.min(SAVED_SLOTS - 1))
            .filter(|&n| self.occupied(n) && !(self.constructing && n == self.selected))
            .count();
        counted.checked_sub(1)
    }

    /// `E 2012`: the selected row.
    #[must_use]
    pub fn selected_index(&self) -> Option<usize> {
        self.index_for_slot(self.selected)
    }

    /// `E 2013(-1, index)`: selects a row.
    pub fn select_index(&mut self, index: usize) {
        self.selected = self.slot_for_index(index);
        self.dirty = true;
    }

    /// The selected record's plane.
    #[must_use]
    pub fn selected_plane(&self) -> Option<&SavedPlane> {
        self.planes.get(self.selected).and_then(Option::as_ref)
    }

    /// `E 2011`: string 207 over the selected plane's name and its airframe's
    /// name; `None` without a plane or the strings.
    #[must_use]
    pub fn plane_text(&self) -> Option<(&SlotText, String)> {
        let plane = self.selected_plane()?;
        let template = self.strings.name_line.as_deref()?;
        let airframe = self.airframe_name(plane.airframe)?;
        Some((
            &self.plane_line,
            fill(template, &[plane.name.as_str(), airframe]),
        ))
    }

    /// `E 2135`: string 1258 over the selected plane's value.
    #[must_use]
    pub fn value_text(&self) -> Option<(&SlotText, String)> {
        let plane = self.selected_plane()?;
        let template = self.strings.value_line.as_deref()?;
        Some((
            &self.value_line,
            fill(template, &[&plane.value.to_string()]),
        ))
    }

    fn airframe_name(&self, airframe: u32) -> Option<&str> {
        self.strings
            .airframe_names
            .get(airframe as usize)?
            .as_deref()
    }

    /// How many rows the open list shows: one per saved plane, as the walk of
    /// `0x4101d0` counts them.
    #[must_use]
    pub fn row_count(&self) -> usize {
        (self.saved_count() as usize).min(SAVED_SLOTS)
    }

    /// Where row `index` of the open list is: the rows hang under the closed
    /// box, one box height apart (as the hub's drop-downs do; the control's
    /// own geometry is unread).
    #[must_use]
    pub fn item_rect(&self, index: usize) -> ShellRect {
        let closed = self.list.rect;
        ShellRect {
            x: closed.x,
            y: closed.y + ((index as u32 + 1) * closed.height) as i32,
            width: closed.width,
            height: closed.height,
        }
    }

    /// The plane row `index` stands for, when the record holds one.
    #[must_use]
    pub fn plane_at(&self, index: usize) -> Option<&SavedPlane> {
        self.planes
            .get(self.slot_for_index(index))
            .and_then(Option::as_ref)
    }

    /// Whether the list is open.
    #[must_use]
    pub const fn list_open(&self) -> bool {
        self.list_open
    }

    /// Closes the open list; `true` if one was open.
    pub fn close_list(&mut self) -> bool {
        let was = std::mem::take(&mut self.list_open);
        self.dirty |= was;
        was
    }

    /// A press at a logical point: opens or closes the list, or selects the
    /// row under it (`E 2013(-1, row)`). `true` if the page took the press.
    /// While a box is up the page takes nothing.
    pub fn press_at(&mut self, x: i32, y: i32) -> bool {
        if self.asking.is_some() {
            return false;
        }
        if self.list_open {
            self.list_open = false;
            self.dirty = true;
            if let Some(row) = (0..self.row_count()).find(|row| self.item_rect(*row).contains(x, y))
            {
                self.select_index(row);
                return true;
            }
            return self.list.rect.contains(x, y);
        }
        if self.list.rect.contains(x, y) {
            self.list_open = true;
            self.dirty = true;
            return true;
        }
        false
    }

    fn airframe_short(&self, airframe: u32) -> Option<&str> {
        self.strings
            .airframe_short
            .get(airframe as usize)?
            .as_deref()
    }

    /// A press on one of the page's buttons.
    pub fn press(&mut self, button: &str) -> HangarOutcome {
        if self.asking.is_some() {
            return HangarOutcome::Nothing;
        }
        if button.eq_ignore_ascii_case(DONE_BUTTON) {
            // `E 2013(-1, 0)`.
            self.select_index(0);
            HangarOutcome::Done
        } else if button.eq_ignore_ascii_case(SELL_BUTTON) {
            self.press_sell()
        } else if button.eq_ignore_ascii_case(EXPORT_BUTTON) {
            let text = self.selected_plane().and_then(|plane| {
                let short = self.airframe_short(plane.airframe)?;
                let template = self.strings.export.as_deref()?;
                Some(fill(template, &[short]))
            });
            HangarOutcome::Export { text }
        } else {
            HangarOutcome::Nothing
        }
    }

    fn press_sell(&mut self) -> HangarOutcome {
        if self.saved_count() < SELL_MIN_PLANES {
            return HangarOutcome::TooFew;
        }
        let Some(plane) = self.selected_plane() else {
            return HangarOutcome::Nothing;
        };
        let short = self.airframe_short(plane.airframe).unwrap_or_default();
        if plane.kind == KIND_NOT_SOLD {
            let text = self
                .strings
                .not_sold
                .as_deref()
                .map_or_else(String::new, |t| fill(t, &[short, &plane.name]));
            return HangarOutcome::NotSold { text };
        }
        let text = self
            .strings
            .sell_prompt
            .as_deref()
            .map_or_else(String::new, |t| fill(t, &[short, &plane.value.to_string()]));
        self.asking = Some(text.clone());
        self.dirty = true;
        HangarOutcome::ConfirmSell { text }
    }

    /// Answers the yes/no box: [`YES_ANSWER`] runs `E 2109` (the value into
    /// the cash, the record emptied) and then selects row 0; any other answer
    /// keeps the plane. With no box up it does nothing.
    pub fn answer_sell(&mut self, answer: u32) -> HangarOutcome {
        if self.asking.take().is_none() {
            return HangarOutcome::Nothing;
        }
        self.dirty = true;
        if answer != YES_ANSWER {
            return HangarOutcome::Kept;
        }
        let slot = self.selected;
        let Some(plane) = self.planes.get_mut(slot).and_then(Option::take) else {
            return HangarOutcome::Nothing;
        };
        self.cash += plane.value;
        self.select_index(0);
        HangarOutcome::Sold {
            slot,
            value: plane.value,
        }
    }
}

/// The Sell and Export buttons' captions (`IDS_PS_B_SELL`, `IDS_PS_B_EXPORT`)
/// with the four colours of a `B` row's columns 14 to 17, as the hub's tabs
/// read them.
fn read_captions(section: &Section, name: &dyn Fn(&str) -> Option<String>) -> Vec<TabCaption> {
    [SELL_BUTTON, EXPORT_BUTTON]
        .into_iter()
        .filter_map(|id| {
            let button = section
                .buttons
                .iter()
                .find(|button| button.id.eq_ignore_ascii_case(id))?;
            let res = button
                .fields
                .iter()
                .find(|field| field.trim().starts_with("IDS_"))?;
            let raw = name(res.trim())?;
            let (size, text) = split_tag(&raw);
            let mut colors = [0xFF00_0000; 4];
            for (slot, index) in colors.iter_mut().zip(14..18) {
                let field = button.fields.get(index)?.trim();
                let digits = field
                    .strip_prefix("0x")
                    .or_else(|| field.strip_prefix("0X"))?;
                *slot = u32::from_str_radix(digits, 16).ok()?;
            }
            Some(TabCaption {
                button: button.id.to_ascii_uppercase(),
                text: text.to_owned(),
                size,
                colors,
            })
        })
        .collect()
}

/// Puts the arguments into the string's `%1!d!` / `%1!s!` holes (`%2!..!`, ...).
fn fill(template: &str, args: &[&str]) -> String {
    let mut out = template.to_owned();
    for (n, value) in args.iter().enumerate() {
        let n = n + 1;
        out = out
            .replace(&format!("%{n}!d!"), value)
            .replace(&format!("%{n}!s!"), value);
    }
    out
}

/// The paint the picture format reads: 13 reads as 11 (`0x40d559`).
const fn picture_paint(paint: u32) -> u32 {
    if paint == 13 { 11 } else { paint }
}

/// The picture of slot `layer` (`E 2243(layer, art, 1)`): the small icon of
/// the airframe in the paint.
#[must_use]
pub fn slot_icon_art(airframe: u32, paint: u32, layer: u32) -> String {
    format!("PX_Icon_{airframe}_{}_{layer}.tga", picture_paint(paint))
}

/// The airframe's blueprint (`E 2243(.., 0)`).
#[must_use]
pub fn blueprint_art(airframe: u32) -> String {
    format!("PX_{airframe}_Blueprint.tga")
}

/// How many picture slots the hub has.
pub const SLOT_ICONS: usize = 4;

/// One of the hub's four picture panes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotIcon {
    /// The pane row (`PX_P_PLANE<R>`).
    pub id: String,
    /// `R`, the picture's slot.
    pub layer: u32,
    /// Where the pane is drawn.
    pub origin: (i32, i32),
    /// The `Z` column: panes draw in ascending order.
    pub z: i32,
}

/// The hub's four picture panes, as the layout places them.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotIcons {
    icons: Vec<SlotIcon>,
}

impl SlotIcons {
    /// Reads `PX_P_PLANE0..3` out of the hub's section; `None` unless all four
    /// rows are there.
    #[must_use]
    pub fn from_section(section: &Section) -> Option<Self> {
        let mut icons = Vec::new();
        for layer in 0..SLOT_ICONS as u32 {
            let id = format!("PX_P_PLANE{layer}");
            let row = section
                .panes
                .iter()
                .find(|row| row.id.eq_ignore_ascii_case(&id))?;
            let number = |index: usize| row.fields.get(index)?.trim().parse::<i32>().ok();
            icons.push(SlotIcon {
                id: id.clone(),
                layer,
                origin: (number(1)?, number(2)?),
                z: number(3)?,
            });
        }
        Some(Self { icons })
    }

    /// The panes in declaration order (slot `R` at index `R`).
    #[must_use]
    pub fn icons(&self) -> &[SlotIcon] {
        &self.icons
    }

    /// The panes in drawing order (`Z` ascending, ties in declaration order).
    #[must_use]
    pub fn drawing_order(&self) -> Vec<&SlotIcon> {
        let mut order: Vec<&SlotIcon> = self.icons.iter().collect();
        order.sort_by_key(|icon| icon.z);
        order
    }

    /// The pictures to draw, back to front, for a plane of `airframe` in
    /// `paint`: the art's name and the pane's origin.
    #[must_use]
    pub fn pictures(&self, airframe: u32, paint: u32) -> Vec<(String, (i32, i32))> {
        self.drawing_order()
            .into_iter()
            .map(|icon| (slot_icon_art(airframe, paint, icon.layer), icon.origin))
            .collect()
    }
}

//! The Airframe and Engine panels of the Plane Construction hub (task #2043,
//! part of #2036): the `[@AirFrame@]` and `[@Engine@]` sections of
//! `ASSETS/LAYOUT.CSV` with the panel scripts `ASSETS/SCRIPTS/AIRFRAME.SCRIPT`
//! and `ENGINE.SCRIPT` run over them, as the model a presenter draws and a
//! headless drive reads. The hub ([`super::construction_hub`]) owns both.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Layout.** Each panel is a title text (`AF_T_TITLE` string
//!   `IDS_AF_TITLE`, `EN_T_TITLE` `IDS_EN_TITLE`), a one-line name text the
//!   script fills at run time (`AF_T_AIRFRAME`, `EN_T_ENGINENAME`, no
//!   resource id), a scrolling description box (`AF_S_AIRFRAMEDESC`,
//!   `EN_S_ENGINEDESC`) and one drop-down (`AF_D_AIRFRAME` 11 rows displayed,
//!   `EN_D_ENGINE` 7), all absolute.
//! * **Script.** Both scripts start the panel with `callback($$A$$, 2507, n)`
//!   (2 for the airframe, 3 for the engine) and fill the drop-down from the
//!   engine: the airframe's current index is `E 2217` (handler `0x40d272`,
//!   `0x410170([0x64cba4])`), the engine's `E 2219(0)`. The drop-down sends
//!   the script three messages:
//!   * `10015`, an item chosen. Airframe: `E 2216(index, 1)` (handler
//!     `0x40d28d`: `[0x64cba4] = 0x410120(index)` and the *Pending* flag
//!     `[0x647ba4] = (second argument == 0)`, so `1` commits and clears it).
//!     If the choice differs from the one the panel opened with, `E 2221`
//!     (the hub's "plane differs from the saved one") is asked first; when it
//!     is set message box 8 asks `E 2222` (unread) whether to switch
//!     (`@globals@OR.VR` 3 or 4 switches, 2 keeps the old airframe). Engine:
//!     `E 2219(1, index)` stores the engine at `[0x64cba8]`.
//!   * `10013`, an item rolled over. Airframe: `E 2216(item, 0)`, which sets
//!     the Pending flag, so the hub's weight line shows *Pending* while the
//!     pointer is over an item that is not the choice. Engine: `E 2219(1, item)`.
//!   * `10001`, the box's own init, re-applies the held choice.
//!
//!   After a change the script mails the hub `12001` (refresh the statistics
//!   and costs; `12002`/`12003` for the airframe's roll-over refresh).
//! * **Names.** The airframe's long name is string `3000 + airframe` and its
//!   short name `3020 + airframe` (the hub's findings).
//! * **Engine list.** The engine script gives its drop-down `HP.TJ = 2218`
//!   (the airframe's is `2200`, unread). `E 2218` answers 4, or 7 when
//!   `[0x64b678] > 7` or one of `[0x647b5c]`, `[0x647b68]`, `[0x647b6c]` is set
//!   (handler `0x40bce1`, asked with a null target); read as the length of
//!   the engine list, which is an inference. `E 2219`'s index limit is 3, or
//!   6 on the same condition, and an index equal to it is stored as 6
//!   (`0x40be05`). What those globals are is unread.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The descriptions.** `E 2223` (airframe) and `E 2224` (engine) fill the
//!   scrolling box; their string source is unread, so the box is not drawn.
//! * **The engine names** (`E 2220(2, ..)`) and the airframe-to-engine
//!   pairing: unread. The names of the options come from the feed.
//! * **The campaign gate** on the lists (the four globals above).
//! * **Message box 8** (the switch-airframe prompt, `E 2222`): the model
//!   refuses nothing and asks nothing; a caller that wants the prompt keys on
//!   the hub's dirty flag.
//! * **Which content id is airframe `n`** and where the numbers come from in a
//!   real run: the feed's job (#2052).
//! * **The statistics bars** the choice refreshes (`E 2015`, `PX_P_OS/AD/SI/OC`):
//!   #2051.
//! * **The drop-down's look.** The scroll-bar, arrow and highlight art are the
//!   control's; the page draws a stand-in box and list.

use std::fmt;

use cs_content::construction::ConstructionSchemaError;
use cs_types::content::ContentId;

use super::original::{OriginalLayout, Row, Section};
use super::previous_missions::{colour, split_tag};
use super::shell::ShellRect;
use super::widgets::{DropdownRow, WidgetError, dropdown_rows};
use crate::construction::ConstructionScreen;

/// Which of the two panels.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ChoiceKind {
    /// `AIRFRAME.SCRIPT`, the `AirFrame` section.
    Airframe,
    /// `ENGINE.SCRIPT`, the `Engine` section.
    Engine,
}

impl ChoiceKind {
    /// The layout section.
    #[must_use]
    pub const fn section(self) -> &'static str {
        match self {
            Self::Airframe => "AirFrame",
            Self::Engine => "Engine",
        }
    }

    /// The title text row.
    #[must_use]
    pub const fn title_row(self) -> &'static str {
        match self {
            Self::Airframe => "AF_T_TITLE",
            Self::Engine => "EN_T_TITLE",
        }
    }

    /// The one-line name text row, which the script fills at run time.
    #[must_use]
    pub const fn name_row(self) -> &'static str {
        match self {
            Self::Airframe => "AF_T_AIRFRAME",
            Self::Engine => "EN_T_ENGINENAME",
        }
    }

    /// The scrolling description box.
    #[must_use]
    pub const fn description_row(self) -> &'static str {
        match self {
            Self::Airframe => "AF_S_AIRFRAMEDESC",
            Self::Engine => "EN_S_ENGINEDESC",
        }
    }

    /// The drop-down row.
    #[must_use]
    pub const fn dropdown_row(self) -> &'static str {
        match self {
            Self::Airframe => "AF_D_AIRFRAME",
            Self::Engine => "EN_D_ENGINE",
        }
    }
}

/// Why a panel could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoiceError {
    /// The layout has no such section.
    NoSection(&'static str),
    /// A row the panel needs is not in the section.
    RowMissing(&'static str),
    /// A column is not the number the row layout states.
    Column {
        /// The row.
        row: &'static str,
    },
    /// The drop-down row is unreadable.
    Dropdown(WidgetError),
}

impl fmt::Display for ChoiceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection(name) => write!(f, "the layout has no {name} section"),
            Self::RowMissing(id) => write!(f, "the panel has no {id} row"),
            Self::Column { row } => write!(f, "{row}: a column is unreadable"),
            Self::Dropdown(error) => write!(f, "the drop-down is unreadable: {error:?}"),
        }
    }
}

impl std::error::Error for ChoiceError {}

/// One entry of the drop-down.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceOption {
    /// The content the entry stands for.
    pub id: ContentId,
    /// The entry's text.
    pub name: String,
}

/// A text of the panel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ChoiceText {
    /// The row id.
    pub id: String,
    /// The finished text.
    pub text: String,
    /// The `[FONT]` tag's size.
    pub size: Option<u32>,
    /// The box.
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// What a press at a point did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ChoicePress {
    /// The press is not the panel's.
    Nothing,
    /// The list opened or closed without a choice.
    Toggled,
    /// An entry was chosen (`mail 10015`).
    Chose(ContentId),
}

/// The Airframe or Engine panel.
#[derive(Clone, Debug)]
pub struct ChoicePanel {
    kind: ChoiceKind,
    title: Option<ChoiceText>,
    name: Option<ChoiceText>,
    description: ShellRect,
    dropdown: DropdownRow,
    options: Vec<ChoiceOption>,
    selected: Option<usize>,
    rolled: Option<usize>,
    open: bool,
    dirty: bool,
}

impl ChoicePanel {
    /// Reads the panel's rows. `name` answers an `IDS_*` name with the
    /// installation's text, `[FONT]` tag included.
    ///
    /// # Errors
    ///
    /// [`ChoiceError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        kind: ChoiceKind,
        name: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, ChoiceError> {
        let section = layout
            .section(kind.section())
            .ok_or(ChoiceError::NoSection(kind.section()))?;
        let title = text_row(section, kind.title_row(), Some(name))?;
        let name_text = text_row(section, kind.name_row(), None)?;
        let description = section
            .other
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(kind.description_row()))
            .ok_or(ChoiceError::RowMissing(kind.description_row()))?;
        let number = |index: usize| {
            description
                .fields
                .get(index)
                .and_then(|value| value.trim().parse::<i32>().ok())
                .ok_or(ChoiceError::Column {
                    row: kind.description_row(),
                })
        };
        // `S,Frame,Ground,Slider,Up,Down,X,Y,0,Width,Height,..`.
        let description = ShellRect {
            x: number(5)?,
            y: number(6)?,
            width: number(8)?.max(0) as u32,
            height: number(9)?.max(0) as u32,
        };
        let dropdown = dropdown_rows(layout, kind.section())
            .map_err(ChoiceError::Dropdown)?
            .into_iter()
            .find(|row| row.id.eq_ignore_ascii_case(kind.dropdown_row()))
            .ok_or(ChoiceError::RowMissing(kind.dropdown_row()))?;
        Ok(Self {
            kind,
            title: Some(title),
            name: Some(name_text),
            description,
            dropdown,
            options: Vec::new(),
            selected: None,
            rolled: None,
            open: false,
            dirty: true,
        })
    }

    /// Which panel this is.
    #[must_use]
    pub const fn kind(&self) -> ChoiceKind {
        self.kind
    }

    /// The closed box.
    #[must_use]
    pub const fn dropdown(&self) -> &DropdownRow {
        &self.dropdown
    }

    /// The scrolling description box. Nothing is drawn in it: its text comes
    /// from `E 2223`/`E 2224`, unread.
    #[must_use]
    pub const fn description_rect(&self) -> ShellRect {
        self.description
    }

    /// The entries.
    #[must_use]
    pub fn options(&self) -> &[ChoiceOption] {
        &self.options
    }

    /// The chosen entry's index.
    #[must_use]
    pub const fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The chosen entry.
    #[must_use]
    pub fn selected_option(&self) -> Option<&ChoiceOption> {
        self.selected.and_then(|index| self.options.get(index))
    }

    /// Whether the list is open.
    #[must_use]
    pub const fn is_open(&self) -> bool {
        self.open
    }

    /// The entry the pointer is over while the list is open.
    #[must_use]
    pub const fn rolled(&self) -> Option<usize> {
        self.rolled
    }

    /// Whether the hub's weight line shows *Pending*: the airframe panel's
    /// list is open with the pointer over an entry that is not the choice
    /// (`E 2216(item, 0)` sets `[0x647ba4]`, the choice clears it).
    #[must_use]
    pub fn weight_pending(&self) -> bool {
        self.kind == ChoiceKind::Airframe
            && self.open
            && self
                .rolled
                .is_some_and(|rolled| Some(rolled) != self.selected)
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Hands the panel its entries and the one held (`selected` is the id the
    /// draft carries; an id not among the entries leaves no choice).
    pub fn set_options(&mut self, options: Vec<ChoiceOption>, selected: Option<&ContentId>) {
        self.selected =
            selected.and_then(|wanted| options.iter().position(|option| &option.id == wanted));
        self.options = options;
        self.rolled = None;
        self.open = false;
        self.dirty = true;
    }

    /// Follows the draft: the choice is `selected` again.
    pub fn follow(&mut self, selected: Option<&ContentId>) {
        let at =
            selected.and_then(|wanted| self.options.iter().position(|option| &option.id == wanted));
        if at != self.selected {
            self.selected = at;
            self.dirty = true;
        }
    }

    /// The rect of the list's `index`-th row when open: below the box, one box
    /// height each.
    #[must_use]
    pub fn item_rect(&self, index: usize) -> ShellRect {
        let box_rect = self.dropdown.rect;
        ShellRect {
            x: box_rect.x,
            y: box_rect.y + ((index as u32 + 1) * box_rect.height) as i32,
            width: box_rect.width,
            height: box_rect.height,
        }
    }

    /// The pointer moved to a logical point: while the list is open the entry
    /// under it is rolled over (`mail 10013`).
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let rolled = if self.open {
            point.and_then(|(x, y)| {
                (0..self.options.len()).find(|index| self.item_rect(*index).contains(x, y))
            })
        } else {
            None
        };
        if rolled != self.rolled {
            self.rolled = rolled;
            self.dirty = true;
        }
    }

    /// A press at a logical point: opens or closes the list, or chooses the
    /// entry under the point. A press elsewhere closes an open list and is
    /// not the panel's.
    pub fn press_at(&mut self, x: i32, y: i32) -> ChoicePress {
        if self.options.is_empty() {
            return ChoicePress::Nothing;
        }
        if self.open {
            let picked =
                (0..self.options.len()).find(|index| self.item_rect(*index).contains(x, y));
            self.open = false;
            self.rolled = None;
            self.dirty = true;
            if let Some(index) = picked {
                self.selected = Some(index);
                return ChoicePress::Chose(self.options[index].id.clone());
            }
            return if self.dropdown.rect.contains(x, y) {
                ChoicePress::Toggled
            } else {
                ChoicePress::Nothing
            };
        }
        if self.dropdown.rect.contains(x, y) {
            self.open = true;
            self.dirty = true;
            return ChoicePress::Toggled;
        }
        ChoicePress::Nothing
    }

    /// Steps the choice by `direction` entries without the list (a keyboard
    /// stand-in: the drop-down's own keys are unread). Returns the entry
    /// chosen, if it moved.
    pub fn step(&mut self, direction: i32) -> Option<ContentId> {
        if self.options.is_empty() {
            return None;
        }
        let at = self.selected.map_or(-1, |index| index as i32);
        let next = (at + direction).clamp(0, self.options.len() as i32 - 1) as usize;
        if self.selected == Some(next) {
            return None;
        }
        self.selected = Some(next);
        self.dirty = true;
        Some(self.options[next].id.clone())
    }

    /// Closes the list; whether it was open.
    pub fn close_list(&mut self) -> bool {
        let was = self.open;
        if was {
            self.open = false;
            self.rolled = None;
            self.dirty = true;
        }
        was
    }

    /// The texts to draw: the title, and the chosen entry's name on the name
    /// line (the script's `E 2220`).
    #[must_use]
    pub fn texts(&self) -> Vec<ChoiceText> {
        let mut out = Vec::new();
        if let Some(title) = &self.title {
            out.push(title.clone());
        }
        if let (Some(line), Some(option)) = (&self.name, self.selected_option()) {
            out.push(ChoiceText {
                text: option.name.clone(),
                ..line.clone()
            });
        }
        out
    }
}

/// Applies a chosen entry to the draft: the airframe picker's or the engine
/// picker's edit on the production screen.
///
/// # Errors
///
/// [`ConstructionSchemaError`] when `id` is not of the panel's kind; the
/// draft is unchanged.
pub fn apply_choice(
    kind: ChoiceKind,
    id: ContentId,
    screen: &mut ConstructionScreen,
) -> Result<(), ConstructionSchemaError> {
    match kind {
        ChoiceKind::Airframe => screen.set_airframe(id),
        ChoiceKind::Engine => screen.set_engine(id),
    }
}

/// Answers an `IDS_*` name with the installation's text.
type NameFn<'a> = &'a dyn Fn(&str) -> Option<String>;

fn text_row(
    section: &Section,
    id: &'static str,
    name: Option<NameFn<'_>>,
) -> Result<ChoiceText, ChoiceError> {
    let row: &Row = section
        .other
        .iter()
        .find(|row| row.id.eq_ignore_ascii_case(id))
        .ok_or(ChoiceError::RowMissing(id))?;
    let bad = ChoiceError::Column { row: id };
    let number = |index: usize| {
        row.fields
            .get(index)
            .and_then(|value| value.trim().parse::<i32>().ok())
    };
    let (Some(x), Some(y)) = (number(1), number(2)) else {
        return Err(bad);
    };
    let color = row
        .fields
        .get(6)
        .and_then(|value| colour(value))
        .ok_or_else(|| bad.clone())?;
    let justify = row
        .fields
        .get(7)
        .and_then(|value| value.trim().parse::<u32>().ok())
        .ok_or(bad)?;
    let (size, text) = match (name, row.fields.first().map(|field| field.trim())) {
        (Some(name), Some(res)) if !res.is_empty() => {
            let raw = name(res).ok_or(ChoiceError::RowMissing(id))?;
            let (size, text) = split_tag(&raw);
            (size, text.to_owned())
        }
        _ => (None, String::new()),
    };
    Ok(ChoiceText {
        id: id.to_ascii_uppercase(),
        text,
        size,
        rect: ShellRect {
            x,
            y,
            width: number(4).unwrap_or(0).max(0) as u32,
            height: number(5).unwrap_or(0).max(0) as u32,
        },
        color,
        justify,
    })
}

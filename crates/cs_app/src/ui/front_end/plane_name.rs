//! The cabin's Plane Construction button opens this screen (task #1958): the
//! `[@PlaneName@]` section of `ASSETS/LAYOUT.CSV` with the dialog
//! `ASSETS/SCRIPTS/PLANENAME.SCRIPT` runs over it, as the model a presenter
//! draws and a headless drive reads.
//!
//! The panes and buttons draw through the [`super::MenuShell`] like every
//! other section; this page owns what the shell only counts: the name edit
//! box, the captions, the *Default loadout* check box's state and the
//! refusal of an empty name. Nothing here renders.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Layout.** Two panes (`PX_P_BACKGROUND`, the full-screen backdrop, and
//!   `PN_P_BACKGROUND`, the dialog plate the script centres on the surface),
//!   three buttons (`PN_B_OK`, `PN_B_CANCEL` and the check box
//!   `PN_B_DEFAULT`, a `1`-type button that starts checked), two `T` rows
//!   (`PN_T_TITLE`, `PN_T_DEFAULT`) and the `E` row `PN_E_NAME` (font
//!   `FONT_PN_EDIT`, 211 x 16, 16 characters, red text).
//! * **Placement.** The script centres the plate on the surface
//!   (`location = (resolution - plate) / 2`) and sets `relative = 1` on every
//!   control it creates but the backdrop, so the rows' `X,Y` are offsets from
//!   the plate. The shell applies [`centre_dialog`] when it presents the
//!   section; the page applies the same origin to its edit box and captions.
//!   Integer division is assumed.
//! * **The script.** Escape and *Cancel* leave the screen; *OK* with an empty
//!   name opens message box 203 and keeps the dialog; *OK* with a name runs
//!   `E 2215(1, name)`, starts `planeconstruction.script` and ends this one.
//!   Leaving by Cancel or Esc runs `E 2014(-1, -3, 0)` and then returns to
//!   whoever opened the dialog: `$$TGA$$` set means the multiplayer menu,
//!   `$$CV$$` set means Instant Action, neither means the cabin. The cabin
//!   button's own arm sets both to 0, so from the cabin it is the cabin.
//! * **Native `E 2215`** (engine image, dispatcher `0x4093a0`, jump-table
//!   entry for `0x8a7`, handler `0x40ad9b`, static). With a first argument of
//!   1 it copies the string argument into the engine buffer `0x64cb7c`: the
//!   plane's name. With any other first argument it answers that buffer.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The *Default loadout* check box** (`PN_B_DEFAULT`, string
//!   `IDS_PN_DEFAULTLOADOUT`). The script never reads it; what the engine does
//!   with its state (probably when `E 2215` or the construction screen starts)
//!   is unread. The page records the state and hands it to the caller.
//! * **`callback($$NB$$, 4005, …)`** at creation (it fills a value from the
//!   engine and the script clears it again) and **`mail(10021, MB)`** on OK
//!   (a refusal the edit control can answer) are unread; the page models
//!   neither, so an edit box that holds text is always accepted.
//! * **Enter** pressing OK is inferred from `MB.JM = NOA`.
//! * **`E 2014(-1, -3, 0)`** on leaving.
//! * **The edit box's face.** `FONT_PN_EDIT` is a GDI font id whose face is
//!   not in the installation; the presenter draws a stand-in.
//! * **Message box 203's look.** The original opens its `MessageBox` section;
//!   that presenter is not built, so the page shows the string's text under the
//!   edit box until the next key.
//! * **Where the original puts the buttons' captions** (as on the other
//!   pages): they are centred.

use std::fmt;

use super::original::OriginalLayout;
use super::pilot_page::EditBox;
use super::previous_missions::{ButtonCaption, TocLabel, colour, read_captions, read_labels};
use super::shell::{SECTION_SURFACE, ShellRect, ShellSection};

/// The layout section this page belongs to.
pub const NAME_SECTION: &str = "PlaneName";
/// The name's edit box row.
pub const EDIT_ROW: &str = "PN_E_NAME";
/// The full-screen backdrop pane: the one row the script leaves absolute.
pub const BACKDROP_ROW: &str = "PX_P_BACKGROUND";
/// The dialog plate pane the script centres; every other row is relative to it.
pub const PLATE_ROW: &str = "PN_P_BACKGROUND";
/// OK.
pub const OK_BUTTON: &str = "PN_B_OK";
/// Cancel.
pub const CANCEL_BUTTON: &str = "PN_B_CANCEL";
/// The *Default loadout* check box.
pub const DEFAULT_BUTTON: &str = "PN_B_DEFAULT";
/// The string of message box 203, which refuses an empty name.
pub const NAME_REQUIRED_STRING: u32 = 203;

/// Why the page could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameError {
    /// The layout has no `PlaneName` section.
    NoSection,
    /// A row the page needs is not in the section.
    RowMissing(&'static str),
    /// The plate's art is not measured, so the dialog cannot be centred.
    PlateUnread,
    /// A column is not the number the row layout states.
    Column {
        /// The row.
        row: &'static str,
        /// The column.
        column: &'static str,
    },
}

impl fmt::Display for NameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no {NAME_SECTION} section"),
            Self::RowMissing(id) => write!(f, "{NAME_SECTION} has no {id} row"),
            Self::PlateUnread => write!(f, "{NAME_SECTION}: the plate's art extent is unread"),
            Self::Column { row, column } => write!(f, "{row}: the {column} column is unreadable"),
        }
    }
}

impl std::error::Error for NameError {}

/// Where the script puts the dialog plate's corner: the surface minus the
/// plate, halved.
#[must_use]
pub fn plate_origin(plate: (u32, u32)) -> (i32, i32) {
    (
        (SECTION_SURFACE.0 as i32 - plate.0 as i32) / 2,
        (SECTION_SURFACE.1 as i32 - plate.1 as i32) / 2,
    )
}

/// Moves every pane but the backdrop, and every button, of the presented
/// `PlaneName` section from plate-relative to surface coordinates, as the
/// script's `relative = 1` does. Returns the origin, or `None` when the
/// section has no plate pane (its art was skipped).
pub(super) fn centre_dialog(section: &mut ShellSection) -> Option<(i32, i32)> {
    let plate = section
        .panes
        .iter()
        .find(|pane| pane.id.eq_ignore_ascii_case(PLATE_ROW))?;
    let (dx, dy) = plate_origin((plate.rect.width, plate.rect.height));
    for pane in &mut section.panes {
        if !pane.id.eq_ignore_ascii_case(BACKDROP_ROW) {
            pane.rect.x += dx;
            pane.rect.y += dy;
        }
    }
    for button in &mut section.buttons {
        button.rect.x += dx;
        button.rect.y += dy;
    }
    Some((dx, dy))
}

/// What a button press or Esc on the page did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NameOutcome {
    /// The press is not the page's.
    Nothing,
    /// OK with an empty name: the dialog stays and message box 203's text is
    /// shown ([`PlaneNamePage::notice`]).
    NameRequired,
    /// OK with a name: `E 2215(1, name)` and the construction screen.
    Construct {
        /// The plane's name, as typed.
        name: String,
        /// The *Default loadout* check box.
        default_loadout: bool,
    },
    /// Cancel or Esc: back to the cabin.
    Cancel,
}

/// The Plane Name page.
#[derive(Clone, Debug)]
pub struct PlaneNamePage {
    edit: EditBox,
    labels: Vec<TocLabel>,
    captions: Vec<ButtonCaption>,
    text: String,
    default_loadout: bool,
    required: Option<String>,
    notice: Option<String>,
    dirty: bool,
}

impl PlaneNamePage {
    /// Reads the `[@PlaneName@]` rows. `name` answers an `IDS_*` name and
    /// `id` a string-table id with the installation's text, `[FONT]` tag
    /// included; `measure` the extent of an art name (the plate's extent places
    /// the dialog).
    ///
    /// # Errors
    ///
    /// [`NameError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        name: &dyn Fn(&str) -> Option<String>,
        id: &dyn Fn(u32) -> Option<String>,
        measure: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Result<Self, NameError> {
        let section = layout.section(NAME_SECTION).ok_or(NameError::NoSection)?;
        let e = section
            .other
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(EDIT_ROW))
            .ok_or(NameError::RowMissing(EDIT_ROW))?;
        let col = |index: usize, column: &'static str| {
            e.fields
                .get(index)
                .and_then(|value| value.trim().parse::<i32>().ok())
                .ok_or(NameError::Column {
                    row: EDIT_ROW,
                    column,
                })
        };
        let unsigned = |index: usize, column: &'static str| {
            col(index, column).and_then(|value| {
                u32::try_from(value).map_err(|_| NameError::Column {
                    row: EDIT_ROW,
                    column,
                })
            })
        };
        let plate = section
            .panes
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(PLATE_ROW))
            .ok_or(NameError::RowMissing(PLATE_ROW))?;
        let frames = plate
            .fields
            .get(4)
            .and_then(|field| field.trim().parse::<u32>().ok())
            .filter(|frames| *frames > 0)
            .ok_or(NameError::Column {
                row: PLATE_ROW,
                column: "NumFrames",
            })?;
        let (plate_width, plate_height) = plate
            .fields
            .first()
            .and_then(|art| measure(art))
            .ok_or(NameError::PlateUnread)?;
        let (dx, dy) = plate_origin((plate_width, plate_height / frames));
        let edit = EditBox {
            font: e.fields.first().cloned().unwrap_or_default(),
            rect: ShellRect {
                x: col(1, "X")? + dx,
                y: col(2, "Y")? + dy,
                width: unsigned(4, "Width")?,
                height: unsigned(5, "Height")?,
            },
            max_chars: unsigned(6, "MaxChars")? as usize,
            text_color: e.fields.get(7).and_then(|value| colour(value)).ok_or(
                NameError::Column {
                    row: EDIT_ROW,
                    column: "TextColor",
                },
            )?,
            frame_color: e.fields.get(8).and_then(|value| colour(value)),
            cursor_color: e.fields.get(9).and_then(|value| colour(value)).ok_or(
                NameError::Column {
                    row: EDIT_ROW,
                    column: "CursorColor",
                },
            )?,
        };
        let mut labels = read_labels(section, name).map_err(|error| match error {
            super::previous_missions::TocError::Column { row, column } => {
                NameError::Column { row, column }
            }
            _ => NameError::Column {
                row: "T",
                column: "X/Y/Width/Height/Color/Justify",
            },
        })?;
        for label in &mut labels {
            label.rect.x += dx;
            label.rect.y += dy;
        }
        let captions = read_captions(section, name);
        let default_loadout = section
            .buttons
            .iter()
            .find(|button| button.id.eq_ignore_ascii_case(DEFAULT_BUTTON))
            .and_then(|button| button.fields.get(13))
            .is_some_and(|field| field.trim() == "1");
        let required = id(NAME_REQUIRED_STRING)
            .map(|raw| super::previous_missions::split_tag(&raw).1.to_owned());
        Ok(Self {
            edit,
            labels,
            captions,
            text: String::new(),
            default_loadout,
            required,
            notice: None,
            dirty: true,
        })
    }

    /// The edit box row.
    #[must_use]
    pub fn edit(&self) -> &EditBox {
        &self.edit
    }

    /// The `T` captions that resolved.
    #[must_use]
    pub fn labels(&self) -> &[TocLabel] {
        &self.labels
    }

    /// The buttons' captions that resolved.
    #[must_use]
    pub fn captions(&self) -> &[ButtonCaption] {
        &self.captions
    }

    /// The typed name.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The *Default loadout* check box.
    #[must_use]
    pub fn default_loadout(&self) -> bool {
        self.default_loadout
    }

    /// The text of the message box the last OK opened, while it is up.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// Typed text: printable characters up to the row's `MaxChars`.
    pub fn type_text(&mut self, text: &str) {
        for character in text.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < self.edit.max_chars {
                self.text.push(character);
            }
        }
        self.notice = None;
        self.dirty = true;
    }

    /// Backspace.
    pub fn backspace(&mut self) {
        self.text.pop();
        self.notice = None;
        self.dirty = true;
    }

    /// The shell toggled the check box.
    pub fn set_default_loadout(&mut self, checked: bool) {
        self.default_loadout = checked;
        self.dirty = true;
    }

    /// Esc: the script's `gui_char` leaves the screen as Cancel does.
    pub fn escape(&mut self) -> NameOutcome {
        NameOutcome::Cancel
    }

    /// Enter in the edit box presses OK. An inference from the script's
    /// `MB.JM = NOA` (the edit control's partner is the OK button); the
    /// property's meaning is unread.
    pub fn press_enter(&mut self) -> NameOutcome {
        self.press(OK_BUTTON)
    }

    /// A press on one of the page's buttons.
    pub fn press(&mut self, button: &str) -> NameOutcome {
        if button.eq_ignore_ascii_case(CANCEL_BUTTON) {
            NameOutcome::Cancel
        } else if button.eq_ignore_ascii_case(OK_BUTTON) {
            if self.text.is_empty() {
                self.notice = self.required.clone();
                self.dirty = true;
                NameOutcome::NameRequired
            } else {
                NameOutcome::Construct {
                    name: self.text.clone(),
                    default_loadout: self.default_loadout,
                }
            }
        } else {
            NameOutcome::Nothing
        }
    }
}

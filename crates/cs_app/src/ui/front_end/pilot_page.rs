//! The Campaign pilot-selection page's model (FE-MENU-3, task #1835): the
//! `[@Campaign@]` section's edit box `CM_E_NAME`, its list box
//! `CM_L_PLAYERS` and the Continue / Delete Player buttons, over the
//! existing [`ProfileSession`].
//!
//! The panes and buttons draw through the [`super::MenuShell`] like every
//! other section; the shell counts `E` and `L` rows and never draws them,
//! so this page reads those two rows from the [`OriginalLayout`] and owns
//! their behaviour. Nothing here renders: the windowed presenter draws what
//! [`PilotPage`] holds, and a headless drive reads the same model.
//!
//! # What the original data says (static readings, not `verified_original`)
//!
//! `LAYOUT.CSV` `[@Campaign@]`:
//!
//! * `CM_E_NAME=E,FONT_CM_EDIT,243,297,0,221,22,16,0xFFFF0000,,0xFFEF0010`.
//!   The header comment writes `ID=E,FontID,X,Y,Z,Width,Height,MaxChars,
//!   HelpID,TabOrder,TexTCOLOR,FrameColor,CursorColor`; the data rows omit
//!   the `HelpID` and `TabOrder` columns (the same header staleness the
//!   `M` and `B` headers have), so the columns read here are font, X, Y, Z,
//!   width, height, max characters, text colour (`0xFFFF0000`, red),
//!   frame colour (empty: "0 means use default", no frame is drawn) and
//!   caret colour.
//! * `CM_L_PLAYERS=L,CM_B_ScrollBar.png,CM_B_ScrollUp.png,
//!   CM_B_ScrollDown.png,245,356,0,305,20,7`. The header writes a
//!   `ScriptPointer` column the row omits; the rest reads slider, up, down,
//!   X, Y, Z, width, **row height 20** and **7 rows displayed** (the
//!   script's own list draw computes a row height and a visible-row count
//!   from them: `XB * ZB`, `parent.TM`).
//! * `CAMPAIGN.SCRIPT`: Enter in the edit box (message 10026) and the
//!   Continue button (10003 from `CM_B_START`) both run message 11004,
//!   which refuses an empty name (message box 200), looks the name up
//!   (`callback 2099`), creates or selects it (`callback 2100`) and starts
//!   the passenger cabin; the Delete button asks a yes/no message box (201)
//!   before it deletes the named pilot; a list row press (10015) copies the
//!   row's name into the edit box; the list draws a black ground, the
//!   selected row filled `0xFF800000`, the hovered row framed
//!   `0xFFFF0000`, row text in the engine's `gfont3d`, white.
//!   The pilot limit is the script's `24 > IB`.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The original save layout.** The original stores pilots through
//!   engine callbacks (`callback 2099/2100/2101/2106`) whose file format is
//!   unread. Pilots persist in **this project's own profile files**
//!   ([`ProfileSession`]); the original layout is recorded as unknown.
//! * **`FONT_CM_EDIT`.** A GDI font id whose face and height are not in the
//!   installation (`docs/findings/2026-10-01-f51-fontcell-…`). The windowed
//!   presenter draws the text with a stand-in face and says so.
//! * **The list's scroll bar.** The slider, up and down art are `ctl`
//!   objects the script configures at run time; their placement, tiling and
//!   frame layout are unread, so the art is *not drawn* (a stretched guess
//!   showed as a flat block in the windowed capture) and scrolling is by
//!   the arrows and [`PilotPage::scroll`]. The art names are kept on
//!   [`ListBox`] for the task that reads the slider control.
//! * **The delete confirmation.** The original opens its `MessageBox`
//!   section (string 201, a yes/no button set). That dialog's presenter is
//!   not built, so the first Delete press *arms* the delete and names the
//!   pilot, and a second press confirms; any other input cancels. A design
//!   choice, so no pilot is ever deleted by one stray click.
//! * **Esc.** The measured finding
//!   `2026-10-22-parity5-start-flow-transitions.md` found no Esc handler on
//!   this page; the task asks for Esc = Cancel, which is implemented as a
//!   design addition and recorded as such.
//! * The script's `crashcheat!` name, the `mail(10021, MB)` check and the
//!   `$$A$$` sound callbacks are unmodelled.

use cs_types::profile::ProfileId;

use super::original::{OriginalLayout, Row};
use super::shell::ShellRect;
use crate::profile::{ProfileSession, SessionError};

/// The layout section the page belongs to.
pub const PILOT_SECTION: &str = "Campaign";

/// The edit box row id (upper-cased, as the layout stores ids).
pub const EDIT_ROW: &str = "CM_E_NAME";

/// The list box row id.
pub const LIST_ROW: &str = "CM_L_PLAYERS";

/// The Continue button's row id.
pub const CONTINUE_BUTTON: &str = "CM_B_START";

/// The Delete Player button's row id.
pub const DELETE_BUTTON: &str = "CM_B_DELETEPLAYER";

/// The Cancel button's row id.
pub const CANCEL_BUTTON: &str = "CM_B_CANCEL";

/// The most pilots the list holds: the script's `24 > IB` guard.
pub const MAX_PILOTS: usize = 24;

/// The list's selected-row fill, `0xAARRGGBB` (the script's `setpencolor`).
pub const SELECTED_FILL: u32 = 0xFF80_0000;

/// The list's hovered-row frame colour.
pub const HOVER_FRAME: u32 = 0xFFFF_0000;

/// The list's row text colour (the script's `print3d_attributes`).
pub const ROW_TEXT: u32 = 0xFFFF_FFFF;

/// Why the page could not be built from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PilotPageError {
    /// The layout holds no `Campaign` section.
    NoSection,
    /// The section holds no such `E`/`L` row.
    RowMissing {
        /// The row id looked for.
        id: &'static str,
    },
    /// A column the page needs is empty or not a number.
    FieldUnread {
        /// The row id.
        id: &'static str,
        /// The column's name.
        field: &'static str,
    },
}

impl std::fmt::Display for PilotPageError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no {PILOT_SECTION} section"),
            Self::RowMissing { id } => write!(f, "the {PILOT_SECTION} section has no {id} row"),
            Self::FieldUnread { id, field } => write!(f, "{id}'s {field} column is not readable"),
        }
    }
}

impl std::error::Error for PilotPageError {}

/// The `E` row: the pilot name's edit box.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct EditBox {
    /// The font id as written (`FONT_CM_EDIT`); its face is unread.
    pub font: String,
    /// The box on the 800x600 surface.
    pub rect: ShellRect,
    /// The row's `MaxChars`.
    pub max_chars: usize,
    /// The text colour, `0xAARRGGBB`.
    pub text_color: u32,
    /// The frame colour; `None` when the column is empty ("use default").
    pub frame_color: Option<u32>,
    /// The caret colour.
    pub cursor_color: u32,
}

/// The `L` row: the pilot list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListBox {
    /// The scroll bar art name as written.
    pub slider: String,
    /// The scroll-up art name.
    pub up: String,
    /// The scroll-down art name.
    pub down: String,
    /// The list's rect: the row's X, Y, width and `row_height × visible`.
    pub rect: ShellRect,
    /// One row's height in pixels (the row's `Height` column).
    pub row_height: u32,
    /// Rows displayed at once (the row's last column).
    pub visible: usize,
}

/// One pilot in the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PilotEntry {
    /// The profile's persistent id.
    pub id: ProfileId,
    /// The pilot's name as stored.
    pub name: String,
}

/// What a page action did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PilotOutcome {
    /// Nothing happened (or the page only changed its own state).
    Nothing,
    /// Continue chose a pilot: the screen should go on to the cabin.
    Continued {
        /// The pilot.
        id: ProfileId,
        /// The pilot's name.
        name: String,
        /// Whether Continue created it (a new name) or chose an existing one.
        created: bool,
    },
    /// A pilot was deleted.
    Deleted {
        /// The pilot's name.
        name: String,
    },
}

/// A line of feedback the page shows under the list.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PilotNotice {
    /// Continue with an empty name (the original opens message box 200).
    NameRequired,
    /// Continue would create a 25th pilot.
    ListFull,
    /// First Delete press: names the pilot and asks for the second.
    ConfirmDelete(String),
    /// Delete with no listed pilot named.
    NothingToDelete,
    /// The profile store refused; the reason as text.
    Store(String),
}

impl PilotNotice {
    /// The text the presenter draws.
    #[must_use]
    pub fn text(&self) -> String {
        match self {
            Self::NameRequired => "Enter a pilot name.".to_owned(),
            Self::ListFull => format!("The list holds at most {MAX_PILOTS} pilots."),
            Self::ConfirmDelete(name) => {
                format!("Delete {name}? Press Delete Player again to confirm.")
            }
            Self::NothingToDelete => "Choose a listed pilot to delete.".to_owned(),
            Self::Store(reason) => reason.clone(),
        }
    }
}

/// The pilot-selection page: edit text, list, selection and feedback.
#[derive(Clone, Debug)]
pub struct PilotPage {
    edit: EditBox,
    list: ListBox,
    text: String,
    entries: Vec<PilotEntry>,
    selected: Option<usize>,
    top: usize,
    hover: Option<usize>,
    pending_delete: Option<ProfileId>,
    notice: Option<PilotNotice>,
    dirty: bool,
}

fn number(
    row: &Row,
    index: usize,
    id: &'static str,
    field: &'static str,
) -> Result<i32, PilotPageError> {
    row.fields
        .get(index)
        .and_then(|value| value.trim().parse::<i32>().ok())
        .ok_or(PilotPageError::FieldUnread { id, field })
}

fn unsigned(
    row: &Row,
    index: usize,
    id: &'static str,
    field: &'static str,
) -> Result<u32, PilotPageError> {
    number(row, index, id, field).and_then(|value| {
        u32::try_from(value).map_err(|_| PilotPageError::FieldUnread { id, field })
    })
}

fn colour(text: &str) -> Option<u32> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
}

impl PilotPage {
    /// Reads the page's two rows from the layout's `Campaign` section.
    ///
    /// # Errors
    ///
    /// [`PilotPageError`] when the section, a row or a needed column is
    /// absent — never a guessed geometry.
    pub fn from_layout(layout: &OriginalLayout) -> Result<Self, PilotPageError> {
        let section = layout
            .section(PILOT_SECTION)
            .ok_or(PilotPageError::NoSection)?;
        let find = |id: &'static str| {
            section
                .other
                .iter()
                .find(|row| row.id.eq_ignore_ascii_case(id))
                .ok_or(PilotPageError::RowMissing { id })
        };
        let e = find(EDIT_ROW)?;
        let edit = EditBox {
            font: e.fields.first().cloned().unwrap_or_default(),
            rect: ShellRect {
                x: number(e, 1, EDIT_ROW, "X")?,
                y: number(e, 2, EDIT_ROW, "Y")?,
                width: unsigned(e, 4, EDIT_ROW, "Width")?,
                height: unsigned(e, 5, EDIT_ROW, "Height")?,
            },
            max_chars: unsigned(e, 6, EDIT_ROW, "MaxChars")? as usize,
            text_color: e.fields.get(7).and_then(|value| colour(value)).ok_or(
                PilotPageError::FieldUnread {
                    id: EDIT_ROW,
                    field: "TextColor",
                },
            )?,
            frame_color: e.fields.get(8).and_then(|value| colour(value)),
            cursor_color: e.fields.get(9).and_then(|value| colour(value)).ok_or(
                PilotPageError::FieldUnread {
                    id: EDIT_ROW,
                    field: "CursorColor",
                },
            )?,
        };
        let l = find(LIST_ROW)?;
        let row_height = unsigned(l, 7, LIST_ROW, "Height")?;
        let visible = unsigned(l, 8, LIST_ROW, "TotalDisplayed")? as usize;
        if row_height == 0 || visible == 0 {
            return Err(PilotPageError::FieldUnread {
                id: LIST_ROW,
                field: "Height",
            });
        }
        let list = ListBox {
            slider: l.fields.first().cloned().unwrap_or_default(),
            up: l.fields.get(1).cloned().unwrap_or_default(),
            down: l.fields.get(2).cloned().unwrap_or_default(),
            rect: ShellRect {
                x: number(l, 3, LIST_ROW, "X")?,
                y: number(l, 4, LIST_ROW, "Y")?,
                width: unsigned(l, 6, LIST_ROW, "Width")?,
                height: row_height * visible as u32,
            },
            row_height,
            visible,
        };
        Ok(Self {
            edit,
            list,
            text: String::new(),
            entries: Vec::new(),
            selected: None,
            top: 0,
            hover: None,
            pending_delete: None,
            notice: None,
            dirty: true,
        })
    }

    /// The edit box row.
    #[must_use]
    pub fn edit(&self) -> &EditBox {
        &self.edit
    }

    /// The list box row.
    #[must_use]
    pub fn list(&self) -> &ListBox {
        &self.list
    }

    /// The edit box's text.
    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    /// The listed pilots, in the library's order.
    #[must_use]
    pub fn entries(&self) -> &[PilotEntry] {
        &self.entries
    }

    /// The selected row.
    #[must_use]
    pub fn selected(&self) -> Option<usize> {
        self.selected
    }

    /// The first row shown.
    #[must_use]
    pub fn top(&self) -> usize {
        self.top
    }

    /// The hovered row.
    #[must_use]
    pub fn hover(&self) -> Option<usize> {
        self.hover
    }

    /// The feedback line, when there is one.
    #[must_use]
    pub fn notice(&self) -> Option<&PilotNotice> {
        self.notice.as_ref()
    }

    /// Whether a delete has been armed by a first press.
    #[must_use]
    pub fn delete_armed(&self) -> bool {
        self.pending_delete.is_some()
    }

    /// Whether the drawn state changed since [`Self::take_dirty`].
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    fn touch(&mut self) {
        self.dirty = true;
    }

    fn disarm(&mut self) {
        if self.pending_delete.take().is_some() {
            self.notice = None;
        }
    }

    /// Re-reads the pilot names from the session's library. Keeps the edit
    /// text; selection follows the text when it names a listed pilot.
    pub fn refresh(&mut self, session: &ProfileSession) {
        let mut entries = Vec::new();
        for id in session.library().live() {
            if let Ok(loaded) = session.library().load(*id)
                && let Some(document) = loaded.document
            {
                entries.push(PilotEntry {
                    id: *id,
                    name: document.display_name,
                });
            }
        }
        self.entries = entries;
        self.selected = self.find(&self.text);
        self.top = self
            .top
            .min(self.entries.len().saturating_sub(self.list.visible));
        self.touch();
    }

    fn find(&self, name: &str) -> Option<usize> {
        let name = name.trim();
        if name.is_empty() {
            return None;
        }
        self.entries
            .iter()
            .position(|entry| entry.name.eq_ignore_ascii_case(name))
    }

    /// Typed text: printable characters up to the row's `MaxChars`.
    pub fn type_text(&mut self, text: &str) {
        self.disarm();
        for character in text.chars().filter(|c| !c.is_control()) {
            if self.text.chars().count() < self.edit.max_chars {
                self.text.push(character);
            }
        }
        self.selected = self.find(&self.text);
        self.notice = None;
        self.touch();
    }

    /// Backspace.
    pub fn backspace(&mut self) {
        self.disarm();
        self.text.pop();
        self.selected = self.find(&self.text);
        self.notice = None;
        self.touch();
    }

    fn reveal(&mut self, row: usize) {
        if row < self.top {
            self.top = row;
        } else if row >= self.top + self.list.visible {
            self.top = row + 1 - self.list.visible;
        }
    }

    /// A press on list row `row`: selects it and copies its name into the edit
    /// box (the script's message 10015).
    pub fn select_row(&mut self, row: usize) {
        self.disarm();
        let Some(entry) = self.entries.get(row) else {
            return;
        };
        self.text = entry.name.chars().take(self.edit.max_chars).collect();
        self.selected = Some(row);
        self.reveal(row);
        self.notice = None;
        self.touch();
    }

    /// Up/Down arrows: moves the selection one row, clamped, and selects it.
    pub fn move_selection(&mut self, forward: bool) {
        if self.entries.is_empty() {
            return;
        }
        let last = self.entries.len() - 1;
        let row = match (self.selected, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(row), true) => (row + 1).min(last),
            (Some(row), false) => row.saturating_sub(1),
        };
        self.select_row(row);
    }

    /// Wheel scroll: moves the first shown row without selecting.
    pub fn scroll(&mut self, rows: i32) {
        let max = self.entries.len().saturating_sub(self.list.visible);
        let top = (self.top as i64 + i64::from(rows)).clamp(0, max as i64);
        if top as usize != self.top {
            self.top = top as usize;
            self.touch();
        }
    }

    /// The list row under the logical point, if any.
    #[must_use]
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.list.rect.contains(x, y) {
            return None;
        }
        let offset = u32::try_from(y - self.list.rect.y).ok()? / self.list.row_height;
        let row = self.top + offset as usize;
        (row < self.entries.len() && (offset as usize) < self.list.visible).then_some(row)
    }

    /// Hover follows the pointer (`None` off the list).
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let row = point.and_then(|(x, y)| self.row_at(x, y));
        if row != self.hover {
            self.hover = row;
            self.touch();
        }
    }

    /// Esc: cancels an armed delete and reports whether it consumed the
    /// key (otherwise the screen's Cancel runs).
    pub fn escape(&mut self) -> bool {
        if self.pending_delete.is_some() {
            self.disarm();
            self.touch();
            return true;
        }
        false
    }

    fn store_error(&mut self, error: &SessionError) {
        self.notice = Some(PilotNotice::Store(error.to_string()));
        self.touch();
    }

    /// Continue / Enter: an empty name is refused; a listed name selects its
    /// pilot; any other name creates one (up to [`MAX_PILOTS`]).
    pub fn press_continue(&mut self, session: &mut ProfileSession) -> PilotOutcome {
        self.disarm();
        let name = self.text.trim().to_owned();
        if name.is_empty() {
            self.notice = Some(PilotNotice::NameRequired);
            self.touch();
            return PilotOutcome::Nothing;
        }
        let (id, created) = match self.find(&name) {
            Some(row) => {
                let entry = self.entries[row].clone();
                if let Err(error) = session.select(entry.id) {
                    self.store_error(&error);
                    return PilotOutcome::Nothing;
                }
                (entry.id, false)
            }
            None => {
                if self.entries.len() >= MAX_PILOTS {
                    self.notice = Some(PilotNotice::ListFull);
                    self.touch();
                    return PilotOutcome::Nothing;
                }
                match session.create(&name) {
                    Ok(id) => (id, true),
                    Err(error) => {
                        self.store_error(&error);
                        return PilotOutcome::Nothing;
                    }
                }
            }
        };
        self.notice = None;
        self.refresh(session);
        PilotOutcome::Continued { id, name, created }
    }

    /// Delete Player: the first press arms the delete for the pilot the edit
    /// box names; the second confirms and deletes it.
    pub fn press_delete(&mut self, session: &mut ProfileSession) -> PilotOutcome {
        let Some(row) = self.find(&self.text) else {
            self.pending_delete = None;
            self.notice = Some(PilotNotice::NothingToDelete);
            self.touch();
            return PilotOutcome::Nothing;
        };
        let entry = self.entries[row].clone();
        if self.pending_delete != Some(entry.id) {
            self.pending_delete = Some(entry.id);
            self.notice = Some(PilotNotice::ConfirmDelete(entry.name));
            self.touch();
            return PilotOutcome::Nothing;
        }
        self.pending_delete = None;
        match session.delete(entry.id) {
            Ok(()) => {
                self.text.clear();
                self.notice = None;
                self.refresh(session);
                PilotOutcome::Deleted { name: entry.name }
            }
            Err(error) => {
                self.store_error(&error);
                PilotOutcome::Nothing
            }
        }
    }
}

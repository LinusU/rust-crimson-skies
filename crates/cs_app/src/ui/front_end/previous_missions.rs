//! The cabin's Previous Missions screen (task #1956): the `[@ScrapBook_TOC@]`
//! section of `ASSETS/LAYOUT.CSV` with the list `ASSETS/SCRIPTS/
//! SCRAPBOOK_TOC.SCRIPT` fills from native calls, as the model a presenter
//! draws and a headless drive reads.
//!
//! The panes and buttons draw through the [`super::MenuShell`] like every
//! other section; this page owns what the shell only counts: the list box
//! (`SBTOC_L_TOCList`), its rows, the selection, the two `T` rows and the
//! buttons' captions. Nothing here renders.
//!
//! # What the original says (static readings, not `verified_original`)
//!
//! * **Layout.** `SBTOC_BACKGROUND` (`SB_BackgroundTOC.jpg`), four buttons
//!   (`SBTOC_B_VIEW`, `SBTOC_B_REPLAY`, `SBTOC_B_RETURN`, whose
//!   `ScriptToExe` is `PassengerCabin`, and the tab `SBTOC_B_CURRENT`), the
//!   `T` rows `SBTOC_T_CHARACTER` (no resource id: filled at run time) and
//!   `SBTOC_T_MISSIONS` (`IDS_SBTOC_PREVIOUS`), and the list `SBTOC_L_TOCList`
//!   (`420,140`, 325 wide, rows 80 high, 4 shown).
//! * **The list** (`SCRAPBOOK_TOC.SCRIPT`, class `NTA`). Each row is one
//!   `callback($$E$$, 2409, row, icon, line1, line2, line3)`: an icon frame
//!   of `fc_planeicons.png` (a vertical strip of 12 frames) and three text lines drawn 10, 30
//!   and 50 px down the row, black, at the icon's width plus 20. A press on a
//!   row selects it (`UT` = the row index, the mission number); the selected
//!   row has a `0x80f2e7b7` fill and an `0xffdd9017` frame, the hovered row
//!   an `0xffdd9017` frame over a `0x40f2e7b7` fill.
//! * **Native `E 2409`** (engine image, handler `0x40a4b8`, static).
//!   Row `-1` answers the count: the completed-mission count `[0x64b678]`
//!   plus one (the debug code `ispy` swaps in 24 → 25 rows). Row `0` is a
//!   header row: icon frame 11, lines `string 1217`, `string 1218` and a
//!   third built from `string 511`. Row `n >= 1` is mission `n`: icon from
//!   the pilot's mission record, line 1 `string 3479 + n`, line 2
//!   `string 1219 + k` where `k` comes from session query `A 7` (its value
//!   domain is unread), line 3 the record's stored text.
//! * **Native `E 2405`** reads the selected mission `[0x64b340]`; **`E 2411`**
//!   (`0x40a408`) answers whether a mission is replayable: it is when either
//!   of two words of the pilot's record for it is non-zero (mission 0 never).
//!   The script mails `10018` to the Replay button when it is, `10000` when
//!   it is not.
//! * **Buttons.** Replay runs `E 2104(UT)` and, for a mission that is not the
//!   one just played, `A 31` then `A 6` (the briefing). View Selected and the
//!   Current Mission tab (`E 2405(1, …)` then message `20002`) open the
//!   `SCRAPBOOK` journal screen.
//!
//! # What is *not* known, and so not claimed
//!
//! * **The per-pilot mission records.** The words `E 2411` tests, the icon
//!   and the stored text line live in an engine table this project does not
//!   write yet (the start flow keeps no per-pilot progress). [`MissionProgress`]
//!   is the declared input; [`MissionProgress::fresh`] is a pilot with no
//!   completed mission, so the list shows its header row only and Replay
//!   stays off.
//! * **Line 2** of a mission row (`A 7`'s domain and its table) and the
//!   **third header line**'s format are unread: those lines are `None` and
//!   draw nothing.
//! * **`SBTOC_T_CHARACTER`**'s text (`callback($$NB$$, 90)` joined with
//!   `E 2101`) is unread: the row draws nothing.
//! * **What `10018` / `10000` do** to the Replay button is unread. The page
//!   ignores a Replay press while the gate is off (an inference from the
//!   names), and the button is not drawn disabled.
//! * **The caption face.** The buttons' `IDS_*` captions carry a `[TREB9B]`
//!   tag; where the original puts them over the art is unread, the presenter
//!   centres them.
//! * **The `ispy` code** (`gui_char`) is not modelled.
//! * **The journal screen** (`SCRAPBOOK.SCRIPT`) View Selected and Current
//!   Mission open is not built; a press reports [`TocOutcome::JournalNotBuilt`].

use std::fmt;

use super::original::OriginalLayout;
use super::pilot_page::ListBox;
use super::shell::ShellRect;

/// The layout section this page belongs to.
pub const TOC_SECTION: &str = "ScrapBook_TOC";
/// The list row.
pub const TOC_LIST_ROW: &str = "SBTOC_L_TOCList";
/// View Selected.
pub const VIEW_BUTTON: &str = "SBTOC_B_VIEW";
/// Replay Mission.
pub const REPLAY_BUTTON: &str = "SBTOC_B_REPLAY";
/// Return to Cabin.
pub const RETURN_BUTTON: &str = "SBTOC_B_RETURN";
/// The Current Mission tab.
pub const CURRENT_BUTTON: &str = "SBTOC_B_CURRENT";
/// The `T` row whose text is filled at run time.
pub const CHARACTER_ROW: &str = "SBTOC_T_CHARACTER";

/// Missions in the campaign: the cap `E 2104` clamps to and `ispy` lists.
pub const CAMPAIGN_MISSIONS: u32 = 24;
/// The string id mission `n`'s first line is `n` past (`0xd97`).
pub const MISSION_TITLE_BASE: u32 = 0xd97;
/// The header row's first and second lines (`0x4c1`, `0x4c2`).
pub const HEADER_STRINGS: [u32; 2] = [0x4c1, 0x4c2];
/// The header row's icon frame.
pub const HEADER_ICON: u32 = 11;
/// Icon frames in `fc_planeicons.png` (`BK = 12`), stacked vertically.
pub const ICON_FRAMES: u32 = 12;
/// The icon strip.
pub const ICON_ART: &str = "fc_planeicons.png";
/// Text lines in a row start this far down and are this far apart.
pub const LINE_TOP: u32 = 10;
/// The distance between a row's text lines.
pub const LINE_STEP: u32 = 20;
/// Gap between the icon and the text.
pub const ICON_GAP: u32 = 20;
/// The selected row's fill (`setpencolor(0x80f2e7b7)`).
pub const SELECTED_FILL: u32 = 0x80F2_E7B7;
/// The row frame, selected or hovered (`0xffdd9017`).
pub const FRAME: u32 = 0xFFDD_9017;
/// The hovered row's fill (`0x40f2e7b7`).
pub const HOVER_FILL: u32 = 0x40F2_E7B7;
/// The row text colour (`0xff000000`).
pub const ROW_TEXT: u32 = 0xFF00_0000;

/// Why the page could not be read from the layout.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TocError {
    /// The layout has no `ScrapBook_TOC` section.
    NoSection,
    /// A row the page needs is not in the section.
    RowMissing(&'static str),
    /// A column is not the number the row layout states.
    Column {
        /// The row.
        row: &'static str,
        /// The column.
        column: &'static str,
    },
}

impl fmt::Display for TocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no {TOC_SECTION} section"),
            Self::RowMissing(id) => write!(f, "{TOC_SECTION} has no {id} row"),
            Self::Column { row, column } => write!(f, "{row}: the {column} column is unreadable"),
        }
    }
}

impl std::error::Error for TocError {}

/// What the pilot has done, as far as the list needs it. A declared input:
/// the engine table behind it is not written by this project yet.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MissionProgress {
    /// Missions completed (`[0x64b678]`), clamped to the campaign.
    pub completed: u32,
    /// The selected mission (`[0x64b340]`, read by `E 2405(0)`).
    pub selected: u32,
    /// Missions whose record `E 2411` answers replayable.
    pub replayable: Vec<u32>,
    /// Mission records' icon frames (`mission, frame`).
    pub icons: Vec<(u32, u32)>,
    /// Mission records' stored text line (`mission, text`).
    pub stored_text: Vec<(u32, String)>,
}

impl MissionProgress {
    /// A pilot who has completed nothing and stands on the first mission.
    #[must_use]
    pub fn fresh() -> Self {
        Self {
            completed: 0,
            selected: 1,
            replayable: Vec::new(),
            icons: Vec::new(),
            stored_text: Vec::new(),
        }
    }
}

/// One list row: what `E 2409` fills in.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocRow {
    /// The row index, which is the mission number (row 0 is the header).
    pub index: u32,
    /// The `fc_planeicons.png` frame, when the record names one.
    pub icon: Option<u32>,
    /// The three text lines; `None` where the original's source is unread.
    pub lines: [Option<String>; 3],
}

/// A `T` row's caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TocLabel {
    /// The row id.
    pub id: String,
    /// The text.
    pub text: String,
    /// The `[FONT]` tag's size, when it names one.
    pub size: Option<u32>,
    /// The box.
    pub rect: ShellRect,
    /// `0xAARRGGBB`.
    pub color: u32,
    /// 0 left, 1 centre, 2 right.
    pub justify: u32,
}

/// A button's caption.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ButtonCaption {
    /// The button id (upper-cased).
    pub button: String,
    /// The text.
    pub text: String,
    /// The `[FONT]` tag's size, when it names one.
    pub size: Option<u32>,
}

/// What a button press on the page did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TocOutcome {
    /// The press is not the page's.
    Nothing,
    /// Replay with the gate off: ignored.
    ReplayOff,
    /// Replay of a replayable mission: the briefing for it is wanted.
    Replay {
        /// The mission number.
        mission: u32,
    },
    /// View Selected or the Current Mission tab: the journal screen the
    /// original opens is not built.
    JournalNotBuilt {
        /// The button.
        button: &'static str,
    },
}

/// The Previous Missions page.
#[derive(Clone, Debug)]
pub struct PreviousMissionsPage {
    list: ListBox,
    labels: Vec<TocLabel>,
    captions: Vec<ButtonCaption>,
    progress: MissionProgress,
    rows: Vec<TocRow>,
    icon_frame: Option<(u32, u32)>,
    selected: u32,
    top: usize,
    hover: Option<usize>,
    dirty: bool,
}

fn split_tag(raw: &str) -> (Option<u32>, &str) {
    let Some(rest) = raw.strip_prefix('[') else {
        return (None, raw);
    };
    let Some((tag, text)) = rest.split_once(']') else {
        return (None, raw);
    };
    let digits: String = tag.chars().filter(char::is_ascii_digit).collect();
    (digits.parse().ok(), text)
}

fn colour(text: &str) -> Option<u32> {
    let digits = text
        .trim()
        .strip_prefix("0x")
        .or_else(|| text.trim().strip_prefix("0X"))?;
    u32::from_str_radix(digits, 16).ok()
}

impl PreviousMissionsPage {
    /// Reads the `[@ScrapBook_TOC@]` rows. `name` answers an `IDS_*` name and
    /// `id` a string-table id with the installation's text, `[FONT]` tag
    /// included; `measure` the extent of an art name (the icon strip's
    /// frame width places the text).
    ///
    /// # Errors
    ///
    /// [`TocError`] for a missing section or row or an unreadable column.
    pub fn from_layout(
        layout: &OriginalLayout,
        name: &dyn Fn(&str) -> Option<String>,
        id: &dyn Fn(u32) -> Option<String>,
        measure: &dyn Fn(&str) -> Option<(u32, u32)>,
        progress: MissionProgress,
    ) -> Result<Self, TocError> {
        let section = layout.section(TOC_SECTION).ok_or(TocError::NoSection)?;
        let l = section
            .other
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case(TOC_LIST_ROW))
            .ok_or(TocError::RowMissing(TOC_LIST_ROW))?;
        let col = |index: usize, column: &'static str| {
            l.fields
                .get(index)
                .and_then(|value| value.trim().parse::<i32>().ok())
                .ok_or(TocError::Column {
                    row: TOC_LIST_ROW,
                    column,
                })
        };
        let row_height = u32::try_from(col(7, "Height")?).unwrap_or(0);
        let visible = usize::try_from(col(8, "TotalDisplayed")?).unwrap_or(0);
        if row_height == 0 || visible == 0 {
            return Err(TocError::Column {
                row: TOC_LIST_ROW,
                column: "Height",
            });
        }
        let list = ListBox {
            slider: l.fields.first().cloned().unwrap_or_default(),
            up: l.fields.get(1).cloned().unwrap_or_default(),
            down: l.fields.get(2).cloned().unwrap_or_default(),
            rect: ShellRect {
                x: col(3, "X")?,
                y: col(4, "Y")?,
                width: u32::try_from(col(6, "Width")?).unwrap_or(0),
                height: row_height * visible as u32,
            },
            row_height,
            visible,
        };
        let mut labels = Vec::new();
        for row in section
            .other
            .iter()
            .filter(|row| row.kind.eq_ignore_ascii_case("T"))
        {
            let Some(res) = row
                .fields
                .first()
                .map(|field| field.trim())
                .filter(|field| !field.is_empty())
            else {
                // The row with no resource id is filled at run time.
                continue;
            };
            let Some(raw) = name(res) else {
                continue;
            };
            let number = |index: usize| {
                row.fields
                    .get(index)
                    .and_then(|value| value.trim().parse::<i32>().ok())
            };
            let (Some(x), Some(y), Some(width), Some(height), Some(color), Some(justify)) = (
                number(1),
                number(2),
                number(4),
                number(5),
                row.fields.get(6).and_then(|value| colour(value)),
                number(7),
            ) else {
                return Err(TocError::Column {
                    row: "T",
                    column: "X/Y/Width/Height/Color/Justify",
                });
            };
            let (size, text) = split_tag(&raw);
            labels.push(TocLabel {
                id: row.id.clone(),
                text: text.to_owned(),
                size,
                rect: ShellRect {
                    x,
                    y,
                    width: width.max(0) as u32,
                    height: height.max(0) as u32,
                },
                color,
                justify: justify.max(0) as u32,
            });
        }
        let captions = section
            .buttons
            .iter()
            .filter_map(|button| {
                let res = button
                    .fields
                    .iter()
                    .find(|field| field.trim().starts_with("IDS_"))?;
                let raw = name(res.trim())?;
                let (size, text) = split_tag(&raw);
                Some(ButtonCaption {
                    button: button.id.to_ascii_uppercase(),
                    text: text.to_owned(),
                    size,
                })
            })
            .collect();
        let icon_frame = measure(ICON_ART).map(|(width, height)| (width, height / ICON_FRAMES));
        let mut progress = progress;
        progress.completed = progress.completed.min(CAMPAIGN_MISSIONS);
        progress.selected = progress.selected.min(CAMPAIGN_MISSIONS);
        let rows = build_rows(&progress, id);
        Ok(Self {
            list,
            labels,
            captions,
            selected: progress.selected,
            progress,
            rows,
            icon_frame,
            top: 0,
            hover: None,
            dirty: true,
        })
    }

    /// The list box.
    #[must_use]
    pub fn list(&self) -> &ListBox {
        &self.list
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

    /// The list rows: the header, then one per completed mission.
    #[must_use]
    pub fn rows(&self) -> &[TocRow] {
        &self.rows
    }

    /// The selected mission number (`UT`).
    #[must_use]
    pub fn selected(&self) -> u32 {
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

    /// The icon strip's frame size, when the strip was measured.
    #[must_use]
    pub fn icon_frame(&self) -> Option<(u32, u32)> {
        self.icon_frame
    }

    /// Where a row's text starts, from the list's left edge: the icon's width
    /// plus 20 (the script's `EZ`), or the gap alone when the strip is
    /// unmeasured.
    #[must_use]
    pub fn text_offset(&self) -> u32 {
        self.icon_frame.map_or(0, |(width, _)| width) + ICON_GAP
    }

    /// The replay gate (`E 2411`): the selected mission's record is
    /// replayable.
    #[must_use]
    pub fn replay_enabled(&self) -> bool {
        self.selected != 0 && self.progress.replayable.contains(&self.selected)
    }

    /// Whether the model changed since the last call; clears the flag.
    pub fn take_dirty(&mut self) -> bool {
        std::mem::take(&mut self.dirty)
    }

    /// The row under the logical point.
    #[must_use]
    pub fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        if !self.list.rect.contains(x, y) {
            return None;
        }
        let offset = u32::try_from(y - self.list.rect.y).ok()? / self.list.row_height;
        let row = self.top + offset as usize;
        (row < self.rows.len() && (offset as usize) < self.list.visible).then_some(row)
    }

    /// Hover follows the pointer.
    pub fn hover_at(&mut self, point: Option<(i32, i32)>) {
        let row = point.and_then(|(x, y)| self.row_at(x, y));
        if row != self.hover {
            self.hover = row;
            self.dirty = true;
        }
    }

    /// A press on a row (message `10015`): the row index becomes the selected
    /// mission.
    pub fn select_row(&mut self, row: usize) {
        if row < self.rows.len() {
            self.selected = self.rows[row].index;
            self.reveal(row);
            self.dirty = true;
        }
    }

    /// Up/Down: moves the selection one row, clamped.
    pub fn move_selection(&mut self, forward: bool) {
        let last = self.rows.len().saturating_sub(1);
        let current = self.rows.iter().position(|row| row.index == self.selected);
        let row = match (current, forward) {
            (None, true) => 0,
            (None, false) => last,
            (Some(row), true) => (row + 1).min(last),
            (Some(row), false) => row.saturating_sub(1),
        };
        self.select_row(row);
    }

    /// Wheel scroll: moves the first shown row.
    pub fn scroll(&mut self, rows: i32) {
        let max = self.rows.len().saturating_sub(self.list.visible);
        let top = (self.top as i64 + i64::from(rows)).clamp(0, max as i64) as usize;
        if top != self.top {
            self.top = top;
            self.dirty = true;
        }
    }

    fn reveal(&mut self, row: usize) {
        if row < self.top {
            self.top = row;
        } else if row >= self.top + self.list.visible {
            self.top = row + 1 - self.list.visible;
        }
    }

    /// A press on one of the page's buttons.
    pub fn press(&mut self, button: &str) -> TocOutcome {
        if button.eq_ignore_ascii_case(REPLAY_BUTTON) {
            if self.replay_enabled() {
                TocOutcome::Replay {
                    mission: self.selected,
                }
            } else {
                TocOutcome::ReplayOff
            }
        } else if button.eq_ignore_ascii_case(VIEW_BUTTON) {
            TocOutcome::JournalNotBuilt {
                button: VIEW_BUTTON,
            }
        } else if button.eq_ignore_ascii_case(CURRENT_BUTTON) {
            TocOutcome::JournalNotBuilt {
                button: CURRENT_BUTTON,
            }
        } else {
            TocOutcome::Nothing
        }
    }
}

/// The rows `E 2409` produces for `progress`: row 0 is the header, then one
/// per completed mission.
fn build_rows(progress: &MissionProgress, id: &dyn Fn(u32) -> Option<String>) -> Vec<TocRow> {
    let line = |string: u32| id(string).map(|raw| split_tag(&raw).1.to_owned());
    let mut rows = vec![TocRow {
        index: 0,
        icon: Some(HEADER_ICON),
        lines: [line(HEADER_STRINGS[0]), line(HEADER_STRINGS[1]), None],
    }];
    for mission in 1..=progress.completed {
        rows.push(TocRow {
            index: mission,
            icon: progress
                .icons
                .iter()
                .find(|(m, _)| *m == mission)
                .map(|(_, frame)| *frame),
            lines: [
                line(MISSION_TITLE_BASE + mission),
                None,
                progress
                    .stored_text
                    .iter()
                    .find(|(m, _)| *m == mission)
                    .map(|(_, text)| text.clone()),
            ],
        });
    }
    rows
}

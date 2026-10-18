//! The original front-end screen layout, read from the installation's own
//! `ASSETS/LAYOUT.CSV` (task #742 / F45-B.1).
//!
//! # What the original defines, and where
//!
//! The original front end is not a set of full-screen pictures with hotspot
//! rectangles. `GOSDATA/ASSETS/crimson.rof` holds one text member,
//! `ASSETS/LAYOUT.CSV`, whose `[@Name@]` sections (one per `ASSETS/SCRIPTS`
//! screen script) list the screen's **panes** (art at an `X,Y`), **buttons**
//! (art at an `X,Y`, the script they start) and its other controls. A button
//! has no stored size: its region is its art's own extent, and the art is a
//! vertical strip of equal state frames. This module reads that file; it
//! invents nothing the file does not state:
//!
//! * [`OriginalLayout::parse`] reads the sections, the `[GLOBALVARS]` and the
//!   per-section `V#=NAME,value` variables and substitutes `<NAME>`
//!   placeholders. A placeholder that no variable defines is **kept as
//!   unresolved** on the row, never replaced by a number.
//! * [`OriginalLayout::screen`] turns one section into the authored-screen
//!   assets the F45-B deck consumes ([`ScreenAssets`]), or says exactly why it
//!   cannot ([`ScreenRefusal`]) and which original buttons it left out
//!   ([`UnboundButton`]).
//!
//! # The two designed decisions (not original facts)
//!
//! * [`SCREEN_SECTIONS`] pairs a section with the F45-A [`Screen`] it is the
//!   original of, **by name**. It is a design choice, listed so a reader can
//!   disagree; no original file states it.
//! * A button's [`Action`] is **derived**, not assigned: the section's own
//!   `ScriptToExe` names the screen script the button starts, and the button
//!   is bound only when that script is a paired section and the state table
//!   has exactly one user row from this screen to the paired one. Every other
//!   button is reported as unbound with the reason. A button the original
//!   draws but the designed table has no action for is therefore *visible in
//!   the report and absent from the hotspots*, which is exactly what the deck's
//!   [`DeckError::HiddenAction`] check then measures from the other side.
//!
//! # Still unknown (recorded in `docs/findings/2026-10-15-f45-b-1-original-layout.md`)
//!
//! The file's own header comment lists `TabOrder` and `HelpID` columns for a
//! button, but no data row carries them, so the original focus/tab order is
//! **not** in the data; focus stays the declaration order. That a button's art
//! is [`BUTTON_STATE_FRAMES`] frames is read from the art, not from the file.

use std::collections::BTreeMap;
use std::fmt;

use cs_content::ui_layout::{Hotspot, LayoutError, Rect, ScreenLayout};
use cs_types::content::{ContentId, ContentKind};

use super::{Action, ActionSource, Screen, rows_on};
use super::{DeckError, ScreenAssetError, ScreenAssets, ScreenDeck};

/// The layout member's spelling inside the screen container
/// ([`super::SCREEN_CONTAINER`]).
pub const LAYOUT_MEMBER: &str = "ASSETS/LAYOUT.CSV";

/// The number of equal vertical state frames a button's art carries: the four
/// colour sets the file's own header names (`ColorSet=0-3`, with the globals
/// `DISABLED`, `ACTIVE`, `ROLLOVER`, `DEPRESSED`). Measured on the main-menu,
/// cabin and flight-check button art; an art whose height this does not
/// divide, or whose name says `8STATES`, is refused instead of guessed.
pub const BUTTON_STATE_FRAMES: u32 = 4;

/// The sections the designed front end pairs with an original section, by
/// name. A designed decision, not an original fact (see the module docs).
pub const SCREEN_SECTIONS: &[(&str, Screen)] = &[
    ("MainMenu", Screen::MainMenu),
    ("Preferences", Screen::Settings),
    ("PassengerCabin", Screen::Cabin),
    ("ScrapBook_TOC", Screen::Scrapbook),
    ("FlightCheck", Screen::FlightCheck),
    ("PlaneConstruction", Screen::Construction),
];

/// The measured extent of an art file, by the name the layout writes.
pub trait ArtExtents {
    /// The `(width, height)` of the art, or `None` when it cannot be read.
    fn extent(&self, art: &str) -> Option<(u32, u32)>;
}

impl<F: Fn(&str) -> Option<(u32, u32)>> ArtExtents for F {
    fn extent(&self, art: &str) -> Option<(u32, u32)> {
        self(art)
    }
}

/// One `ID=KIND,fields…` row with its placeholders substituted.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Row {
    /// The row id as written, upper-cased.
    pub id: String,
    /// The row's kind letter, upper-cased (`P`, `B`, `T`, `A`, `D`, `S`, `M`,
    /// …). The header comment names the kinds; a row's own kind is the letter
    /// before its first comma.
    pub kind: String,
    /// The fields after the kind letter, trimmed, placeholders substituted.
    pub fields: Vec<String>,
    /// The names of the `<NAME>` placeholders no variable defined.
    pub unresolved: Vec<String>,
}

impl Row {
    fn field(&self, index: usize) -> &str {
        self.fields.get(index).map_or("", String::as_str)
    }

    fn number(&self, index: usize) -> Option<u32> {
        self.field(index).parse().ok()
    }
}

/// One `[@Name@]` section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Section {
    /// The section name as written.
    pub name: String,
    /// `P` rows in declaration order.
    pub panes: Vec<Row>,
    /// `B` rows in declaration order.
    pub buttons: Vec<Row>,
    /// Every other row (text, text list, dropdown, scrolling text, movie, …),
    /// in declaration order. [`Self::panes`] and [`Self::buttons`] hold the two
    /// kinds this loader builds screens from; the rest are kept so a later
    /// measurement can read the screen a control starts *and what that screen
    /// holds* (F45-C.5) without re-parsing the file.
    pub other: Vec<Row>,
}

/// A parsed `LAYOUT.CSV`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OriginalLayout {
    sections: Vec<Section>,
}

/// Why the layout text could not be read at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutTextError {
    /// A row appears before any section.
    RowOutsideSection {
        /// The 1-based line.
        line: usize,
    },
    /// A line is neither a comment, a section nor `ID=VALUE`.
    Malformed {
        /// The 1-based line.
        line: usize,
    },
    /// Two sections share a name.
    DuplicateSection {
        /// The repeated name.
        name: String,
    },
}

impl fmt::Display for LayoutTextError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::RowOutsideSection { line } => write!(f, "line {line}: a row outside a section"),
            Self::Malformed { line } => write!(f, "line {line}: not a comment, section or row"),
            Self::DuplicateSection { name } => write!(f, "section {name} is declared twice"),
        }
    }
}

impl std::error::Error for LayoutTextError {}

impl OriginalLayout {
    /// Parses the layout text.
    ///
    /// # Errors
    ///
    /// [`LayoutTextError`] for a row outside any section, a line that is not
    /// a comment, section or `ID=VALUE`, or a repeated section name.
    pub fn parse(text: &str) -> Result<Self, LayoutTextError> {
        let mut globals: BTreeMap<String, String> = BTreeMap::new();
        let mut in_globals = false;
        let mut sections: Vec<(Section, BTreeMap<String, String>)> = Vec::new();

        for (index, raw) in text.lines().enumerate() {
            let line = index + 1;
            let trimmed = raw.trim();
            if trimmed.is_empty() || trimmed.starts_with(';') || trimmed.starts_with(':') {
                continue;
            }
            if let Some(inner) = trimmed.strip_prefix('[').and_then(|s| s.strip_suffix(']')) {
                if let Some(name) = inner.strip_prefix('@').and_then(|s| s.strip_suffix('@')) {
                    if sections
                        .iter()
                        .any(|(s, _)| s.name.eq_ignore_ascii_case(name))
                    {
                        return Err(LayoutTextError::DuplicateSection {
                            name: name.to_owned(),
                        });
                    }
                    in_globals = false;
                    sections.push((
                        Section {
                            name: name.to_owned(),
                            panes: Vec::new(),
                            buttons: Vec::new(),
                            other: Vec::new(),
                        },
                        BTreeMap::new(),
                    ));
                } else if inner.eq_ignore_ascii_case("GLOBALVARS") {
                    in_globals = true;
                } else {
                    return Err(LayoutTextError::Malformed { line });
                }
                continue;
            }
            let Some((id, value)) = trimmed.split_once('=') else {
                return Err(LayoutTextError::Malformed { line });
            };
            let id = id.trim();
            let is_variable = id.len() > 1
                && matches!(id.as_bytes()[0], b'V' | b'G' | b'v' | b'g')
                && id[1..].bytes().all(|b| b.is_ascii_digit());
            if is_variable {
                let Some((name, value)) = value.split_once(',') else {
                    return Err(LayoutTextError::Malformed { line });
                };
                let target = if in_globals {
                    &mut globals
                } else {
                    let Some((_, vars)) = sections.last_mut() else {
                        return Err(LayoutTextError::RowOutsideSection { line });
                    };
                    vars
                };
                target.insert(name.trim().to_ascii_uppercase(), value.trim().to_owned());
                continue;
            }
            if in_globals {
                return Err(LayoutTextError::Malformed { line });
            }
            let Some((section, vars)) = sections.last_mut() else {
                return Err(LayoutTextError::RowOutsideSection { line });
            };
            let mut parts = value.split(',');
            let kind = parts.next().unwrap_or("").trim().to_ascii_uppercase();
            let mut unresolved = Vec::new();
            let fields: Vec<String> = parts
                .map(|field| substitute(field.trim(), vars, &globals, &mut unresolved))
                .collect();
            let row = Row {
                id: id.to_ascii_uppercase(),
                kind: kind.clone(),
                fields,
                unresolved,
            };
            match kind.as_str() {
                "P" => section.panes.push(row),
                "B" => section.buttons.push(row),
                _ => section.other.push(row),
            }
        }
        Ok(Self {
            sections: sections.into_iter().map(|(section, _)| section).collect(),
        })
    }

    /// The sections in file order.
    #[must_use]
    pub fn sections(&self) -> &[Section] {
        &self.sections
    }

    /// The section with this name (ASCII case-insensitive).
    #[must_use]
    pub fn section(&self, name: &str) -> Option<&Section> {
        self.sections
            .iter()
            .find(|section| section.name.eq_ignore_ascii_case(name))
    }

    /// Reads one paired section into assets, or reports why it cannot.
    ///
    /// The result always lists every button of the section: those that became
    /// hotspots in [`ScreenReport::bound`] and the rest, with their reason, in
    /// [`ScreenReport::unbound`].
    #[must_use]
    pub fn screen(&self, screen: Screen, extents: &dyn ArtExtents) -> ScreenReport {
        self.screen_with_scripts(screen, extents, &NoScripts)
    }

    /// As [`Self::screen`], also reading the handlers of the buttons that
    /// start no screen script from the screen's own script (task #1456).
    #[must_use]
    pub fn screen_with_scripts(
        &self,
        screen: Screen,
        extents: &dyn ArtExtents,
        scripts: &dyn ScreenScripts,
    ) -> ScreenReport {
        let Some(&(section_name, _)) = SCREEN_SECTIONS.iter().find(|(_, s)| *s == screen) else {
            return ScreenReport::empty(screen, "", ScreenRefusal::ScreenNotPaired);
        };
        let Some(section) = self.section(section_name) else {
            return ScreenReport::empty(screen, section_name, ScreenRefusal::SectionMissing);
        };
        let mut report = ScreenReport::empty(screen, &section.name, ScreenRefusal::NoFullPane);
        let handlers = scripts
            .script(&section.name)
            .map(|text| ScreenHandlers::parse(&text));
        report.handlers = handlers;

        // The screen's art: the one single-frame pane placed at the origin.
        let mut candidates = Vec::new();
        for pane in &section.panes {
            let art = pane.field(0);
            if art.is_empty()
                || !pane.unresolved.is_empty()
                || pane.number(1) != Some(0)
                || pane.number(2) != Some(0)
                || pane.number(4) != Some(1)
            {
                continue;
            }
            candidates.push((pane, art));
        }
        let art = match candidates.as_slice() {
            [] => {
                report.result = Err(ScreenRefusal::NoFullPane);
                report.classify_buttons(section, None, extents);
                return report;
            }
            [one] => *one,
            many => {
                report.result = Err(ScreenRefusal::AmbiguousFullPane {
                    panes: many.iter().map(|(pane, _)| pane.id.clone()).collect(),
                });
                report.classify_buttons(section, None, extents);
                return report;
            }
        };
        let Some(image) = extents.extent(art.1) else {
            report.result = Err(ScreenRefusal::ArtUnread {
                art: art.1.to_owned(),
            });
            report.classify_buttons(section, None, extents);
            return report;
        };
        report.art_name = Some(art.1.to_owned());
        report.classify_buttons(section, Some(image), extents);

        let art_id = match ContentId::from_source(ContentKind::Image, &art_key(art.1)) {
            Ok(id) => id,
            Err(_) => {
                report.result = Err(ScreenRefusal::ArtUnread {
                    art: art.1.to_owned(),
                });
                return report;
            }
        };
        let hotspots = report
            .bound
            .iter()
            .map(|bound| Hotspot {
                id: bound.id.clone(),
                action: bound.action.key().to_owned(),
                rect: bound.rect,
            })
            .collect();
        report.result = match ScreenLayout::new(image, hotspots) {
            Err(error) => Err(ScreenRefusal::Layout(error)),
            Ok(layout) => ScreenAssets::new(art_id, &layout).map_err(ScreenRefusal::Assets),
        };
        report
    }
}

/// The text of a screen's own script, by the section name.
pub trait ScreenScripts {
    /// The decoded script text of the section (e.g. `MainMenu` →
    /// `ASSETS/SCRIPTS/MAINMENU.SCRIPT`), or `None` when it cannot be read.
    fn script(&self, section: &str) -> Option<String>;
}

impl<F: Fn(&str) -> Option<String>> ScreenScripts for F {
    fn script(&self, section: &str) -> Option<String> {
        self(section)
    }
}

/// No scripts are available.
struct NoScripts;

impl ScreenScripts for NoScripts {
    fn script(&self, _section: &str) -> Option<String> {
        None
    }
}

/// The member a section's script is stored under.
#[must_use]
pub fn script_member(section: &str) -> String {
    format!("ASSETS/SCRIPTS/{}.SCRIPT", section.to_ascii_uppercase())
}

/// The plane slot a flight-check control edits (F45-C.5).
///
/// Measured from the screen script itself, not assigned: in
/// `ASSETS/SCRIPTS/FLIGHTCHECK.SCRIPT` the pilot's plane picture is
/// initialised with the native call `callback($$E$$, 2010, -1)` and the
/// wingmate's with `-2`, and the controls that edit a plane store
/// `@globals@ZQ = -1` (pilot) or `-2` (wingmate). `ORDINANCELAYOUT.SCRIPT`
/// then reads `@globals@ZQ` to pick the plane it edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FlightCheckSlot {
    /// The pilot's own plane (`@globals@ZQ = -1`).
    Player,
    /// The wingmate's plane (`@globals@ZQ = -2`).
    Wingmate,
}

/// One `B` row of the original `FlightCheck` section: the control, the screen
/// its `ScriptToExe` column starts, and the plane slot its handler selects.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightCheckControl {
    /// The layout row id, upper-cased as written.
    pub id: String,
    /// The section the control starts, as written.
    pub starts: String,
    /// The slot the control edits, read from its arm in the screen script.
    /// `None` when its arm states none.
    pub slot: Option<FlightCheckSlot>,
}

/// What the original flight check's own section and controls state (F45-C.5),
/// measured from `ASSETS/LAYOUT.CSV` and the screen scripts.
///
/// This is a **measurement**, not a designed selection rule: the controls and
/// where they go come from the layout's `ScriptToExe` column, the slot each
/// edits comes from the screen script's own `@globals@ZQ` state, and the
/// selectors come from the destination sections' own rows. It deliberately
/// says nothing about *which* aircraft or loadouts are offered: on this
/// installation the flight check's own section holds no selector row at all,
/// and the screens its buttons start fill their lists from scripting natives
/// (`callback($$E$$, 2018/2019/2020, …)`, `callback($$E$$, 2027/2028, …)`)
/// whose tables are not readable. No per-mission option list is read here
/// because the original data states none.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightCheckSelection {
    /// Every control of the section that starts a screen, in declaration
    /// order.
    pub controls: Vec<FlightCheckControl>,
    /// The section's own selector rows: its rows of kind `D` (a dropdown is
    /// the one row kind that offers a choice). Empty on this installation.
    pub selectors: Vec<String>,
    /// Every section a control starts, once each in first-seen order, with
    /// that section's own selector rows.
    pub destinations: Vec<(String, Vec<String>)>,
}

impl FlightCheckSelection {
    /// Whether the measurement places a control on `slot` that starts a screen
    /// holding selectors — that slot's plane is editable there.
    ///
    /// The designed projection of the flight check's ammunition onto a
    /// wingmate's equipment is supported only where the original's own data
    /// offers that slot a control (F45-C.5).
    #[must_use]
    pub fn edits(&self, slot: FlightCheckSlot) -> bool {
        self.controls.iter().any(|control| {
            control.slot == Some(slot)
                && self.destinations.iter().any(|(starts, selectors)| {
                    starts.eq_ignore_ascii_case(&control.starts) && !selectors.is_empty()
                })
        })
    }
}

/// Why the original flight check's selection could not be measured.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlightCheckSelectionError {
    /// The layout has no `FlightCheck` section.
    NoSection,
}

impl fmt::Display for FlightCheckSelectionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => f.write_str("the layout has no FlightCheck section"),
        }
    }
}

impl std::error::Error for FlightCheckSelectionError {}

impl OriginalLayout {
    /// Measures the original flight check's own controls and the option
    /// surfaces they reach (F45-C.5): its `B` rows, the screen each starts,
    /// the plane slot its screen-script arm selects, and the selector rows of
    /// both the section itself and the sections it starts.
    ///
    /// # Errors
    ///
    /// [`FlightCheckSelectionError::NoSection`] when the layout has no
    /// `FlightCheck` section.
    pub fn flight_check_selection(
        &self,
        scripts: &dyn ScreenScripts,
    ) -> Result<FlightCheckSelection, FlightCheckSelectionError> {
        let section = self
            .section("FlightCheck")
            .ok_or(FlightCheckSelectionError::NoSection)?;
        let handlers = scripts
            .script("FlightCheck")
            .map(|text| ScreenHandlers::parse(&text));
        let mut controls = Vec::new();
        let mut destinations: Vec<(String, Vec<String>)> = Vec::new();
        for button in &section.buttons {
            // Fields after the kind letter: art, X, Y, Z, ResID, ScriptToExe, …
            let starts = button.field(5);
            if starts.is_empty() {
                continue;
            }
            controls.push(FlightCheckControl {
                id: button.id.clone(),
                starts: starts.to_owned(),
                slot: handlers
                    .as_ref()
                    .and_then(|handlers| handlers.handler(&button.id))
                    .and_then(selected_slot),
            });
            if let Some(target) = self.section(starts)
                && !destinations
                    .iter()
                    .any(|(name, _)| name.eq_ignore_ascii_case(starts))
            {
                destinations.push((starts.to_owned(), selectors(target)));
            }
        }
        Ok(FlightCheckSelection {
            controls,
            selectors: selectors(section),
            destinations,
        })
    }
}

/// The selector (dropdown) row ids of a section, in declaration order.
fn selectors(section: &Section) -> Vec<String> {
    section
        .other
        .iter()
        .filter(|row| row.kind == "D")
        .map(|row| row.id.clone())
        .collect()
}

/// The plane slot a handler's statements select, from the last `@globals@ZQ`
/// assignment in them: `-1` is the pilot's plane and `-2` the wingmate's.
fn selected_slot(statements: &[String]) -> Option<FlightCheckSlot> {
    statements.iter().rev().find_map(|statement| {
        let (name, value) = statement.split_once('=')?;
        if !name.trim().eq_ignore_ascii_case("@globals@ZQ") {
            return None;
        }
        match value.trim() {
            "-1" => Some(FlightCheckSlot::Player),
            "-2" => Some(FlightCheckSlot::Wingmate),
            _ => None,
        }
    })
}

/// The button handlers of one screen script: for each button control the
/// script creates, the statements of its `case` arm in `gui_mailbox`.
///
/// This is a line reader, not a script interpreter. An arm ends at the first
/// `break`, next `case` or closing brace; a run of `case` labels shares the
/// arm that follows. Anything it cannot place yields no handler.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ScreenHandlers {
    /// Lower-cased button id → the statements of its arm.
    handlers: BTreeMap<String, Vec<String>>,
    /// Lower-cased ids of the buttons the script creates.
    created: Vec<String>,
}

impl ScreenHandlers {
    /// Reads the handlers out of a script's text.
    #[must_use]
    pub fn parse(text: &str) -> Self {
        // handle -> button id, from `object X = @ctl@BE` + `X.YC = "id"`.
        let mut buttons: BTreeMap<String, String> = BTreeMap::new();
        let mut pending: Vec<String> = Vec::new();
        let mut in_mailbox = false;
        let mut result = Self::default();
        let mut labels: Vec<String> = Vec::new();
        let mut arm: Vec<String> = Vec::new();
        let mut open = false;

        let close = |result: &mut Self,
                     labels: &mut Vec<String>,
                     arm: &mut Vec<String>,
                     open: &mut bool,
                     buttons: &BTreeMap<String, String>| {
            if *open {
                for label in labels.iter() {
                    if let Some(id) = buttons.get(label) {
                        result
                            .handlers
                            .entry(id.clone())
                            .or_insert_with(|| arm.clone());
                    }
                }
            }
            labels.clear();
            arm.clear();
            *open = false;
        };

        for raw in text.lines() {
            let line = raw.trim();
            if line.is_empty() {
                continue;
            }
            if !in_mailbox {
                if line == "gui_mailbox" {
                    in_mailbox = true;
                } else if let Some(rest) = line.strip_prefix("object ")
                    && let Some((handle, kind)) = rest.split_once('=')
                    && kind.trim() == "@ctl@BE"
                {
                    pending.push(handle.trim().to_owned());
                } else if let Some((handle, value)) = line.split_once(".YC")
                    && pending.iter().any(|p| p == handle.trim())
                    && let Some(id) = value.trim().strip_prefix('=').map(str::trim)
                {
                    let id = id.trim_matches('"').to_ascii_lowercase();
                    buttons.insert(handle.trim().to_owned(), id.clone());
                    result.created.push(id);
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("case ") {
                let (label, tail) = match rest.split_once(':') {
                    Some((label, tail)) => (label.trim(), tail.trim()),
                    None => (rest.trim(), ""),
                };
                if open && !arm.is_empty() {
                    close(&mut result, &mut labels, &mut arm, &mut open, &buttons);
                }
                labels.push(label.to_owned());
                open = true;
                if !tail.is_empty() {
                    arm.push(tail.to_owned());
                }
                continue;
            }
            if line == "break" || line == "}" {
                close(&mut result, &mut labels, &mut arm, &mut open, &buttons);
                continue;
            }
            if open {
                arm.push(line.to_owned());
            }
        }
        result
    }

    /// The statements of the arm of the button with this layout row id
    /// (ASCII case-insensitive).
    #[must_use]
    pub fn handler(&self, button: &str) -> Option<&[String]> {
        self.handlers
            .get(&button.to_ascii_lowercase())
            .map(Vec::as_slice)
    }

    /// The ids of the buttons the script creates, in creation order.
    #[must_use]
    pub fn buttons(&self) -> &[String] {
        &self.created
    }
}

/// The lower-case key an art file name is known by: its stem.
fn art_key(art: &str) -> String {
    let stem = art.rsplit_once('.').map_or(art, |(stem, _)| stem);
    stem.to_ascii_lowercase()
}

/// Replaces every `<NAME>` of `field`; an undefined name stays in place and is
/// recorded.
fn substitute(
    field: &str,
    section: &BTreeMap<String, String>,
    globals: &BTreeMap<String, String>,
    unresolved: &mut Vec<String>,
) -> String {
    let mut out = String::with_capacity(field.len());
    let mut rest = field;
    while let Some(open) = rest.find('<') {
        let Some(close) = rest[open..].find('>') else {
            break;
        };
        let name = &rest[open + 1..open + close];
        out.push_str(&rest[..open]);
        match section
            .get(&name.to_ascii_uppercase())
            .or_else(|| globals.get(&name.to_ascii_uppercase()))
        {
            Some(value) => out.push_str(value),
            None => {
                unresolved.push(name.to_owned());
                out.push_str(&rest[open..=open + close]);
            }
        }
        rest = &rest[open + close + 1..];
    }
    out.push_str(rest);
    out
}

/// Why a section did not become screen assets.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScreenRefusal {
    /// The designed screen has no paired original section.
    ScreenNotPaired,
    /// The layout has no section of the paired name.
    SectionMissing,
    /// No single-frame pane sits at the origin, so the section names no
    /// screen-sized artwork. Nothing else is promoted to be the art.
    NoFullPane,
    /// More than one pane qualifies; which is the screen's art is unknown.
    AmbiguousFullPane {
        /// The competing pane ids.
        panes: Vec<String>,
    },
    /// The art's extent could not be read.
    ArtUnread {
        /// The art file as the layout writes it.
        art: String,
    },
    /// The hotspots do not form a layout.
    Layout(LayoutError),
    /// The layout does not form screen assets.
    Assets(ScreenAssetError),
}

impl fmt::Display for ScreenRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScreenNotPaired => write!(f, "no original section is paired with the screen"),
            Self::SectionMissing => write!(f, "the layout has no such section"),
            Self::NoFullPane => write!(f, "no single-frame pane at the origin: no screen art"),
            Self::AmbiguousFullPane { panes } => {
                write!(f, "several panes could be the art: {}", panes.join(", "))
            }
            Self::ArtUnread { art } => write!(f, "the art {art} could not be measured"),
            Self::Layout(error) => write!(f, "{error}"),
            Self::Assets(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ScreenRefusal {}

/// A button that became a hotspot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BoundButton {
    /// Its `ui-resource` id (the original row id, lower-cased).
    pub id: ContentId,
    /// The designed action derived from its script.
    pub action: Action,
    /// Its region in logical image pixels.
    pub rect: Rect,
    /// The script it starts, as written.
    pub script: String,
}

/// Why an original button is not a hotspot.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UnboundReason {
    /// A field the row needs holds a placeholder no variable defines.
    UnresolvedPlaceholder {
        /// The placeholder name.
        name: String,
    },
    /// `X` or `Y` is not a number.
    PositionUnread,
    /// The button's art extent could not be read.
    ArtUnread,
    /// The art's frame count is not [`BUTTON_STATE_FRAMES`].
    FrameCountUnknown {
        /// The art's height.
        height: u32,
    },
    /// The button starts no screen script and the screen's own script was not
    /// read (see [`OriginalLayout::screen_with_scripts`]).
    NoScript,
    /// The screen script was read and has no handler for this button.
    NoHandler,
    /// The button's handler in the screen script states something other than
    /// a bindable transition. The statements are kept as written; what their
    /// natives do is unknown.
    HandlerNotATransition {
        /// The handler's statements, trimmed, in order.
        statements: Vec<String>,
    },
    /// The script it starts is not a paired section.
    ScriptNotPaired {
        /// The script name.
        script: String,
    },
    /// The state table has no, or several, user rows from this screen to the
    /// paired screen the script names.
    NoUniqueTransition {
        /// The paired screen.
        to: Screen,
        /// How many user rows lead there.
        rows: usize,
    },
    /// The script is the screen itself (a button that stays put).
    SelfScript,
    /// The region leaves the screen art.
    OutsideArt,
}

/// An original button the report left out of the hotspots, with its reason.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct UnboundButton {
    /// The original row id.
    pub id: String,
    /// Why it is not a hotspot.
    pub reason: UnboundReason,
}

/// One designed screen read from the original layout.
#[derive(Clone, Debug)]
pub struct ScreenReport {
    /// The designed screen.
    pub screen: Screen,
    /// The paired section name (empty when none is paired).
    pub section: String,
    /// The art file the layout names for the screen, when one was found and
    /// measured.
    pub art_name: Option<String>,
    /// The assets, or why there are none.
    pub result: Result<ScreenAssets, ScreenRefusal>,
    /// Buttons that became hotspots, in declaration order.
    pub bound: Vec<BoundButton>,
    /// Buttons that did not, in declaration order.
    pub unbound: Vec<UnboundButton>,
    /// The handlers read from the screen's own script, when it was supplied.
    pub handlers: Option<ScreenHandlers>,
}

impl ScreenReport {
    fn empty(screen: Screen, section: &str, refusal: ScreenRefusal) -> Self {
        Self {
            screen,
            section: section.to_owned(),
            art_name: None,
            result: Err(refusal),
            bound: Vec::new(),
            unbound: Vec::new(),
            handlers: None,
        }
    }

    fn classify_buttons(
        &mut self,
        section: &Section,
        image: Option<(u32, u32)>,
        extents: &dyn ArtExtents,
    ) {
        let handlers = self.handlers.take();
        for button in &section.buttons {
            match bind_button(self.screen, button, image, extents, handlers.as_ref()) {
                Ok(bound) => self.bound.push(bound),
                Err(reason) => self.unbound.push(UnboundButton {
                    id: button.id.clone(),
                    reason,
                }),
            }
        }
        self.handlers = handlers;
    }
}

fn bind_button(
    screen: Screen,
    button: &Row,
    image: Option<(u32, u32)>,
    extents: &dyn ArtExtents,
    handlers: Option<&ScreenHandlers>,
) -> Result<BoundButton, UnboundReason> {
    // Fields after the kind letter: art, X, Y, Z, ResID, ScriptToExe, …
    if let Some(name) = button.unresolved.first() {
        return Err(UnboundReason::UnresolvedPlaceholder { name: name.clone() });
    }
    let (Some(x), Some(y)) = (button.number(1), button.number(2)) else {
        return Err(UnboundReason::PositionUnread);
    };
    let art = button.field(0);
    let (width, height) = extents.extent(art).ok_or(UnboundReason::ArtUnread)?;
    if height == 0
        || height % BUTTON_STATE_FRAMES != 0
        || art_key(art).contains("8states")
        || width == 0
    {
        return Err(UnboundReason::FrameCountUnknown { height });
    }
    let rect = Rect {
        x,
        y,
        width,
        height: height / BUTTON_STATE_FRAMES,
    };
    if let Some((iw, ih)) = image
        && (u64::from(x) + u64::from(rect.width) > u64::from(iw)
            || u64::from(y) + u64::from(rect.height) > u64::from(ih))
    {
        return Err(UnboundReason::OutsideArt);
    }
    let script = button.field(5);
    if script.is_empty() {
        let Some(handlers) = handlers else {
            return Err(UnboundReason::NoScript);
        };
        let Some(statements) = handlers.handler(&button.id) else {
            return Err(UnboundReason::NoHandler);
        };
        if statements != ["terminate"] {
            return Err(UnboundReason::HandlerNotATransition {
                statements: statements.to_vec(),
            });
        }
        let quits: Vec<Action> = rows_on(screen)
            .filter(|row| row.action == Action::Quit && row.action.source() == ActionSource::User)
            .map(|row| row.action)
            .collect();
        let [action] = quits.as_slice() else {
            return Err(UnboundReason::NoUniqueTransition {
                to: screen,
                rows: quits.len(),
            });
        };
        let id = ContentId::from_source(ContentKind::UiResource, &button.id.to_ascii_lowercase())
            .map_err(|_| UnboundReason::PositionUnread)?;
        return Ok(BoundButton {
            id,
            action: *action,
            rect,
            script: "terminate".to_owned(),
        });
    }
    let Some(&(_, to)) = SCREEN_SECTIONS
        .iter()
        .find(|(name, _)| name.eq_ignore_ascii_case(script))
    else {
        return Err(UnboundReason::ScriptNotPaired {
            script: script.to_owned(),
        });
    };
    if to == screen {
        return Err(UnboundReason::SelfScript);
    }
    let leading: Vec<Action> = rows_on(screen)
        .filter(|row| row.to == to && row.action.source() == ActionSource::User)
        .map(|row| row.action)
        .collect();
    let [action] = leading.as_slice() else {
        return Err(UnboundReason::NoUniqueTransition {
            to,
            rows: leading.len(),
        });
    };
    let action = *action;
    let id = ContentId::from_source(ContentKind::UiResource, &button.id.to_ascii_lowercase())
        .map_err(|_| UnboundReason::PositionUnread)?;
    Ok(BoundButton {
        id,
        action,
        rect,
        script: script.to_owned(),
    })
}

/// Every paired screen read from one layout, and the deck the screens that
/// pass the state-table validation form.
#[derive(Clone, Debug)]
pub struct OriginalScreens {
    reports: Vec<ScreenReport>,
    verdicts: BTreeMap<Screen, Result<(), DeckError>>,
    deck: ScreenDeck,
}

impl OriginalScreens {
    /// Reads every screen of [`SCREEN_SECTIONS`] and validates each one that
    /// produced assets as a one-screen deck against the state table.
    ///
    /// A screen whose assets the deck refuses (a dead button, no escape, a
    /// hidden designed button) keeps its report and its refusal in
    /// [`Self::verdict`] and is **not** in [`Self::deck`]; nothing is relaxed
    /// to make it fit.
    #[must_use]
    pub fn build(layout: &OriginalLayout, extents: &dyn ArtExtents) -> Self {
        Self::build_with_scripts(layout, extents, &NoScripts)
    }

    /// As [`Self::build`], also reading the handlers of the buttons that
    /// start no screen script from the screens' own scripts.
    #[must_use]
    pub fn build_with_scripts(
        layout: &OriginalLayout,
        extents: &dyn ArtExtents,
        scripts: &dyn ScreenScripts,
    ) -> Self {
        let mut reports = Vec::new();
        let mut verdicts = BTreeMap::new();
        let mut accepted = Vec::new();
        for &(_, screen) in SCREEN_SECTIONS {
            let report = layout.screen_with_scripts(screen, extents, scripts);
            if let Ok(assets) = &report.result {
                match ScreenDeck::new(vec![(screen, assets.clone())]) {
                    Ok(_) => {
                        verdicts.insert(screen, Ok(()));
                        accepted.push((screen, assets.clone()));
                    }
                    Err(error) => {
                        verdicts.insert(screen, Err(error));
                    }
                }
            }
            reports.push(report);
        }
        // Each accepted screen passed alone and the deck rules are per screen,
        // so the union is valid; an empty deck is a valid deck of no screens.
        let deck = ScreenDeck::new(accepted).unwrap_or_else(|_| {
            ScreenDeck::new(Vec::new()).unwrap_or_else(|_| unreachable!("an empty deck is valid"))
        });
        Self {
            reports,
            verdicts,
            deck,
        }
    }

    /// The report of every paired screen, in [`SCREEN_SECTIONS`] order.
    #[must_use]
    pub fn reports(&self) -> &[ScreenReport] {
        &self.reports
    }

    /// The report of one screen.
    #[must_use]
    pub fn report(&self, screen: Screen) -> Option<&ScreenReport> {
        self.reports.iter().find(|report| report.screen == screen)
    }

    /// The deck's verdict on a screen that produced assets: `Ok` when it is in
    /// the deck, the refusal otherwise. `None` when no assets were produced.
    #[must_use]
    pub fn verdict(&self, screen: Screen) -> Option<&Result<(), DeckError>> {
        self.verdicts.get(&screen)
    }

    /// The deck of the screens the original data supports.
    #[must_use]
    pub fn deck(&self) -> &ScreenDeck {
        &self.deck
    }
}

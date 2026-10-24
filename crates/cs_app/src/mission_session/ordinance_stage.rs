//! The Flight Check's Change Ammo buttons open the `OrdinanceLayout` screen
//! (task #2070, follow-up to #2062 / #2066): `FC_B_CHANGEAMMO` (`ZQ = -1`, the
//! pilot's plane) and `FC_B_CHANGEAMMOW` (`ZQ = -2`, the wingmate's) show the
//! screen over the Flight Check, fed from the plane's record
//! ([`ordinance_feed`]), and the stores it makes are applied to a draft that
//! the accept button commits.
//!
//! # What the script says (static readings, `ORDINANCELAYOUT.SCRIPT`)
//!
//! * The Flight Check sets `ZQ` only; `XQ` (the script's `DKA`) is set by the
//!   Campaign page and the debrief journal, so it is `0` on this entry.
//! * Accept calls `E 2034` and, with `DKA == 0`, `callback($$A$$, 12, 0)`,
//!   which leaves the screen. Cancel acts only with `DKA != 0`, so here it
//!   does nothing. Escape is the pre-mission phase's way back
//!   ([`FlightCheckPhase::press_back`]): it closes an open list first, then
//!   leaves the screen **without** committing the draft.
//!
//! # What is not known
//!
//! * The campaign state the rocket table's gate reads (`0x647b5c`,
//!   `0x647b68`, `0x647b6c`, `0x64b678`); [`ProgressGate::default`] (nothing
//!   unlocked, progress `0`) stands in for it. Affected: which rockets the
//!   rocket lists offer (`docs/findings/2026-10-24-parity5-ordinance-feed.md`).
//! * Any consumer of the committed record: the mission start does not read
//!   it yet (the committed record is kept on the phase,
//!   [`FlightCheckPhase::ordinance_record`]).
//! * The description texts, the plane-info line and the diagrams, as for the
//!   screen itself (`docs/findings/2026-10-24-parity5-ordinance-layout-screen.md`).

use std::collections::BTreeMap;

use cs_content::ui_layout::Rect;

use super::flight_check::{FlightCheckPhaseError, FlightCheckStrings};
use crate::engine_facts::{FlightCheckPlaneSlot, engine_facts};
use crate::ui::front_end::ordinance_feed::{
    OrdinanceDraft, PlaneRecord, ProgressGate, RocketTable, ordinance_feed,
};
use crate::ui::front_end::ordinance_layout::{
    OrdinanceLayoutScreen, OrdinancePress, OrdinanceSide, SECTION,
};
use crate::ui::front_end::{
    BUTTON_STATE_FRAMES, FlightCheckSlot, FrontEndScreens, OriginalLayout, SECTION_SURFACE,
};

/// The accept button's layout row.
pub const ACCEPT_ROW: &str = "OL_B_ACCEPT";
/// The cancel button's layout row.
pub const CANCEL_ROW: &str = "OL_B_CANCEL";

/// The first string id past each list the feed reads: the gun names and the
/// rocket rows are read for ids up to these many past their base.
const GUN_KINDS: u64 = 8;

/// One button of the screen, as drawn.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StageButton {
    /// The layout row.
    pub id: &'static str,
    /// The art's file name.
    pub art: String,
    /// One state frame, in the 800x600 logical screen.
    pub rect: Rect,
}

/// What the screen needs from the installation, read once.
#[derive(Clone, Debug)]
pub struct OrdinanceSetup {
    /// The screen of each side, not yet fed: pilot, then wingmate.
    screens: [OrdinanceLayoutScreen; 2],
    /// The string-table texts the feed names, `[FONT]` tags stripped.
    texts: BTreeMap<u32, String>,
    /// The background pane's art.
    background: String,
    /// The buttons: accept, then cancel.
    buttons: [StageButton; 2],
    /// Each side's committed record: pilot, then wingmate.
    records: [PlaneRecord; 2],
    /// The rocket table's gate.
    gate: ProgressGate,
}

const fn index(side: OrdinanceSide) -> usize {
    match side {
        OrdinanceSide::Pilot => 0,
        OrdinanceSide::Wingmate => 1,
    }
}

impl OrdinanceSetup {
    /// Reads the screen's rows from the layout. `strings` resolves a name
    /// (`IDS_OL_TITLE`) with its `[FONT]` tag kept, `text` a string id with
    /// the tag stripped; `extent` is an art file's size.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::Read`] for a missing or unreadable row or
    /// art, or when the committed engine facts do not load.
    pub fn build(
        layout: &OriginalLayout,
        strings: &dyn Fn(&str) -> Option<String>,
        text: &dyn Fn(u32) -> Option<String>,
        extent: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Result<Self, FlightCheckPhaseError> {
        let read =
            |reason: String| FlightCheckPhaseError::Read(format!("OrdinanceLayout: {reason}"));
        let facts = engine_facts().map_err(|error| read(error.to_string()))?;
        let section = layout
            .section(SECTION)
            .ok_or_else(|| read("the layout has no section".to_owned()))?;
        let background = section
            .panes
            .iter()
            .find(|row| row.id.eq_ignore_ascii_case("OL_BACKGROUND"))
            .and_then(|row| row.fields.first().cloned())
            .ok_or_else(|| read("no OL_BACKGROUND pane".to_owned()))?;
        let screen = |side| {
            OrdinanceLayoutScreen::from_layout(layout, side, 0, strings)
                .map_err(|error| read(error.to_string()))
        };
        let screens = [
            screen(OrdinanceSide::Pilot)?,
            screen(OrdinanceSide::Wingmate)?,
        ];
        let button = |id: &'static str| {
            let row = section
                .buttons
                .iter()
                .find(|row| row.id.eq_ignore_ascii_case(id))
                .ok_or_else(|| read(format!("no {id} button")))?;
            let art = row.fields.first().cloned().unwrap_or_default();
            let at = |column: usize| {
                row.fields
                    .get(column)
                    .and_then(|field| field.trim().parse::<u32>().ok())
                    .ok_or_else(|| read(format!("{id}: column {column} is not a number")))
            };
            let (width, height) =
                extent(&art).ok_or_else(|| read(format!("{art} is unreadable")))?;
            if height == 0 || height % BUTTON_STATE_FRAMES != 0 {
                return Err(read(format!("{id}: art height {height} is not a strip")));
            }
            Ok(StageButton {
                id,
                art,
                rect: Rect {
                    x: at(1)?,
                    y: at(2)?,
                    width,
                    height: height / BUTTON_STATE_FRAMES,
                },
            })
        };
        let buttons = [button(ACCEPT_ROW)?, button(CANCEL_ROW)?];
        let feed = facts.ordinance();
        let mut texts = BTreeMap::new();
        let ranges = [
            (feed.value("gun_name_base"), GUN_KINDS),
            (feed.value("ammo_row_base"), feed.value("ammo_rows")),
            (
                feed.value("rocket_row_base"),
                feed.rocket_entries().len() as u64,
            ),
        ];
        for (base, count) in ranges {
            for id in base..base + count {
                if let Some(line) = u32::try_from(id).ok().and_then(text) {
                    texts.insert(id as u32, line);
                }
            }
        }
        let records = [
            PlaneRecord::fresh_campaign(facts, FlightCheckPlaneSlot::Pilot),
            PlaneRecord::fresh_campaign(facts, FlightCheckPlaneSlot::Wingman),
        ];
        Ok(Self {
            screens,
            texts,
            background,
            buttons,
            records,
            gate: ProgressGate::default(),
        })
    }

    /// Reads the setup from the retail screens.
    ///
    /// # Errors
    ///
    /// As [`Self::build`], and when the layout cannot be read.
    pub fn from_retail(
        screens: &FrontEndScreens,
        ui: &FlightCheckStrings,
    ) -> Result<Self, FlightCheckPhaseError> {
        let layout = screens
            .original_layout()
            .map_err(|error| FlightCheckPhaseError::Read(error.to_string()))?;
        Self::build(
            &layout,
            &|name| ui.resolve_tagged(name),
            &|id| {
                ui.resolve_id_tagged(id)
                    .map(|text| crate::ui::front_end::strip_font_tag(&text).to_owned())
            },
            &|art| screens.art_extent(art),
        )
    }

    /// Every art file the screen draws, background first.
    #[must_use]
    pub fn art_names(&self) -> Vec<&str> {
        let mut names = vec![self.background.as_str()];
        for button in &self.buttons {
            if !names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&button.art))
            {
                names.push(&button.art);
            }
        }
        names
    }

    /// The background pane's art.
    #[must_use]
    pub fn background(&self) -> &str {
        &self.background
    }

    /// The accept and cancel buttons.
    #[must_use]
    pub const fn buttons(&self) -> &[StageButton; 2] {
        &self.buttons
    }

    /// The record committed for a side.
    #[must_use]
    pub const fn record(&self, side: OrdinanceSide) -> &PlaneRecord {
        &self.records[index(side)]
    }

    /// Opens the screen for a side: the feed from the committed record, and
    /// a working copy to store into.
    ///
    /// # Errors
    ///
    /// The engine facts' load error, as text.
    pub fn open(&self, side: OrdinanceSide) -> Result<OrdinanceStage, String> {
        let facts = engine_facts().map_err(|error| error.to_string())?;
        let live = self.records[index(side)].clone();
        let feed = ordinance_feed(facts, &live, self.gate, &|id| self.texts.get(&id).cloned());
        let mut screen = self.screens[index(side)].clone();
        screen.init(feed);
        Ok(OrdinanceStage {
            side,
            screen,
            draft: OrdinanceDraft::load(live),
            version: 0,
        })
    }

    /// Applies a press to the stage: the stores go to the draft (`E 2036`,
    /// `E 2037`), accept commits it (`E 2034`). Answers whether the press
    /// leaves the screen, and whether it committed.
    pub fn apply(&mut self, stage: &mut OrdinanceStage, press: &OrdinancePress) -> Applied {
        let Ok(facts) = engine_facts() else {
            return Applied::default();
        };
        match *press {
            OrdinancePress::Ammo { slot, choice } => {
                stage.draft.choose_ammo(slot, choice);
            }
            OrdinancePress::Rocket { slot, choice } => {
                let table = RocketTable::new(facts.ordinance(), self.gate);
                stage.draft.choose_rocket(&table, slot, choice);
            }
            OrdinancePress::Accept { leave } => {
                self.records[index(stage.side)] = stage.draft.commit().clone();
                return Applied {
                    leaves: leave,
                    committed: true,
                };
            }
            OrdinancePress::Cancel => {
                return Applied {
                    leaves: true,
                    committed: false,
                };
            }
            OrdinancePress::Nothing | OrdinancePress::Toggled => {}
        }
        Applied::default()
    }
}

/// What a press did to the stage.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Applied {
    /// The screen is left.
    pub leaves: bool,
    /// The draft was committed.
    pub committed: bool,
}

/// The screen while it is showing.
#[derive(Clone, Debug)]
pub struct OrdinanceStage {
    side: OrdinanceSide,
    screen: OrdinanceLayoutScreen,
    draft: OrdinanceDraft,
    /// Counts the changes the drawing must follow.
    version: u64,
}

impl OrdinanceStage {
    /// Whose plane.
    #[must_use]
    pub const fn side(&self) -> OrdinanceSide {
        self.side
    }

    /// The screen's model.
    #[must_use]
    pub const fn screen(&self) -> &OrdinanceLayoutScreen {
        &self.screen
    }

    /// The screen's model, for the presses.
    pub fn screen_mut(&mut self) -> &mut OrdinanceLayoutScreen {
        &mut self.screen
    }

    /// The working copy the stores have written so far.
    #[must_use]
    pub const fn draft(&self) -> &OrdinanceDraft {
        &self.draft
    }

    /// Changes since the stage opened; the drawing is rebuilt when it moves.
    #[must_use]
    pub const fn version(&self) -> u64 {
        self.version
    }

    /// Folds the model's dirty flag into [`Self::version`].
    pub(super) fn touch(&mut self) {
        if self.screen.take_dirty() {
            self.version += 1;
        }
    }
}

/// The side a Change Ammo button edits.
#[must_use]
pub const fn side_of(slot: FlightCheckSlot) -> OrdinanceSide {
    match slot {
        FlightCheckSlot::Player => OrdinanceSide::Pilot,
        FlightCheckSlot::Wingmate => OrdinanceSide::Wingmate,
    }
}

/// The logical point (800x600) under a window point, if inside the fitted
/// screen.
#[must_use]
pub fn logical_point(surface: (u32, u32), x: u32, y: u32) -> Option<(i32, i32)> {
    let fit = cs_content::ui_layout::AspectFit::new(SECTION_SURFACE, surface)?;
    let image = fit.image_rect();
    if !image.contains(x, y) {
        return None;
    }
    let scale = |value: u32, origin: u32, span: u32, logical: u32| {
        (u64::from(value - origin) * u64::from(logical) / u64::from(span)) as i32
    };
    Some((
        scale(x, image.x, image.width, SECTION_SURFACE.0),
        scale(y, image.y, image.height, SECTION_SURFACE.1),
    ))
}

//! The windowed pre-mission phase: the Flight Check, drawn from the original
//! layout and artwork, gates the mission on Fly Mission (task #1671
//! `M01-LC-FC-C`, a slice of #1661).
//!
//! The retail game shows the Flight Check between the briefing and the
//! mission. This module is that phase for `cs --mission M01`:
//!
//! * [`FlightCheckScene`] reads the `[FlightCheck]` section of the original
//!   `ASSETS/LAYOUT.CSV` ([`OriginalLayout`], F45-B.1) and the
//!   [`FlightCheckView`] (M01-LC-FC-A) into what is drawn: the background
//!   pane, the six buttons at their stated positions, and the text rows with
//!   the title, the objectives, the plane names and the measured loadout
//!   lists the view reads from the mission's own data. Every value the data
//!   does not state — the wingman's plane, a list the measured defaults
//!   cannot fill — is drawn as `unknown` and listed in
//!   [`FlightCheckScene::unknowns`], never as a designed stand-in.
//! * [`FlightCheckPhase`] holds the real [`FrontEnd`] machine on
//!   [`Screen::FlightCheck`]. **Fly Mission** (a click on the
//!   `FC_B_FLYMISSION` hotspot, or Enter) is delivered as
//!   [`Action::Launch`] followed by the system result
//!   [`Action::LoadSucceeded`] (the mission is already loaded behind the
//!   screen), so the machine ends on [`Screen::Flight`]. Nothing else lets the
//!   mission start.
//! * [`install_flight_check`] holds the mission while the phase is pending:
//!   the virtual clock is paused, so the fixed schedule (physics, the mission
//!   host, the objective session, the radio queue) never advances, and the
//!   audio systems do not run, so no engine loop starts. The phase's release
//!   unpauses the clock.
//!
//! # What is designed, and what is not built
//!
//! * The committed loadout is the **designed** one the mission flies with: the
//!   player's measured airframe and the designed ammunition id of
//!   [`super::mission_ammunition_id`]. The original's own loadout is unknown
//!   (`docs/findings/2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`).
//! * **Return to Briefing** is drawn and reports as unavailable: the briefing
//!   screen is the briefing slice of #1642, which does not exist yet.
//! * The **Change Ammo** button is drawn and reports that its screen
//!   (`OrdinanceLayout`) is out of scope; so do the **Change Plane** pills
//!   when the plane choice offers enough planes to show them
//!   (`PlaneSelection` is likewise out of scope).
//! * The plane pictures (`FC_P_PILOTPLANE` / `FC_P_WINGPLANE`) draw the frame
//!   of `FC_PlaneIcons.Png` the live record's `plane_id` selects — native
//!   `2010`'s answer (`docs/findings/2026-10-21-flight-check-page.md`). The
//!   original's Windows fonts are not bundled (licensing undecided), so text
//!   uses the engine's default font; the button rollover frames are not
//!   drawn because the state order of the sheet is unread. All of it is
//!   labelled designed.
//! * The **Change Plane** pills stay hidden while the plane-choice count is
//!   unmeasured: the script deactivates them when native `2018` answers under
//!   three, and the M01 Flight Check draws neither.
//! * Developer shortcuts (`CS_MISSION_START_M` and the capture scenario) skip
//!   the phase and say so ([`SKIP_NOTICE`]). The owner-command smoke does not:
//!   it photographs the screen, checks it against the scene and delivers Fly
//!   Mission itself (#1672 `M01-LC-FC-D`).
//!
//! Nothing here is `verified_original`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, BackgroundColor, ButtonInput, Color, Commands, Component, Entity, GlobalZIndex, Image,
    ImageNode, KeyCode, Last, MouseButton, Node, Plugin, PositionType, PreUpdate, Query, Res,
    ResMut, Resource, Text, TextColor, TextFont, Update, Val, Window, With, default,
};
use bevy::text::FontSize;
use bevy::time::{Time, Virtual};
use cs_content::briefing::DialogButtonKind;
use cs_content::ui_layout::{AspectFit, Rect};
use cs_types::content::ContentId;

use super::MissionStage;
use super::briefing_screen::{BriefingSource, read_briefing};
use crate::ui::front_end::{
    Action, ArtExtents, Artwork, BUTTON_STATE_FRAMES, BriefingPlayback, BriefingScene, Entry,
    FlightCheckButton, FlightCheckSlot, FlightCheckView, FrontEnd, FrontEndScreens,
    LayoutRow as Row, Loadout, OriginalLayout, Screen,
};

/// What a run prints when the phase is skipped.
pub const SKIP_NOTICE: &str = "mission flight check: skipped — a developer shortcut (capture \
     scenario) starts the mission directly";

/// Why Return to Briefing does nothing when the phase was opened without a
/// briefing (the Flight Check alone, a developer composition).
pub const RETURN_TO_BRIEFING_UNAVAILABLE: &str = "Return to Briefing is unavailable: this run \
     opened on the Flight Check without a briefing screen";

/// Why Return to Cabin does nothing yet: the briefing screen's button is wired
/// to the front-end machine's `Back` row (Briefing to Cabin), but the window
/// holds no cabin hub screen to land on until the hub task (#1749) installs
/// one and declares it with [`FlightCheckPhase::with_cabin_hub`].
pub const RETURN_TO_CABIN_UNAVAILABLE: &str = "Return to Cabin is unavailable: the cabin hub \
     screen is not built yet (#1749), so there is nothing to return to";

/// Why the Change Ammo buttons do nothing yet.
pub const CHANGE_AMMO_UNAVAILABLE: &str = "Change Ammo is out of scope here: its screen \
     (`OrdinanceLayout`) is not built, and the original's ammunition lists are unread (F45-C.5)";

/// Why the Change Plane buttons do nothing yet.
pub const CHANGE_PLANE_UNAVAILABLE: &str = "Change Plane is out of scope here: its screen \
     (`PlaneSelection`) is not built";

/// The layout section this phase draws.
const SECTION: &str = "FlightCheck";

/// The designed text height of a row whose layout height is an unresolved
/// placeholder, in logical pixels.
pub(super) const DESIGNED_TEXT_HEIGHT: u32 = 16;

/// The script's Change Plane gate (`if (3 > JQ) deactivate`): the pills show
/// only when the plane choice offers at least this many planes.
const CHANGE_PLANE_MIN_CHOICES: u32 = 3;

/// What a hotspot of the screen does.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ButtonRole {
    /// Fly Mission: delivered through the machine as [`Action::Launch`].
    FlyMission,
    /// Return to Briefing: unavailable until the briefing exists.
    ReturnToBriefing,
    /// Change Ammo for one plane: its screen is out of scope.
    ChangeAmmo(FlightCheckSlot),
    /// Change Plane for one plane: its screen is out of scope.
    ChangePlane(FlightCheckSlot),
}

impl ButtonRole {
    fn of(hotspot: &str) -> Option<Self> {
        let known = [
            FlightCheckButton::FlyMission,
            FlightCheckButton::ReturnToBriefing,
            FlightCheckButton::ChangeAmmoPilot,
            FlightCheckButton::ChangeAmmoWingman,
        ]
        .into_iter()
        .find(|button| button.hotspot().eq_ignore_ascii_case(hotspot));
        match known {
            Some(FlightCheckButton::FlyMission) => Some(Self::FlyMission),
            Some(FlightCheckButton::ReturnToBriefing) => Some(Self::ReturnToBriefing),
            Some(FlightCheckButton::ChangeAmmoPilot) => {
                Some(Self::ChangeAmmo(FlightCheckSlot::Player))
            }
            Some(FlightCheckButton::ChangeAmmoWingman) => {
                Some(Self::ChangeAmmo(FlightCheckSlot::Wingmate))
            }
            None if hotspot.eq_ignore_ascii_case("FC_B_CHANGEPLANE") => {
                Some(Self::ChangePlane(FlightCheckSlot::Player))
            }
            None if hotspot.eq_ignore_ascii_case("FC_B_CHANGEPLANEW") => {
                Some(Self::ChangePlane(FlightCheckSlot::Wingmate))
            }
            None => None,
        }
    }

    /// The reason a press does nothing, for a role that does nothing.
    #[must_use]
    pub const fn unavailable(&self) -> Option<&'static str> {
        match self {
            Self::FlyMission => None,
            Self::ReturnToBriefing => Some(RETURN_TO_BRIEFING_UNAVAILABLE),
            Self::ChangeAmmo(_) => Some(CHANGE_AMMO_UNAVAILABLE),
            Self::ChangePlane(_) => Some(CHANGE_PLANE_UNAVAILABLE),
        }
    }
}

/// One drawn button: the layout row's art at its stated position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneButton {
    /// The layout row id (`FC_B_FLYMISSION`).
    pub hotspot: String,
    /// What it does.
    pub role: ButtonRole,
    /// The art's file name as the layout spells it.
    pub art: String,
    /// The region in logical screen pixels: one state frame of the art.
    pub rect: Rect,
    /// The label drawn over the art, resolved from the row's string id;
    /// `None` when the art carries its own caption.
    pub label: Option<String>,
}

/// One drawn text row.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneText {
    /// The layout row id.
    pub id: String,
    /// The region in logical screen pixels (the height is the row's line
    /// height; the text wraps inside the width).
    pub rect: Rect,
    /// The colour, `0xAARRGGBB`, as the row states it.
    pub color: u32,
    /// The text.
    pub text: String,
    /// Whether the text is the word `unknown` standing for a value the data
    /// does not state.
    pub unknown: bool,
}

/// One drawn plane pane: the layout row's strip art at its stated position,
/// showing the frame the live record's plane id selects (native `2010`;
/// `docs/findings/2026-10-21-flight-check-page.md`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenePane {
    /// The layout row id (`FC_P_PILOTPLANE`).
    pub id: String,
    /// The art's file name as the layout spells it (`FC_PlaneIcons.Png`).
    pub art: String,
    /// The region in logical screen pixels: one frame of the strip.
    pub rect: Rect,
    /// The number of frames the strip holds (`NUMFRAMES`).
    pub frames: u32,
    /// The frame shown: the slot's `plane_id`.
    pub frame: u32,
}

/// A value the screen shows as unknown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SceneUnknown {
    /// The layout row that shows it.
    pub row: String,
    /// Why it is unknown.
    pub reason: String,
    /// What resolves it.
    pub resolving_task: String,
}

/// Why a scene could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlightCheckSceneError {
    /// The layout has no `[FlightCheck]` section.
    NoSection,
    /// The section has no `FC_BACKGROUND` pane.
    NoBackground,
    /// A row's art extent could not be read.
    ArtUnread {
        /// The row.
        row: String,
        /// The art's file name.
        art: String,
    },
    /// A button row's position is not a number.
    PositionUnread {
        /// The row.
        row: String,
    },
    /// A button's art is not a strip of the original's state frames.
    NotAStrip {
        /// The row.
        row: String,
        /// The art's height.
        height: u32,
    },
    /// The section has no Fly Mission button, so the screen could never let
    /// the mission start.
    NoFlyMission,
}

impl fmt::Display for FlightCheckSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no [{SECTION}] section"),
            Self::NoBackground => write!(f, "[{SECTION}] has no FC_BACKGROUND pane"),
            Self::ArtUnread { row, art } => write!(f, "{row}: the art {art} is not readable"),
            Self::PositionUnread { row } => write!(f, "{row}: its X or Y is not a number"),
            Self::NotAStrip { row, height } => write!(
                f,
                "{row}: its art height {height} is not {BUTTON_STATE_FRAMES} equal state frames"
            ),
            Self::NoFlyMission => write!(f, "[{SECTION}] has no FC_B_FLYMISSION button"),
        }
    }
}

impl std::error::Error for FlightCheckSceneError {}

/// What the Flight Check draws, resolved from the original layout and the
/// mission's view.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FlightCheckScene {
    /// The background pane's art file name.
    pub background: String,
    /// The background's extent, the logical screen size.
    pub size: (u32, u32),
    /// The plane panes (`FC_P_PILOTPLANE`, `FC_P_WINGPLANE`), in layout order.
    pub panes: Vec<ScenePane>,
    /// The buttons, in layout order.
    pub buttons: Vec<SceneButton>,
    /// The text rows, in layout order.
    pub texts: Vec<SceneText>,
    /// Every value shown as unknown.
    pub unknowns: Vec<SceneUnknown>,
}

pub(super) fn field(row: &Row, index: usize) -> &str {
    row.fields.get(index).map_or("", String::as_str)
}

pub(super) fn number(row: &Row, index: usize) -> Option<u32> {
    field(row, index).parse().ok()
}

/// A colour the layout writes as `0xAARRGGBB` or decimal.
pub(super) fn color_of(text: &str) -> Option<u32> {
    match text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        Some(hex) => u32::from_str_radix(hex, 16).ok(),
        None => text.parse().ok(),
    }
}

impl FlightCheckScene {
    /// Builds the scene.
    ///
    /// `strings` maps a string id (`IDS_FC_TITLE`) to its text.
    ///
    /// # Errors
    ///
    /// [`FlightCheckSceneError`] when the section, its background or a
    /// button's art cannot be read, or when there is no Fly Mission button.
    pub fn build(
        layout: &OriginalLayout,
        view: &FlightCheckView,
        extents: &dyn ArtExtents,
        strings: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, FlightCheckSceneError> {
        let section = layout
            .section(SECTION)
            .ok_or(FlightCheckSceneError::NoSection)?;
        let pane = section
            .panes
            .iter()
            .find(|row| row.id == "FC_BACKGROUND")
            .ok_or(FlightCheckSceneError::NoBackground)?;
        let background = field(pane, 0).to_owned();
        let size = extents
            .extent(&background)
            .ok_or_else(|| FlightCheckSceneError::ArtUnread {
                row: pane.id.clone(),
                art: background.clone(),
            })?;

        let mut unknowns = Vec::new();
        let mut panes = Vec::new();
        for row in &section.panes {
            if row.id == "FC_BACKGROUND" {
                continue;
            }
            let Some(slot) = (match row.id.as_str() {
                "FC_P_PILOTPLANE" => Some(&view.pilot),
                "FC_P_WINGPLANE" => Some(&view.wingman),
                _ => None,
            }) else {
                continue;
            };
            let art = field(row, 0).to_owned();
            let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                continue;
            };
            let frames = number(row, 4).unwrap_or(1).max(1);
            let (width, height) =
                extents
                    .extent(&art)
                    .ok_or_else(|| FlightCheckSceneError::ArtUnread {
                        row: row.id.clone(),
                        art: art.clone(),
                    })?;
            let Entry::Known(frame) = slot.icon_frame else {
                let Entry::Unknown {
                    reason,
                    resolving_task,
                } = &slot.icon_frame
                else {
                    unreachable!("matched Known above")
                };
                unknowns.push(SceneUnknown {
                    row: row.id.clone(),
                    reason: reason.clone(),
                    resolving_task: resolving_task.clone(),
                });
                continue;
            };
            panes.push(ScenePane {
                id: row.id.clone(),
                art,
                rect: Rect {
                    x,
                    y,
                    width,
                    height: height / frames,
                },
                frames,
                frame,
            });
        }

        // The script's `if (3 > JQ) deactivate(OP/AQ)`: with the choice count
        // unmeasured the pills stay hidden, which is what the M01 Flight Check
        // shows.
        let change_plane_offered = view
            .plane_choice_count
            .is_some_and(|count| count >= CHANGE_PLANE_MIN_CHOICES);
        let mut buttons = Vec::new();
        for row in &section.buttons {
            let Some(role) = ButtonRole::of(&row.id) else {
                continue;
            };
            if matches!(role, ButtonRole::ChangePlane(_)) && !change_plane_offered {
                unknowns.push(SceneUnknown {
                    row: row.id.clone(),
                    reason: format!(
                        "the script deactivates {id} when `callback($$E$$,2018)` answers under \
                         {CHANGE_PLANE_MIN_CHOICES} selectable planes; the count's source is \
                         unmeasured and the owner capture of the M01 Flight Check draws no \
                         Change Plane pill",
                        id = row.id
                    ),
                    resolving_task:
                        "the plane-choice count native 2018 answers (the `0x40fd10` routine of \
                         `docs/findings/2026-10-21-flight-check-page.md`)"
                            .to_owned(),
                });
                continue;
            }
            let art = field(row, 0).to_owned();
            let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                return Err(FlightCheckSceneError::PositionUnread {
                    row: row.id.clone(),
                });
            };
            let (width, height) =
                extents
                    .extent(&art)
                    .ok_or_else(|| FlightCheckSceneError::ArtUnread {
                        row: row.id.clone(),
                        art: art.clone(),
                    })?;
            if height == 0 || height % BUTTON_STATE_FRAMES != 0 {
                return Err(FlightCheckSceneError::NotAStrip {
                    row: row.id.clone(),
                    height,
                });
            }
            let label = Some(field(row, 4))
                .filter(|id| id.starts_with("IDS_"))
                .and_then(strings);
            buttons.push(SceneButton {
                hotspot: row.id.clone(),
                role,
                art,
                rect: Rect {
                    x,
                    y,
                    width,
                    height: height / BUTTON_STATE_FRAMES,
                },
                label,
            });
        }
        if !buttons
            .iter()
            .any(|button| button.role == ButtonRole::FlyMission)
        {
            return Err(FlightCheckSceneError::NoFlyMission);
        }

        let mut texts = Vec::new();
        let mut unknown_text = |row: &Row, rect: Rect, reason: &str, task: &str| {
            unknowns.push(SceneUnknown {
                row: row.id.clone(),
                reason: reason.to_owned(),
                resolving_task: task.to_owned(),
            });
            SceneText {
                id: row.id.clone(),
                rect,
                color: 0xFF80_8080,
                text: "unknown".to_owned(),
                unknown: true,
            }
        };
        for row in &section.other {
            match row.kind.as_str() {
                "T" => {
                    let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                        continue;
                    };
                    let rect = Rect {
                        x,
                        y,
                        width: number(row, 4).unwrap_or(size.0.saturating_sub(x)),
                        height: number(row, 5)
                            .filter(|height| *height <= size.1)
                            .unwrap_or(DESIGNED_TEXT_HEIGHT),
                    };
                    let color = color_of(field(row, 6)).unwrap_or(0xFF00_0000);
                    let entry = match row.id.as_str() {
                        "FC_T_MISSION" => Some(view.title.clone()),
                        "FC_T_PILOTPLANE" => Some(view.pilot.name.clone()),
                        "FC_T_WINGMANPLANE" => Some(view.wingman.name.clone()),
                        "FC_T_OBJECTIVES" => Some(objectives_text(view)),
                        _ => None,
                    };
                    match entry {
                        Some(Entry::Known(text)) => texts.push(SceneText {
                            id: row.id.clone(),
                            rect,
                            color,
                            text,
                            unknown: false,
                        }),
                        Some(Entry::Unknown {
                            reason,
                            resolving_task,
                        }) => texts.push(unknown_text(row, rect, &reason, &resolving_task)),
                        None => {
                            // A static caption: the row's own string id.
                            if let Some(text) = Some(field(row, 0))
                                .filter(|id| id.starts_with("IDS_"))
                                .and_then(strings)
                            {
                                texts.push(SceneText {
                                    id: row.id.clone(),
                                    rect,
                                    color,
                                    text,
                                    unknown: false,
                                });
                            }
                        }
                    }
                }
                "A" => {
                    // The gun and rocket lists: fields are X, Y, Z, W, H.
                    let (Some(x), Some(y)) = (number(row, 0), number(row, 1)) else {
                        continue;
                    };
                    let rect = Rect {
                        x,
                        y,
                        width: number(row, 3).unwrap_or(DESIGNED_TEXT_HEIGHT * 8),
                        height: number(row, 4).unwrap_or(DESIGNED_TEXT_HEIGHT),
                    };
                    let slot = if row.id.ends_with('W') {
                        &view.wingman
                    } else {
                        &view.pilot
                    };
                    let list = if row.id.contains("GUN") {
                        &slot.guns
                    } else {
                        &slot.rockets
                    };
                    match list {
                        Entry::Known(items) => texts.push(SceneText {
                            id: row.id.clone(),
                            rect,
                            color: color_of(field(row, 5)).unwrap_or(0xFF00_0000),
                            text: items.join("\n"),
                            unknown: false,
                        }),
                        Entry::Unknown {
                            reason,
                            resolving_task,
                        } => texts.push(unknown_text(row, rect, reason, resolving_task)),
                    }
                }
                _ => {}
            }
        }

        Ok(Self {
            background,
            size,
            panes,
            buttons,
            texts,
            unknowns,
        })
    }

    /// Reads the scene for one retail mission.
    ///
    /// `strings` resolves the layout's `IDS_*` string names; production
    /// passes [`FlightCheckStrings`].
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError`] when the screens, the layout or the view
    /// cannot be read, or the layout does not describe a usable screen.
    pub fn from_retail(
        screens: &FrontEndScreens,
        view: &FlightCheckView,
        strings: &dyn Fn(&str) -> Option<String>,
    ) -> Result<Self, FlightCheckPhaseError> {
        let layout = screens
            .original_layout()
            .map_err(|error| FlightCheckPhaseError::Read(error.to_string()))?;
        Self::build(&layout, view, &|art: &str| screens.art_extent(art), strings)
            .map_err(FlightCheckPhaseError::Scene)
    }

    /// Every art file the scene draws, once each, background first.
    #[must_use]
    pub fn art_names(&self) -> Vec<&str> {
        let mut names = vec![self.background.as_str()];
        for pane in &self.panes {
            if !names
                .iter()
                .any(|name| name.eq_ignore_ascii_case(&pane.art))
            {
                names.push(&pane.art);
            }
        }
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
}

/// The objectives as one block: a known row's text, or a visible unknown
/// naming the message key.
fn objectives_text(view: &FlightCheckView) -> Entry<String> {
    if view.objectives.is_empty() {
        return Entry::Unknown {
            reason: "the mission declares no objective message".to_owned(),
            resolving_task: "the mission's objective record".to_owned(),
        };
    }
    let mut lines = Vec::new();
    for row in &view.objectives {
        match &row.text {
            Entry::Known(text) => lines.push(text.clone()),
            Entry::Unknown { .. } => lines.push(format!("unknown ({})", row.key)),
        }
    }
    Entry::Known(lines.join("\n"))
}

/// Why the phase could not be opened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum FlightCheckPhaseError {
    /// The layout, its art or the mission's view could not be read.
    Read(String),
    /// The scene could not be built.
    Scene(FlightCheckSceneError),
    /// The state table has no walk from its first screen to the Flight
    /// Check.
    NoWalk,
    /// The machine refused the loadout.
    Loadout(String),
    /// The briefing could not be read (#1750).
    Briefing(String),
}

impl fmt::Display for FlightCheckPhaseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the Flight Check cannot be read: {reason}"),
            Self::Scene(error) => write!(f, "the Flight Check screen cannot be built: {error}"),
            Self::NoWalk => f.write_str("the front-end table has no walk to the Flight Check"),
            Self::Loadout(reason) => write!(f, "the Flight Check loadout is refused: {reason}"),
            Self::Briefing(error) => write!(f, "the briefing screen cannot be opened: {error}"),
        }
    }
}

impl std::error::Error for FlightCheckPhaseError {}

/// What a press did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PhaseResponse {
    /// Fly Mission went through the machine; the mission is released.
    Launched,
    /// Go to Flight Check went through the machine; the Flight Check is open.
    FlightCheckOpened,
    /// Return to Briefing went through the machine; the briefing plays again.
    BriefingOpened,
    /// Replay Briefing restarted the briefing's clock and voice-over.
    Replayed,
    /// Return to Cabin went through the machine; the cabin hub is next.
    CabinOpened,
    /// The button is drawn but does nothing yet; the reason is named.
    Unavailable {
        /// The hotspot.
        hotspot: String,
        /// Why.
        reason: &'static str,
    },
    /// The machine refused the press.
    Refused(String),
    /// Nothing is under the point.
    NoButton,
    /// The phase is already over.
    Over,
}

/// Which pre-mission screen the window shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PreMissionStage {
    /// The briefing map screen (#1750).
    Briefing,
    /// The Flight Check (#1671).
    FlightCheck,
    /// The cabin hub: reached by Return to Cabin once the hub exists.
    Cabin,
    /// Fly Mission went through: the mission runs.
    Released,
}

/// The briefing screen's state inside the phase.
#[derive(Clone, Debug)]
pub struct BriefingStage {
    scene: BriefingScene,
    playback: BriefingPlayback,
    cabin_hub: bool,
    notice: Option<String>,
}

impl BriefingStage {
    /// What the screen draws.
    #[must_use]
    pub fn scene(&self) -> &BriefingScene {
        &self.scene
    }

    /// The briefing's clock.
    #[must_use]
    pub fn playback(&self) -> &BriefingPlayback {
        &self.playback
    }

    /// The briefing's clock, for the system that advances it.
    pub fn playback_mut(&mut self) -> &mut BriefingPlayback {
        &mut self.playback
    }

    /// The last thing a press reported that is worth showing.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Whether a cabin hub screen exists for Return to Cabin to land on.
    #[must_use]
    pub const fn has_cabin_hub(&self) -> bool {
        self.cabin_hub
    }
}

/// The pre-mission phase: the machine on the Flight Check and the scene it
/// draws.
#[derive(Clone, Debug, Resource)]
pub struct FlightCheckPhase {
    front_end: FrontEnd,
    scene: FlightCheckScene,
    release_pending: bool,
    notice: Option<String>,
    /// The loadout the Flight Check commits; selected on every entry to the
    /// screen, because leaving it for the briefing drops the selection.
    loadout: Loadout,
    /// The briefing the phase opens on, when it has one.
    briefing: Option<BriefingStage>,
}

impl FlightCheckPhase {
    /// Opens the phase: walks the real machine from its first screen to the
    /// Flight Check through the table's own rows, then selects `loadout`.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::NoWalk`] when no walk reaches the screen,
    /// [`FlightCheckPhaseError::Loadout`] when the machine refuses the
    /// loadout.
    pub fn open(scene: FlightCheckScene, loadout: Loadout) -> Result<Self, FlightCheckPhaseError> {
        let mut front_end = walk_to(Screen::FlightCheck).ok_or(FlightCheckPhaseError::NoWalk)?;
        front_end
            .select_loadout(loadout.clone())
            .map_err(|refusal| FlightCheckPhaseError::Loadout(refusal.to_string()))?;
        Ok(Self {
            front_end,
            scene,
            release_pending: false,
            notice: None,
            loadout,
            briefing: None,
        })
    }

    /// Opens the phase on the briefing screen (#1750): the machine is walked
    /// to [`Screen::Briefing`], and **Go to Flight Check** carries it on to
    /// the Flight Check with `loadout` selected, the way the original's
    /// briefing leads to its Flight Check.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::NoWalk`] when no walk reaches the briefing.
    pub fn open_with_briefing(
        scene: FlightCheckScene,
        briefing: BriefingScene,
        loadout: Loadout,
    ) -> Result<Self, FlightCheckPhaseError> {
        let front_end = walk_to(Screen::Briefing).ok_or(FlightCheckPhaseError::NoWalk)?;
        Ok(Self {
            front_end,
            scene,
            release_pending: false,
            notice: None,
            loadout,
            briefing: Some(BriefingStage {
                scene: briefing,
                playback: BriefingPlayback::new(),
                cabin_hub: false,
                notice: None,
            }),
        })
    }

    /// Declares that a cabin hub screen exists, so Return to Cabin delivers
    /// the machine's `Back` row (Briefing to Cabin) instead of reporting the
    /// hub missing. The hub task (#1749) calls this when it installs the hub.
    #[must_use]
    pub fn with_cabin_hub(mut self) -> Self {
        if let Some(briefing) = &mut self.briefing {
            briefing.cabin_hub = true;
        }
        self
    }

    /// Which pre-mission screen is showing.
    #[must_use]
    pub fn stage(&self) -> PreMissionStage {
        match self.front_end.screen() {
            Screen::Briefing | Screen::Recon => PreMissionStage::Briefing,
            Screen::FlightCheck => PreMissionStage::FlightCheck,
            Screen::Cabin => PreMissionStage::Cabin,
            _ => PreMissionStage::Released,
        }
    }

    /// The briefing screen's state, when the phase has a briefing.
    #[must_use]
    pub fn briefing(&self) -> Option<&BriefingStage> {
        self.briefing.as_ref()
    }

    /// The briefing screen's state, for the systems that advance it.
    pub fn briefing_mut(&mut self) -> Option<&mut BriefingStage> {
        self.briefing.as_mut()
    }

    /// Reads and opens the phase for one retail mission, on the Flight Check.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError`] from any stage.
    pub fn from_retail(
        install_root: &Path,
        mission: &str,
        stage: &MissionStage,
    ) -> Result<(Self, FlightCheckArtwork), FlightCheckPhaseError> {
        let parts = RetailParts::read(install_root, mission, stage)?;
        Ok((Self::open(parts.scene, parts.loadout)?, parts.artwork))
    }

    /// Reads and opens the phase for one retail mission on its briefing
    /// screen (#1750): the Flight Check follows Go to Flight Check.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError`] from any stage, the briefing's own read
    /// included ([`FlightCheckPhaseError::Briefing`]).
    pub fn from_retail_with_briefing(
        install_root: &Path,
        mission: &str,
        stage: &MissionStage,
    ) -> Result<(Self, FlightCheckArtwork, BriefingSource), FlightCheckPhaseError> {
        let parts = RetailParts::read(install_root, mission, stage)?;
        let strings = |id: &str| stage_string(stage, id);
        let objectives = parts
            .view
            .objectives
            .iter()
            .map(|row| match &row.text {
                Entry::Known(text) => text.clone(),
                Entry::Unknown { .. } => format!("unknown ({})", row.key),
            })
            .collect();
        let source = read_briefing(install_root, mission, &parts.screens, &strings, objectives)
            .map_err(|error| FlightCheckPhaseError::Briefing(error.to_string()))?;
        let phase = Self::open_with_briefing(parts.scene, source.scene.clone(), parts.loadout)?;
        Ok((phase, parts.artwork, source))
    }

    /// Whether the mission is still held: a pre-mission screen is showing.
    #[must_use]
    pub fn is_pending(&self) -> bool {
        self.stage() != PreMissionStage::Released
    }

    /// The machine behind the screen.
    #[must_use]
    pub fn front_end(&self) -> &FrontEnd {
        &self.front_end
    }

    /// What the screen draws.
    #[must_use]
    pub fn scene(&self) -> &FlightCheckScene {
        &self.scene
    }

    /// The last thing a press reported that is worth showing: an unavailable
    /// button or a refusal.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Enter: the default button of the screen showing — Go to Flight Check
    /// on the briefing, Fly Mission on the Flight Check.
    pub fn press_enter(&mut self) -> PhaseResponse {
        match self.stage() {
            PreMissionStage::Briefing => self.press_briefing_button(DialogButtonKind::FlightCheck),
            PreMissionStage::FlightCheck => self.fly_mission(),
            _ => PhaseResponse::Over,
        }
    }

    /// Escape: the way back — Return to Cabin on the briefing, Return to
    /// Briefing on the Flight Check.
    pub fn press_back(&mut self) -> PhaseResponse {
        match self.stage() {
            PreMissionStage::Briefing => {
                self.press_briefing_button(DialogButtonKind::ReturnToCabin)
            }
            PreMissionStage::FlightCheck => self.press_hotspot("FC_B_RETURNBRIEF"),
            _ => PhaseResponse::Over,
        }
    }

    /// Presses a Flight Check hotspot by its layout id.
    pub fn press_hotspot(&mut self, hotspot: &str) -> PhaseResponse {
        if self.stage() != PreMissionStage::FlightCheck {
            return PhaseResponse::Over;
        }
        let Some(button) = self
            .scene
            .buttons
            .iter()
            .find(|button| button.hotspot.eq_ignore_ascii_case(hotspot))
        else {
            return PhaseResponse::NoButton;
        };
        match &button.role {
            ButtonRole::FlyMission => self.fly_mission(),
            ButtonRole::ReturnToBriefing if self.briefing.is_some() => self.return_to_briefing(),
            role => {
                let hotspot = button.hotspot.clone();
                let reason = role.unavailable().unwrap_or(RETURN_TO_BRIEFING_UNAVAILABLE);
                self.notice = Some(reason.to_owned());
                PhaseResponse::Unavailable { hotspot, reason }
            }
        }
    }

    /// Presses one of the briefing's three buttons.
    ///
    /// * **Replay Briefing** goes through the machine's `ReplayBriefing` row
    ///   (Briefing to Briefing, which never starts the mission) and restarts the
    ///   briefing's clock; the screen then restarts the voice-over.
    /// * **Return to Cabin** goes through the machine's `Back` row when a cabin
    ///   hub exists ([`Self::with_cabin_hub`]) and otherwise reports
    ///   [`RETURN_TO_CABIN_UNAVAILABLE`].
    /// * **Go to Flight Check** goes through `ContinueToFlightCheck` and
    ///   selects the loadout the Flight Check commits.
    pub fn press_briefing_button(&mut self, kind: DialogButtonKind) -> PhaseResponse {
        if self.stage() != PreMissionStage::Briefing || self.briefing.is_none() {
            return PhaseResponse::Over;
        }
        let hotspot = kind.key().to_owned();
        let refuse = |this: &mut Self, reason: String| {
            if let Some(briefing) = &mut this.briefing {
                briefing.notice = Some(reason.clone());
            }
            PhaseResponse::Refused(reason)
        };
        match kind {
            DialogButtonKind::Replay => match self.front_end.apply(Action::ReplayBriefing) {
                Ok(_) => {
                    if let Some(briefing) = &mut self.briefing {
                        briefing.playback.restart();
                        briefing.notice = None;
                    }
                    PhaseResponse::Replayed
                }
                Err(refusal) => refuse(self, refusal.to_string()),
            },
            DialogButtonKind::ReturnToCabin => {
                if !self.briefing.as_ref().is_some_and(|b| b.cabin_hub) {
                    if let Some(briefing) = &mut self.briefing {
                        briefing.notice = Some(RETURN_TO_CABIN_UNAVAILABLE.to_owned());
                    }
                    return PhaseResponse::Unavailable {
                        hotspot,
                        reason: RETURN_TO_CABIN_UNAVAILABLE,
                    };
                }
                match self.front_end.apply(Action::Back) {
                    Ok(_) => PhaseResponse::CabinOpened,
                    Err(refusal) => refuse(self, refusal.to_string()),
                }
            }
            DialogButtonKind::FlightCheck => {
                let opened = self
                    .front_end
                    .apply(Action::ContinueToFlightCheck)
                    .map_err(|refusal| refusal.to_string())
                    .and_then(|_| {
                        self.front_end
                            .select_loadout(self.loadout.clone())
                            .map_err(|refusal| refusal.to_string())
                    });
                match opened {
                    Ok(()) => {
                        if let Some(briefing) = &mut self.briefing {
                            briefing.notice = None;
                        }
                        self.notice = None;
                        PhaseResponse::FlightCheckOpened
                    }
                    Err(reason) => refuse(self, reason),
                }
            }
        }
    }

    /// Return to Briefing: the machine's `Back` row from the Flight Check
    /// (dropping the loadout selection, which the next entry makes again), then
    /// the briefing plays from its start. Whether the original resumes or
    /// restarts is unread ([`BriefingScene::unknowns`]).
    fn return_to_briefing(&mut self) -> PhaseResponse {
        let mut moved = self.front_end.apply(Action::Back);
        // The selection is the phase's own loadout, not a draft the player
        // made: the machine's discard question is answered yes.
        if moved.as_ref().is_ok_and(|outcome| {
            outcome
                .effects
                .iter()
                .any(|effect| matches!(effect, crate::ui::front_end::Effect::AskDiscard))
        }) {
            moved = self.front_end.confirm_discard();
        }
        match moved {
            Ok(_) if self.front_end.screen() == Screen::Briefing => {
                if let Some(briefing) = &mut self.briefing {
                    briefing.playback.restart();
                    briefing.notice = None;
                }
                self.notice = None;
                PhaseResponse::BriefingOpened
            }
            Ok(_) => {
                let reason = "Return to Briefing did not reach the briefing".to_owned();
                self.notice = Some(reason.clone());
                PhaseResponse::Refused(reason)
            }
            Err(refusal) => {
                let reason = refusal.to_string();
                self.notice = Some(reason.clone());
                PhaseResponse::Refused(reason)
            }
        }
    }

    /// A click at a window point, `surface` being the window's size: hit-tests
    /// through the same aspect-fit the showing screen is drawn with.
    pub fn click(&mut self, surface: (u32, u32), x: u32, y: u32) -> PhaseResponse {
        match self.stage() {
            PreMissionStage::Briefing => {
                let Some(briefing) = &self.briefing else {
                    return PhaseResponse::Over;
                };
                let Some(fit) = AspectFit::new(briefing.scene.size, surface) else {
                    return PhaseResponse::NoButton;
                };
                let hit = briefing
                    .scene
                    .buttons
                    .iter()
                    .rev()
                    .find(|button| fit.map_rect(button.rect).contains(x, y))
                    .map(|button| button.kind);
                match hit {
                    Some(kind) => self.press_briefing_button(kind),
                    None => PhaseResponse::NoButton,
                }
            }
            PreMissionStage::FlightCheck => {
                let Some(fit) = AspectFit::new(self.scene.size, surface) else {
                    return PhaseResponse::NoButton;
                };
                let hit = self
                    .scene
                    .buttons
                    .iter()
                    .rev()
                    .find(|button| fit.map_rect(button.rect).contains(x, y))
                    .map(|button| button.hotspot.clone());
                match hit {
                    Some(hotspot) => self.press_hotspot(&hotspot),
                    None => PhaseResponse::NoButton,
                }
            }
            _ => PhaseResponse::Over,
        }
    }

    /// Delivers Fly Mission through the machine: `Launch` on the Flight
    /// Check, then the system result that the load finished.
    fn fly_mission(&mut self) -> PhaseResponse {
        if self.stage() != PreMissionStage::FlightCheck {
            return PhaseResponse::Over;
        }
        let launched = self
            .front_end
            .apply(Action::Launch)
            .and_then(|_| self.front_end.apply(Action::LoadSucceeded));
        match launched {
            Ok(_) => {
                self.release_pending = true;
                self.notice = None;
                PhaseResponse::Launched
            }
            Err(refusal) => {
                let reason = refusal.to_string();
                self.notice = Some(reason.clone());
                PhaseResponse::Refused(reason)
            }
        }
    }

    /// The release the gate system consumes once.
    fn take_release(&mut self) -> bool {
        std::mem::take(&mut self.release_pending)
    }

    /// The lines a run prints about the phase.
    #[must_use]
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines = Vec::new();
        if let Some(briefing) = &self.briefing {
            let scene = &briefing.scene;
            lines.push(format!(
                "mission briefing: script {} on {}x{}, {} elements, {} markers of the voice-over, {} \
                 buttons; the Flight Check follows Go to Flight Check, verified_original=false",
                scene.script_key,
                scene.size.0,
                scene.size.1,
                scene.timeline.tracks().len(),
                scene.markers.len(),
                scene.buttons.len()
            ));
            if !briefing.cabin_hub {
                lines.push(format!("mission briefing: {RETURN_TO_CABIN_UNAVAILABLE}"));
            }
            for unknown in &scene.unknowns {
                lines.push(format!(
                    "mission briefing: unknown — {}; drawn as: {}",
                    unknown.what, unknown.drawn_as
                ));
            }
        }
        lines.push(format!(
            "mission flight check: {} buttons and {} text rows drawn from the original layout \
             [{SECTION}]; the mission is held until Fly Mission, loadout is the designed one, \
             verified_original=false",
            self.scene.buttons.len(),
            self.scene.texts.len()
        ));
        for button in &self.scene.buttons {
            if button.role == ButtonRole::ReturnToBriefing && self.briefing.is_some() {
                continue;
            }
            if let Some(reason) = button.role.unavailable() {
                lines.push(format!(
                    "mission flight check: {}: {reason}",
                    button.hotspot
                ));
            }
        }
        for unknown in &self.scene.unknowns {
            lines.push(format!(
                "mission flight check: {} shown as unknown ({}); resolved by {}",
                unknown.row, unknown.reason, unknown.resolving_task
            ));
        }
        lines
    }
}

/// The string-table text of a message id, through the stage's catalog.
pub(super) fn stage_string(stage: &MissionStage, id: &str) -> Option<String> {
    let catalog = stage.messages.catalog()?;
    match catalog.resolve(id, None) {
        cs_content::config::MessageLookup::Found { row, .. } => row.text.clone(),
        _ => None,
    }
}

/// The shipped resource headers inside the screen container whose `#define`s
/// pair the layout's `IDS_*` names with the UI string image's ids.
const UI_STRING_HEADERS: [&str; 2] = ["ASSETS/SCRIPTS/RESOURCE.H", "ASSETS/SCRIPTS/RESRC1.H"];

/// The UI string image the header ids address.
const UI_STRING_IMAGE: &str = "GOSDATA/ASSETS/BINARIES/langui.dll";

/// A layout row's `IDS_*` name resolved to the installation's own text: a
/// `#define` of the shipped resource headers pairs the name with a
/// string-table id, and the UI string image (`langui.dll`) answers the id —
/// the same pairing the original's own `GetString` calls make
/// (`docs/findings/2026-10-21-flight-check-page.md`).
///
/// A name no single `#define` spells, an id the image answers with nothing or
/// with two rows, and a row whose units do not decode all answer `None` —
/// never a guess.
pub struct FlightCheckStrings {
    /// `IDS_*` name → string id; names the headers contradict each other on
    /// are absent rather than chosen between.
    defines: BTreeMap<String, u32>,
    /// The UI string image's rows.
    catalog: cs_content::config::StringCatalog,
}

impl FlightCheckStrings {
    /// Reads both headers out of the screen container and the UI string
    /// image out of the installation.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::Read`] when a header member or its text, or
    /// the string image, cannot be read.
    pub fn read(
        install_root: &Path,
        screens: &FrontEndScreens,
    ) -> Result<Self, FlightCheckPhaseError> {
        let read = |reason: String| FlightCheckPhaseError::Read(reason);
        let mut defines = BTreeMap::<String, Option<u32>>::new();
        for member in UI_STRING_HEADERS {
            let key = cs_types::asset_id::AssetKey::from_spelling(
                screens.screens_container().namespace().as_str(),
                member,
                cs_content::textures::TEXTURE_ARCHIVE_VARIANT,
            )
            .map_err(|error| read(format!("{member}: {error}")))?;
            let bytes = screens
                .screens_container()
                .read(&key)
                .map_err(|error| read(format!("{member}: {error}")))?
                .data;
            let header = cs_formats::text::read_resource_header(
                &mut cs_formats::ParseContext::with_defaults(member),
                &bytes,
            )
            .map_err(|error| read(format!("{member}: {error}")))?;
            for define in header.defines() {
                let Ok(name) = std::str::from_utf8(define.name) else {
                    continue;
                };
                defines
                    .entry(name.to_owned())
                    .and_modify(|slot| {
                        if *slot != define.resource_id() {
                            // A second spelling of the name with a different
                            // id is ambiguous: keep it unanswered.
                            *slot = None;
                        }
                    })
                    .or_insert_with(|| define.resource_id());
            }
        }
        let defines = defines
            .into_iter()
            .filter_map(|(name, id)| id.map(|id| (name, id)))
            .collect();

        let path = install_root.join(UI_STRING_IMAGE);
        let bytes =
            std::fs::read(&path).map_err(|error| read(format!("{}: {error}", path.display())))?;
        let install = cs_types::evidence::ContentHash::from_hex(screens.install_sha256())
            .map_err(|error| read(format!("the installation fingerprint: {error}")))?;
        let source = cs_types::asset_id::SourceSpan::new(
            install,
            UI_STRING_IMAGE,
            None,
            0,
            bytes.len() as u64,
            None,
        )
        .map_err(|error| read(format!("{UI_STRING_IMAGE}: {error}")))?;
        let catalog = cs_content::config::StringCatalog::read(
            &mut cs_formats::ParseContext::with_defaults(UI_STRING_IMAGE),
            source,
            &bytes,
        )
        .map_err(|error| read(format!("{UI_STRING_IMAGE}: {error}")))?;

        Ok(Self { defines, catalog })
    }

    /// The text `name` names — its `[FONT]` tag stripped, the way the screen
    /// draws it — or `None`.
    #[must_use]
    pub fn resolve(&self, name: &str) -> Option<String> {
        self.resolve_tagged(name)
            .map(|text| crate::ui::front_end::strip_font_tag(&text).to_owned())
    }

    /// The text `name` names with its `[FONT]` tag kept, for a screen that
    /// reads the face from it (the Audio page, #1996).
    #[must_use]
    pub fn resolve_tagged(&self, name: &str) -> Option<String> {
        self.resolve_id_tagged(*self.defines.get(name)?)
    }

    /// The text string-table id `id` answers, `[FONT]` tag kept, for a native
    /// call that addresses a string by number (the Previous Missions list,
    /// #1956), or `None` when the image answers nothing or two rows.
    #[must_use]
    pub fn resolve_id_tagged(&self, id: u32) -> Option<String> {
        match self.catalog.resolve(id, None) {
            cs_content::config::StringLookup::Found(row) => row.text.clone(),
            _ => None,
        }
    }
}

/// What both retail openings read.
struct RetailParts {
    view: FlightCheckView,
    screens: FrontEndScreens,
    scene: FlightCheckScene,
    artwork: FlightCheckArtwork,
    loadout: Loadout,
}

impl RetailParts {
    fn read(
        install_root: &Path,
        mission: &str,
        stage: &MissionStage,
    ) -> Result<Self, FlightCheckPhaseError> {
        let read = |reason: String| FlightCheckPhaseError::Read(reason);
        let view = FlightCheckView::from_retail(install_root, mission)
            .map_err(|error| read(error.to_string()))?;
        let screens =
            FrontEndScreens::open(install_root).map_err(|error| read(error.to_string()))?;
        let ui = FlightCheckStrings::read(install_root, &screens)?;
        let strings = |id: &str| ui.resolve(id).or_else(|| stage_string(stage, id));
        let scene = FlightCheckScene::from_retail(&screens, &view, &strings)?;
        let artwork = FlightCheckArtwork::read(&screens, &scene)?;
        let airframe = designed_player_airframe(install_root, mission)?;
        let loadout = Loadout {
            player: Some(airframe),
            wingmate: None,
            ammunition: Some(super::mission_ammunition_id()),
        };
        Ok(Self {
            view,
            screens,
            scene,
            artwork,
            loadout,
        })
    }
}

/// Walks a fresh machine to `target` through the table's own rows
/// (breadth first over the real `apply`).
fn walk_to(target: Screen) -> Option<FrontEnd> {
    let start = FrontEnd::new();
    // A state is a screen and the profile choice made on it: the profile screen's
    // New/Continue rows stay on the screen and only set the choice Confirm needs.
    let mut seen = vec![(start.screen(), start.profile_intent())];
    let mut frontier = vec![start];
    while !frontier.is_empty() {
        let mut next = Vec::new();
        for machine in frontier {
            if machine.screen() == target {
                return Some(machine);
            }
            for action in Action::ALL {
                let mut trial = machine.clone();
                if trial.apply(action).is_ok()
                    && !seen.contains(&(trial.screen(), trial.profile_intent()))
                {
                    seen.push((trial.screen(), trial.profile_intent()));
                    next.push(trial);
                }
            }
        }
        frontier = next;
    }
    None
}

/// The airframe the mission starts the player in, as the mission start
/// configuration binds it.
fn designed_player_airframe(
    install_root: &Path,
    mission: &str,
) -> Result<ContentId, FlightCheckPhaseError> {
    let start = crate::mission_start::recover_retail_start_configuration(install_root, mission)
        .map_err(|error| FlightCheckPhaseError::Read(error.to_string()))?;
    match start.airframe() {
        cs_types::content::Resolved::Known(known) => Ok(known.value.clone()),
        cs_types::content::Resolved::Unknown { reason, .. } => Err(FlightCheckPhaseError::Read(
            format!("the player airframe is unknown: {reason}"),
        )),
    }
}

/// The decoded art the windowed screen draws, by lower-cased file name.
#[derive(Clone, Debug, Default, Resource)]
pub struct FlightCheckArtwork {
    images: BTreeMap<String, Artwork>,
}

impl FlightCheckArtwork {
    /// Decodes every art file the scene draws from the screen container.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::Read`] for an art file the container does not
    /// hold or will not decode.
    pub fn read(
        screens: &FrontEndScreens,
        scene: &FlightCheckScene,
    ) -> Result<Self, FlightCheckPhaseError> {
        Self::read_names(screens, scene.art_names())
    }

    /// Decodes the named art files from the screen container.
    ///
    /// # Errors
    ///
    /// [`FlightCheckPhaseError::Read`] for an art file the container does not
    /// hold or will not decode.
    pub fn read_names<'a>(
        screens: &FrontEndScreens,
        names: impl IntoIterator<Item = &'a str>,
    ) -> Result<Self, FlightCheckPhaseError> {
        let mut images = BTreeMap::new();
        for name in names {
            let wanted = format!("{}{name}", crate::ui::front_end::GRAPHICS_PREFIX);
            let spelling = screens
                .screens_container()
                .members()
                .find(|member| member.spelling.eq_ignore_ascii_case(&wanted))
                .map(|member| member.spelling.clone())
                .ok_or_else(|| {
                    FlightCheckPhaseError::Read(format!("the container holds no {wanted}"))
                })?;
            let image = crate::ui::front_end::OriginalImage {
                source: crate::ui::front_end::ImageSource::ScreenContainer,
                name: spelling,
                format: String::new(),
                width: 0,
                height: 0,
                bytes: 0,
                sha256: String::new(),
                decodable: true,
                refusal: String::new(),
            };
            let artwork = screens
                .artwork(&image)
                .map_err(|error| FlightCheckPhaseError::Read(error.to_string()))?;
            images.insert(name.to_ascii_lowercase(), artwork);
        }
        Ok(Self { images })
    }

    /// One art file.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Artwork> {
        self.images.get(&name.to_ascii_lowercase())
    }
}

/// Holds the mission behind the phase.
///
/// The virtual clock is paused, so the fixed schedule does not run, and the
/// phase resource is what the audio systems read to stay silent.
pub fn install_flight_check(app: &mut App, phase: FlightCheckPhase) {
    app.insert_resource(phase);
    app.world_mut().resource_mut::<Time<Virtual>>().pause();
    // Systems, not a plugin: the headless composition has already finished
    // its plugins when a test installs the phase.
    app.add_systems(PreUpdate, release_clock);
    // The playtest's `sync_pause` makes the virtual clock follow the input
    // session's pause every frame, which would undo this hold; the hold is
    // therefore re-asserted last in every frame, before the next frame's
    // clock update reads it.
    app.add_systems(Last, hold_clock);
}

fn hold_clock(phase: Option<Res<FlightCheckPhase>>, mut clock: ResMut<Time<Virtual>>) {
    if phase.is_some_and(|phase| phase.is_pending()) && !clock.is_paused() {
        clock.pause();
    }
}

/// Draws the phase and takes its input (windowed compositions only).
pub fn install_flight_check_screen(app: &mut App, artwork: FlightCheckArtwork) {
    app.insert_resource(artwork);
    app.add_plugins(FlightCheckScreenPlugin);
}

/// Whether the mission is held behind the Flight Check. The audio systems run
/// only while this is false.
#[must_use]
pub fn mission_is_held(phase: Option<Res<FlightCheckPhase>>) -> bool {
    phase.is_some_and(|phase| phase.is_pending())
}

/// Whether the mission is free to run: the run condition of the systems that
/// must not start before Fly Mission.
#[must_use]
pub fn mission_is_released(phase: Option<Res<FlightCheckPhase>>) -> bool {
    !mission_is_held(phase)
}

fn release_clock(phase: Option<ResMut<FlightCheckPhase>>, mut clock: ResMut<Time<Virtual>>) {
    if let Some(mut phase) = phase
        && phase.take_release()
    {
        clock.unpause();
    }
}

/// The window's face of the phase.
#[derive(Clone, Copy, Debug, Default)]
pub struct FlightCheckScreenPlugin;

impl Plugin for FlightCheckScreenPlugin {
    fn build(&self, app: &mut App) {
        // The screen is spawned when the Flight Check becomes the stage and
        // despawned when it stops being it (Return to Briefing, Fly Mission),
        // so the briefing can come before it and Return to Briefing can come
        // back to it.
        app.init_resource::<FlightCheckShown>().add_systems(
            Update,
            (
                sync_flight_check_screen,
                phase_input,
                fit_screen,
                show_notice,
            )
                .chain(),
        );
    }
}

/// Everything the screen spawns.
#[derive(Component)]
struct ScreenPart;

/// Whether the Flight Check's parts are spawned.
#[derive(Resource, Default)]
struct FlightCheckShown(bool);

/// Everything the screen draws over its background pane: the buttons, their
/// labels, the text rows and the notice line. `row` names the layout row a
/// button (its hotspot) or a text row (its id) is drawn for. The owner-command
/// smoke hides exactly these to measure what they add to the frame.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub(super) struct ScreenOverlay {
    /// The layout row, `None` for a button label and the notice line.
    pub(super) row: Option<String>,
}

/// A part's region in the logical screen.
#[derive(Component)]
struct Logical(Rect);

/// A text part's designed height in logical pixels.
#[derive(Component)]
struct LogicalFont(f32);

/// The notice line.
#[derive(Component)]
struct NoticeLine;

pub(super) fn argb(color: u32) -> Color {
    let [a, r, g, b] = color.to_be_bytes();
    Color::srgba_u8(r, g, b, a)
}

pub(super) fn to_image(artwork: &Artwork) -> Image {
    artwork.image()
}

fn spawn_screen(
    commands: &mut Commands,
    phase: &FlightCheckPhase,
    artwork: &FlightCheckArtwork,
    images: &mut bevy::prelude::Assets<Image>,
) {
    let scene = phase.scene();
    // Above the HUD, the letterbox and every other overlay: while the phase
    // is pending nothing else of the mission is shown.
    let z = GlobalZIndex(2000);
    // Three layers, because roots of one global index are stacked in no
    // stated order (the cover, the pane and the overlay live in different
    // archetypes): the overlay must be above the pane it is drawn on.
    let pane = GlobalZIndex(2001);
    let above = GlobalZIndex(2002);
    let full = Node {
        position_type: PositionType::Absolute,
        left: Val::Px(0.0),
        top: Val::Px(0.0),
        width: Val::Percent(100.0),
        height: Val::Percent(100.0),
        ..default()
    };
    commands.spawn((full, BackgroundColor(Color::BLACK), z, ScreenPart));
    if let Some(art) = artwork.get(&scene.background) {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            ImageNode::new(images.add(to_image(art))),
            Logical(Rect {
                x: 0,
                y: 0,
                width: scene.size.0,
                height: scene.size.1,
            }),
            pane,
            ScreenPart,
        ));
    }
    for plane in &scene.panes {
        let Some(art) = artwork.get(&plane.art) else {
            continue;
        };
        let frames = plane.frames.max(1);
        let frame_height = art.height() / frames;
        let top = plane.frame.min(frames - 1) * frame_height;
        let mut node = ImageNode::new(images.add(to_image(art)));
        let (left, upper) = art.presented_texel(0.0, top as f32);
        let (right, lower) = art.presented_texel(art.width() as f32, (top + frame_height) as f32);
        node.rect = Some(bevy::math::Rect::new(left, upper, right, lower));
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            node,
            Logical(plane.rect),
            above,
            ScreenPart,
            ScreenOverlay {
                row: Some(plane.id.clone()),
            },
        ));
    }
    for button in &scene.buttons {
        let Some(art) = artwork.get(&button.art) else {
            continue;
        };
        // The first state frame of the strip, the one at rest; the sheet's
        // state order is unread, so no other frame is drawn.
        let frame = art.height() / BUTTON_STATE_FRAMES;
        let mut node = ImageNode::new(images.add(to_image(art)));
        let (right, lower) = art.presented_texel(art.width() as f32, frame as f32);
        node.rect = Some(bevy::math::Rect::new(0.0, 0.0, right, lower));
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            node,
            Logical(button.rect),
            above,
            ScreenPart,
            ScreenOverlay {
                row: Some(button.hotspot.clone()),
            },
        ));
        if let Some(label) = &button.label {
            commands.spawn((
                Node {
                    position_type: PositionType::Absolute,
                    ..default()
                },
                Text::new(label.clone()),
                TextFont {
                    font_size: FontSize::Px(14.0),
                    ..default()
                },
                TextColor(Color::WHITE),
                Logical(button.rect),
                LogicalFont(14.0),
                above,
                ScreenPart,
                ScreenOverlay { row: None },
            ));
        }
    }
    for text in &scene.texts {
        let size = text.rect.height.max(DESIGNED_TEXT_HEIGHT) as f32 * 0.8;
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Text::new(text.text.clone()),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(argb(text.color)),
            Logical(text.rect),
            LogicalFont(size),
            above,
            ScreenPart,
            ScreenOverlay {
                row: Some(text.id.clone()),
            },
        ));
    }
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(8.0),
            bottom: Val::Px(8.0),
            ..default()
        },
        Text::new(""),
        TextFont {
            font_size: FontSize::Px(16.0),
            ..default()
        },
        TextColor(Color::srgb(1.0, 0.85, 0.2)),
        above,
        NoticeLine,
        ScreenPart,
        ScreenOverlay { row: None },
    ));
}

/// A placed part: its region, its node and, for text, its designed size.
type PlacedPart<'a> = (
    &'a Logical,
    &'a mut Node,
    Option<(&'a LogicalFont, &'a mut TextFont)>,
);

fn fit_screen(phase: Res<FlightCheckPhase>, windows: Query<&Window>, mut parts: Query<PlacedPart>) {
    let Ok(window) = windows.single() else { return };
    let surface = (window.width() as u32, window.height() as u32);
    let Some(fit) = AspectFit::new(phase.scene().size, surface) else {
        return;
    };
    let scale = fit.image_rect().width as f32 / phase.scene().size.0 as f32;
    for (logical, mut node, font) in &mut parts {
        let rect = fit.map_rect(logical.0);
        node.left = Val::Px(rect.x as f32);
        node.top = Val::Px(rect.y as f32);
        node.width = Val::Px(rect.width as f32);
        if font.is_none() {
            node.height = Val::Px(rect.height as f32);
        }
        if let Some((designed, mut text_font)) = font {
            text_font.font_size = FontSize::Px(designed.0 * scale);
        }
    }
}

fn phase_input(
    mut phase: ResMut<FlightCheckPhase>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
    keys: Res<ButtonInput<KeyCode>>,
) {
    if !phase.is_pending() {
        return;
    }
    let response = if keys.just_pressed(KeyCode::Enter) || keys.just_pressed(KeyCode::NumpadEnter) {
        Some(phase.press_enter())
    } else if keys.just_pressed(KeyCode::Escape) {
        Some(phase.press_back())
    } else if mouse.just_pressed(MouseButton::Left) {
        windows.single().ok().and_then(|window| {
            let at = window.cursor_position()?;
            let surface = (window.width() as u32, window.height() as u32);
            Some(phase.click(surface, at.x as u32, at.y as u32))
        })
    } else {
        None
    };
    match response {
        Some(PhaseResponse::Launched) => println!("mission flight check: Fly Mission — released"),
        Some(PhaseResponse::FlightCheckOpened) => {
            println!("mission briefing: Go to Flight Check — the Flight Check is open");
        }
        Some(PhaseResponse::BriefingOpened) => {
            println!("mission flight check: Return to Briefing — the briefing plays again");
        }
        Some(PhaseResponse::Replayed) => {
            println!("mission briefing: Replay Briefing — the briefing restarts");
        }
        Some(PhaseResponse::CabinOpened) => {
            println!("mission briefing: Return to Cabin — the cabin hub is next");
        }
        Some(PhaseResponse::Unavailable { reason, .. }) => {
            println!("mission pre-flight: {reason}");
        }
        Some(PhaseResponse::Refused(reason)) => {
            println!("mission pre-flight: press refused: {reason}");
        }
        _ => {}
    }
}

fn show_notice(phase: Res<FlightCheckPhase>, mut lines: Query<&mut Text, With<NoticeLine>>) {
    if let Ok(mut line) = lines.single_mut() {
        let wanted = phase.notice().unwrap_or("");
        if line.0 != wanted {
            line.0 = wanted.to_owned();
        }
    }
}

fn sync_flight_check_screen(
    mut commands: Commands,
    phase: Res<FlightCheckPhase>,
    artwork: Res<FlightCheckArtwork>,
    mut images: ResMut<bevy::prelude::Assets<Image>>,
    mut shown: ResMut<FlightCheckShown>,
    parts: Query<Entity, With<ScreenPart>>,
) {
    let wanted = phase.stage() == PreMissionStage::FlightCheck;
    if wanted && !shown.0 {
        spawn_screen(&mut commands, &phase, &artwork, &mut images);
        shown.0 = true;
    } else if !wanted && shown.0 {
        for part in &parts {
            commands.entity(part).despawn();
        }
        shown.0 = false;
    }
}

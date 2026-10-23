//! The campaign journal's debrief screen, shown when a mission run settles as
//! a failure (task #1655 `PARITY-M01-OUTCOME-C`, part of #1645).
//!
//! The original shows the run's results on its ScrapBook screen: the open
//! journal, the title `<player> - <mission>`, a result card headed `Mission
//! Failed` / `Mission Results` with four stat rows, the `Best to Date` and
//! `Most Recent` tabs, and the buttons `Replay Mission`, `View All Missions`
//! and `Return to Cabin`. Everything the screen is made of is read from the
//! installation:
//!
//! * the layout is the `[ScrapBook]` section of `ASSETS/LAYOUT.CSV` (panes,
//!   buttons and text rows with their positions, state strips and colours);
//! * every caption, and the message formats of the values, are the UI string
//!   image's rows the section's `IDS_*` names address (`IDS_SB_FAILURE`,
//!   `IDS_SB_RESULTS_TITLE`, `IDS_SB_PERCENTAGE_VAL`, ...);
//! * the values are the run's own [`MissionDebrief`], settled by the mission
//!   host ([`super::debrief`]).
//!
//! [`DebriefSource`] reads all of that once at launch, so the screen can be
//! built the moment a failure settles without touching the installation.
//! [`DebriefScreenPlugin`] draws the resulting [`DebriefScreen`] over the
//! mission and takes the buttons' input.
//!
//! # What is drawn from what the data states, and what is not
//!
//! * **Mission Failed only.** The owner's frames show the failure variant.
//!   The Mission Succeeded variant (`IDS_SB_SUCCESS` is `Mission Completed`)
//!   is not photographed, so the screen is built for a failure and a settled
//!   success keeps the run's exit unchanged. Recorded in
//!   [`DebriefScene::unknowns`].
//! * **Run Time** is drawn `m:ss`, the way the owner's frame draws it. The
//!   string image's own format (`IDS_SB_TIME_VAL`, `%1!02d!:%2!02d!`) would pad
//!   the minutes (`01:37`), which the frame does not show; which of the two the
//!   original draws for a longer run is unmeasured.
//! * **Gun Hit Ratio** is the whole-number percentage through
//!   `IDS_SB_PERCENTAGE_VAL`; with no shot fired it is `0%`, the value the
//!   owner's frames show. The original's rounding is unmeasured; the percentage
//!   is truncated.
//! * **Cash Earned** is drawn `unknown`: no cash rule is measured
//!   ([`super::debrief`]). The owner's frames show `$0` for failed runs, one
//!   observation that does not state the rule.
//! * **Rockets Expended** has a row in the layout (`SB_T_ROCKETSTITLE`) that
//!   the owner's frames do not draw. Why is unknown, so the row is left out
//!   and listed.
//! * **The tabs** `Best to Date` / `Most Recent`, the page arrows and
//!   `View All Missions` are drawn at their stated places with the art's first
//!   state frame; what pressing them shows, which tab is selected and which of
//!   the strip's frames is the selected one are not photographed, so a press
//!   reports a notice and changes nothing.
//! * **Replay Mission** (#1656) closes the screen and raises the mission
//!   host's own restart request ([`apply_response`]): the composed
//!   `mission_host_restart` rebuilds the authored initial state under a fresh
//!   session generation, the path the playtest's `R` takes. **Return to
//!   Cabin** has no cabin hub to land on (#1749); it reports why instead of
//!   doing nothing silently.
//! * The player's name is the installation's default
//!   (`IDS_DEFAULTPLAYERNAME`): the windowed mission run has no profile.
//! * The original's Windows fonts are not bundled, so text uses the engine's
//!   default font. The `SB_P_GRIME` overlay (a pane at `-1,-1`) is not drawn:
//!   its placement is not a position and the owner's frames show no overlay
//!   that can be told from the parchment.
//!
//! Nothing here is `verified_original`.

use std::collections::BTreeMap;
use std::fmt;
use std::path::Path;

use bevy::ecs::schedule::IntoScheduleConfigs;
use bevy::prelude::{
    App, BackgroundColor, ButtonInput, Color, Commands, Component, Entity, GlobalZIndex, Image,
    ImageNode, Last, MouseButton, Node, Plugin, PositionType, Query, Res, ResMut, Resource, Text,
    TextColor, TextFont, Update, Val, Window, With, default,
};
use bevy::text::{FontSize, Justify, TextLayout};
use bevy::time::{Time, Virtual};
use cs_content::campaign_bindings::SourceContext;
use cs_content::ui_layout::{AspectFit, Rect};
use cs_types::content::Resolved;

use super::debrief::MissionDebrief;
use super::flight_check::{
    DESIGNED_TEXT_HEIGHT, FlightCheckArtwork, FlightCheckPhaseError, FlightCheckStrings, SceneText,
    SceneUnknown, argb, color_of, field, number, to_image,
};
use super::host::MissionHostRestartRequest;
use crate::ui::front_end::{
    ArtExtents, Artwork, BUTTON_STATE_FRAMES, FrontEndScreens, OriginalLayout, strip_font_tag,
};

/// The layout section the screen is read from.
pub const SECTION: &str = "ScrapBook";

/// The caption of a failed run (`Mission Failed`).
pub const FAILURE_TITLE_STRING: &str = "IDS_SB_FAILURE";

/// The format of the title line (`%1!s! - %2!s!`: player, mission).
pub const NAME_AND_AREA_STRING: &str = "IDS_SB_NAMEANDAREA";

/// The installation's default player name.
pub const DEFAULT_PLAYER_NAME_STRING: &str = "IDS_DEFAULTPLAYERNAME";

/// The format of the Gun Hit Ratio value (`%1!d!%%`).
pub const PERCENTAGE_STRING: &str = "IDS_SB_PERCENTAGE_VAL";

/// The format of the Cash Earned value (`$%1!d!`).
pub const CASH_STRING: &str = "IDS_SB_CASH_VAL";

/// The format of the Overall Planes Downed value (`%1!d!`).
pub const PLANES_STRING: &str = "IDS_SB_PLANES_VAL";

/// Why Return to Cabin does nothing.
pub const CABIN_UNAVAILABLE: &str = "Return to Cabin is unavailable: the cabin hub screen is \
     not built yet (#1749), so there is nothing to return to";

/// Why View All Missions does nothing.
pub const VIEW_ALL_UNAVAILABLE: &str = "View All Missions is unavailable: its screen \
     (`ScrapBook_TOC`) is not built for the mission run";

/// Why the tabs and page arrows do nothing.
pub const TAB_UNAVAILABLE: &str = "The journal's tabs and page arrows are unavailable: what they \
     show (Best to Date, Most Recent, other pages) has not been photographed from the original";

// ------------------------------------------------------------ the message --

/// One argument of a message format.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Arg<'a> {
    /// A `%N!s!` argument.
    Text(&'a str),
    /// A `%N!d!` argument.
    Number(i64),
}

/// Formats a string-image message (`FormatMessage` style): `%N!s!`, `%N!d!`
/// and the zero-padded `%N!02d!`, with `%%` a literal percent sign.
///
/// `None` when the format is malformed, names an argument that was not given
/// or uses a conversion the image does not use.
#[must_use]
pub fn format_message(format: &str, args: &[Arg<'_>]) -> Option<String> {
    let mut out = String::new();
    let mut chars = format.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '%' {
            out.push(c);
            continue;
        }
        if chars.peek() == Some(&'%') {
            chars.next();
            out.push('%');
            continue;
        }
        let mut index = String::new();
        while let Some(d) = chars.peek().copied().filter(char::is_ascii_digit) {
            index.push(d);
            chars.next();
        }
        let index: usize = index.parse().ok()?;
        if chars.next()? != '!' {
            return None;
        }
        let mut spec = String::new();
        loop {
            let next = chars.next()?;
            if next == '!' {
                break;
            }
            spec.push(next);
        }
        let arg = args.get(index.checked_sub(1)?)?;
        match (arg, spec.strip_suffix('s'), spec.strip_suffix('d')) {
            (Arg::Text(text), Some(""), _) => out.push_str(text),
            (Arg::Number(number), _, Some(width)) => {
                let width = if width.is_empty() {
                    0
                } else {
                    width.trim_start_matches('0').parse::<usize>().ok()?
                };
                let zero = width > 0 && spec.starts_with('0');
                if zero {
                    out.push_str(&format!("{number:0width$}"));
                } else {
                    out.push_str(&format!("{number:width$}"));
                }
            }
            _ => return None,
        }
    }
    Some(out)
}

// -------------------------------------------------------------- the scene --

/// What a button of the screen is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebriefRole {
    /// `SB_B_REPLAY`.
    ReplayMission,
    /// `SB_B_TOC`.
    ViewAllMissions,
    /// `SB_B_RETURNPC`.
    ReturnToCabin,
    /// `SB_B_BEST`.
    BestToDate,
    /// `SB_B_MOST`.
    MostRecent,
    /// `SB_B_NEXT`.
    NextPage,
    /// `SB_B_PREV`.
    PreviousPage,
}

impl DebriefRole {
    fn of(hotspot: &str) -> Option<Self> {
        match hotspot.to_ascii_uppercase().as_str() {
            "SB_B_REPLAY" => Some(Self::ReplayMission),
            "SB_B_TOC" => Some(Self::ViewAllMissions),
            "SB_B_RETURNPC" => Some(Self::ReturnToCabin),
            "SB_B_BEST" => Some(Self::BestToDate),
            "SB_B_MOST" => Some(Self::MostRecent),
            "SB_B_NEXT" => Some(Self::NextPage),
            "SB_B_PREV" => Some(Self::PreviousPage),
            _ => None,
        }
    }

    /// Why a press of this button does nothing; `None` for Replay Mission,
    /// which restarts the mission.
    #[must_use]
    pub const fn unavailable(self) -> Option<&'static str> {
        match self {
            Self::ReplayMission => None,
            Self::ViewAllMissions => Some(VIEW_ALL_UNAVAILABLE),
            Self::ReturnToCabin => Some(CABIN_UNAVAILABLE),
            Self::BestToDate | Self::MostRecent | Self::NextPage | Self::PreviousPage => {
                Some(TAB_UNAVAILABLE)
            }
        }
    }
}

/// One drawn button: the layout row's art at its stated position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebriefButton {
    /// The layout row id (`SB_B_REPLAY`).
    pub hotspot: String,
    /// What it is.
    pub role: DebriefRole,
    /// The art's file name as the layout spells it.
    pub art: String,
    /// The region in logical screen pixels: one state frame of the art.
    pub rect: Rect,
    /// The layout's stacking order (`Z`), lowest first.
    pub z: u32,
    /// The label drawn over the art, resolved from the row's string id;
    /// `None` when the art carries its own caption.
    pub label: Option<String>,
}

/// One drawn pane: the layout row's art at its stated position.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebriefPane {
    /// The layout row id (`SB_STATCARD`).
    pub id: String,
    /// The art's file name as the layout spells it.
    pub art: String,
    /// The region in logical screen pixels: one frame of the art.
    pub rect: Rect,
    /// The number of frames the art holds.
    pub frames: u32,
    /// The frame drawn.
    pub frame: u32,
}

/// Why the scene could not be built.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebriefSceneError {
    /// The layout has no `[ScrapBook]` section.
    NoSection,
    /// The section has no `SB_BACKGROUND` pane.
    NoBackground,
    /// A row's art extent could not be read.
    ArtUnread {
        /// The row.
        row: String,
        /// The art's file name.
        art: String,
    },
    /// A row's position is not a number.
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
    /// A string the screen cannot be drawn without does not resolve or does
    /// not format.
    String {
        /// The `IDS_*` name.
        name: String,
    },
}

impl fmt::Display for DebriefSceneError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no [{SECTION}] section"),
            Self::NoBackground => write!(f, "[{SECTION}] has no SB_BACKGROUND pane"),
            Self::ArtUnread { row, art } => write!(f, "{row}: the art {art} is not readable"),
            Self::PositionUnread { row } => write!(f, "{row}: its X or Y is not a number"),
            Self::NotAStrip { row, height } => write!(
                f,
                "{row}: its art height {height} is not {BUTTON_STATE_FRAMES} equal state frames"
            ),
            Self::String { name } => write!(f, "the string {name} does not resolve or format"),
        }
    }
}

impl std::error::Error for DebriefSceneError {}

/// The run-dependent facts the scene shows.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebriefFacts {
    /// The player's name, when the installation states one.
    pub player_name: Option<String>,
    /// The mission's long name, when the campaign data states it.
    pub mission_title: Option<String>,
    /// The settled run's fields.
    pub debrief: MissionDebrief,
}

/// What the screen draws.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebriefScene {
    /// The background pane's art file name.
    pub background: String,
    /// The background's extent, the logical screen size.
    pub size: (u32, u32),
    /// The panes over the background, in layout order.
    pub panes: Vec<DebriefPane>,
    /// The buttons, in layout order.
    pub buttons: Vec<DebriefButton>,
    /// The text rows, in layout order.
    pub texts: Vec<SceneText>,
    /// Every value or element shown as unknown or left out, with why.
    pub unknowns: Vec<SceneUnknown>,
}

/// The task that resolves what the frames cannot show.
const FOLLOW_UP: &str = "a follow-up task asking the owner for original frames of the Mission \
     Succeeded debrief and the Best to Date / Most Recent tab states";

impl DebriefScene {
    /// Builds the failure scene.
    ///
    /// `strings` maps an `IDS_*` name to its text.
    ///
    /// # Errors
    ///
    /// [`DebriefSceneError`] when the section, its background or a button's
    /// art cannot be read, or a string the screen needs does not resolve.
    pub fn build(
        layout: &OriginalLayout,
        extents: &dyn ArtExtents,
        strings: &dyn Fn(&str) -> Option<String>,
        facts: &DebriefFacts,
    ) -> Result<Self, DebriefSceneError> {
        let section = layout
            .section(SECTION)
            .ok_or(DebriefSceneError::NoSection)?;
        let pane = section
            .panes
            .iter()
            .find(|row| row.id == "SB_BACKGROUND")
            .ok_or(DebriefSceneError::NoBackground)?;
        let background = field(pane, 0).to_owned();
        let size = extents
            .extent(&background)
            .ok_or_else(|| DebriefSceneError::ArtUnread {
                row: pane.id.clone(),
                art: background.clone(),
            })?;
        let string = |name: &str| {
            strings(name).ok_or_else(|| DebriefSceneError::String {
                name: name.to_owned(),
            })
        };

        let mut unknowns = vec![
            SceneUnknown {
                row: "SB_T_COMPLETETITLE".to_owned(),
                reason: "the Mission Succeeded variant (`IDS_SB_SUCCESS`) has not been \
                         photographed from the original, so the screen is built for a failure \
                         only and a settled success keeps the run's exit"
                    .to_owned(),
                resolving_task: FOLLOW_UP.to_owned(),
            },
            SceneUnknown {
                row: "SB_B_BEST / SB_B_MOST".to_owned(),
                reason: "which tab is selected, the strip frame the selected tab uses and what \
                         each tab shows are not visible in the owner's frames"
                    .to_owned(),
                resolving_task: FOLLOW_UP.to_owned(),
            },
        ];

        let mut panes = Vec::new();
        for row in &section.panes {
            match row.id.as_str() {
                "SB_BACKGROUND" => {}
                "SB_STATCARD" => {
                    let art = field(row, 0).to_owned();
                    let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                        return Err(DebriefSceneError::PositionUnread {
                            row: row.id.clone(),
                        });
                    };
                    let frames = number(row, 4).unwrap_or(1).max(1);
                    let (width, height) =
                        extents
                            .extent(&art)
                            .ok_or_else(|| DebriefSceneError::ArtUnread {
                                row: row.id.clone(),
                                art: art.clone(),
                            })?;
                    panes.push(DebriefPane {
                        id: row.id.clone(),
                        art,
                        rect: Rect {
                            x,
                            y,
                            width,
                            height: height / frames,
                        },
                        frames,
                        frame: 0,
                    });
                }
                id if id.starts_with("SB_KILL") => {
                    if !unknowns
                        .iter()
                        .any(|unknown| unknown.row == "SB_KILL0..SB_KILL10")
                    {
                        unknowns.push(SceneUnknown {
                            row: "SB_KILL0..SB_KILL10".to_owned(),
                            reason: "the layout has eleven kill markers (a strip of \
                                     `NUMFRAMES` frames each, with a count text) at fixed \
                                     places; the owner's frames are of runs with no kill, so \
                                     how a kill is drawn is not photographed and none is \
                                     drawn"
                                .to_owned(),
                            resolving_task: FOLLOW_UP.to_owned(),
                        });
                    }
                }
                _ => unknowns.push(SceneUnknown {
                    row: row.id.clone(),
                    reason: "a pane the screen does not draw: its placement (`-1,-1`, a strip \
                             taller than the screen) is not a position the layout states, and \
                             the owner's frames show no overlay that can be told from the \
                             parchment"
                        .to_owned(),
                    resolving_task: "an owner frame that shows the overlay, or a measurement of \
                                     the ScrapBook script"
                        .to_owned(),
                }),
            }
        }

        let mut buttons = Vec::new();
        for row in &section.buttons {
            let Some(role) = DebriefRole::of(&row.id) else {
                unknowns.push(SceneUnknown {
                    row: row.id.clone(),
                    reason: "a button the screen does not draw: the owner's frames of the \
                             debrief show no such button"
                        .to_owned(),
                    resolving_task: FOLLOW_UP.to_owned(),
                });
                continue;
            };
            let art = field(row, 0).to_owned();
            let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                return Err(DebriefSceneError::PositionUnread {
                    row: row.id.clone(),
                });
            };
            let (width, height) =
                extents
                    .extent(&art)
                    .ok_or_else(|| DebriefSceneError::ArtUnread {
                        row: row.id.clone(),
                        art: art.clone(),
                    })?;
            if height == 0 || height % BUTTON_STATE_FRAMES != 0 {
                return Err(DebriefSceneError::NotAStrip {
                    row: row.id.clone(),
                    height,
                });
            }
            let label = Some(field(row, 4))
                .filter(|id| id.starts_with("IDS_"))
                .and_then(strings);
            buttons.push(DebriefButton {
                hotspot: row.id.clone(),
                role,
                art,
                rect: Rect {
                    x,
                    y,
                    width,
                    height: height / BUTTON_STATE_FRAMES,
                },
                z: number(row, 3).unwrap_or(0),
                label,
            });
        }

        let debrief = &facts.debrief;
        let name_and_area = format_message(
            &string(NAME_AND_AREA_STRING)?,
            &[
                Arg::Text(facts.player_name.as_deref().unwrap_or("unknown")),
                Arg::Text(facts.mission_title.as_deref().unwrap_or("unknown")),
            ],
        )
        .ok_or_else(|| DebriefSceneError::String {
            name: NAME_AND_AREA_STRING.to_owned(),
        })?;
        if facts.player_name.is_none() {
            unknowns.push(SceneUnknown {
                row: "SB_T_NAMEANDAREA".to_owned(),
                reason: format!("the installation states no `{DEFAULT_PLAYER_NAME_STRING}`"),
                resolving_task: "the profile name binding of the mission run".to_owned(),
            });
        }
        if facts.mission_title.is_none() {
            unknowns.push(SceneUnknown {
                row: "SB_T_NAMEANDAREA".to_owned(),
                reason: "the campaign data states no long name for the mission".to_owned(),
                resolving_task: "the campaign source binding (`cs_content::campaign_bindings`)"
                    .to_owned(),
            });
        }
        let percent = debrief
            .gun_hit_ratio()
            .map_or(0, |ratio| (ratio * 100.0).floor() as i64);
        let percent = format_message(&string(PERCENTAGE_STRING)?, &[Arg::Number(percent)])
            .ok_or_else(|| DebriefSceneError::String {
                name: PERCENTAGE_STRING.to_owned(),
            })?;
        let planes = format_message(
            &string(PLANES_STRING)?,
            &[Arg::Number(
                i64::try_from(debrief.planes_downed).unwrap_or(i64::MAX),
            )],
        )
        .ok_or_else(|| DebriefSceneError::String {
            name: PLANES_STRING.to_owned(),
        })?;
        let (cash, cash_known) = match &debrief.cash_earned {
            Resolved::Known(known) => (
                format_message(
                    &string(CASH_STRING)?,
                    &[Arg::Number(i64::try_from(known.value).unwrap_or(i64::MAX))],
                )
                .ok_or_else(|| DebriefSceneError::String {
                    name: CASH_STRING.to_owned(),
                })?,
                true,
            ),
            Resolved::Unknown { reason, .. } => {
                unknowns.push(SceneUnknown {
                    row: "SB_T_CASH".to_owned(),
                    reason: reason.clone(),
                    resolving_task: "the cash award rule of the original (unmeasured)".to_owned(),
                });
                ("unknown".to_owned(), false)
            }
        };

        let mut texts = Vec::new();
        for row in &section.other {
            if row.kind != "T" {
                continue;
            }
            let (Some(x), Some(y)) = (number(row, 1), number(row, 2)) else {
                continue;
            };
            let rect = Rect {
                x,
                y,
                width: number(row, 4)
                    .filter(|width| *width > 0)
                    .unwrap_or(size.0.saturating_sub(x)),
                height: number(row, 5)
                    .filter(|height| (1..=size.1).contains(height))
                    .unwrap_or(DESIGNED_TEXT_HEIGHT),
            };
            let color = color_of(field(row, 6)).unwrap_or(0xFF00_0000);
            let (text, unknown) = match row.id.as_str() {
                "SB_T_NAMEANDAREA" => (name_and_area.clone(), false),
                "SB_T_COMPLETETITLE" => (string(FAILURE_TITLE_STRING)?, false),
                "SB_T_TIME" => (debrief.run_time(), false),
                "SB_T_HIT" => (percent.clone(), false),
                "SB_T_CASH" => (cash.clone(), !cash_known),
                "SB_T_PLANES" => (planes.clone(), false),
                "SB_T_ROCKETSTITLE" | "SB_T_ROCKETS" => {
                    if row.id == "SB_T_ROCKETSTITLE" {
                        unknowns.push(SceneUnknown {
                            row: "SB_T_ROCKETSTITLE / SB_T_ROCKETS".to_owned(),
                            reason: "the layout has a Rockets Expended row, and the owner's \
                                     debrief frames do not draw it; why (a rule on the run, the \
                                     mission, or the value) is not stated"
                                .to_owned(),
                            resolving_task: FOLLOW_UP.to_owned(),
                        });
                    }
                    continue;
                }
                id if id.ends_with("TITLE") => {
                    match Some(field(row, 0))
                        .filter(|name| name.starts_with("IDS_"))
                        .and_then(strings)
                    {
                        Some(text) => (text, false),
                        None => continue,
                    }
                }
                _ => continue,
            };
            texts.push(SceneText {
                id: row.id.clone(),
                rect,
                color: if unknown { 0xFF80_8080 } else { color },
                text,
                unknown,
            });
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

    /// Every art file the scene draws, once each, background first.
    #[must_use]
    pub fn art_names(&self) -> Vec<&str> {
        let mut names = vec![self.background.as_str()];
        for art in self
            .panes
            .iter()
            .map(|pane| pane.art.as_str())
            .chain(self.buttons.iter().map(|button| button.art.as_str()))
        {
            if !names.iter().any(|known| known.eq_ignore_ascii_case(art)) {
                names.push(art);
            }
        }
        names
    }
}

// -------------------------------------------------------------- the phase --

/// What a press on the screen did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebriefResponse {
    /// Replay Mission: the caller restarts the mission ([`apply_response`]).
    Replay,
    /// Return to Cabin in a run the start flow handed over: the caller ends
    /// the flight and the front end opens on the cabin ([`apply_response`]).
    ReturnToCabin,
    /// The button exists and does nothing yet; `reason` is also the notice.
    Unavailable {
        /// The button.
        role: DebriefRole,
        /// Why.
        reason: &'static str,
    },
    /// The press hit no button.
    NoButton,
}

/// The shown debrief: its scene and the notice line.
#[derive(Clone, Debug, Resource)]
pub struct DebriefScreen {
    scene: DebriefScene,
    notice: Option<String>,
    cabin: bool,
}

impl DebriefScreen {
    /// A screen showing `scene`.
    #[must_use]
    pub const fn new(scene: DebriefScene) -> Self {
        Self {
            scene,
            notice: None,
            cabin: false,
        }
    }

    /// The same screen with a cabin to return to: Return to Cabin then
    /// answers [`DebriefResponse::ReturnToCabin`] instead of its notice.
    #[must_use]
    pub const fn with_cabin(mut self) -> Self {
        self.cabin = true;
        self
    }

    /// What is drawn.
    #[must_use]
    pub const fn scene(&self) -> &DebriefScene {
        &self.scene
    }

    /// The notice line, set by a press that does nothing.
    #[must_use]
    pub fn notice(&self) -> Option<&str> {
        self.notice.as_deref()
    }

    /// Presses a button by its layout row id.
    pub fn press_hotspot(&mut self, hotspot: &str) -> DebriefResponse {
        let Some(button) = self
            .scene
            .buttons
            .iter()
            .find(|button| button.hotspot.eq_ignore_ascii_case(hotspot))
        else {
            return DebriefResponse::NoButton;
        };
        let role = button.role;
        if self.cabin && role == DebriefRole::ReturnToCabin {
            return DebriefResponse::ReturnToCabin;
        }
        let Some(reason) = role.unavailable() else {
            return DebriefResponse::Replay;
        };
        self.notice = Some(reason.to_owned());
        DebriefResponse::Unavailable { role, reason }
    }

    /// A click at a window point, `surface` being the window's size: hit-tests
    /// through the same aspect-fit the shown screen is drawn with.
    pub fn click(&mut self, surface: (u32, u32), x: u32, y: u32) -> DebriefResponse {
        let Some(fit) = AspectFit::new(self.scene.size, surface) else {
            return DebriefResponse::NoButton;
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
            None => DebriefResponse::NoButton,
        }
    }
}

// ------------------------------------------------------------- the source --

/// Why the debrief's source could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebriefReadError {
    /// A reader of the installation refused.
    Read(String),
    /// The scene could not be built from what was read.
    Scene(DebriefSceneError),
}

impl fmt::Display for DebriefReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Read(reason) => write!(f, "the debrief screen cannot be read: {reason}"),
            Self::Scene(error) => write!(f, "the debrief screen cannot be built: {error}"),
        }
    }
}

impl std::error::Error for DebriefReadError {}

impl From<FlightCheckPhaseError> for DebriefReadError {
    fn from(error: FlightCheckPhaseError) -> Self {
        Self::Read(error.to_string())
    }
}

/// The decoded art the debrief draws.
#[derive(Clone, Debug, Default, Resource)]
pub struct DebriefArtwork(FlightCheckArtwork);

impl DebriefArtwork {
    /// One art file.
    #[must_use]
    pub fn get(&self, name: &str) -> Option<&Artwork> {
        self.0.get(name)
    }
}

/// Everything the debrief needs from the installation, read at launch.
///
/// The screen is built from it the moment a failure settles
/// ([`Self::screen_for`]), so no file is touched while the mission runs.
#[derive(Clone, Debug, Resource)]
pub struct DebriefSource {
    layout: OriginalLayout,
    extents: BTreeMap<String, (u32, u32)>,
    strings: BTreeMap<String, String>,
    player_name: Option<String>,
    mission_title: Option<String>,
}

impl DebriefSource {
    /// Reads the layout, the strings, the art and the mission's title.
    ///
    /// Returns the source with the art its scene draws.
    ///
    /// # Errors
    ///
    /// [`DebriefReadError`] when the installation's screens, layout, strings
    /// or art cannot be read, or the layout does not describe a usable screen.
    pub fn read(
        install_root: &Path,
        mission: &str,
    ) -> Result<(Self, DebriefArtwork), DebriefReadError> {
        let read = |reason: String| DebriefReadError::Read(reason);
        let screens =
            FrontEndScreens::open(install_root).map_err(|error| read(error.to_string()))?;
        let ui = FlightCheckStrings::read(install_root, &screens)?;
        let layout = screens
            .original_layout()
            .map_err(|error| read(error.to_string()))?;
        let section = layout
            .section(SECTION)
            .ok_or(DebriefReadError::Scene(DebriefSceneError::NoSection))?;

        // Every string name the section spells, plus the formats the values
        // go through.
        let mut names: Vec<String> = [
            FAILURE_TITLE_STRING,
            NAME_AND_AREA_STRING,
            PERCENTAGE_STRING,
            CASH_STRING,
            PLANES_STRING,
        ]
        .map(str::to_owned)
        .to_vec();
        for row in section.buttons.iter().chain(&section.other) {
            for index in [0, 4] {
                let name = field(row, index);
                if name.starts_with("IDS_") {
                    names.push(name.to_owned());
                }
            }
        }
        let strings: BTreeMap<String, String> = names
            .into_iter()
            .filter_map(|name| ui.resolve(&name).map(|text| (name, text)))
            .collect();

        // The art: read once, its extents kept for the scene.
        let mut art_names: Vec<String> = Vec::new();
        for row in section.panes.iter().chain(&section.buttons) {
            let art = field(row, 0);
            if !art.is_empty()
                && !art_names
                    .iter()
                    .any(|known| known.eq_ignore_ascii_case(art))
                && !art.starts_with('<')
            {
                art_names.push(art.to_owned());
            }
        }
        let artwork =
            FlightCheckArtwork::read_names(&screens, art_names.iter().map(String::as_str))?;
        let extents = art_names
            .iter()
            .filter_map(|name| {
                artwork
                    .get(name)
                    .map(|art| (name.to_ascii_lowercase(), (art.width(), art.height())))
            })
            .collect();

        let mission_title = short_title(install_root, mission).map_err(read)?;
        // The default name's row carries a stray `]` where a font tag's
        // opening was cut (`]Nathan Zachary`); a tag remnant is not part of the
        // name.
        let player_name = ui
            .resolve(DEFAULT_PLAYER_NAME_STRING)
            .map(|name| name.trim_start_matches(']').to_owned());
        Ok((
            Self {
                layout,
                extents,
                strings,
                player_name,
                mission_title,
            },
            DebriefArtwork(artwork),
        ))
    }

    /// A source over an already-read layout, art extents and strings: what
    /// [`Self::read`] holds, for a caller that authors them (a headless test).
    #[must_use]
    pub fn from_parts(
        layout: OriginalLayout,
        extents: BTreeMap<String, (u32, u32)>,
        strings: BTreeMap<String, String>,
        player_name: Option<String>,
        mission_title: Option<String>,
    ) -> Self {
        Self {
            layout,
            extents,
            strings,
            player_name,
            mission_title,
        }
    }

    /// Builds the screen of one settled run.
    ///
    /// # Errors
    ///
    /// [`DebriefSceneError`] from [`DebriefScene::build`].
    pub fn screen_for(&self, debrief: &MissionDebrief) -> Result<DebriefScreen, DebriefSceneError> {
        let scene = DebriefScene::build(
            &self.layout,
            &|art: &str| self.extents.get(&art.to_ascii_lowercase()).copied(),
            &|name: &str| self.strings.get(name).cloned(),
            &DebriefFacts {
                player_name: self.player_name.clone(),
                mission_title: self.mission_title.clone(),
                debrief: debrief.clone(),
            },
        )?;
        Ok(DebriefScreen::new(scene))
    }
}

/// The mission's short name (`The Lost Treasure`): the row of the
/// campaign-length title block *without* a region prefix at the mission's
/// campaign position. The owner's frames draw this name beside the player's
/// (`Nathan Zachary - The Lost Treasure`); the region-prefixed block holds the
/// long names the Flight Check shows.
///
/// `Ok(None)` when the campaign data states no such row.
fn short_title(install_root: &Path, mission: &str) -> Result<Option<String>, String> {
    let context = SourceContext::read(install_root).map_err(|error| error.to_string())?;
    let agreement = context.join_agreement();
    if !agreement.establishes() {
        return Ok(None);
    }
    let wanted = format!("{}/", mission.to_ascii_lowercase());
    let Some(position) = context.campaign().iter().position(|entry| {
        entry
            .program_asset
            .to_ascii_lowercase()
            .starts_with(&wanted)
    }) else {
        return Ok(None);
    };
    let campaign_length = context.campaign().len();
    let Some(block) = agreement.blocks.iter().find(|block| {
        usize::try_from(block.last_id() - block.first_id() + 1) == Ok(campaign_length)
            && !agreement
                .grouped
                .iter()
                .any(|grouped| grouped.block == **block)
    }) else {
        return Ok(None);
    };
    let Some(id) = u32::try_from(position)
        .ok()
        .and_then(|position| block.first_id().checked_add(position))
        .filter(|id| block.contains(*id))
    else {
        return Ok(None);
    };
    Ok(context
        .string_rows()
        .iter()
        .find(|row| row.id == id)
        .and_then(|row| row.text.as_deref())
        .map(|text| strip_font_tag(text).to_owned()))
}

/// Installs the source and the plugin that draws the screen once it exists
/// (windowed compositions only).
pub fn install_debrief_screen(app: &mut App, source: DebriefSource, artwork: DebriefArtwork) {
    app.insert_resource(source);
    app.insert_resource(artwork);
    app.add_plugins(DebriefScreenPlugin);
}

/// Takes the settled failure's debrief into the window: builds the screen and
/// holds the mission behind it.
///
/// Returns whether the screen took the run over. `false` — no source
/// installed (a headless run) — leaves the run's exit to the caller.
pub fn show_debrief(world: &mut bevy::prelude::World, debrief: &MissionDebrief) -> bool {
    let Some(source) = world.get_resource::<DebriefSource>() else {
        return false;
    };
    match source.screen_for(debrief) {
        Ok(screen) => {
            let screen = if world.contains_resource::<super::ReturnToFrontEnd>() {
                screen.with_cabin()
            } else {
                screen
            };
            for line in screen.report_lines() {
                println!("{line}");
            }
            world.insert_resource(screen);
            true
        }
        Err(error) => {
            println!("mission debrief: the screen cannot be built: {error}");
            false
        }
    }
}

/// Carries out what a press on the screen asked for.
///
/// Replay Mission drops the screen (releasing the held clock and bringing the
/// HUD back) and raises [`MissionHostRestartRequest`], which the composed
/// `mission_host_restart` consumes in `PostUpdate`: the same rebuild, under a
/// fresh session generation, as the playtest's `R`. Any other response is
/// already reported by the screen's notice.
pub fn apply_response(world: &mut bevy::prelude::World, response: &DebriefResponse) {
    if matches!(response, DebriefResponse::Replay) {
        println!("mission debrief: Replay Mission restarts the mission");
        world.remove_resource::<DebriefScreen>();
        world.insert_resource(MissionHostRestartRequest);
    }
    if matches!(response, DebriefResponse::ReturnToCabin) {
        println!("mission debrief: Return to Cabin leaves the flight for the front end");
        world.remove_resource::<DebriefScreen>();
        world.insert_resource(super::MissionLeft(super::MissionEnd::ReturnedToCabin));
        if let Some(cell) = world.get_resource::<super::MissionLeftCell>() {
            cell.record(super::MissionEnd::ReturnedToCabin);
        }
        world.write_message(bevy::app::AppExit::Success);
    }
}

/// Presses `hotspot` on the shown screen and carries the press out. `None`
/// when no screen is up.
pub fn press_debrief_hotspot(
    world: &mut bevy::prelude::World,
    hotspot: &str,
) -> Option<DebriefResponse> {
    let response = world
        .get_resource_mut::<DebriefScreen>()?
        .press_hotspot(hotspot);
    apply_response(world, &response);
    Some(response)
}

impl DebriefScreen {
    /// The lines a run prints about the screen.
    #[must_use]
    pub fn report_lines(&self) -> Vec<String> {
        let mut lines = vec![format!(
            "mission debrief screen: {} texts, {} buttons, {} panes, verified_original=false",
            self.scene.texts.len(),
            self.scene.buttons.len(),
            self.scene.panes.len(),
        )];
        for text in &self.scene.texts {
            lines.push(format!(
                "mission debrief text: {} = {:?}{}",
                text.id,
                text.text,
                if text.unknown { " (unknown)" } else { "" }
            ));
        }
        for unknown in &self.scene.unknowns {
            lines.push(format!(
                "mission debrief unknown: {} — {} (resolved by: {})",
                unknown.row, unknown.reason, unknown.resolving_task
            ));
        }
        lines
    }
}

// ----------------------------------------------------------- the window --

/// Draws the debrief and takes its input.
#[derive(Clone, Copy, Debug, Default)]
pub struct DebriefScreenPlugin;

impl Plugin for DebriefScreenPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<DebriefShown>().add_systems(
            Update,
            (
                sync_debrief_screen,
                debrief_input,
                fit_debrief,
                show_debrief_notice,
            )
                .chain(),
        );
        // The screen holds the mission: nothing steps behind it. Re-asserted
        // last in every frame, as the Flight Check's hold is.
        app.add_systems(Last, hold_clock);
    }
}

fn hold_clock(screen: Option<Res<DebriefScreen>>, mut clock: ResMut<Time<Virtual>>) {
    if screen.is_some() && !clock.is_paused() {
        clock.pause();
    }
}

/// Everything the screen spawns.
#[derive(Component)]
struct DebriefPart;

/// Whether the screen's parts are spawned.
#[derive(Resource, Default)]
struct DebriefShown(bool);

/// Everything the screen draws over its background pane, by the layout row it
/// is drawn for (`None` for a button label and the notice line). The
/// owner-command smoke hides exactly these to measure what they add.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct DebriefOverlay {
    /// The layout row.
    pub row: Option<String>,
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

fn spawn_debrief(
    commands: &mut Commands,
    screen: &DebriefScreen,
    artwork: &DebriefArtwork,
    images: &mut bevy::prelude::Assets<Image>,
) {
    let scene = screen.scene();
    // Above the HUD, the letterbox and every other overlay. Three layers,
    // because roots of one global index are stacked in no stated order.
    let cover = GlobalZIndex(3000);
    let pane = GlobalZIndex(3001);
    let card = GlobalZIndex(3002);
    let buttons_z = GlobalZIndex(3003);
    let text_z = GlobalZIndex(3004);
    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        cover,
        DebriefPart,
    ));
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
            DebriefPart,
        ));
    }
    for part in &scene.panes {
        let Some(art) = artwork.get(&part.art) else {
            continue;
        };
        let frames = part.frames.max(1);
        let frame_height = art.height() / frames;
        let top = part.frame.min(frames - 1) * frame_height;
        let mut node = ImageNode::new(images.add(to_image(art)));
        node.rect = Some(bevy::math::Rect::new(
            0.0,
            top as f32,
            art.width() as f32,
            (top + frame_height) as f32,
        ));
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            node,
            Logical(part.rect),
            card,
            DebriefPart,
        ));
    }
    let mut buttons: Vec<&DebriefButton> = scene.buttons.iter().collect();
    buttons.sort_by_key(|button| button.z);
    for button in buttons {
        let Some(art) = artwork.get(&button.art) else {
            continue;
        };
        // The first state frame of the strip, the one at rest; the sheet's
        // state order is unread, so no other frame is drawn.
        let frame = art.height() / BUTTON_STATE_FRAMES;
        let mut node = ImageNode::new(images.add(to_image(art)));
        node.rect = Some(bevy::math::Rect::new(
            0.0,
            0.0,
            art.width() as f32,
            frame as f32,
        ));
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            node,
            Logical(button.rect),
            buttons_z,
            DebriefPart,
            DebriefOverlay {
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
                TextColor(Color::BLACK),
                Logical(Rect {
                    y: button.rect.y + button.rect.height.saturating_sub(14) / 2,
                    height: 14,
                    ..button.rect
                }),
                TextLayout::justify(Justify::Center),
                LogicalFont(14.0),
                text_z,
                DebriefPart,
                DebriefOverlay { row: None },
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
            text_z,
            DebriefPart,
            DebriefOverlay {
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
        text_z,
        NoticeLine,
        DebriefPart,
        DebriefOverlay { row: None },
    ));
}

/// A placed part: its region, its node and, for text, its designed size.
type PlacedPart<'a> = (
    &'a Logical,
    &'a mut Node,
    Option<(&'a LogicalFont, &'a mut TextFont)>,
);

fn fit_debrief(
    screen: Option<Res<DebriefScreen>>,
    windows: Query<&Window>,
    mut parts: Query<PlacedPart>,
) {
    let (Some(screen), Ok(window)) = (screen, windows.single()) else {
        return;
    };
    let surface = (window.width() as u32, window.height() as u32);
    let Some(fit) = AspectFit::new(screen.scene().size, surface) else {
        return;
    };
    let scale = fit.image_rect().width as f32 / screen.scene().size.0 as f32;
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

fn debrief_input(
    mut commands: Commands,
    screen: Option<ResMut<DebriefScreen>>,
    windows: Query<&Window>,
    mouse: Res<ButtonInput<MouseButton>>,
) {
    let Some(mut screen) = screen else { return };
    if !mouse.just_pressed(MouseButton::Left) {
        return;
    }
    let response = windows.single().ok().and_then(|window| {
        let at = window.cursor_position()?;
        let surface = (window.width() as u32, window.height() as u32);
        Some(screen.click(surface, at.x as u32, at.y as u32))
    });
    match response {
        Some(DebriefResponse::Unavailable { reason, .. }) => {
            println!("mission debrief: {reason}");
        }
        Some(response @ (DebriefResponse::Replay | DebriefResponse::ReturnToCabin)) => {
            commands.queue(move |world: &mut bevy::prelude::World| {
                apply_response(world, &response);
            });
        }
        Some(DebriefResponse::NoButton) | None => {}
    }
}

fn show_debrief_notice(
    screen: Option<Res<DebriefScreen>>,
    mut lines: Query<&mut Text, With<NoticeLine>>,
) {
    if let (Some(screen), Ok(mut line)) = (screen, lines.single_mut()) {
        let wanted = screen.notice().unwrap_or("");
        if line.0 != wanted {
            line.0 = wanted.to_owned();
        }
    }
}

fn sync_debrief_screen(
    mut commands: Commands,
    screen: Option<Res<DebriefScreen>>,
    artwork: Res<DebriefArtwork>,
    mut images: ResMut<bevy::prelude::Assets<Image>>,
    mut shown: ResMut<DebriefShown>,
    parts: Query<Entity, With<DebriefPart>>,
) {
    match (&screen, shown.0) {
        (Some(screen), false) => {
            spawn_debrief(&mut commands, screen, &artwork, &mut images);
            shown.0 = true;
        }
        (None, true) => {
            for part in &parts {
                commands.entity(part).despawn();
            }
            shown.0 = false;
        }
        _ => {}
    }
}

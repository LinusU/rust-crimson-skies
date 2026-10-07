//! The front-end state table (F45-A).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-A`. Shared contract: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path. A UI action
//! requests a domain transaction; it does not directly edit campaign cash,
//! ownership or objective fields.").
//!
//! * [`Screen`] — every screen from install selection to results.
//! * [`Action`] — everything that can happen on a screen: a button
//!   ([`ActionSource::User`]) or an app-side result such as a finished load
//!   ([`ActionSource::System`]). A button's stable key is what a
//!   `cs_content::ui_layout` hotspot names.
//! * [`TABLE`] — one [`Row`] per `(screen, action)`: target screen, [`Guard`]
//!   and the [`RequestKind`] it asks of the domain. [`validate_table`] proves
//!   the table total: no duplicate, every screen reachable, every screen has an
//!   escape and a path back to the main menu.
//! * [`resources`] — what each screen holds (input context, audio scope,
//!   world). A transition releases before it acquires, so no input context is
//!   ever bound twice and leaving the mission releases the world.
//! * [`check_layout`] — a visible hotspot must request a user action that has a
//!   transition on that screen.
//! * [`FrontEnd`] (in `machine`) — the machine that walks the table.
//! * [`ScreenSession`] (in `screens`, F45-B) — the front end presented through
//!   a validated [`ScreenDeck`] of authored artwork and hotspots: pointer
//!   hit-testing through the image's own aspect-fit transform and focus in the
//!   authored order, with every action still applied by the table above.
//! * [`review_navigation`] (in `paths`, F45-D) — the navigation review: every
//!   row of the table applied on the real machine, with every screen either
//!   reached or named as not reached and every refusal carried by its code.
//! * [`capture_screen`] (in `capture`, F45-D) — one screen drawn on the real
//!   renderer, artwork and hotspots and focus, offscreen, with a named refusal
//!   for every frame that is not evidence of a drawn screen.
//! * [`FrontEndScreens`] (in `retail`, F45-D) — the original installation's
//!   front-end screen artwork: the complete inventory of both sources and the
//!   decode that feeds a capture. `retail` here is read access to the owner's
//!   files, never a run of the original executable.
//!
//! The table, the machine, the authored presentation and the navigation walk
//! are **designed**; no original screen list, hotspot coordinate or flow was
//! read. [`FrontEndScreens`] (F45-D, `retail`) reads original **artwork** only
//! — which original image belongs to which screen, and where the original puts
//! a button, stays unread: no original front-end layout is decoded anywhere in
//! this repository, so F45-B validates and presents whatever a loader supplies
//! and refuses a screen it does not carry rather than showing a placeholder
//! (see `docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`), and
//! F45-D's captures are this renderer drawing decoded original pixels, never
//! the original executable. See
//! `docs/findings/2026-10-01-f45-a-frontend-state-table.md` for F45-A's
//! unknowns and `docs/findings/2026-10-07-f45-d-front-end-screen-capture.md`
//! for F45-D's.

mod capture;
mod flow;
mod machine;
mod paths;
mod retail;
mod screens;

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ui_layout::ScreenLayout;

pub use capture::{
    Artwork, CapturedButton, SCREEN_CAPTURE_HEIGHT, SCREEN_CAPTURE_SURFACE, SCREEN_CAPTURE_WIDTH,
    ScreenCapture, ScreenCaptureError, capture_artwork, capture_screen,
};
pub use flow::{
    ConstructionInputs, FlowDomainView, FlowError, FlowSetup, FrontEndFlow, LoadError, LoadFlow,
    LoadPlan, LoadVerdict, ResourceLedger, ResourceProblem,
};
pub use machine::{
    ConstructionDraft, Effect, FrontEnd, LoadFailure, Loadout, LoadoutProblem, MissionOutcome,
    Outcome, Plan, ProfileIntent, Refusal, Request,
};
pub use paths::{
    NavigationInputs, PathReview, PathStep, StepOutcome, SuppliedInput, review_navigation,
    review_navigation_with,
};
pub use retail::{
    FrontEndInventory, FrontEndScreens, GRAPHICS_PREFIX, ImageSource, MINIMUM_SCREEN_EXTENT,
    OriginalImage, SCREEN_CONTAINER, UI_IMAGE_CONTAINER,
};
pub use screens::{
    Button, ButtonView, DeckError, ScreenAssetError, ScreenAssets, ScreenDeck, ScreenSession,
    ScreenSessionError, ScreenView,
};

/// A front-end screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Screen {
    /// Choose the original installation.
    InstallSelect,
    /// Diagnose missing or unusable content.
    ContentDiagnosis,
    /// The main menu.
    MainMenu,
    /// Create or choose the profile.
    ProfileSelect,
    /// Settings opened from the menu.
    Settings,
    /// The campaign cabin hub.
    Cabin,
    /// The scrapbook.
    Scrapbook,
    /// Mission briefing (and its replay).
    Briefing,
    /// Recon images; never starts the mission.
    Recon,
    /// Construction (blueprint editing).
    Construction,
    /// Flight check: aircraft and ammunition.
    FlightCheck,
    /// Loading the mission.
    Loading,
    /// Flying.
    Flight,
    /// Paused in flight.
    Pause,
    /// Settings opened from the pause screen.
    PauseSettings,
    /// Mission results.
    Results,
}

impl Screen {
    /// Every screen.
    pub const ALL: [Self; 16] = [
        Self::InstallSelect,
        Self::ContentDiagnosis,
        Self::MainMenu,
        Self::ProfileSelect,
        Self::Settings,
        Self::Cabin,
        Self::Scrapbook,
        Self::Briefing,
        Self::Recon,
        Self::Construction,
        Self::FlightCheck,
        Self::Loading,
        Self::Flight,
        Self::Pause,
        Self::PauseSettings,
        Self::Results,
    ];

    /// The screen the application starts on.
    pub const START: Self = Self::InstallSelect;
}

/// Where an action comes from.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActionSource {
    /// A button, key or controller press.
    User,
    /// A result reported by the application (a load finishing, a mission ending).
    System,
}

/// Something that happens on a screen.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Action {
    /// The installation passed discovery.
    InstallVerified,
    /// The installation is missing or unusable.
    InstallRejected,
    /// Content went missing after the menu was reached.
    ContentMissing,
    /// Return from diagnosis to choosing an installation.
    ChooseAnotherInstall,
    /// Leave the application.
    Quit,
    /// Start a new profile.
    NewProfile,
    /// Continue an existing profile.
    ContinueProfile,
    /// Accept the profile choice.
    ConfirmProfile,
    /// Open settings.
    OpenSettings,
    /// Open the scrapbook.
    OpenScrapbook,
    /// Open the mission briefing.
    OpenBriefing,
    /// Replay the briefing.
    ReplayBriefing,
    /// Open recon images.
    OpenRecon,
    /// Open construction.
    OpenConstruction,
    /// Commit the construction draft.
    CommitConstruction,
    /// Go on to the flight check.
    ContinueToFlightCheck,
    /// Launch the mission.
    Launch,
    /// The mission finished loading.
    LoadSucceeded,
    /// The mission failed to load.
    LoadFailed,
    /// Pause the flight.
    Pause,
    /// Resume the flight.
    Resume,
    /// Abandon the flight from the pause screen.
    AbortMission,
    /// The mission ended in success.
    MissionSucceeded,
    /// The mission ended in failure.
    MissionFailed,
    /// Retry the failed mission.
    Retry,
    /// Return to the cabin.
    ReturnToCabin,
    /// Return to the main menu.
    ReturnToMenu,
    /// Back one screen.
    Back,
    /// Cancel the current step.
    Cancel,
}

impl Action {
    /// Every action.
    pub const ALL: [Self; 29] = [
        Self::InstallVerified,
        Self::InstallRejected,
        Self::ContentMissing,
        Self::ChooseAnotherInstall,
        Self::Quit,
        Self::NewProfile,
        Self::ContinueProfile,
        Self::ConfirmProfile,
        Self::OpenSettings,
        Self::OpenScrapbook,
        Self::OpenBriefing,
        Self::ReplayBriefing,
        Self::OpenRecon,
        Self::OpenConstruction,
        Self::CommitConstruction,
        Self::ContinueToFlightCheck,
        Self::Launch,
        Self::LoadSucceeded,
        Self::LoadFailed,
        Self::Pause,
        Self::Resume,
        Self::AbortMission,
        Self::MissionSucceeded,
        Self::MissionFailed,
        Self::Retry,
        Self::ReturnToCabin,
        Self::ReturnToMenu,
        Self::Back,
        Self::Cancel,
    ];

    /// The stable key a layout hotspot names.
    #[must_use]
    pub fn key(self) -> &'static str {
        match self {
            Self::InstallVerified => "install-verified",
            Self::InstallRejected => "install-rejected",
            Self::ContentMissing => "content-missing",
            Self::ChooseAnotherInstall => "choose-another-install",
            Self::Quit => "quit",
            Self::NewProfile => "new-profile",
            Self::ContinueProfile => "continue-profile",
            Self::ConfirmProfile => "confirm-profile",
            Self::OpenSettings => "open-settings",
            Self::OpenScrapbook => "open-scrapbook",
            Self::OpenBriefing => "open-briefing",
            Self::ReplayBriefing => "replay-briefing",
            Self::OpenRecon => "open-recon",
            Self::OpenConstruction => "open-construction",
            Self::CommitConstruction => "commit-construction",
            Self::ContinueToFlightCheck => "continue-to-flight-check",
            Self::Launch => "launch",
            Self::LoadSucceeded => "load-succeeded",
            Self::LoadFailed => "load-failed",
            Self::Pause => "pause",
            Self::Resume => "resume",
            Self::AbortMission => "abort-mission",
            Self::MissionSucceeded => "mission-succeeded",
            Self::MissionFailed => "mission-failed",
            Self::Retry => "retry",
            Self::ReturnToCabin => "return-to-cabin",
            Self::ReturnToMenu => "return-to-menu",
            Self::Back => "back",
            Self::Cancel => "cancel",
        }
    }

    /// The action with this key, if any.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|action| action.key() == key)
    }

    /// Whether a person or the application raises this action.
    #[must_use]
    pub fn source(self) -> ActionSource {
        match self {
            Self::InstallVerified
            | Self::InstallRejected
            | Self::ContentMissing
            | Self::LoadSucceeded
            | Self::LoadFailed
            | Self::MissionSucceeded
            | Self::MissionFailed => ActionSource::System,
            _ => ActionSource::User,
        }
    }

    /// Whether this action leaves its screen without committing anything: the
    /// escape every screen must offer.
    #[must_use]
    pub fn is_escape(self) -> bool {
        matches!(self, Self::Back | Self::Cancel | Self::Quit | Self::Pause)
    }
}

/// A condition on a row.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Guard {
    /// Always allowed.
    None,
    /// The flight-check loadout must satisfy the mission's rules.
    ValidLoadout,
    /// The last mission must have ended in failure.
    FailedOutcome,
    /// Leaving discards the draft; a dirty draft asks for confirmation first.
    DiscardsDraft,
}

/// What a transition asks of the domain.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RequestKind {
    /// Nothing.
    None,
    /// Create or open the profile.
    OpenProfile,
    /// Save and close the profile.
    CloseProfile,
    /// Commit the construction draft.
    CommitBlueprint,
    /// Select the flight-check aircraft and ammunition together.
    CommitLoadout,
    /// Apply the mission outcome to the campaign.
    ApplyOutcome,
    /// Abandon the running mission.
    AbandonMission,
}

/// One row of the transition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// The screen the action happens on.
    pub from: Screen,
    /// The action.
    pub action: Action,
    /// The screen it leads to.
    pub to: Screen,
    /// The condition.
    pub guard: Guard,
    /// The domain request.
    pub request: RequestKind,
}

const fn row(from: Screen, action: Action, to: Screen, guard: Guard, request: RequestKind) -> Row {
    Row {
        from,
        action,
        to,
        guard,
        request,
    }
}

use Action as A;
use Guard as G;
use RequestKind as R;
use Screen as S;

/// The transition table, in the order focus visits a screen's buttons.
pub const TABLE: &[Row] = &[
    row(
        S::InstallSelect,
        A::InstallVerified,
        S::MainMenu,
        G::None,
        R::None,
    ),
    row(
        S::InstallSelect,
        A::InstallRejected,
        S::ContentDiagnosis,
        G::None,
        R::None,
    ),
    row(
        S::InstallSelect,
        A::Quit,
        S::InstallSelect,
        G::None,
        R::None,
    ),
    row(
        S::ContentDiagnosis,
        A::ChooseAnotherInstall,
        S::InstallSelect,
        G::None,
        R::None,
    ),
    row(
        S::ContentDiagnosis,
        A::Quit,
        S::ContentDiagnosis,
        G::None,
        R::None,
    ),
    row(
        S::MainMenu,
        A::NewProfile,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(
        S::MainMenu,
        A::ContinueProfile,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(S::MainMenu, A::OpenSettings, S::Settings, G::None, R::None),
    row(
        S::MainMenu,
        A::ContentMissing,
        S::ContentDiagnosis,
        G::None,
        R::None,
    ),
    row(S::MainMenu, A::Quit, S::MainMenu, G::None, R::None),
    row(
        S::ProfileSelect,
        A::ConfirmProfile,
        S::Cabin,
        G::None,
        R::OpenProfile,
    ),
    row(S::ProfileSelect, A::Back, S::MainMenu, G::None, R::None),
    row(S::Settings, A::Back, S::MainMenu, G::None, R::None),
    row(S::Cabin, A::OpenBriefing, S::Briefing, G::None, R::None),
    row(S::Cabin, A::OpenScrapbook, S::Scrapbook, G::None, R::None),
    row(
        S::Cabin,
        A::OpenConstruction,
        S::Construction,
        G::None,
        R::None,
    ),
    row(S::Cabin, A::Back, S::MainMenu, G::None, R::CloseProfile),
    row(S::Scrapbook, A::Back, S::Cabin, G::None, R::None),
    row(
        S::Briefing,
        A::ReplayBriefing,
        S::Briefing,
        G::None,
        R::None,
    ),
    row(S::Briefing, A::OpenRecon, S::Recon, G::None, R::None),
    row(
        S::Briefing,
        A::ContinueToFlightCheck,
        S::FlightCheck,
        G::None,
        R::None,
    ),
    row(S::Briefing, A::Back, S::Cabin, G::None, R::None),
    row(S::Recon, A::Back, S::Briefing, G::None, R::None),
    row(
        S::Construction,
        A::CommitConstruction,
        S::Cabin,
        G::None,
        R::CommitBlueprint,
    ),
    row(
        S::Construction,
        A::Cancel,
        S::Cabin,
        G::DiscardsDraft,
        R::None,
    ),
    row(
        S::Construction,
        A::Back,
        S::Cabin,
        G::DiscardsDraft,
        R::None,
    ),
    row(
        S::FlightCheck,
        A::Launch,
        S::Loading,
        G::ValidLoadout,
        R::CommitLoadout,
    ),
    row(
        S::FlightCheck,
        A::Cancel,
        S::Briefing,
        G::DiscardsDraft,
        R::None,
    ),
    row(
        S::FlightCheck,
        A::Back,
        S::Briefing,
        G::DiscardsDraft,
        R::None,
    ),
    row(S::Loading, A::LoadSucceeded, S::Flight, G::None, R::None),
    row(S::Loading, A::LoadFailed, S::FlightCheck, G::None, R::None),
    row(S::Loading, A::Cancel, S::FlightCheck, G::None, R::None),
    row(S::Flight, A::Pause, S::Pause, G::None, R::None),
    row(
        S::Flight,
        A::MissionSucceeded,
        S::Results,
        G::None,
        R::ApplyOutcome,
    ),
    row(
        S::Flight,
        A::MissionFailed,
        S::Results,
        G::None,
        R::ApplyOutcome,
    ),
    row(S::Pause, A::Resume, S::Flight, G::None, R::None),
    row(
        S::Pause,
        A::OpenSettings,
        S::PauseSettings,
        G::None,
        R::None,
    ),
    row(
        S::Pause,
        A::AbortMission,
        S::Cabin,
        G::None,
        R::AbandonMission,
    ),
    row(S::Pause, A::Back, S::Flight, G::None, R::None),
    row(S::PauseSettings, A::Back, S::Pause, G::None, R::None),
    row(S::Results, A::Retry, S::Loading, G::FailedOutcome, R::None),
    row(S::Results, A::OpenScrapbook, S::Scrapbook, G::None, R::None),
    row(S::Results, A::ReturnToCabin, S::Cabin, G::None, R::None),
    row(
        S::Results,
        A::ReturnToMenu,
        S::MainMenu,
        G::None,
        R::CloseProfile,
    ),
    row(S::Results, A::Back, S::Cabin, G::None, R::None),
];

/// The row for an action on a screen.
#[must_use]
pub fn find(from: Screen, action: Action) -> Option<&'static Row> {
    TABLE
        .iter()
        .find(|row| row.from == from && row.action == action)
}

/// The rows on a screen, in focus order.
pub fn rows_on(screen: Screen) -> impl Iterator<Item = &'static Row> {
    TABLE.iter().filter(move |row| row.from == screen)
}

/// What the front end can acquire from the rest of the application.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Resource {
    /// An input binding context; exactly one is ever held.
    Input(InputContext),
    /// An audio scope.
    Audio(AudioScope),
    /// The simulated world.
    World,
}

/// Input binding contexts: a joystick trigger is bound by exactly one.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum InputContext {
    /// Menu navigation and buttons.
    Menu,
    /// Flight controls and guns.
    Flight,
}

/// Audio scopes (the F41 bus assignment is F45-B's; these only mark ownership).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum AudioScope {
    /// Menu and cabin sound.
    FrontEnd,
    /// Briefing voice.
    Briefing,
    /// Mission sound.
    Flight,
}

/// What a screen holds while it is current.
#[must_use]
pub fn resources(screen: Screen) -> BTreeSet<Resource> {
    let (input, audio, world) = match screen {
        S::Briefing | S::Recon => (InputContext::Menu, AudioScope::Briefing, false),
        S::Loading => (InputContext::Menu, AudioScope::FrontEnd, true),
        S::Flight => (InputContext::Flight, AudioScope::Flight, true),
        S::Pause | S::PauseSettings => (InputContext::Menu, AudioScope::Flight, true),
        _ => (InputContext::Menu, AudioScope::FrontEnd, false),
    };
    let mut held = BTreeSet::from([Resource::Input(input), Resource::Audio(audio)]);
    if world {
        held.insert(Resource::World);
    }
    held
}

/// Why the table is not sound.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TableProblem {
    /// Two rows share `(from, action)`.
    Duplicate {
        /// The screen.
        from: Screen,
        /// The action.
        action: Action,
    },
    /// No row leaves this screen with an escape action.
    NoEscape {
        /// The screen.
        screen: Screen,
    },
    /// The screen cannot be reached from [`Screen::START`].
    Unreachable {
        /// The screen.
        screen: Screen,
    },
    /// The screen has no path to the main menu.
    NoPathToMenu {
        /// The screen.
        screen: Screen,
    },
}

/// Checks the production [`TABLE`].
#[must_use]
pub fn validate_table() -> Vec<TableProblem> {
    validate_rows(TABLE)
}

/// Checks any table with the same rules; the tests feed it broken tables.
#[must_use]
pub fn validate_rows(rows: &[Row]) -> Vec<TableProblem> {
    let mut problems = Vec::new();
    let mut seen = BTreeSet::new();
    for row in rows {
        if !seen.insert((row.from, row.action)) {
            problems.push(TableProblem::Duplicate {
                from: row.from,
                action: row.action,
            });
        }
    }
    let mut forward: BTreeMap<Screen, Vec<Screen>> = BTreeMap::new();
    let mut backward: BTreeMap<Screen, Vec<Screen>> = BTreeMap::new();
    for row in rows {
        forward.entry(row.from).or_default().push(row.to);
        backward.entry(row.to).or_default().push(row.from);
    }
    let from_start = closure(Screen::START, &forward);
    let to_menu = closure(Screen::MainMenu, &backward);
    for screen in Screen::ALL {
        if !rows
            .iter()
            .any(|row| row.from == screen && row.action.is_escape())
        {
            problems.push(TableProblem::NoEscape { screen });
        }
        if !from_start.contains(&screen) {
            problems.push(TableProblem::Unreachable { screen });
        }
        if !to_menu.contains(&screen) {
            problems.push(TableProblem::NoPathToMenu { screen });
        }
    }
    problems
}

fn closure(start: Screen, edges: &BTreeMap<Screen, Vec<Screen>>) -> BTreeSet<Screen> {
    let mut reached = BTreeSet::from([start]);
    let mut frontier = vec![start];
    while let Some(screen) = frontier.pop() {
        for next in edges.get(&screen).into_iter().flatten() {
            if reached.insert(*next) {
                frontier.push(*next);
            }
        }
    }
    reached
}

/// Why a layout hotspot is not a working button.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LayoutProblem {
    /// The hotspot names a key that is no action.
    UnknownAction {
        /// The hotspot id.
        hotspot: String,
        /// The key it names.
        key: String,
    },
    /// The hotspot names an application-side result, not a button.
    NotAButton {
        /// The hotspot id.
        hotspot: String,
        /// The action.
        action: Action,
    },
    /// The action has no transition on this screen: a visible button that does
    /// nothing.
    NoTransition {
        /// The hotspot id.
        hotspot: String,
        /// The action.
        action: Action,
    },
}

/// Checks that every hotspot of a screen's layout is a working button.
#[must_use]
pub fn check_layout(screen: Screen, layout: &ScreenLayout) -> Vec<LayoutProblem> {
    let mut problems = Vec::new();
    for hotspot in layout.hotspots() {
        let name = hotspot.id.as_str().to_owned();
        let Some(action) = Action::from_key(&hotspot.action) else {
            problems.push(LayoutProblem::UnknownAction {
                hotspot: name,
                key: hotspot.action.clone(),
            });
            continue;
        };
        if action.source() != ActionSource::User {
            problems.push(LayoutProblem::NotAButton {
                hotspot: name,
                action,
            });
        } else if find(screen, action).is_none() {
            problems.push(LayoutProblem::NoTransition {
                hotspot: name,
                action,
            });
        }
    }
    problems
}

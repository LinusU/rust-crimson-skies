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
//! * Instant Action (F49-C2): [`Screen::IaSelect`], [`Screen::IaCustomize`],
//!   [`Screen::IaFlight`] and [`Screen::IaResults`] are the rows of
//!   `ui::instant_action::IA_TABLE` as front-end screens, entered from the main
//!   menu and left for it; [`FrontEndFlow`] drives
//!   `ui::instant_action::InstantActionFlow` behind them.
//! * The multiplayer lobby (F55-CU1): [`Screen::Lobby`] is reached from the
//!   main menu's `OpenLobby` and left by `Back`. Its join, host and room
//!   stages are `ui::lobby::LobbyScreen`'s own stage table;
//!   [`FrontEndFlow`] holds the screen and the [`crate::ui::lobby::LobbyLink`]
//!   it drives,
//!   and supplies what the forms need from outside through
//!   [`LobbyInputs`]: the session epoch and compatibility signature a
//!   host opens under, the resolved endpoint and client hello a join
//!   offers, and the shared `cs_net::lobby::LoadoutValidator`.
//! * [`FlightCheckView`] (in `flight_check_view`, M01-LC-FC-A) — the Flight
//!   Check screen's view model read from the mission's own data, with the
//!   gun and rocket lists unknown until the loadout is measured.
//! * [`BriefingScene`] (in `briefing`, #1750) — the original briefing map
//!   screen: `Briefing.zrd`'s script compiled into a [`Timeline`] against the
//!   voice-over's cue points, the objectives panel and the three buttons.
//! * [`FrontEnd`] (in `machine`) — the machine that walks the table.
//! * [`ScreenSession`] (in `screens`, F45-B) — the front end presented through
//!   a validated [`ScreenDeck`] of authored artwork and hotspots: pointer
//!   hit-testing through the image's own aspect-fit transform and focus in the
//!   authored order, with every action still applied by the table above.
//! * [`ImportDialog`] (in `import_dialog`) — the [`Screen::ImportDialog`]
//!   state: the legacy-import dialog that draws
//!   [`crate::ui::import::MigrationView`]'s report lines (F64-C) unchanged,
//!   offers [`Action::ConfirmImport`]/[`Action::Retry`]/[`Action::Cancel`]
//!   through the table above, and holds the
//!   [`crate::ui::import::ConfirmedImport`] a persistence layer will carry.
//! * [`review_navigation`] (in `paths`, F45-D) — the navigation review: every
//!   row of the table applied on the real machine, with every screen either
//!   reached or named as not reached and every refusal carried by its code.
//! * [`capture_screen`] (in `capture`, F45-D) — one screen drawn on the real
//!   renderer, artwork and hotspots and focus, offscreen, with a named refusal
//!   for every frame that is not evidence of a drawn screen.
//! * [`MenuAudio`] (in `sound`, FE-MENU-4) — the front end's music loop and
//!   rollover/click/text-entry cues, read from `GLOBALS.SCRIPT`'s own `sound`
//!   declarations and played over any audio device.
//! * [`FrontEndScreens`] (in `retail`, F45-D) — the original installation's
//!   front-end screen artwork: the complete inventory of both sources and the
//!   decode that feeds a capture. `retail` here is read access to the owner's
//!   files, never a run of the original executable.
//!
//! The table, the machine, the authored presentation, the navigation walk and
//! the import dialog are **designed**; no original screen list, hotspot
//! coordinate or flow was read. The dialog's rows are F64-C's own report
//! lines, drawn verbatim: a fixture report is labelled as one by the
//! producer, never by this layer. [`FrontEndScreens`] (F45-D, `retail`) reads
//! original **artwork** only
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

pub mod audio_page;
mod audio_view;
mod briefing;
mod cabin;
mod capture;
pub mod construction_armor_paint;
pub mod construction_hangar;
pub mod construction_hub;
mod construction_hub_view;
pub mod construction_panels;
pub mod construction_slots;
mod flight_check_view;
mod flow;
mod import_dialog;
mod intro;
mod machine;
mod memento;
mod movie;
mod notice_view;
pub mod ordinance_layout;
mod original;
mod pane_transform;
mod paths;
mod pilot_page;
mod pilot_view;
pub mod plane_name;
mod plane_name_view;
pub mod previous_missions;
mod previous_missions_view;
mod retail;
mod screens;
mod shell;
mod sound;
mod start_flow;
pub mod widgets;
mod window;

use std::collections::{BTreeMap, BTreeSet};

use cs_content::ui_layout::ScreenLayout;

pub use audio_page::{
    AUDIO_SECTION, AudioAccept, AudioControl, AudioPage, AudioPageError, AudioSettingsFile,
    AudioValues, SoundQuality,
};
pub use briefing::{
    BriefingButton, BriefingPlayback, BriefingScene, BriefingSceneError, DESIGNED_LINE_HEIGHT,
    DrawnElement, ElementKind, PANEL_ELEMENT, SceneUnknown, Timeline, TimelineError, Track,
};
pub use cabin::{
    CABIN_SECTION, CabinApplied, CabinError, CabinHub, FRAME_PANE, HANGAR_COUNT,
    MEMENTO_PANE as CABIN_MEMENTO_PANE, PIN_PANES, PLANE_PANE, SAVE_BUTTON,
    show_memento as show_cabin_memento,
};
pub use capture::{
    Artwork, CapturedButton, SCREEN_CAPTURE_HEIGHT, SCREEN_CAPTURE_SURFACE, SCREEN_CAPTURE_WIDTH,
    ScreenCapture, ScreenCaptureError, capture_artwork, capture_screen,
};
pub use construction_hub::{
    HUB_SECTION, HUB_UNAVAILABLE, HubError, HubFeed, HubOutcome, HubPage, Panel as HubPanel,
    READY_NOT_BUILT, SELL_NOT_BUILT, StartingLoadout,
};
pub(crate) use flight_check_view::strip_font_tag;
pub use flight_check_view::{
    Entry, FlightCheckButton, FlightCheckView, FlightCheckViewError, LOADOUT_RESOLVING_TASK,
    LOADOUT_UNKNOWN_REASON, ObjectiveRow, SlotView, objective_keys,
};
pub use flow::{
    ConstructionInputs, FlowDomainView, FlowError, FlowSetup, FrontEndFlow, InstantActionInputs,
    LoadError, LoadFlow, LoadPlan, LoadVerdict, LobbyInputs, LobbyStep, ResourceLedger,
    ResourceProblem, ScrapbookInputs,
};
pub use import_dialog::ImportDialog;
pub use intro::{INTRO_FILE, IntroMovie, IntroState, follows as intro_follows};
pub use machine::{
    ConstructionDraft, Effect, FrontEnd, LoadFailure, Loadout, LoadoutProblem, MissionOutcome,
    Outcome, PageMove, Plan, ProfileIntent, Refusal, Request,
};
pub use memento::{
    ACCEPT_BUTTON as MEMENTO_ACCEPT_BUTTON, ARROW_LEFT_BUTTON as MEMENTO_ARROW_LEFT_BUTTON,
    ARROW_RIGHT_BUTTON as MEMENTO_ARROW_RIGHT_BUTTON, CANCEL_BUTTON as MEMENTO_CANCEL_BUTTON,
    DEFAULT_MEMENTO, Direction as MementoDirection, Gate as MementoGate, MEMENTO_PANE,
    MEMENTO_SECTION, MEMENTO_TABLE_CLAIM, MEMENTO_TABLE_VA, MEMENTOS, MISSION_FLAGS_VA,
    MementoEntry, MementoScreen, MementoUnlocks, PICTURE_FOLDER, picture_art, step as memento_step,
};
pub use movie::{MovieBackdrop, MovieSource, movie_media};
pub use original::Row as LayoutRow;
pub use original::{
    ArtExtents, BUTTON_STATE_FRAMES, BoundButton, FlightCheckControl, FlightCheckSelection,
    FlightCheckSelectionError, FlightCheckSlot, LAYOUT_MEMBER, LayoutTextError, OriginalLayout,
    OriginalScreens, SCREEN_SECTIONS, ScreenHandlers, ScreenRefusal, ScreenReport, ScreenScripts,
    UnboundButton, UnboundReason, script_member,
};
pub use pane_transform::{
    CABIN_MEMENTO, CABIN_MEMENTO_POSITION, HALF_PIXEL_BIAS, PERSPECTIVE_DISTANCE, PaneTransform,
    SCALE_UNIT, TURN_RADIANS, radians as pane_rotate_radians,
};
pub use paths::{
    NavigationInputs, PathReview, PathStep, StepOutcome, SuppliedInput, review_navigation,
    review_navigation_with,
};
pub use pilot_page::{
    CANCEL_BUTTON, CONTINUE_BUTTON, DELETE_BUTTON, EDIT_ROW, EditBox, HOVER_FRAME, LIST_ROW,
    ListBox, MAX_PILOTS, PILOT_SECTION, PilotEntry, PilotNotice, PilotOutcome, PilotPage,
    PilotPageError, ROW_TEXT, SELECTED_FILL,
};
pub use plane_name::{NAME_SECTION, NameError, NameOutcome, PlaneNamePage};
pub use previous_missions::{
    MissionProgress, PreviousMissionsPage, TOC_SECTION, TocError, TocOutcome,
};
pub use retail::{
    FrontEndInventory, FrontEndScreens, GRAPHICS_PREFIX, ImageSource, MINIMUM_SCREEN_EXTENT,
    MOVIE_DIRECTORY, OriginalImage, SCREEN_CONTAINER, SoundMember, UI_IMAGE_CONTAINER,
};
pub use screens::{
    Button, ButtonView, DeckError, ScreenAssetError, ScreenAssets, ScreenDeck, ScreenSession,
    ScreenSessionError, ScreenView,
};
pub use shell::{
    ButtonEffect, ButtonVisual, CURSOR_CANDIDATES, MenuShell, SECTION_SURFACE, START_SECTION,
    ShellButton, ShellButtonView, ShellError, ShellMovie, ShellOutcome, ShellPane, ShellPaneView,
    ShellRect, ShellSection, ShellView, SkippedReason, SkippedRow, UnhandledEffect,
};
pub use sound::{
    DeclaredSound, GLOBALS_SECTION, MenuAudio, MenuAudioEvent, MenuCue, MenuMix, MenuMusic,
    MenuSamples, MenuSoundError, MenuSoundTable, SOUND_DIRECTORY, cue_sample_id, decode_wave,
    music_sample_id,
};
pub use start_flow::{
    FIRST_MISSION, FRONT_END_SMOKE_SUBDIR, MISSION_SMOKE_SUBDIR, MissionEnd, NOT_IMPLEMENTED,
    NotImplemented, Presentation, REPLAY_SMOKE_SUBDIR, RESUMED_SMOKE_SUBDIR, StartFlow,
    StartFlowError, StartFlowRequest, StartRefusal, StartStep, handover_args, presentation,
    return_args, return_args_smoke, return_to_front_end, run_start_flow, screen_of_section,
};
pub use window::{
    DEFAULT_FRONT_END_CAPTURE_DIR, FrontEndError, FrontEndRequest, FrontEndShellPlugin,
    MIN_SHELL_SMOKE_SECONDS, PROFILE_DIR_ENV, PilotSetup, SMOKE_PROFILE_SUBDIR, SMOKE_SCENE_ENV,
    ShellResource, ShellShotRecord, ShellSmokeHandle, ShellSmokePlugin, ShellSmokeReport,
    SmokeScene, headless_app, headless_app_audio, headless_app_audio_page,
    headless_app_construction, headless_app_flow, headless_app_intro, headless_app_pilots,
    headless_app_plane_name, headless_app_previous_missions, run_front_end, shell_app,
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
    /// The legacy-import dialog (F64-C's report; see [`import_dialog`]).
    ImportDialog,
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
    /// Instant Action: choose a preset or start a custom scenario (F49-C2).
    IaSelect,
    /// Instant Action: edit the custom scenario.
    IaCustomize,
    /// Instant Action: a scenario session is running.
    IaFlight,
    /// Instant Action: the session ended; retry or finish.
    IaResults,
    /// The F55-C multiplayer lobby (F55-CU1): the join / host / room
    /// screen the main menu's `OpenLobby` reaches. Its join, host and
    /// room stages are `ui::lobby::LobbyScreen`'s own table; the front
    /// end only owns entering and leaving.
    Lobby,
    /// The credits, opened from the main menu's Credits button. The original's
    /// content for it is unread; the screen only owns entering and leaving.
    Credits,
}

impl Screen {
    /// Every screen.
    pub const ALL: [Self; 23] = [
        Self::InstallSelect,
        Self::ContentDiagnosis,
        Self::MainMenu,
        Self::ProfileSelect,
        Self::ImportDialog,
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
        Self::IaSelect,
        Self::IaCustomize,
        Self::IaFlight,
        Self::IaResults,
        Self::Lobby,
        Self::Credits,
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
    /// Open the campaign (profile list) from the main menu: the original's
    /// Campaign button.
    OpenCampaign,
    /// Open the credits from the main menu: the original's Credits button.
    OpenCredits,
    /// Start a new profile.
    NewProfile,
    /// Continue an existing profile.
    ContinueProfile,
    /// Accept the profile choice.
    ConfirmProfile,
    /// Offer a legacy source to the import dialog.
    OpenImportDialog,
    /// Accept the import dialog's report: persist the confirmed import.
    ConfirmImport,
    /// Open settings.
    OpenSettings,
    /// Open the scrapbook.
    OpenScrapbook,
    /// Open the Instant Action screens from the main menu.
    OpenInstantAction,
    /// Open the multiplayer lobby from the main menu (F55-CU1).
    OpenLobby,
    /// Start a custom Instant Action scenario from the selected preset.
    StartCustom,
    /// Record the ended Instant Action session and return to the main menu.
    Finish,
    /// The Instant Action session ended with an outcome.
    InstantActionEnded,
    /// Turn to the scrapbook's next page.
    NextPage,
    /// Turn to the scrapbook's previous page.
    PrevPage,
    /// Select the scrapbook entry the presentation declared.
    SelectEntry,
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
    pub const ALL: [Self; 41] = [
        Self::InstallVerified,
        Self::InstallRejected,
        Self::ContentMissing,
        Self::ChooseAnotherInstall,
        Self::Quit,
        Self::OpenCampaign,
        Self::OpenCredits,
        Self::NewProfile,
        Self::ContinueProfile,
        Self::ConfirmProfile,
        Self::OpenImportDialog,
        Self::ConfirmImport,
        Self::OpenSettings,
        Self::OpenScrapbook,
        Self::OpenInstantAction,
        Self::OpenLobby,
        Self::StartCustom,
        Self::Finish,
        Self::InstantActionEnded,
        Self::NextPage,
        Self::PrevPage,
        Self::SelectEntry,
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
            Self::OpenCampaign => "open-campaign",
            Self::OpenCredits => "open-credits",
            Self::NewProfile => "new-profile",
            Self::ContinueProfile => "continue-profile",
            Self::ConfirmProfile => "confirm-profile",
            Self::OpenImportDialog => "open-import-dialog",
            Self::ConfirmImport => "confirm-import",
            Self::OpenSettings => "open-settings",
            Self::OpenScrapbook => "open-scrapbook",
            Self::OpenInstantAction => "open-instant-action",
            Self::OpenLobby => "open-lobby",
            Self::StartCustom => "start-custom",
            Self::Finish => "finish",
            Self::InstantActionEnded => "instant-action-ended",
            Self::NextPage => "next-page",
            Self::PrevPage => "prev-page",
            Self::SelectEntry => "select-entry",
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
            | Self::MissionFailed
            | Self::InstantActionEnded => ActionSource::System,
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
    /// As [`Guard::DiscardsDraft`] for the Instant Action scenario draft,
    /// whose leaving is itself a domain request (it ends or releases the
    /// Instant Action session state), so the row carries one.
    DiscardsScenarioDraft,
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
    /// Persist the import dialog's confirmed import.
    ConfirmImport,
    /// Select the flight-check aircraft and ammunition together.
    CommitLoadout,
    /// Apply the mission outcome to the campaign.
    ApplyOutcome,
    /// Abandon the running mission.
    AbandonMission,
    /// Open the paged scrapbook on the selected profile's records.
    OpenScrapbook,
    /// Turn the scrapbook's page in the direction the action names.
    ScrapbookPage,
    /// Select the scrapbook entry the presentation declared.
    ScrapbookSelect,
    /// Launch the selected entry's replay.
    LaunchReplay,
    /// Enter the Instant Action flow: it exists only when the application
    /// supplied its catalog and profile.
    OpenInstantAction,
    /// Seed a custom scenario draft from the selected preset.
    StartCustomScenario,
    /// Lower the Instant Action selection and begin a session generation.
    LaunchInstantAction,
    /// Leave the current Instant Action screen one step back (discarding the
    /// draft, abandoning the session or leaving for the menu).
    InstantActionBack,
    /// Retry the Instant Action scenario in a new session generation.
    RetryInstantAction,
    /// Record the ended session, end its generation and leave for the menu.
    FinishInstantAction,
    /// Settle the running session from its reported snapshot.
    EndInstantAction,
    /// Enter the lobby: it exists only when the application supplied the
    /// session epoch, compatibility signature, client hello and shared
    /// `LoadoutValidator` the forms need (`LobbyInputs`, F55-CU1).
    OpenLobby,
    /// Leave the lobby screen one `LobbyScreen::back` step — a form's
    /// `Back::Exit`, which discards the drafts. Inside the room `back`
    /// only tears the room down, so `FrontEndFlow::lobby_back` performs
    /// that step first and this request is refused while the room is
    /// still open.
    LobbyBack,
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
    // The main menu carries the original's six buttons (Campaign, Instant
    // Action, Multiplayer, Preferences, Credits, Quit; `MainMenu` in
    // `ASSETS/LAYOUT.CSV`), in that order. The original's focus order is
    // unread, so this order is a design choice. Campaign opens the profile
    // screen, where the new/continue choice is made: the original's profile
    // list screen is unread, so carrying that choice there is a stated design
    // choice (docs/findings/2026-10-19-f45-a-1-original-screen-set.md).
    row(
        S::MainMenu,
        A::OpenCampaign,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(
        S::MainMenu,
        A::OpenInstantAction,
        S::IaSelect,
        G::None,
        R::OpenInstantAction,
    ),
    // The lobby (F55-CU1): another profile-free branch off the menu.
    row(S::MainMenu, A::OpenLobby, S::Lobby, G::None, R::OpenLobby),
    row(S::MainMenu, A::OpenSettings, S::Settings, G::None, R::None),
    row(S::MainMenu, A::OpenCredits, S::Credits, G::None, R::None),
    row(
        S::MainMenu,
        A::ContentMissing,
        S::ContentDiagnosis,
        G::None,
        R::None,
    ),
    row(S::MainMenu, A::Quit, S::MainMenu, G::None, R::None),
    // The profile choice is made on the profile screen itself.
    row(
        S::ProfileSelect,
        A::NewProfile,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(
        S::ProfileSelect,
        A::ContinueProfile,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(
        S::ProfileSelect,
        A::ConfirmProfile,
        S::Cabin,
        G::None,
        R::OpenProfile,
    ),
    row(
        S::ProfileSelect,
        A::OpenImportDialog,
        S::ImportDialog,
        G::None,
        R::None,
    ),
    row(S::ProfileSelect, A::Back, S::MainMenu, G::None, R::None),
    // The legacy-import dialog (F64-C). Confirm asks the domain for the one
    // transaction this screen has — persisting the confirmed import — and only
    // when the report says it may (`MigrationView::confirmable`); the report
    // and the confirmed value stay in the dialog, so the screen never moves
    // off it on an action the domain cannot take yet. Retry re-runs the whole
    // pipeline over the same source; Cancel is the teardown, and the one way
    // out of the dialog.
    row(
        S::ImportDialog,
        A::ConfirmImport,
        S::ImportDialog,
        G::None,
        R::ConfirmImport,
    ),
    row(S::ImportDialog, A::Retry, S::ImportDialog, G::None, R::None),
    row(
        S::ImportDialog,
        A::Cancel,
        S::ProfileSelect,
        G::None,
        R::None,
    ),
    row(S::Settings, A::Back, S::MainMenu, G::None, R::None),
    row(S::Cabin, A::OpenBriefing, S::Briefing, G::None, R::None),
    row(
        S::Cabin,
        A::OpenScrapbook,
        S::Scrapbook,
        G::None,
        R::OpenScrapbook,
    ),
    row(
        S::Cabin,
        A::OpenConstruction,
        S::Construction,
        G::None,
        R::None,
    ),
    row(S::Cabin, A::Back, S::MainMenu, G::None, R::CloseProfile),
    row(
        S::Scrapbook,
        A::NextPage,
        S::Scrapbook,
        G::None,
        R::ScrapbookPage,
    ),
    row(
        S::Scrapbook,
        A::PrevPage,
        S::Scrapbook,
        G::None,
        R::ScrapbookPage,
    ),
    row(
        S::Scrapbook,
        A::SelectEntry,
        S::Scrapbook,
        G::None,
        R::ScrapbookSelect,
    ),
    row(
        S::Scrapbook,
        A::Launch,
        S::Loading,
        G::None,
        R::LaunchReplay,
    ),
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
    row(
        S::Results,
        A::OpenScrapbook,
        S::Scrapbook,
        G::None,
        R::OpenScrapbook,
    ),
    row(S::Results, A::ReturnToCabin, S::Cabin, G::None, R::None),
    row(
        S::Results,
        A::ReturnToMenu,
        S::MainMenu,
        G::None,
        R::CloseProfile,
    ),
    row(S::Results, A::Back, S::Cabin, G::None, R::None),
    // The Instant Action screens (F49-C2) mirror `ui::instant_action::IA_TABLE`
    // screen for screen: every row is one `IaAction` the domain applies before
    // the machine moves, and a refusal of it leaves this table where it was.
    // `Back` on the results screen is the one way out that is not a retry, and
    // the Instant Action flow has a single way to leave a finished session —
    // `Finish`, which records it — so Back asks for exactly that.
    row(
        S::IaSelect,
        A::Launch,
        S::IaFlight,
        G::None,
        R::LaunchInstantAction,
    ),
    row(
        S::IaSelect,
        A::StartCustom,
        S::IaCustomize,
        G::None,
        R::StartCustomScenario,
    ),
    row(
        S::IaSelect,
        A::Back,
        S::MainMenu,
        G::DiscardsScenarioDraft,
        R::InstantActionBack,
    ),
    row(
        S::IaCustomize,
        A::Launch,
        S::IaFlight,
        G::None,
        R::LaunchInstantAction,
    ),
    row(
        S::IaCustomize,
        A::Back,
        S::IaSelect,
        G::DiscardsScenarioDraft,
        R::InstantActionBack,
    ),
    row(
        S::IaFlight,
        A::InstantActionEnded,
        S::IaResults,
        G::None,
        R::EndInstantAction,
    ),
    row(
        S::IaFlight,
        A::Back,
        S::IaSelect,
        G::None,
        R::InstantActionBack,
    ),
    row(
        S::IaResults,
        A::Retry,
        S::IaFlight,
        G::None,
        R::RetryInstantAction,
    ),
    row(
        S::IaResults,
        A::Finish,
        S::MainMenu,
        G::None,
        R::FinishInstantAction,
    ),
    row(
        S::IaResults,
        A::Back,
        S::MainMenu,
        G::None,
        R::FinishInstantAction,
    ),
    // The lobby's only front-end row (F55-CU1): `Back` on the lobby
    // screen asks `LobbyScreen::back`, which on a join or host form is
    // `Back::Exit` — the draft-discarding leave for the menu. Inside the
    // room `back` only leaves the room, so `FrontEndFlow::lobby_back`
    // performs that first; this row is refused while the room is open.
    row(S::Lobby, A::Back, S::MainMenu, G::None, R::LobbyBack),
    row(S::Credits, A::Back, S::MainMenu, G::None, R::None),
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
        S::Flight | S::IaFlight => (InputContext::Flight, AudioScope::Flight, true),
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

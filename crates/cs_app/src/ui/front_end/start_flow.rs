//! The player-facing start flow: bare `cs --cs-path <retail>` (Rally #1830,
//! `PARITY5-START-FLOW.02`, part of #1747).
//!
//! The window is the [`MenuShell`] presenter of `--front-end` (the original
//! `ASSETS/LAYOUT.CSV` sections, mouse and keyboard), but **what may happen is
//! decided by the F45 [`FrontEnd`] machine**: the shell proposes a move (a
//! button started another screen script, a `terminate` button was pressed), the
//! [`StartFlow`] looks the move up in the state table ([`super::TABLE`]) and
//! either applies it or refuses it by name, and a refused move is undone on the
//! shell. The machine starts on `InstallSelect` and is taken to the main menu
//! by the table's own `InstallVerified` row (the shell opened the installation
//! before the window existed).
//!
//! # Where it stops, and why
//!
//! * A screen whose presenter has not landed is **refused**, never replaced
//!   with a stand-in: see [`presentation`] for the screens and the tasks that
//!   own them (the pilot-selection page #1835, the cabin hub #1840 …). The
//!   refusal is a [`StartRefusal::NotYetPresented`] naming the screen and its
//!   task.
//! * The Briefing, the Flight Check, loading and flight are presented by the
//!   mission session (`cs --mission`, [`crate::mission_session`]), whose
//!   `FlightCheckPhase` walks its own machine through Briefing, Flight Check
//!   and **Fly Mission**. Entering [`Screen::Briefing`] from here is therefore
//!   the hand-over: [`StartStep::Launch`], carried out by [`handover_args`] —
//!   the same `--cs-path <dir> --mission <id>` command a player could type.
//!   It is a fresh process image on purpose (winit allows one event loop per
//!   process), so `--mission` behaves exactly as it does today.
//! * Esc is bound where the original binds it. The measured finding
//!   `docs/findings/2026-10-22-parity5-start-flow-transitions.md` found no Esc
//!   handler on the main menu, Campaign, Flight Check or Preferences pages,
//!   and one on Credits; so [`StartFlow::escape`] steps back from Credits only
//!   and ignores Esc elsewhere (in particular it never quits). Cancel, Return
//!   and Quit are buttons: their moves come from the layout and the table.
//! * Domain requests (`OpenProfile`, `CloseProfile`) are **recorded**
//!   ([`StartFlow::requests`]); the pilot page (#1835, [`super::PilotPage`])
//!   carries them out against the profile session and reports the choice
//!   through [`StartFlow::choose_pilot`].
//!
//! Nothing here is `verified_original`: the machine's table is designed, the
//! transitions it is checked against are static readings.

use std::fmt;
use std::path::PathBuf;

use bevy::app::AppExit;

use super::window::{ShellResource, shell_app_with};
use super::{
    Action, ActionSource, Effect, FrontEnd, MenuShell, Request, SCREEN_SECTIONS, Screen,
    ShellOutcome, find, rows_on,
};
use super::{FrontEndError, FrontEndRequest};

/// The mission a new campaign starts on: the first mission, since this flow
/// keeps no per-pilot progress yet (the original selects "the next one" from
/// the pilot's completed count; persistence is #1835).
pub const FIRST_MISSION: &str = "M01";

/// How a flight the start flow handed over to ended, when the player chose to
/// come back to the front end (task #1831 `PARITY5-START-FLOW.03`).
///
/// A mission that ends any other way (the window closed, a settled success
/// that has no screen yet) simply ends the process, as `--mission` always has.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionEnd {
    /// The failure debrief's **Return to Cabin** (`SB_B_RETURNPC`): the table's
    /// `Flight --MissionFailed--> Results --ReturnToCabin--> Cabin`.
    ReturnedToCabin,
    /// The Esc pause map's **Quit**: the table's
    /// `Flight --Pause--> Pause --AbortMission--> Cabin`. The target is the
    /// table's design; the original's Quit handler is unread (finding
    /// `2026-10-22-parity5-start-flow-transitions.md`).
    Abandoned,
}

impl MissionEnd {
    /// The value of `--resume-start-flow`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Self::ReturnedToCabin => "cabin",
            Self::Abandoned => "abandoned",
        }
    }

    /// The end `--resume-start-flow <key>` names.
    #[must_use]
    pub fn from_key(key: &str) -> Option<Self> {
        [Self::ReturnedToCabin, Self::Abandoned]
            .into_iter()
            .find(|end| end.key() == key)
    }
}

/// What `cs --cs-path <retail>` was asked to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StartFlowRequest {
    /// The original installation root.
    pub cs_path: PathBuf,
    /// `--resume-start-flow <end>`: the flow comes back from a mission that
    /// ended this way and opens on the cabin instead of the main menu.
    pub resume: Option<MissionEnd>,
}

/// How a front-end screen is put in front of the player.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Presentation {
    /// The `--front-end` shell draws the screen's `LAYOUT.CSV` section.
    Shell,
    /// The mission session presents it (Briefing → Flight Check → Fly
    /// Mission): entering it is the hand-over to `--mission`.
    MissionLaunch,
    /// No presenter has landed. The owning task, when one exists.
    Pending {
        /// What the screen is.
        what: &'static str,
        /// The Rally task that builds it.
        task: Option<u32>,
    },
}

/// How `screen` is presented today.
#[must_use]
pub fn presentation(screen: Screen) -> Presentation {
    match screen {
        // The cabin is the `PassengerCabin` layout section's own buttons; the
        // hub's photograph, pins and pilot data are #1840's. The pilot page
        // and the Instant Action selection open as their `LAYOUT.CSV`
        // sections (FE-MENU-2): the controls #1835 and the Instant Action
        // flow own stay drawn-but-recorded until those land.
        Screen::MainMenu
        | Screen::Settings
        | Screen::Credits
        | Screen::Cabin
        | Screen::ProfileSelect
        | Screen::IaSelect => Presentation::Shell,
        Screen::Briefing
        | Screen::Recon
        | Screen::FlightCheck
        | Screen::Loading
        | Screen::Flight => Presentation::MissionLaunch,
        Screen::Results => Presentation::Pending {
            what: "the debrief",
            task: Some(1655),
        },
        Screen::Pause | Screen::PauseSettings => Presentation::Pending {
            what: "the Esc pause map",
            task: Some(1740),
        },
        Screen::Scrapbook => Presentation::Pending {
            what: "the Previous Missions list",
            task: None,
        },
        Screen::Construction => Presentation::Pending {
            what: "Plane Construction",
            task: None,
        },
        Screen::InstallSelect | Screen::ContentDiagnosis | Screen::ImportDialog => {
            Presentation::Pending {
                what: "the installation and import screens",
                task: None,
            }
        }
        Screen::IaCustomize | Screen::IaFlight | Screen::IaResults => Presentation::Pending {
            what: "Instant Action",
            task: None,
        },
        Screen::Lobby => Presentation::Pending {
            what: "Multiplayer",
            task: None,
        },
    }
}

/// Layout sections that start a front-end screen besides
/// [`SCREEN_SECTIONS`]' pairs. The names are the `ScriptToExe` values of the
/// main menu's buttons; the pairing is a design choice, like the first list.
const EXTRA_SECTIONS: &[(&str, Screen)] = &[
    ("Campaign", Screen::ProfileSelect),
    ("Credits", Screen::Credits),
    ("InstantAction", Screen::IaSelect),
    ("MultiPlayerMain", Screen::Lobby),
];

/// The F45 screen a layout section is the original of.
#[must_use]
pub fn screen_of_section(section: &str) -> Option<Screen> {
    SCREEN_SECTIONS
        .iter()
        .chain(EXTRA_SECTIONS)
        .find(|(name, _)| name.eq_ignore_ascii_case(section))
        .map(|(_, screen)| *screen)
}

/// Buttons whose press the shell cannot run from the layout (a native call, or
/// a script `if`), and the table action each stands for
/// (`docs/findings/2026-10-22-parity5-start-flow-transitions.md`):
///
/// * the Cabin's Next Mission is `E 2104(-2)` then `A 6`, open the briefing;
/// * Preferences' main-menu button is `activate(@mainmenu@)` +
///   `script_end "preferences.script"` when the in-game flag `$$KW$$` is clear,
///   which it is in the start flow;
/// * the main menu's Multiplayer is `activate(@MultiPlayerMain@)`, a section
///   the layout does not hold — the press stands for the table's
///   `OpenLobby`, which `Multiplayer`'s presenter being unbuilt refuses
///   by name.
const BUTTON_ACTIONS: &[(&str, Action)] = &[
    ("PC_B_NEWMISSION", Action::OpenBriefing),
    ("PF_B_MAINMENU", Action::Back),
    ("MM_B_MULTIPLAYER", Action::OpenLobby),
];

/// The layout section that presents `screen` through the shell.
fn section_of_screen(screen: Screen) -> Option<&'static str> {
    SCREEN_SECTIONS
        .iter()
        .chain(EXTRA_SECTIONS)
        .find(|(_, paired)| *paired == screen)
        .map(|(name, _)| *name)
}

/// Why a proposed move did not happen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartRefusal {
    /// The layout section is not the original of any front-end screen.
    SectionHasNoScreen {
        /// The section as the layout writes it.
        section: String,
    },
    /// The table has no user action from the current screen to that one.
    NoTransition {
        /// The screen the machine is on.
        from: Screen,
        /// The screen the shell tried to reach.
        to: Screen,
    },
    /// The screen's presenter has not landed.
    NotYetPresented {
        /// The screen.
        screen: Screen,
        /// What it is.
        what: &'static str,
        /// The task that builds it.
        task: Option<u32>,
    },
    /// The machine refused the action.
    Machine(String),
}

impl fmt::Display for StartRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::SectionHasNoScreen { section } => write!(
                f,
                "the {section} screen is not part of the front-end state table"
            ),
            Self::NoTransition { from, to } => {
                write!(f, "the state table has no button from {from:?} to {to:?}")
            }
            Self::NotYetPresented {
                screen,
                what,
                task: Some(task),
            } => write!(f, "{screen:?} ({what}) is not built yet: task #{task}"),
            Self::NotYetPresented {
                screen,
                what,
                task: None,
            } => write!(f, "{screen:?} ({what}) is not built yet"),
            Self::Machine(reason) => write!(f, "{reason}"),
        }
    }
}

/// What a move did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StartStep {
    /// Nothing changed (an Esc the original does not bind, a press with no
    /// table counterpart that the shell already recorded).
    Stay,
    /// The machine moved between two screens the shell presents.
    Moved {
        /// The screen before.
        from: Screen,
        /// The screen after.
        to: Screen,
    },
    /// The machine reached a screen the mission session presents: hand over to
    /// `--mission <mission>`.
    Launch {
        /// The mission label.
        mission: String,
    },
    /// The application should end.
    Quit,
    /// The move was refused; the machine is where it was.
    Refused(StartRefusal),
}

/// The F45 machine behind the start-flow window.
#[derive(Clone, Debug)]
pub struct StartFlow {
    front_end: FrontEnd,
    mission: String,
    siblings_landed: bool,
    requests: Vec<Request>,
    refusals: Vec<StartRefusal>,
    launch: Option<String>,
}

impl Default for StartFlow {
    fn default() -> Self {
        Self::new()
    }
}

impl StartFlow {
    /// A machine on the main menu: the installation was verified before the
    /// window opened, so the table's `InstallVerified` row has been taken.
    #[must_use]
    pub fn new() -> Self {
        let mut front_end = FrontEnd::new();
        front_end
            .apply(Action::InstallVerified)
            .expect("the table's first row takes the start screen to the main menu");
        Self {
            front_end,
            mission: FIRST_MISSION.to_owned(),
            siblings_landed: false,
            requests: Vec::new(),
            refusals: Vec::new(),
            launch: None,
        }
    }

    /// The machine after the player left a flight handed over from here: the
    /// walk the table prescribes from the main menu to the mission and back,
    /// so the world the flight acquired is released and the machine stands on
    /// the cabin. The mission is the one [`FIRST_MISSION`] names.
    ///
    /// The pilot-selection page has not landed (#1835), so the walk takes the
    /// table's `ProfileSelect` rows itself; the requests they ask for
    /// (`OpenProfile`, `ApplyOutcome` …) are recorded in [`Self::requests`].
    ///
    /// # Errors
    ///
    /// [`StartRefusal::Machine`] when the table refuses a step of the walk.
    pub fn after_mission(end: MissionEnd) -> Result<Self, StartRefusal> {
        let mut flow = Self::new();
        let to_flight = [
            Action::OpenCampaign,
            // The pilot already exists: the flight was theirs.
            Action::ContinueProfile,
            Action::ConfirmProfile,
            Action::OpenBriefing,
            Action::ContinueToFlightCheck,
            Action::Launch,
            Action::LoadSucceeded,
        ];
        let leave: &[Action] = match end {
            MissionEnd::ReturnedToCabin => &[Action::MissionFailed, Action::ReturnToCabin],
            MissionEnd::Abandoned => &[Action::Pause, Action::AbortMission],
        };
        for action in to_flight.iter().chain(leave) {
            // The Flight Check that launched the flight is the mission
            // session's, so the machine's own loadout draft is empty: the
            // launch row's guard is not asked again.
            let outcome = flow
                .front_end
                .apply_replayed(*action)
                .map_err(|refusal| StartRefusal::Machine(refusal.to_string()))?;
            for effect in outcome.effects {
                if let Effect::Request(request) = effect {
                    flow.requests.push(request);
                }
            }
        }
        Ok(flow)
    }

    /// The layout section the shell shows for the machine's screen, when the
    /// shell presents it.
    #[must_use]
    pub fn section(&self) -> Option<&'static str> {
        section_of_screen(self.screen())
    }

    /// The same flow with every [`Presentation::Pending`] screen treated as
    /// presented: what the walk will do once the sibling tasks have landed,
    /// for a test to exercise the table beyond today's frontier.
    #[must_use]
    pub fn assuming_siblings_landed(mut self) -> Self {
        self.siblings_landed = true;
        self
    }

    /// The machine.
    #[must_use]
    pub fn front_end(&self) -> &FrontEnd {
        &self.front_end
    }

    /// The screen the machine is on.
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.front_end.screen()
    }

    /// The domain requests the machine asked for, in order. Nothing consumes
    /// them yet (#1835).
    #[must_use]
    pub fn requests(&self) -> &[Request] {
        &self.requests
    }

    /// Every refusal so far, in order.
    #[must_use]
    pub fn refusals(&self) -> &[StartRefusal] {
        &self.refusals
    }

    /// The mission the hand-over was made for, once it was.
    #[must_use]
    pub fn handover(&self) -> Option<&str> {
        self.launch.as_deref()
    }

    /// The shell started the layout section `section`: look up the move.
    pub fn enter_section(&mut self, section: &str) -> StartStep {
        let Some(to) = screen_of_section(section) else {
            return self.refuse(StartRefusal::SectionHasNoScreen {
                section: section.to_owned(),
            });
        };
        let from = self.screen();
        let action = rows_on(from)
            .find(|row| {
                row.to == to
                    && row.action.source() == ActionSource::User
                    && row.action != Action::Quit
            })
            .map(|row| row.action);
        match action {
            Some(action) => self.go(action),
            None => self.refuse(StartRefusal::NoTransition { from, to }),
        }
    }

    /// A button with no screen script was pressed: the table action it stands
    /// for, when it is one of the native calls this flow models.
    pub fn press_button(&mut self, id: &str) -> StartStep {
        match BUTTON_ACTIONS
            .iter()
            .find(|(button, _)| button.eq_ignore_ascii_case(id))
        {
            Some((_, action)) => self.go(*action),
            None => StartStep::Stay,
        }
    }

    /// Applies a table action directly: what a screen's own control does when
    /// it is not a layout button (the pilot page's choice and Continue).
    pub fn press_machine(&mut self, action: Action) -> StartStep {
        self.go(action)
    }

    /// A `terminate` button: the table's `Quit` row.
    pub fn terminate(&mut self) -> StartStep {
        self.go(Action::Quit)
    }

    /// The pilot page chose a pilot (Continue or Enter): the table's
    /// `NewProfile`/`ContinueProfile` then `ConfirmProfile` rows, which end
    /// on the cabin. A refusal leaves the machine where the first refused
    /// step left it.
    pub fn choose_pilot(&mut self, created: bool) -> StartStep {
        let choice = if created {
            Action::NewProfile
        } else {
            Action::ContinueProfile
        };
        match self.go(choice) {
            StartStep::Moved { .. } | StartStep::Stay => self.go(Action::ConfirmProfile),
            other => other,
        }
    }

    /// Esc. Bound on Credits (the original's `gui_char` there) and, as a
    /// design addition the task asks for, on the pilot page, where it is
    /// Cancel; every other screen ignores it, the main menu included.
    pub fn escape(&mut self) -> StartStep {
        if matches!(self.screen(), Screen::Credits | Screen::ProfileSelect) {
            self.go(Action::Back)
        } else {
            StartStep::Stay
        }
    }

    /// Routes what the shell reported for an input: the glue the window and
    /// the tests share. A refused move is undone on the shell.
    pub fn route(&mut self, shell: &mut MenuShell, outcome: ShellOutcome) -> StartStep {
        match outcome {
            ShellOutcome::Entered { section } => {
                let step = self.enter_section(&section);
                if matches!(step, StartStep::Refused(_)) {
                    // The shell already pushed the section; step back off it.
                    let _ = shell.escape();
                }
                step
            }
            ShellOutcome::Unhandled { id, .. } => {
                let step = self.press_button(&id);
                if let StartStep::Moved { to, .. } = step
                    && let Some(section) = section_of_screen(to)
                {
                    // The button's script activated that screen: show it.
                    let _ = shell.open(section);
                }
                step
            }
            ShellOutcome::Quit => self.terminate(),
            ShellOutcome::Back { .. } | ShellOutcome::Nothing => StartStep::Stay,
        }
    }

    /// Routes Esc: the shell steps back only where the original binds it.
    pub fn route_escape(&mut self, shell: &mut MenuShell) -> StartStep {
        let step = self.escape();
        if matches!(step, StartStep::Moved { .. }) {
            let _ = shell.escape();
        }
        step
    }

    fn refuse(&mut self, refusal: StartRefusal) -> StartStep {
        self.refusals.push(refusal.clone());
        StartStep::Refused(refusal)
    }

    /// Applies one table action, or refuses it with the machine untouched.
    fn go(&mut self, action: Action) -> StartStep {
        let from = self.screen();
        let Some(row) = find(from, action) else {
            return self.refuse(StartRefusal::Machine(format!(
                "{from:?} has no action {}",
                action.key()
            )));
        };
        if !self.siblings_landed
            && row.to != from
            && let Presentation::Pending { what, task } = presentation(row.to)
        {
            return self.refuse(StartRefusal::NotYetPresented {
                screen: row.to,
                what,
                task,
            });
        }
        let outcome = match self.front_end.apply(action) {
            Ok(outcome) => outcome,
            Err(refusal) => return self.refuse(StartRefusal::Machine(refusal.to_string())),
        };
        let mut quit = false;
        for effect in &outcome.effects {
            match effect {
                Effect::Request(request) => self.requests.push(request.clone()),
                Effect::ExitApplication => quit = true,
                Effect::Release(_) | Effect::Acquire(_) | Effect::AskDiscard => {}
            }
        }
        if quit {
            return StartStep::Quit;
        }
        if presentation(outcome.to) == Presentation::MissionLaunch {
            self.launch = Some(self.mission.clone());
            return StartStep::Launch {
                mission: self.mission.clone(),
            };
        }
        StartStep::Moved {
            from: outcome.from,
            to: outcome.to,
        }
    }
}

/// The command line the hand-over runs: exactly what a player types for the
/// mission, so `--mission` behaves as it always has. `--from-start-flow`
/// asks the mission to come back here when the player leaves it for the
/// cabin (see [`return_args`]).
#[must_use]
pub fn handover_args(request: &StartFlowRequest, mission: &str) -> Vec<String> {
    vec![
        "--cs-path".to_owned(),
        request.cs_path.display().to_string(),
        "--mission".to_owned(),
        mission.to_owned(),
        "--from-start-flow".to_owned(),
    ]
}

/// The command line the mission runs to come back to the front end.
#[must_use]
pub fn return_args(cs_path: &std::path::Path, end: MissionEnd) -> Vec<String> {
    vec![
        "--cs-path".to_owned(),
        cs_path.display().to_string(),
        "--resume-start-flow".to_owned(),
        end.key().to_owned(),
    ]
}

/// Replaces the finished mission process with the front end, opened on the
/// cabin. A new process image, again because winit allows one event loop per
/// process; the mission's world was torn down before this is called.
///
/// # Errors
///
/// [`StartFlowError::Handover`] when the front end cannot start.
pub fn return_to_front_end(
    cs_path: &std::path::Path,
    end: MissionEnd,
) -> Result<(), StartFlowError> {
    hand_over(&return_args(cs_path, end))
}

/// Why the start flow ended in failure.
#[derive(Debug)]
pub enum StartFlowError {
    /// The window could not open or ended with an error.
    FrontEnd(FrontEndError),
    /// The hand-over to the mission could not start.
    Handover(String),
}

impl fmt::Display for StartFlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::FrontEnd(error) => write!(f, "{error}"),
            Self::Handover(reason) => write!(f, "the hand-over to the mission failed: {reason}"),
        }
    }
}

impl std::error::Error for StartFlowError {}

/// Runs `cs --cs-path <retail>`: opens the window on the main menu, and when
/// the player reaches the mission session's screens replaces this process with
/// `cs --cs-path <retail> --mission <id>`.
///
/// # Errors
///
/// [`StartFlowError::FrontEnd`] when the installation cannot be read or the
/// window ends in error, [`StartFlowError::Handover`] when the mission command
/// cannot start. A normal quit returns `Ok`.
pub fn run_start_flow(request: &StartFlowRequest) -> Result<(), StartFlowError> {
    let window = FrontEndRequest {
        cs_path: request.cs_path.clone(),
        smoke: None,
    };
    let flow = match request.resume {
        Some(end) => StartFlow::after_mission(end)
            .map_err(|refusal| StartFlowError::Handover(refusal.to_string()))?,
        None => StartFlow::new(),
    };
    let (mut app, _) = shell_app_with(&window, Some(flow)).map_err(StartFlowError::FrontEnd)?;
    let exit = app.run();
    let mission = app
        .world()
        .get_resource::<ShellResource>()
        .and_then(ShellResource::flow)
        .and_then(StartFlow::handover)
        .map(str::to_owned);
    drop(app);
    if let AppExit::Error(code) = exit {
        return Err(StartFlowError::FrontEnd(FrontEndError::Exit(format!(
            "exit code {code}"
        ))));
    }
    match mission {
        Some(mission) => hand_over(&handover_args(request, &mission)),
        None => Ok(()),
    }
}

/// Replaces this process with the same binary run on `args`. Winit allows one
/// event loop per process, so the mission cannot open its window from here.
#[cfg(unix)]
fn hand_over(args: &[String]) -> Result<(), StartFlowError> {
    use std::os::unix::process::CommandExt;
    let exe = std::env::current_exe().map_err(|e| StartFlowError::Handover(e.to_string()))?;
    let error = std::process::Command::new(exe).args(args).exec();
    Err(StartFlowError::Handover(error.to_string()))
}

/// As the unix variant, as a child the caller waits for.
#[cfg(not(unix))]
fn hand_over(args: &[String]) -> Result<(), StartFlowError> {
    let exe = std::env::current_exe().map_err(|e| StartFlowError::Handover(e.to_string()))?;
    let status = std::process::Command::new(exe)
        .args(args)
        .status()
        .map_err(|e| StartFlowError::Handover(e.to_string()))?;
    if status.success() {
        Ok(())
    } else {
        Err(StartFlowError::Handover(format!(
            "the mission ended with {status}"
        )))
    }
}

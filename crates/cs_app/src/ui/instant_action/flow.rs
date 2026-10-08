//! The Instant Action screens: selection, customization, loadout and the
//! session they launch (F49-C).
//!
//! Spec: `specs/F49-instant-action-presets-and-custom-scenarios.md`, stage
//! `### F49-C`. Shared contracts: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path", "Back from a
//! draft discards or explicitly confirms changes", "Dropdown selection is a
//! content id, never a transient row number", the path "IA customize ->
//! finish -> main") and `docs/contracts/STATE-TRANSACTIONS.md` ("Retry restores
//! the authored initial state, not a mutated copy of the just-failed world",
//! "`end_session` ... for that generation", "Results, previous targets and
//! delayed callbacks are always generation-qualified", "Persistent profile
//! data receives only an explicit outcome transaction").
//!
//! The earlier stages built the two halves this wires together:
//! [`super::lower_preset`] / [`super::lower_custom`] turn a selection into a
//! [`super::LoweredScenario`] (F49-A), and [`super::evaluate_outcome`] decides
//! whether one has ended and with what [`ScenarioOutcome`] (F49-B). Neither
//! owns a screen. This module is the screen:
//!
//! * [`IA_TABLE`] — one [`IaRow`] per `(screen, action)`: where the transition
//!   goes and what it is guarded by. [`InstantActionFlow::apply`] walks it, so
//!   every screen and every Back/Cancel path is written down once and a test
//!   can prove the table total (every screen reachable, every screen a path
//!   back to [`IaScreen::Select`]).
//! * [`InstantActionFlow`] — the machine: what the player selected
//!   ([`ScenarioSelection`]), the custom draft being edited, the live session
//!   and its [`ScenarioOutcome`], the last launch refusal and the profile's
//!   [`IaRecordBook`]. Setters ([`InstantActionFlow::select_preset`],
//!   [`InstantActionFlow::edit`]) carry the payload a control chose — a
//!   **content id**, never a row number — and every transition returns an
//!   [`IaTransition`] naming the effects the application must perform.
//! * [`IaEffect`] — the domain transaction a transition asks for:
//!   `BeginSession`/`EndSession` for the session generation the simulation
//!   runs under, `DiscardDraft`/`AskDiscard` for the draft, and `LeaveToMenu`
//!   for the contract's "IA customize -> finish -> main". A transition only
//!   *asks*; this module holds no world, no IO and no campaign handle.
//! * [`IaRecordBook`] — the consumer an outcome is written to: an explicit
//!   **profile** scope of its own. It is not the campaign's ledger and it
//!   cannot be handed a campaign outcome, because [`ScenarioOutcome`] carries
//!   no profile, run, node, cash or unlock field to write one with — which is
//!   how F49 non-negotiable 3 ("IA never modifies campaign progression or
//!   money") is enforced at the type boundary rather than by review.
//!
//! # Designed, synthetic
//!
//! The screens, their guards and the record scope are designed: which screens
//! the original Instant Action front end had, whether it let a preset be
//! customized in place, and what it recorded when a session ended, are
//! unmeasured. See `docs/findings/2026-10-01-f49-a-instant-action-scenario-schemas.md`
//! and `docs/findings/2026-10-08-f49-c-instant-action-screen-flow.md`.

use std::fmt;

use cs_content::ai::{CombatSchemaError, DifficultyProfile, DifficultyTier};
use cs_content::environment::EnvironmentId;
use cs_content::instant_action::{
    CustomScenarioDraft, InstantActionCatalog, RosterSlot, ScenarioActorSpec, ScenarioSchemaError,
    ScenarioSeed, ScenarioSide, VictoryRules,
};
use cs_content::world::WorldId;
use cs_sim::campaign::{ProfileId, SessionGeneration};
use cs_types::content::{ContentId, Known, Resolved};

use super::{
    CustomDimension, InstantActionPresetRow, LowerError, LoweredScenario, ScenarioOutcome,
    ScenarioResult, ScenarioSelection, ScenarioSnapshot, custom_dimensions, evaluate_outcome,
    lower_custom, lower_preset, preset_rows,
};

/// The Instant Action screens, in flow order.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IaScreen {
    /// Choose an authored preset, or start a custom scenario.
    Select,
    /// Edit the custom scenario: dimensions, then each roster slot's
    /// airframe and loadout.
    Customize,
    /// A session is running.
    Flight,
    /// The session ended; show the result, then retry or finish.
    Results,
}

/// What happens on a screen, as a payload-carrying action.
///
/// The payload a control chose travels *with* the action (a preset or a
/// subject id) rather than being looked up by position, so reordering a list
/// can never select a different entry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IaAction {
    /// Start a custom scenario from the selected preset, under `subject` —
    /// the `ia_scenario` identity the custom scenario resolves to.
    StartCustom {
        /// The `ia_scenario` id the custom scenario runs under.
        subject: ContentId,
    },
    /// Lower the current selection and start a session.
    Launch,
    /// Cancel: leave the screen, discarding a draft or abandoning a session.
    Back,
    /// Answer a discard prompt: drop the draft and leave.
    ConfirmDiscard,
    /// Answer a discard prompt: keep editing.
    KeepEditing,
    /// Start again from the authored scenario, in a new session generation.
    Retry,
    /// Record the ended session, tear it down and return to the menu.
    Finish,
}

impl IaAction {
    /// The action's identity in [`IA_TABLE`].
    #[must_use]
    pub const fn kind(&self) -> IaActionKind {
        match self {
            Self::StartCustom { .. } => IaActionKind::StartCustom,
            Self::Launch => IaActionKind::Launch,
            Self::Back => IaActionKind::Back,
            Self::ConfirmDiscard => IaActionKind::ConfirmDiscard,
            Self::KeepEditing => IaActionKind::KeepEditing,
            Self::Retry => IaActionKind::Retry,
            Self::Finish => IaActionKind::Finish,
        }
    }
}

/// The identity of an [`IaAction`], without its payload.
///
/// [`IaActionKind::Report`] has no [`IaAction`] variant: it is the one
/// *system* transition, and its payload — the session's snapshot, with its
/// generation — travels with [`InstantActionFlow::report`] rather than with
/// an action a button could press.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum IaActionKind {
    /// See [`IaAction::StartCustom`].
    StartCustom,
    /// See [`IaAction::Launch`].
    Launch,
    /// See [`IaAction::Back`].
    Back,
    /// See [`IaAction::ConfirmDiscard`].
    ConfirmDiscard,
    /// See [`IaAction::KeepEditing`].
    KeepEditing,
    /// See [`IaAction::Retry`].
    Retry,
    /// See [`IaAction::Finish`].
    Finish,
    /// The running session reported a state that ended the scenario.
    Report,
}

/// What must hold before a row's transition may happen.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IaGuard {
    /// Always allowed.
    Always,
    /// A preset is selected to start a custom scenario from.
    NeedsPreset,
    /// A selection exists to lower.
    HasSelection,
    /// A session generation is live to end.
    HasSession,
    /// The session ended and an outcome is waiting.
    HasOutcome,
    /// Leaving with an edited draft asks for confirmation first.
    DiscardsDraft,
}

/// One row of the transition table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct IaRow {
    /// The screen the action happens on.
    pub from: IaScreen,
    /// The action.
    pub action: IaActionKind,
    /// The screen it leads to.
    pub to: IaScreen,
    /// The condition.
    pub guard: IaGuard,
}

const fn row(from: IaScreen, action: IaActionKind, to: IaScreen, guard: IaGuard) -> IaRow {
    IaRow {
        from,
        action,
        to,
        guard,
    }
}

use IaActionKind as K;
use IaGuard as G;
use IaScreen as S;

/// The transition table: every screen, every Back/Cancel path, in flow order.
///
/// `ConfirmDiscard` and `KeepEditing` answer the discard prompt
/// [`IaEffect::AskDiscard`] opens, so they are refused while no prompt is
/// pending (and every other action is refused while one is) — that gating
/// lives in [`InstantActionFlow::apply`], not in a row. Both screens a draft
/// can be lost from carry [`IaGuard::DiscardsDraft`]: `Customize`, where the
/// draft is edited, and `Select`, which a launched custom draft comes back to
/// when its session is abandoned — leaving the Instant Action screens with it
/// still holds the draft, so the same confirmation has to guard it there.
pub const IA_TABLE: &[IaRow] = &[
    row(S::Select, K::Back, S::Select, G::DiscardsDraft),
    row(S::Select, K::StartCustom, S::Customize, G::NeedsPreset),
    row(S::Select, K::Launch, S::Flight, G::HasSelection),
    row(S::Select, K::ConfirmDiscard, S::Select, G::Always),
    row(S::Select, K::KeepEditing, S::Select, G::Always),
    row(S::Customize, K::Launch, S::Flight, G::HasSelection),
    row(S::Customize, K::Back, S::Select, G::DiscardsDraft),
    row(S::Customize, K::ConfirmDiscard, S::Select, G::Always),
    row(S::Customize, K::KeepEditing, S::Customize, G::Always),
    row(S::Flight, K::Back, S::Select, G::HasSession),
    row(S::Flight, K::Report, S::Results, G::HasSession),
    row(S::Results, K::Retry, S::Flight, G::HasOutcome),
    row(S::Results, K::Finish, S::Select, G::HasOutcome),
];

/// The row for `(from, action)`, when that screen has that action.
#[must_use]
pub fn find_row(from: IaScreen, action: IaActionKind) -> Option<&'static IaRow> {
    IA_TABLE
        .iter()
        .find(|row| row.from == from && row.action == action)
}

/// The rows on a screen, in table order.
pub fn rows_on(screen: IaScreen) -> impl Iterator<Item = &'static IaRow> {
    IA_TABLE.iter().filter(move |row| row.from == screen)
}

/// What a transition asks the application to do outside this module.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IaEffect {
    /// A discard prompt is open; answer it with [`IaAction::ConfirmDiscard`]
    /// or [`IaAction::KeepEditing`].
    AskDiscard,
    /// The custom draft was dropped; the preset selection is back.
    DiscardDraft,
    /// A session generation started: stage the authored scenario for it.
    BeginSession {
        /// The generation this run is.
        generation: SessionGeneration,
        /// The `ia_scenario` it runs under.
        subject: ContentId,
    },
    /// That generation ended: cancel its IO, scripts, timers, input ownership
    /// and entity bindings. Always listed **before** the `BeginSession` a
    /// retry emits, so a resource is never held twice.
    EndSession {
        /// The generation that ended.
        generation: SessionGeneration,
    },
    /// Leave the Instant Action screens for the main menu.
    LeaveToMenu,
}

/// One accepted transition: where it came from, where it went, what to do.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IaTransition {
    /// The screen before.
    pub from: IaScreen,
    /// The screen after (the same when only a prompt was opened).
    pub to: IaScreen,
    /// The effects, in order.
    pub effects: Vec<IaEffect>,
}

/// Why an action or an edit was refused. A refusal changes nothing.
#[derive(Clone, Debug, PartialEq)]
pub enum IaRefusal {
    /// The screen has no such action.
    NoTransition {
        /// The screen.
        screen: IaScreen,
        /// The action.
        action: IaActionKind,
    },
    /// The call belongs to another screen.
    WrongScreen {
        /// The screen it needs.
        expected: IaScreen,
        /// The current screen.
        actual: IaScreen,
    },
    /// A discard prompt is open; answer it first.
    ConfirmationPending,
    /// No discard prompt is open.
    NoPendingDiscard,
    /// Starting a custom scenario needs a selected preset to start from.
    NoPresetSelected,
    /// Launching needs a selection and none is selected.
    NoSelection,
    /// The call needs a custom draft and there is none.
    NoDraft,
    /// The selection could not be lowered. Kept verbatim so the screen shows
    /// every problem the catalog reported at once (AC04), and mirrored into
    /// [`InstantActionFlow::last_error`].
    Lower(LowerError),
    /// The edit itself was refused by the declared schema (a slot the roster
    /// does not hold, an id in the wrong namespace).
    Edit(ScenarioSchemaError),
    /// The difficulty profile could not be rebuilt from the chosen tier.
    Difficulty(CombatSchemaError),
    /// Reporting or abandoning needs a live session and there is none.
    NoSession,
    /// A callback from another session generation arrived.
    StaleGeneration {
        /// The generation that is live.
        expected: SessionGeneration,
        /// The generation the callback claimed.
        got: SessionGeneration,
    },
    /// The scenario has not ended yet, so there is no outcome to show.
    StillRunning,
    /// Retrying or finishing needs an outcome and there is none.
    NoOutcome,
    /// The profile's record scope refused the entry.
    Record(IaRecordError),
}

impl IaRefusal {
    /// The stable lowercase identifier for reports and logs.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::NoTransition { .. } => "no_transition",
            Self::WrongScreen { .. } => "wrong_screen",
            Self::ConfirmationPending => "confirmation_pending",
            Self::NoPendingDiscard => "no_pending_discard",
            Self::NoPresetSelected => "no_preset_selected",
            Self::NoSelection => "no_selection",
            Self::NoDraft => "no_draft",
            Self::Lower(_) => "not_lowerable",
            Self::Edit(_) => "edit_refused",
            Self::Difficulty(_) => "difficulty_refused",
            Self::NoSession => "no_session",
            Self::StaleGeneration { .. } => "stale_generation",
            Self::StillRunning => "still_running",
            Self::NoOutcome => "no_outcome",
            Self::Record(error) => error.code(),
        }
    }
}

impl fmt::Display for IaRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTransition { screen, action } => {
                write!(f, "{screen:?} has no action {action:?}")
            }
            Self::WrongScreen { expected, actual } => {
                write!(f, "this belongs on {expected:?}, not {actual:?}")
            }
            Self::ConfirmationPending => f.write_str("a discard prompt is open; answer it first"),
            Self::NoPendingDiscard => f.write_str("no discard prompt is open"),
            Self::NoPresetSelected => {
                f.write_str("choose a preset before starting a custom scenario")
            }
            Self::NoSelection => f.write_str("no Instant Action selection has been made"),
            Self::NoDraft => f.write_str("there is no custom scenario to edit"),
            Self::Lower(error) => write!(f, "{error}"),
            Self::Edit(error) => write!(f, "{error}"),
            Self::Difficulty(error) => write!(f, "the difficulty tier cannot be applied: {error}"),
            Self::NoSession => f.write_str("no Instant Action session is running"),
            Self::StaleGeneration { expected, got } => {
                write!(
                    f,
                    "that report is from session generation {}, but {} is running",
                    got.0, expected.0
                )
            }
            Self::StillRunning => f.write_str("the scenario has not ended"),
            Self::NoOutcome => f.write_str("the session has no outcome yet"),
            Self::Record(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for IaRefusal {}

/// One change the customization screen can make to the draft.
///
/// Every variant is a dimension [`super::custom_dimensions`] offers, so every
/// visible option has exactly one way to reach the scenario (F49
/// non-negotiable 5).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IaEdit {
    /// Select a world.
    World(WorldId),
    /// Select an environment.
    Environment(EnvironmentId),
    /// Select the victory rules.
    Rules(VictoryRules),
    /// Select the skill tier, keeping the profile's declared knob overrides.
    Difficulty(DifficultyTier),
    /// Give one roster slot an airframe.
    SlotAirframe {
        /// The actor's side.
        side: ScenarioSide,
        /// The actor's slot.
        slot: RosterSlot,
        /// The airframe to give it.
        airframe: ContentId,
    },
    /// Give one roster slot a loadout.
    SlotLoadout {
        /// The actor's side.
        side: ScenarioSide,
        /// The actor's slot.
        slot: RosterSlot,
        /// The loadout to give it.
        loadout: ContentId,
    },
}

// -------------------------------------------------------------- records ---

/// One settled Instant Action session in one profile's record scope.
///
/// The profile field **is** the scope: [`IaRecordBook::record`] refuses an
/// entry naming another profile, so an Instant Action result cannot land in
/// another pilot's save by handing the wrong book a value. Deliberately not
/// the campaign's [`OutcomeId`](cs_sim::campaign::OutcomeId) — an Instant
/// Action session has no campaign run and no campaign node, so there is
/// nothing in this type a campaign transaction could be built from (F49
/// non-negotiable 3).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IaRecordEntry {
    profile: ProfileId,
    generation: SessionGeneration,
    subject: ContentId,
    seed: ScenarioSeed,
    result: ScenarioResult,
    ended_tick: u64,
    attempt: u32,
}

impl IaRecordEntry {
    /// One entry for a finished session: its generation, subject, seed and
    /// how it ended, stamped with the profile the flow belongs to.
    #[must_use]
    pub fn new(
        profile: ProfileId,
        generation: SessionGeneration,
        outcome: &ScenarioOutcome,
        attempt: u32,
    ) -> Self {
        Self {
            profile,
            generation,
            subject: outcome.subject().clone(),
            seed: outcome.seed(),
            result: outcome.result(),
            ended_tick: outcome.ended_tick(),
            attempt,
        }
    }

    /// The profile this entry belongs to.
    #[must_use]
    pub const fn profile(&self) -> &ProfileId {
        &self.profile
    }

    /// The session generation that produced it — unique per run, so a replay
    /// of the same scenario is a different entry.
    #[must_use]
    pub const fn generation(&self) -> SessionGeneration {
        self.generation
    }

    /// The `ia_scenario` it ran under.
    #[must_use]
    pub const fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The scenario's root seed, so a replay can reproduce the run
    /// (F49 non-negotiable 4).
    #[must_use]
    pub const fn seed(&self) -> ScenarioSeed {
        self.seed
    }

    /// How it ended.
    #[must_use]
    pub const fn result(&self) -> ScenarioResult {
        self.result
    }

    /// The tick it ended on.
    #[must_use]
    pub const fn ended_tick(&self) -> u64 {
        self.ended_tick
    }

    /// Which session this was for its subject, counting from 1.
    #[must_use]
    pub const fn attempt(&self) -> u32 {
        self.attempt
    }
}

/// Why a record scope refused an entry. The book is unchanged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IaRecordError {
    /// The entry names another profile than the book's.
    ForeignProfile {
        /// The book's profile.
        expected: ProfileId,
        /// The entry's profile.
        offered: ProfileId,
    },
    /// A session generation was already recorded, so recording it again
    /// would duplicate a run that already happened.
    DuplicateGeneration {
        /// The generation offered twice.
        generation: SessionGeneration,
    },
}

impl IaRecordError {
    /// The stable lowercase identifier for reports and logs.
    #[must_use]
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ForeignProfile { .. } => "foreign_profile",
            Self::DuplicateGeneration { .. } => "duplicate_generation",
        }
    }
}

impl fmt::Display for IaRecordError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ForeignProfile { expected, offered } => {
                write!(
                    f,
                    "that record names profile {offered}, but this book holds {expected}"
                )
            }
            Self::DuplicateGeneration { generation } => {
                write!(
                    f,
                    "session generation {} was already recorded",
                    generation.0
                )
            }
        }
    }
}

impl std::error::Error for IaRecordError {}

/// One profile's Instant Action record scope.
///
/// A book belongs to exactly one profile and holds that profile's settled
/// sessions in the order they finished. It is the only consumer an
/// [`ScenarioOutcome`] is written to from here: it takes an
/// [`IaRecordEntry`], which carries no cash, unlock, node or run field, so
/// writing one cannot move campaign progression even if a caller wanted it to
/// (F49 non-negotiable 3, enforced by the type rather than by review).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IaRecordBook {
    profile: ProfileId,
    entries: Vec<IaRecordEntry>,
}

impl IaRecordBook {
    /// A record scope for one profile, holding nothing yet.
    #[must_use]
    pub fn new(profile: ProfileId) -> Self {
        Self {
            profile,
            entries: Vec::new(),
        }
    }

    /// The profile this book belongs to.
    #[must_use]
    pub const fn profile(&self) -> &ProfileId {
        &self.profile
    }

    /// Every entry, oldest first.
    #[must_use]
    pub fn entries(&self) -> &[IaRecordEntry] {
        &self.entries
    }

    /// How many sessions are recorded.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether nothing is recorded yet.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Records one settled session.
    ///
    /// # Errors
    ///
    /// [`IaRecordError::ForeignProfile`] when the entry names another
    /// profile, [`IaRecordError::DuplicateGeneration`] when that generation
    /// is already recorded. Either way the book is unchanged, so a refused
    /// settlement can be retried safely.
    pub fn record(&mut self, entry: IaRecordEntry) -> Result<(), IaRecordError> {
        if entry.profile != self.profile {
            return Err(IaRecordError::ForeignProfile {
                expected: self.profile.clone(),
                offered: entry.profile,
            });
        }
        if let Some(held) = self
            .entries
            .iter()
            .find(|held| held.generation == entry.generation)
        {
            return Err(IaRecordError::DuplicateGeneration {
                generation: held.generation,
            });
        }
        self.entries.push(entry);
        Ok(())
    }
}

// ----------------------------------------------------------------- flow ---

/// The authored scenario of a live session and the generation it runs under.
///
/// The scenario here is the **authored** lowering of the selection and is
/// never mutated: a retry reuses it unchanged, so a second run starts from
/// what was authored rather than from whatever the first run left behind
/// (STATE-TRANSACTIONS, "Session reset").
#[derive(Clone, Debug, PartialEq)]
struct LiveSession {
    generation: SessionGeneration,
    scenario: LoweredScenario,
}

/// The Instant Action screens' state machine.
///
/// One flow serves one visit to the Instant Action screens: it starts on
/// [`IaScreen::Select`], and [`IaAction::Finish`] ends it with the recorded
/// session and a request to leave for the menu. Every transition goes through
/// [`IA_TABLE`]; a refusal changes nothing — not the screen, not the
/// selection, not the live session and not the record scope.
#[derive(Clone, Debug)]
pub struct InstantActionFlow {
    profile: ProfileId,
    catalog: InstantActionCatalog,
    screen: IaScreen,
    selection: Option<ScenarioSelection>,
    /// The preset the custom draft was seeded from, so a clean Back can put
    /// the selection back exactly as it was.
    draft_from: Option<ContentId>,
    draft_edited: bool,
    session: Option<LiveSession>,
    outcome: Option<ScenarioOutcome>,
    last_error: Option<LowerError>,
    records: IaRecordBook,
    generation: SessionGeneration,
    pending_discard: bool,
}

impl InstantActionFlow {
    /// The screens for one profile, on [`IaScreen::Select`] with nothing
    /// selected and no session running.
    #[must_use]
    pub fn new(profile: ProfileId, catalog: InstantActionCatalog) -> Self {
        Self {
            records: IaRecordBook::new(profile.clone()),
            profile,
            catalog,
            screen: IaScreen::Select,
            selection: None,
            draft_from: None,
            draft_edited: false,
            session: None,
            outcome: None,
            last_error: None,
            generation: SessionGeneration(0),
            pending_discard: false,
        }
    }

    /// The current screen.
    #[must_use]
    pub const fn screen(&self) -> IaScreen {
        self.screen
    }

    /// The profile these screens run for — the scope every record is
    /// written under.
    #[must_use]
    pub const fn profile(&self) -> &ProfileId {
        &self.profile
    }

    /// The selected preset, when a preset is selected.
    #[must_use]
    pub fn preset_id(&self) -> Option<&ContentId> {
        self.selection
            .as_ref()
            .and_then(ScenarioSelection::preset_id)
    }

    /// The selection the next launch lowers: a preset — or the custom draft
    /// an abandoned flight came back to — on [`IaScreen::Select`], the
    /// edited draft on [`IaScreen::Customize`].
    #[must_use]
    pub const fn selection(&self) -> Option<&ScenarioSelection> {
        self.selection.as_ref()
    }

    /// The custom draft the flow holds: the one being edited on
    /// [`IaScreen::Customize`], or the one a launched custom session left
    /// behind on [`IaScreen::Select`] when its flight was abandoned.
    #[must_use]
    pub fn draft(&self) -> Option<&CustomScenarioDraft> {
        match &self.selection {
            Some(ScenarioSelection::Custom(draft)) => Some(draft.as_ref()),
            _ => None,
        }
    }

    /// Whether the draft has been edited since it was seeded, which is what
    /// makes Back ask before discarding it — on [`IaScreen::Customize`],
    /// where it is edited, and on [`IaScreen::Select`], where an abandoned
    /// flight can leave it holding those edits.
    #[must_use]
    pub const fn draft_is_dirty(&self) -> bool {
        self.draft_edited
    }

    /// Whether a discard prompt is open.
    #[must_use]
    pub const fn pending_discard(&self) -> bool {
        self.pending_discard
    }

    /// The last launch refusal, for the screen that shows it, until the next
    /// selection or successful transition replaces it.
    #[must_use]
    pub const fn last_error(&self) -> Option<&LowerError> {
        self.last_error.as_ref()
    }

    /// The live session's generation, while one is running.
    #[must_use]
    pub fn generation(&self) -> Option<SessionGeneration> {
        self.session.as_ref().map(|session| session.generation)
    }

    /// The authored scenario the live session runs, for the screen and the
    /// developer tools.
    #[must_use]
    pub fn running_scenario(&self) -> Option<&LoweredScenario> {
        self.session.as_ref().map(|session| &session.scenario)
    }

    /// The scenario's explicit root seed while a session runs: what the
    /// developer overlay displays and what a replay records (F49
    /// non-negotiable 4).
    #[must_use]
    pub fn seed(&self) -> Option<ScenarioSeed> {
        self.running_scenario().map(LoweredScenario::seed)
    }

    /// The authored initial state of the live session: every roster actor
    /// alive at tick 0. What a launch stages and what a retry restores.
    #[must_use]
    pub fn authored_snapshot(&self) -> Option<ScenarioSnapshot> {
        self.session.as_ref().map(|session| {
            ScenarioSnapshot::new(
                0,
                session
                    .scenario
                    .actors()
                    .iter()
                    .map(|actor| (actor.side, actor.slot)),
            )
        })
    }

    /// The outcome the session ended with, once it has one.
    #[must_use]
    pub const fn outcome(&self) -> Option<&ScenarioOutcome> {
        self.outcome.as_ref()
    }

    /// The profile's record scope this flow writes finished sessions into.
    #[must_use]
    pub const fn records(&self) -> &IaRecordBook {
        &self.records
    }

    /// The catalog's presets as selectable rows, in canonical id order.
    #[must_use]
    pub fn preset_rows(&self) -> Vec<InstantActionPresetRow> {
        preset_rows(&self.catalog)
    }

    /// Every dimension the catalog offers, with the values a control offers
    /// for it.
    #[must_use]
    pub fn dimensions(&self) -> Vec<CustomDimension> {
        custom_dimensions(&self.catalog)
    }

    /// Selects a preset to launch, or to start a custom scenario from.
    ///
    /// # Errors
    ///
    /// [`IaRefusal::WrongScreen`] off [`IaScreen::Select`],
    /// [`IaRefusal::ConfirmationPending`] while a discard prompt is open. The
    /// id is **not** checked against the catalog here: an id the catalog does
    /// not hold is refused by the launch, with the catalog's own
    /// [`LowerError::UnknownPreset`], so a selection and a launch report the
    /// same problem.
    pub fn select_preset(&mut self, preset: &ContentId) -> Result<(), IaRefusal> {
        self.expect_screen(IaScreen::Select)?;
        self.selection = Some(ScenarioSelection::Preset(preset.clone()));
        self.draft_from = None;
        self.draft_edited = false;
        self.last_error = None;
        Ok(())
    }

    /// Applies one customization edit to the draft on [`IaScreen::Customize`].
    ///
    /// # Errors
    ///
    /// [`IaRefusal::WrongScreen`] off the customize screen,
    /// [`IaRefusal::NoDraft`] with no draft, [`IaRefusal::Edit`] when the
    /// declared schema refuses the value (a slot the roster does not hold, an
    /// id in the wrong namespace) and [`IaRefusal::Difficulty`] when the tier
    /// cannot be built. The draft is unchanged after a refusal.
    pub fn edit(&mut self, edit: IaEdit) -> Result<(), IaRefusal> {
        self.expect_screen(IaScreen::Customize)?;
        self.apply_edit(edit)?;
        self.draft_edited = true;
        Ok(())
    }

    /// Applies an action through [`IA_TABLE`].
    ///
    /// # Errors
    ///
    /// [`IaRefusal`] — and the machine is exactly as it was, including the
    /// live session and the record scope. [`IaRefusal::Lower`] carries the
    /// lowering's own error (see [`super::LowerError`]) verbatim, so the
    /// screen can render every problem at once, and it is also kept by
    /// [`Self::last_error`].
    pub fn apply(&mut self, action: IaAction) -> Result<IaTransition, IaRefusal> {
        let kind = action.kind();
        let answers_prompt = matches!(
            kind,
            IaActionKind::ConfirmDiscard | IaActionKind::KeepEditing
        );
        if self.pending_discard && !answers_prompt {
            return Err(IaRefusal::ConfirmationPending);
        }
        if !self.pending_discard && answers_prompt {
            return Err(IaRefusal::NoPendingDiscard);
        }
        let row = find_row(self.screen, kind).ok_or(IaRefusal::NoTransition {
            screen: self.screen,
            action: kind,
        })?;
        match row.guard {
            IaGuard::Always => {}
            IaGuard::NeedsPreset => {
                if self.preset_id().is_none() {
                    return Err(IaRefusal::NoPresetSelected);
                }
            }
            IaGuard::HasSelection => {
                if self.selection.is_none() {
                    return Err(IaRefusal::NoSelection);
                }
            }
            IaGuard::HasSession => {
                if self.session.is_none() {
                    return Err(IaRefusal::NoSession);
                }
            }
            IaGuard::HasOutcome => {
                if self.outcome.is_none() {
                    return Err(IaRefusal::NoOutcome);
                }
            }
            IaGuard::DiscardsDraft => {
                if self.draft_edited {
                    // The screen keeps the draft and asks; nothing else moves.
                    self.pending_discard = true;
                    return Ok(IaTransition {
                        from: row.from,
                        to: row.from,
                        effects: vec![IaEffect::AskDiscard],
                    });
                }
            }
        }
        self.perform(action, row)
    }

    /// Reports the live session's state, ending the scenario when the rules
    /// say it has ended.
    ///
    /// The generation must be the live one: a callback from a session that
    /// was torn down or replaced is refused by name rather than settling the
    /// run that is actually flying (STATE-TRANSACTIONS, "Results, previous
    /// targets and delayed callbacks are always generation-qualified").
    ///
    /// # Errors
    ///
    /// [`IaRefusal::WrongScreen`] off [`IaScreen::Flight`],
    /// [`IaRefusal::NoSession`] with no live session,
    /// [`IaRefusal::StaleGeneration`] for another generation's callback, and
    /// [`IaRefusal::StillRunning`] while the scenario has not ended. Nothing
    /// changes in any of those cases.
    pub fn report(
        &mut self,
        generation: SessionGeneration,
        snapshot: ScenarioSnapshot,
    ) -> Result<IaTransition, IaRefusal> {
        self.expect_flight()?;
        // The report transition is a row of the same table a button walks,
        // so the target screen is declared once, with everything else.
        let row =
            find_row(IaScreen::Flight, IaActionKind::Report).ok_or(IaRefusal::NoTransition {
                screen: IaScreen::Flight,
                action: IaActionKind::Report,
            })?;
        let session = self.session.as_ref().ok_or(IaRefusal::NoSession)?;
        if session.generation != generation {
            return Err(IaRefusal::StaleGeneration {
                expected: session.generation,
                got: generation,
            });
        }
        let Some(outcome) = evaluate_outcome(&session.scenario, &snapshot) else {
            return Err(IaRefusal::StillRunning);
        };
        self.outcome = Some(outcome);
        self.screen = row.to;
        Ok(IaTransition {
            from: row.from,
            to: row.to,
            effects: Vec::new(),
        })
    }

    /// The one screen check every transition-independent call makes.
    fn expect_screen(&self, screen: IaScreen) -> Result<(), IaRefusal> {
        if self.pending_discard {
            return Err(IaRefusal::ConfirmationPending);
        }
        if self.screen == screen {
            Ok(())
        } else {
            Err(IaRefusal::WrongScreen {
                expected: screen,
                actual: self.screen,
            })
        }
    }

    /// The flight screen's own check: a report belongs to a running session,
    /// so it is refused anywhere else without consulting a prompt.
    fn expect_flight(&self) -> Result<(), IaRefusal> {
        if self.screen == IaScreen::Flight {
            Ok(())
        } else {
            Err(IaRefusal::WrongScreen {
                expected: IaScreen::Flight,
                actual: self.screen,
            })
        }
    }

    /// Performs a transition whose row and guard already passed.
    fn perform(
        &mut self,
        action: IaAction,
        row: &'static IaRow,
    ) -> Result<IaTransition, IaRefusal> {
        let from = row.from;
        let to = row.to;
        let mut effects = Vec::new();
        match (action, from) {
            (IaAction::StartCustom { subject }, IaScreen::Select) => {
                let preset_id = self
                    .preset_id()
                    .cloned()
                    .ok_or(IaRefusal::NoPresetSelected)?;
                let draft = self.seed_draft(&preset_id, subject)?;
                self.selection = Some(ScenarioSelection::Custom(Box::new(draft)));
                self.draft_from = Some(preset_id);
                self.draft_edited = false;
                self.last_error = None;
            }
            (IaAction::Launch, IaScreen::Select | IaScreen::Customize) => {
                let scenario = self.lower_selection()?;
                let subject = scenario.subject().clone();
                let generation = self.begin_generation();
                self.session = Some(LiveSession {
                    generation,
                    scenario,
                });
                self.outcome = None;
                self.last_error = None;
                effects.push(IaEffect::BeginSession {
                    generation,
                    subject,
                });
            }
            (IaAction::Back, IaScreen::Select) => effects.push(IaEffect::LeaveToMenu),
            (IaAction::Back, IaScreen::Customize) => {
                self.discard_draft();
                effects.push(IaEffect::DiscardDraft);
            }
            (IaAction::Back, IaScreen::Flight) => {
                let session = self.session.take().ok_or(IaRefusal::NoSession)?;
                effects.push(IaEffect::EndSession {
                    generation: session.generation,
                });
            }
            (IaAction::ConfirmDiscard, IaScreen::Select) => {
                // The prompt was opened by `Back` on this screen, which
                // means "leave Instant Action"; answering it replays that
                // intent, so the draft goes and the screens are left.
                self.discard_draft();
                self.pending_discard = false;
                effects.push(IaEffect::DiscardDraft);
                effects.push(IaEffect::LeaveToMenu);
            }
            (IaAction::ConfirmDiscard, IaScreen::Customize) => {
                self.discard_draft();
                self.pending_discard = false;
                effects.push(IaEffect::DiscardDraft);
            }
            (IaAction::KeepEditing, IaScreen::Select | IaScreen::Customize) => {
                self.pending_discard = false;
            }
            (IaAction::Retry, IaScreen::Results) => {
                let (authored, previous) = {
                    let session = self.session.as_ref().ok_or(IaRefusal::NoSession)?;
                    (session.scenario.clone(), session.generation)
                };
                if self.outcome.is_none() {
                    return Err(IaRefusal::NoOutcome);
                }
                self.outcome = None;
                let subject = authored.subject().clone();
                let generation = self.begin_generation();
                self.session = Some(LiveSession {
                    generation,
                    scenario: authored,
                });
                self.last_error = None;
                effects.push(IaEffect::EndSession {
                    generation: previous,
                });
                effects.push(IaEffect::BeginSession {
                    generation,
                    subject,
                });
            }
            (IaAction::Finish, IaScreen::Results) => {
                let (generation, outcome) = {
                    let session = self.session.as_ref().ok_or(IaRefusal::NoSession)?;
                    let outcome = self.outcome.as_ref().ok_or(IaRefusal::NoOutcome)?;
                    (session.generation, outcome)
                };
                let entry = IaRecordEntry::new(
                    self.profile.clone(),
                    generation,
                    outcome,
                    self.attempt_for(outcome.subject()),
                );
                // The record scope refuses before it writes, so a refusal
                // here still leaves the machine exactly where it was.
                self.records.record(entry).map_err(IaRefusal::Record)?;
                self.session = None;
                self.outcome = None;
                self.selection = None;
                self.draft_from = None;
                self.draft_edited = false;
                self.last_error = None;
                effects.push(IaEffect::EndSession { generation });
                effects.push(IaEffect::LeaveToMenu);
            }
            (action, screen) => {
                return Err(IaRefusal::NoTransition {
                    screen,
                    action: action.kind(),
                });
            }
        }
        self.screen = to;
        Ok(IaTransition { from, to, effects })
    }

    /// Seeds a custom draft from the selected preset's authored dimensions.
    ///
    /// The preset's own `ia_scenario` id is **not** copied: a custom scenario
    /// runs under the subject the caller names (a custom scenario is its own
    /// namespace entry, not a second name for the preset). Which identity the
    /// original assigns a saved custom scenario is unmeasured — see the
    /// finding linked from this module.
    fn seed_draft(
        &self,
        preset_id: &ContentId,
        subject: ContentId,
    ) -> Result<CustomScenarioDraft, IaRefusal> {
        let preset = self.catalog.preset(preset_id).ok_or_else(|| {
            IaRefusal::Lower(LowerError::UnknownPreset {
                preset: preset_id.clone(),
            })
        })?;
        let parameters = preset.parameters();
        Ok(CustomScenarioDraft::new()
            .with_subject(subject)
            .with_world(parameters.world().clone())
            .with_environment(parameters.environment().clone())
            .with_roster(parameters.roster().actors().to_vec())
            .with_difficulty(parameters.difficulty().clone())
            .with_rules(*parameters.rules())
            .with_seed(parameters.seed())
            // A preset is authored single-player until evidence says
            // otherwise, exactly as `lower_preset` reads it.
            .with_players(1)
            .with_provenance(preset.provenance().clone()))
    }

    /// Lowers the current selection, keeping the refusal for the screen.
    fn lower_selection(&mut self) -> Result<LoweredScenario, IaRefusal> {
        let lowered = match self.selection.as_ref() {
            None => return Err(IaRefusal::NoSelection),
            Some(ScenarioSelection::Preset(preset)) => lower_preset(&self.catalog, preset),
            Some(ScenarioSelection::Custom(draft)) => {
                lower_custom(&self.catalog, draft.as_ref().clone())
            }
        };
        lowered.map_err(|error| {
            self.last_error = Some(error.clone());
            IaRefusal::Lower(error)
        })
    }

    /// Drops the draft and puts the preset selection back.
    fn discard_draft(&mut self) {
        self.selection = self.draft_from.take().map(ScenarioSelection::Preset);
        self.draft_edited = false;
    }

    /// The next session generation. Never reused, so a torn-down session's
    /// callbacks can always be told apart from a live one's.
    fn begin_generation(&mut self) -> SessionGeneration {
        self.generation = SessionGeneration(self.generation.0.saturating_add(1));
        self.generation
    }

    /// Which session this would be for `subject`: one past what the record
    /// scope already holds for it.
    fn attempt_for(&self, subject: &ContentId) -> u32 {
        let recorded = self
            .records
            .entries()
            .iter()
            .filter(|held| held.subject == *subject)
            .count();
        u32::try_from(recorded)
            .unwrap_or(u32::MAX)
            .saturating_add(1)
    }

    /// Applies one edit to the draft, refusing before anything is written.
    fn apply_edit(&mut self, edit: IaEdit) -> Result<(), IaRefusal> {
        let catalogued = self.catalog.provenance().clone();
        let selection = self.selection.as_mut().ok_or(IaRefusal::NoDraft)?;
        let ScenarioSelection::Custom(draft) = selection else {
            return Err(IaRefusal::NoDraft);
        };
        match edit {
            IaEdit::World(world) => {
                // The value came out of the catalog's option table, so it
                // carries the catalog's provenance.
                let value = Resolved::Known(Known::new(world, catalogued));
                let next = std::mem::take(draft.as_mut()).with_world(value);
                **draft = next;
            }
            IaEdit::Environment(environment) => {
                let value = Resolved::Known(Known::new(environment, catalogued));
                let next = std::mem::take(draft.as_mut()).with_environment(value);
                **draft = next;
            }
            IaEdit::Rules(rules) => {
                let next = std::mem::take(draft.as_mut()).with_rules(rules);
                **draft = next;
            }
            IaEdit::Difficulty(tier) => {
                let current = draft.difficulty().ok_or(IaRefusal::NoDraft)?.clone();
                let profile = DifficultyProfile::try_new(
                    tier,
                    current.overrides().to_vec(),
                    current.provenance().clone(),
                )
                .map_err(IaRefusal::Difficulty)?;
                let next = std::mem::take(draft.as_mut()).with_difficulty(profile);
                **draft = next;
            }
            IaEdit::SlotAirframe {
                side,
                slot,
                airframe,
            } => {
                let actor = held_actor(draft.as_ref(), side, slot)?;
                let updated = ScenarioActorSpec::try_new(
                    side,
                    slot,
                    actor.faction().clone(),
                    Resolved::Known(Known::new(airframe, actor.provenance().clone())),
                    actor.loadout().clone(),
                    actor.pilot().cloned(),
                    actor.survivability().clone(),
                    actor.provenance().clone(),
                )
                .map_err(IaRefusal::Edit)?;
                replace_actor(draft.as_mut(), updated)?;
            }
            IaEdit::SlotLoadout {
                side,
                slot,
                loadout,
            } => {
                let actor = held_actor(draft.as_ref(), side, slot)?;
                let updated = ScenarioActorSpec::try_new(
                    side,
                    slot,
                    actor.faction().clone(),
                    actor.airframe().clone(),
                    Resolved::Known(Known::new(loadout, actor.provenance().clone())),
                    actor.pilot().cloned(),
                    actor.survivability().clone(),
                    actor.provenance().clone(),
                )
                .map_err(IaRefusal::Edit)?;
                replace_actor(draft.as_mut(), updated)?;
            }
        }
        Ok(())
    }
}

/// The actor a roster edit targets, cloned so the edit can be refused
/// without touching the draft.
fn held_actor(
    draft: &CustomScenarioDraft,
    side: ScenarioSide,
    slot: RosterSlot,
) -> Result<ScenarioActorSpec, IaRefusal> {
    draft
        .roster()
        .and_then(|roster| {
            roster
                .iter()
                .find(|held| held.side() == side && held.slot() == slot)
        })
        .cloned()
        .ok_or(IaRefusal::Edit(ScenarioSchemaError::NoSuchRosterSlot {
            side,
            slot,
        }))
}

/// Puts a rebuilt actor back into its slot, refusing before the swap when
/// the schema does not accept it.
fn replace_actor(
    draft: &mut CustomScenarioDraft,
    actor: ScenarioActorSpec,
) -> Result<(), IaRefusal> {
    let next = draft
        .clone()
        .replace_roster_slot(actor)
        .map_err(IaRefusal::Edit)?;
    *draft = next;
    Ok(())
}

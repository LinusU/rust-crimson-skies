//! The construction, flight check, loading and return flows, wired (F45-C).
//!
//! Spec: `specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`,
//! stage `### F45-C`. Shared contracts: `docs/contracts/UI-NETWORK.md` ("A UI
//! action requests a domain transaction; it does not directly edit campaign
//! cash, ownership or objective fields", "Screen transitions explicitly
//! acquire/release input, audio and world resources. Returning to menu cannot
//! leave the old world simulating") and
//! `docs/contracts/STATE-TRANSACTIONS.md` ("A failed load does not consume
//! campaign money or progress", "Retry restores the authored initial state").
//!
//! F45-A built the state table ([`super::TABLE`]) and its machine, F45-B
//! presented that machine through authored artwork ([`super::ScreenSession`]).
//! Neither touched the domain: an accepted transition only *asked* for a
//! transaction ([`Effect::Request`]) and only *declared* a resource change
//! ([`Effect::Acquire`]/[`Effect::Release`]), so nothing in the application
//! actually happened. This stage is where the asking stops being hypothetical:
//!
//! * [`FrontEndFlow`] is the object the application drives instead of a bare
//!   [`ScreenSession`]. Every action goes through **plan → domain →
//!   transition**: [`FrontEnd::plan`] runs the machine on a throwaway copy and
//!   reports what the transition *would* ask for, the domain transaction runs
//!   next, and only then does the real machine move. A domain refusal
//!   ([`FlowError`]) therefore leaves the screen, the focus and the resource
//!   ledger exactly as they were — the contract's "a refusal changes nothing"
//!   extended past the machine to the campaign and the profile.
//! * [`ResourceLedger`] is the consumer of the resource effects: it is fed the
//!   `Release`/`Acquire` stream itself rather than reading the machine's held
//!   set, so a stream that acquires a bound input context twice or releases
//!   something nobody held is an error instead of a silently lost teardown.
//! * [`LoadFlow`] is the loading screen's real load: a
//!   [`crate::loading::LoadingSession`] pumped through the production
//!   [`crate::loading::SessionIo`] over a [`ContentSession`]. Its failures
//!   come back as the machine's own `LoadFailed`, which returns to the flight
//!   check with the draft intact and the world released, and the load is
//!   started again — in the same process — once the dependency is repaired.
//!
//! The domain objects are the ones the earlier stages already wired:
//! [`ProfileSession`] (F48-C), [`CampaignRun`] (F43-C) and
//! [`ConstructionScreen`] (F44-C). This module does not reimplement any of
//! them; it only decides *when* each is called and what a refusal does to the
//! screen. What is still unknown — which dependencies a retail mission load
//! declares, which components a front-end construction draft maps onto which
//! blueprint slot, which option set the flight check may offer — is recorded
//! in `docs/findings/2026-10-07-f45-c-flow-wiring.md` rather than guessed.

use std::collections::BTreeSet;
use std::fmt;
use std::path::PathBuf;

use cs_assets::cache::CacheStore;
use cs_assets::vfs::ContentSession;
use cs_content::airframe_roles::{
    AirframeRoles, LaunchAssignmentError, OwnedLoadout, ResolvedLaunch,
};
use cs_content::construction::{ConstructionPolicy, ConstructionRules, PriceBook};
use cs_content::save::settings::SettingCatalog;
use cs_sim::campaign::{
    AppliedOutcome, CampaignGraph, CampaignRunId, DifficultyId,
    MissionOutcome as MissionOutcomeRecord, Outcome as MissionResult,
    ProfileId as CampaignProfileId,
};
use cs_types::content::ContentId;
use cs_types::profile::{ProfileId, ProfileKind};

use crate::campaign::{CampaignRun, CampaignSaveError};
use crate::construction::{ConstructionContext, ConstructionScreen, ScreenSaveError};
use crate::loading::{
    DriverError, LoadItem, LoadRequest, LoadState, LoadTarget, LoadingScreen, LoadingSession,
    SessionIo, TransitionError,
};
use crate::profile::{ProfileSession, SessionError, SessionOrigin, TeardownReport};

use super::{
    Action, AudioScope, ConstructionDraft, Effect, FrontEnd, InputContext, Loadout, MissionOutcome,
    Outcome, Plan, ProfileIntent, Refusal, Request, Resource, Screen, ScreenDeck, ScreenSession,
    ScreenSessionError,
};

/// Everything a profile, a campaign run and the construction screen need in
/// order to exist. The application builds this once at boot; the flow keeps it
/// so a profile can be closed and reopened without restarting the process.
#[derive(Clone, Debug)]
pub struct FlowSetup {
    /// The population's directory.
    pub base: PathBuf,
    /// Whether a person or automation opens the population. The profile
    /// library refuses an automated caller from the production tree, so this
    /// is what keeps a test out of a player's saves.
    pub origin: SessionOrigin,
    /// The kind of profile population.
    pub kind: ProfileKind,
    /// The settings catalog the session resolves against.
    pub catalog: SettingCatalog,
    /// The name the profile screen gives a newly created profile. The machine
    /// carries no text field, so the name is setup, not a transition payload.
    pub new_profile: String,
    /// The profile the profile screen continues, when it names one; the
    /// population's active pointer decides when it does not.
    pub existing: Option<ProfileId>,
    /// The campaign graph a run plays.
    pub graph: CampaignGraph,
    /// The identity the campaign records the run under. The profile tree's
    /// own id is a slot number and the campaign's is a stable string
    /// (`STATE-TRANSACTIONS`' "stable persisted identity"), and nothing in
    /// this repository derives one from the other: the application supplies
    /// both here so a reopen reads the same run it wrote.
    pub campaign_profile: CampaignProfileId,
    /// The run id a campaign starts or resumes under.
    pub run: CampaignRunId,
    /// The difficulty a new run records.
    pub difficulty: DifficultyId,
    /// The session generation the flight-check selection belongs to. An
    /// unforced launch is not compared against it; it is recorded on the
    /// [`OwnedLoadout`] the resolver is handed.
    pub session_generation: u64,
    /// The airframe roster the flight check resolves a launch through, when
    /// the application supplied one.
    pub roles: Option<AirframeRoles>,
    /// The construction rules a commit is judged by, when supplied.
    pub construction: Option<ConstructionInputs>,
}

/// The immutable inputs a construction commit is judged by — the same three
/// [`ConstructionContext`] takes, held by the flow so a commit can be planned
/// before the machine moves.
#[derive(Clone, Debug)]
pub struct ConstructionInputs {
    /// The airframe's rule profile.
    pub rules: ConstructionRules,
    /// Pairing, banned list and catalog availability.
    pub policy: ConstructionPolicy,
    /// Declared masses and prices.
    pub book: PriceBook,
}

impl ConstructionInputs {
    /// The context a commit is handed.
    #[must_use]
    pub fn context<'a>(&'a self, graph: &'a CampaignGraph) -> ConstructionContext<'a> {
        ConstructionContext {
            rules: &self.rules,
            policy: &self.policy,
            book: &self.book,
            graph,
        }
    }
}

impl FlowSetup {
    /// A setup over `base`'s population playing `graph` under `campaign_profile`/
    /// `run`. The remaining fields start at their designed defaults and are
    /// overridden with the setters before the flow is built.
    #[must_use]
    pub fn new(
        base: PathBuf,
        graph: CampaignGraph,
        campaign_profile: CampaignProfileId,
        run: CampaignRunId,
        difficulty: DifficultyId,
    ) -> Self {
        Self {
            base,
            origin: SessionOrigin::Interactive,
            kind: ProfileKind::Production,
            catalog: SettingCatalog::empty(),
            new_profile: "Pilot".to_owned(),
            existing: None,
            graph,
            campaign_profile,
            run,
            difficulty,
            session_generation: 1,
            roles: None,
            construction: None,
        }
    }

    /// Opens the population as automation over a synthetic profile tree —
    /// the only combination an automated caller may use.
    #[must_use]
    pub fn sandbox(mut self) -> Self {
        self.origin = SessionOrigin::Automated;
        self.kind = ProfileKind::Synthetic;
        self
    }

    /// The name a new profile is created with.
    #[must_use]
    pub fn profile_name(mut self, name: impl Into<String>) -> Self {
        self.new_profile = name.into();
        self
    }

    /// The profile the profile screen continues.
    #[must_use]
    pub fn existing(mut self, id: ProfileId) -> Self {
        self.existing = Some(id);
        self
    }

    /// The session generation the flight check belongs to.
    #[must_use]
    pub fn session_generation(mut self, generation: u64) -> Self {
        self.session_generation = generation;
        self
    }

    /// The airframe roster a launch is resolved through.
    #[must_use]
    pub fn roles(mut self, roles: AirframeRoles) -> Self {
        self.roles = Some(roles);
        self
    }

    /// The construction rules, policy and prices a commit is judged by.
    #[must_use]
    pub fn construction(
        mut self,
        rules: ConstructionRules,
        policy: ConstructionPolicy,
        book: PriceBook,
    ) -> Self {
        self.construction = Some(ConstructionInputs {
            rules,
            policy,
            book,
        });
        self
    }
}

/// Why a resource effect could not be taken. The ledger is fed the effect
/// stream, so a producer that hands out an impossible pair is caught here
/// instead of leaving the application holding a world nobody released.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ResourceProblem {
    /// The resource was acquired while it was already held.
    AcquiredTwice(Resource),
    /// The resource was released while nothing held it.
    ReleasedUnknown(Resource),
    /// An input context was bound while another one was still bound — the
    /// joystick trigger that would end up on a button and a gun at once.
    InputBoundTwice {
        /// The context still bound.
        held: InputContext,
        /// The context the stream wanted to bind.
        offered: InputContext,
    },
}

impl fmt::Display for ResourceProblem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AcquiredTwice(resource) => write!(f, "{resource:?} was acquired while held"),
            Self::ReleasedUnknown(resource) => write!(f, "{resource:?} was released while unheld"),
            Self::InputBoundTwice { held, offered } => {
                write!(f, "{offered:?} bound while {held:?} is still bound")
            }
        }
    }
}

impl std::error::Error for ResourceProblem {}

/// What the application holds, decided from the effect stream rather than
/// read back off the machine: this is the record a renderer, an input mapper
/// and an audio mixer act on, so it is built the way they would build it — one
/// release/acquire at a time, in order.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ResourceLedger {
    input: Option<InputContext>,
    audio: Option<AudioScope>,
    world: bool,
}

impl ResourceLedger {
    /// Takes one effect. Only the resource effects mean anything here; a
    /// request has already run and a prompt or an exit holds nothing.
    ///
    /// # Errors
    ///
    /// [`ResourceProblem`] for an impossible pair.
    pub fn apply(&mut self, effect: &Effect) -> Result<(), ResourceProblem> {
        match effect {
            Effect::Acquire(resource) => self.acquire(*resource),
            Effect::Release(resource) => self.release(*resource),
            Effect::Request(_) | Effect::AskDiscard | Effect::ExitApplication => Ok(()),
        }
    }

    /// Takes every effect of one outcome, in order.
    ///
    /// # Errors
    ///
    /// As [`Self::apply`], naming the effect that broke the rule.
    pub fn apply_all<'a>(
        &mut self,
        effects: impl IntoIterator<Item = &'a Effect>,
    ) -> Result<(), ResourceProblem> {
        for effect in effects {
            self.apply(effect)?;
        }
        Ok(())
    }

    /// The bound input context, if any.
    #[must_use]
    pub fn input(&self) -> Option<InputContext> {
        self.input
    }

    /// The held audio scope, if any.
    #[must_use]
    pub fn audio(&self) -> Option<AudioScope> {
        self.audio
    }

    /// Whether a world is held — the flag that keeps the returning-to-menu
    /// path from leaving the old world simulating.
    #[must_use]
    pub fn holds_world(&self) -> bool {
        self.world
    }

    /// Everything held, in the machine's own [`Resource`] form.
    #[must_use]
    pub fn resources(&self) -> BTreeSet<Resource> {
        let mut held = BTreeSet::new();
        if let Some(context) = self.input {
            held.insert(Resource::Input(context));
        }
        if let Some(scope) = self.audio {
            held.insert(Resource::Audio(scope));
        }
        if self.world {
            held.insert(Resource::World);
        }
        held
    }

    fn acquire(&mut self, resource: Resource) -> Result<(), ResourceProblem> {
        match resource {
            Resource::Input(context) => match self.input {
                Some(held) => Err(ResourceProblem::InputBoundTwice {
                    held,
                    offered: context,
                }),
                None => {
                    self.input = Some(context);
                    Ok(())
                }
            },
            Resource::Audio(_) | Resource::World if self.holds(resource) => {
                Err(ResourceProblem::AcquiredTwice(resource))
            }
            Resource::Audio(scope) => {
                self.audio = Some(scope);
                Ok(())
            }
            Resource::World => {
                self.world = true;
                Ok(())
            }
        }
    }

    fn release(&mut self, resource: Resource) -> Result<(), ResourceProblem> {
        if !self.holds(resource) {
            return Err(ResourceProblem::ReleasedUnknown(resource));
        }
        match resource {
            Resource::Input(_) => self.input = None,
            Resource::Audio(_) => self.audio = None,
            Resource::World => self.world = false,
        }
        Ok(())
    }

    fn holds(&self, resource: Resource) -> bool {
        match resource {
            Resource::Input(context) => self.input == Some(context),
            Resource::Audio(scope) => self.audio == Some(scope),
            Resource::World => self.world,
        }
    }
}

/// What a load is asked to deliver. The mission's own dependency list is
/// content data this stage never reads: the flow runs whatever plan the
/// application declares, and refuses to start without one instead of
/// inventing a closure.
#[derive(Clone, Debug)]
pub struct LoadPlan {
    /// What the load builds.
    pub target: LoadTarget,
    /// The declared closure, in the order the screen shows the work.
    pub items: Vec<LoadItem>,
}

/// Why the loading screen's load could not run.
#[derive(Debug)]
pub enum LoadError {
    /// The application declared no dependency closure for this mission.
    NoPlan,
    /// There is no content session for the reads to resolve through.
    NoContentSession,
    /// The private cache is not available (it is held by a running load).
    NoCacheStore,
    /// A load is already running on the loading screen.
    AlreadyRunning,
    /// The producer's bounded step failed.
    Driver(DriverError),
    /// The transaction refused the lifecycle step.
    Transition(TransitionError),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoPlan => f.write_str("the mission declares no load plan"),
            Self::NoContentSession => f.write_str("no content session to read through"),
            Self::NoCacheStore => f.write_str("the private cache is not available"),
            Self::AlreadyRunning => f.write_str("a load is already running"),
            Self::Driver(error) => write!(f, "the load's bounded step failed: {error}"),
            Self::Transition(error) => write!(f, "the load refused that step: {error}"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<DriverError> for LoadError {
    fn from(error: DriverError) -> Self {
        Self::Driver(error)
    }
}

impl From<TransitionError> for LoadError {
    fn from(error: TransitionError) -> Self {
        Self::Transition(error)
    }
}

/// What one [`FrontEndFlow::pump_load`] step did.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadVerdict {
    /// The loading screen is not running a load.
    Idle,
    /// The load is still working; the screen draws this.
    Running(Box<LoadingScreen>),
    /// Every declared dependency was delivered and the machine moved on to
    /// the flight.
    Ready,
    /// A dependency was missing or unreadable; the machine went back to the
    /// flight check with the draft intact and the reason is what the screen
    /// shows.
    Failed {
        /// The failures in arrival order, each naming its dependency.
        reason: String,
    },
}

/// The loading screen's real load: one in-flight [`LoadingSession`] over the
/// content session the reads resolve through, with the private cache that
/// outlives every attempt.
#[derive(Debug, Default)]
pub struct LoadFlow {
    content: Option<ContentSession>,
    store: Option<CacheStore>,
    plan: Option<LoadPlan>,
    attempt: Option<LoadingSession>,
    attempts: u32,
}

impl LoadFlow {
    /// The content session the next attempt's reads resolve through. After a
    /// dependency is repaired the application mounts the repaired source
    /// again — a new session generation, which is what makes the retry a new
    /// request rather than a replay of the failed one
    /// (`crate::loading::LoadingSession::retry`'s own documentation).
    pub fn set_content_session(&mut self, content: ContentSession) {
        self.content = Some(content);
    }

    /// The mission's declared dependency closure.
    pub fn set_plan(&mut self, plan: LoadPlan) {
        self.plan = Some(plan);
    }

    /// The private cache the attempts run over. It is held by a running load
    /// and handed back by the teardown, so the application supplies it once.
    pub fn set_cache_store(&mut self, store: CacheStore) {
        self.store = Some(store);
    }

    /// The transaction state of the running attempt, if any.
    #[must_use]
    pub fn state(&self) -> Option<LoadState> {
        self.attempt.as_ref().map(LoadingSession::state)
    }

    /// What the running attempt's screen would draw.
    #[must_use]
    pub fn screen(&self) -> Option<LoadingScreen> {
        self.attempt.as_ref().map(LoadingSession::screen)
    }

    /// How many attempts this flow has started — the retry counter a test
    /// reads to know the second load really happened in the same process.
    #[must_use]
    pub fn attempts(&self) -> u32 {
        self.attempts
    }

    /// Whether a load is in flight.
    #[must_use]
    pub fn in_flight(&self) -> bool {
        self.attempt.is_some()
    }

    /// Whether a load could start right now; the plan-before-press check that
    /// keeps the machine from entering a loading screen nothing can leave by
    /// load.
    ///
    /// # Errors
    ///
    /// [`LoadError`] naming what is missing.
    pub fn check_begin(&self) -> Result<(), LoadError> {
        if self.attempt.is_some() {
            return Err(LoadError::AlreadyRunning);
        }
        if self.plan.is_none() {
            return Err(LoadError::NoPlan);
        }
        if self.content.is_none() {
            return Err(LoadError::NoContentSession);
        }
        if self.store.is_none() {
            return Err(LoadError::NoCacheStore);
        }
        Ok(())
    }

    fn begin(&mut self) -> Result<(), LoadError> {
        self.check_begin()?;
        let plan = self.plan.as_ref().expect("checked just above");
        let content = self.content.as_ref().expect("checked just above");
        let store = self.store.take().expect("checked just above");
        let request = LoadRequest {
            session: content.generation(),
            target: plan.target.clone(),
            items: plan.items.clone(),
        };
        let mut attempt = LoadingSession::new(request, store);
        attempt.begin()?;
        self.attempts += 1;
        self.attempt = Some(attempt);
        Ok(())
    }

    /// Runs one bounded step through the production producer and reports
    /// what the transaction settled at.
    ///
    /// # Errors
    ///
    /// [`LoadError`] when there is no producer to read through, or the
    /// driver refused the step.
    fn pump(&mut self) -> Result<(LoadState, Option<LoadingScreen>), LoadError> {
        let Self {
            attempt, content, ..
        } = self;
        let Some(attempt) = attempt.as_mut() else {
            return Ok((LoadState::Cancelled, None));
        };
        if !attempt.state().is_terminal() {
            let content = content.as_ref().ok_or(LoadError::NoContentSession)?;
            let mut io = SessionIo::new(content, |_item, payload| Ok(payload.bytes().to_vec()));
            attempt.pump(&mut io)?;
        }
        let state = attempt.state();
        Ok((state, (!state.is_terminal()).then(|| attempt.screen())))
    }

    /// Tears the attempt down the way leaving the loading screen must: a
    /// still-live load is cancelled at its next chunk boundary and the
    /// private cache comes back for the next attempt. Safe to call twice —
    /// the second call finds nothing to cancel.
    fn teardown(&mut self) {
        if let Some(attempt) = self.attempt.take() {
            self.store = Some(attempt.close());
        }
    }
}

/// Why a wired step could not happen. Every variant leaves the machine where
/// it was: the domain runs before the transition, and the transition runs
/// only after the domain said yes.
#[derive(Debug)]
pub enum FlowError {
    /// The state table or the authored screen refused the action.
    Screen(ScreenSessionError),
    /// A resource effect the application cannot take.
    Resource(ResourceProblem),
    /// The profile session could not be opened, selected, created or closed.
    Profile(SessionError),
    /// A profile population is already open; opening another would double the
    /// population claim.
    ProfileAlreadyOpen,
    /// The profile screen asked to close a profile that is not open.
    NoProfileOpen,
    /// The campaign transaction was refused or could not be saved.
    Campaign(CampaignSaveError),
    /// There is no campaign to transact against: the profile screen has not
    /// confirmed a profile yet.
    NoCampaign,
    /// The construction screen refused or could not save the commit.
    Construction(ScreenSaveError),
    /// The construction request named a blueprint the open construction
    /// screen is not editing — two views that have come apart.
    ConstructionMismatch {
        /// The blueprint the state machine's draft names.
        machine: ContentId,
        /// The blueprint the domain's screen is editing.
        screen: ContentId,
    },
    /// The construction flow was asked to commit with no screen open.
    NoConstructionScreen,
    /// The flow was given no construction rules to judge a commit by.
    NoConstructionInputs,
    /// The flight check was committed with no airframe roster to resolve the
    /// launch against.
    NoRoster,
    /// The flight check was committed with no player aircraft — the machine's
    /// own guard refuses this first, so it reports a wiring mistake rather
    /// than a player's mistake.
    NoPlayerAircraft,
    /// The flight check's aircraft could not be resolved into a launch.
    Launch(LaunchAssignmentError),
    /// The results screen applied an outcome the mission never reported.
    NoMissionOutcome,
    /// The machine's verdict and the mission's own record disagree, so
    /// neither is applied.
    OutcomeMismatch {
        /// What the front end says happened.
        expected: MissionResult,
        /// What the mission's record says happened.
        recorded: MissionResult,
    },
    /// The loading screen's load could not run.
    Load(LoadError),
}

impl fmt::Display for FlowError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Screen(error) => write!(f, "{error}"),
            Self::Resource(error) => write!(f, "{error}"),
            Self::Profile(error) => write!(f, "profile: {error}"),
            Self::ProfileAlreadyOpen => f.write_str("a profile is already open"),
            Self::NoProfileOpen => f.write_str("no profile is open"),
            Self::Campaign(error) => write!(f, "campaign: {error}"),
            Self::NoCampaign => f.write_str("no campaign run is open"),
            Self::Construction(error) => write!(f, "construction: {error}"),
            Self::ConstructionMismatch { machine, screen } => write!(
                f,
                "the flow was asked to commit {machine} while the screen edits {screen}"
            ),
            Self::NoConstructionScreen => f.write_str("no construction screen is open"),
            Self::NoConstructionInputs => f.write_str("no construction rules were supplied"),
            Self::NoRoster => f.write_str("no airframe roster was supplied"),
            Self::NoPlayerAircraft => f.write_str("the flight check committed no aircraft"),
            Self::Launch(error) => write!(f, "launch: {error}"),
            Self::NoMissionOutcome => f.write_str("the mission reported no outcome"),
            Self::OutcomeMismatch { expected, recorded } => {
                write!(
                    f,
                    "the screen says {expected:?} but the record says {recorded:?}"
                )
            }
            Self::Load(error) => write!(f, "load: {error}"),
        }
    }
}

impl std::error::Error for FlowError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Screen(error) => Some(error),
            Self::Resource(error) => Some(error),
            Self::Profile(error) => Some(error),
            Self::Campaign(error) => Some(error),
            Self::Construction(error) => Some(error),
            Self::Launch(error) => Some(error),
            Self::Load(error) => Some(error),
            _ => None,
        }
    }
}

impl From<ScreenSessionError> for FlowError {
    fn from(error: ScreenSessionError) -> Self {
        Self::Screen(error)
    }
}

impl From<ResourceProblem> for FlowError {
    fn from(error: ResourceProblem) -> Self {
        Self::Resource(error)
    }
}

impl From<LoadError> for FlowError {
    fn from(error: LoadError) -> Self {
        Self::Load(error)
    }
}

impl From<Refusal> for FlowError {
    fn from(error: Refusal) -> Self {
        Self::Screen(ScreenSessionError::Refused(error))
    }
}

/// The domain side of the flows: the objects every request reaches, in the
/// order the application owns them.
#[derive(Debug)]
struct FlowDomain {
    setup: FlowSetup,
    profile: Option<ProfileSession>,
    profile_id: Option<ProfileId>,
    run: Option<CampaignRun>,
    construction: Option<ConstructionScreen>,
    construction_id: Option<ContentId>,
    committed: Option<Loadout>,
    launch: Option<ResolvedLaunch>,
    mission_outcome: Option<MissionOutcomeRecord>,
    applied: Option<AppliedOutcome>,
    teardown: Option<TeardownReport>,
}

impl FlowDomain {
    fn new(setup: FlowSetup) -> Self {
        Self {
            setup,
            profile: None,
            profile_id: None,
            run: None,
            construction: None,
            construction_id: None,
            committed: None,
            launch: None,
            mission_outcome: None,
            applied: None,
            teardown: None,
        }
    }

    /// Runs the transaction the machine asked for, before the machine moves.
    ///
    /// # Errors
    ///
    /// A [`FlowError`] naming the producer that refused; nothing has changed
    /// anywhere when one comes back.
    fn apply(&mut self, request: Option<&Request>) -> Result<(), FlowError> {
        match request {
            None => Ok(()),
            Some(Request::OpenProfile(intent)) => self.open_profile(*intent),
            Some(Request::CloseProfile) => self.close_profile(),
            Some(Request::CommitBlueprint(draft)) => self.commit_blueprint(draft),
            Some(Request::CommitLoadout(loadout)) => self.commit_loadout(loadout),
            Some(Request::ApplyOutcome(verdict)) => self.apply_outcome(*verdict),
            // Abandoning a mission is the campaign's *not* happening: the
            // contract's "a failed load does not consume campaign money or
            // progress" holds for an abort too, and the world release is the
            // resource effect the machine already emitted.
            Some(Request::AbandonMission) => Ok(()),
        }
    }

    fn open_profile(&mut self, intent: ProfileIntent) -> Result<(), FlowError> {
        if self.profile.is_some() {
            return Err(FlowError::ProfileAlreadyOpen);
        }
        let setup = &self.setup;
        let mut session =
            ProfileSession::open(&setup.base, setup.origin, setup.kind, &setup.catalog)
                .map_err(FlowError::Profile)?;
        match intent {
            ProfileIntent::New => {
                session
                    .create(&setup.new_profile)
                    .map_err(FlowError::Profile)?;
            }
            ProfileIntent::Existing => {
                if let Some(id) = setup.existing {
                    session.select(id).map_err(FlowError::Profile)?;
                }
            }
        }
        let id = session
            .selected()
            .ok_or(FlowError::Profile(SessionError::NoProfileSelected))?;
        let run = CampaignRun::open(
            &mut session,
            setup.graph.clone(),
            &setup.campaign_profile,
            &setup.run,
            &setup.difficulty,
        )
        .map_err(FlowError::Campaign)?;
        self.profile = Some(session);
        self.profile_id = Some(id);
        self.run = Some(run);
        Ok(())
    }

    fn close_profile(&mut self) -> Result<(), FlowError> {
        let session = self.profile.take().ok_or(FlowError::NoProfileOpen)?;
        let report = session.finish().map_err(FlowError::Profile)?;
        self.profile_id = None;
        self.run = None;
        self.construction = None;
        self.construction_id = None;
        self.committed = None;
        self.launch = None;
        self.mission_outcome = None;
        // The applied outcome belonged to the campaign that just closed: the
        // next profile this process opens must not report it as its own.
        self.applied = None;
        self.teardown = Some(report);
        Ok(())
    }

    fn commit_blueprint(&mut self, draft: &ConstructionDraft) -> Result<(), FlowError> {
        let held = self
            .construction_id
            .as_ref()
            .ok_or(FlowError::NoConstructionScreen)?;
        if *held != draft.blueprint {
            return Err(FlowError::ConstructionMismatch {
                machine: draft.blueprint.clone(),
                screen: held.clone(),
            });
        }
        let run = self.run.as_ref().ok_or(FlowError::NoCampaign)?;
        let profile = self
            .profile
            .as_mut()
            .ok_or(FlowError::Profile(SessionError::NoProfileSelected))?;
        let screen = self
            .construction
            .as_mut()
            .ok_or(FlowError::NoConstructionScreen)?;
        let inputs = self
            .setup
            .construction
            .as_ref()
            .ok_or(FlowError::NoConstructionInputs)?;
        let ctx = inputs.context(run.graph());
        let mut state = run.state().clone();
        screen
            .commit_saved(&ctx, &mut state, profile)
            .map_err(FlowError::Construction)?;
        // The screen wrote the committed state to the profile; the run
        // re-reads it, so the two owners of "the live campaign" cannot
        // diverge (a construction commit and an outcome commit are the only
        // writes, and both end up in the one saved revision).
        self.run = Some(
            CampaignRun::open(
                profile,
                self.setup.graph.clone(),
                &self.setup.campaign_profile,
                &self.setup.run,
                &self.setup.difficulty,
            )
            .map_err(FlowError::Campaign)?,
        );
        Ok(())
    }

    fn commit_loadout(&mut self, loadout: &Loadout) -> Result<(), FlowError> {
        let player = loadout.player.clone().ok_or(FlowError::NoPlayerAircraft)?;
        let roles = self.setup.roles.as_ref().ok_or(FlowError::NoRoster)?;
        let launch = roles
            .resolve_launch(
                &OwnedLoadout {
                    airframe: player,
                    session_generation: self.setup.session_generation,
                },
                None,
            )
            .map_err(FlowError::Launch)?;
        self.committed = Some(loadout.clone());
        self.launch = Some(launch);
        Ok(())
    }

    fn apply_outcome(&mut self, verdict: MissionOutcome) -> Result<(), FlowError> {
        let record = self
            .mission_outcome
            .as_ref()
            .ok_or(FlowError::NoMissionOutcome)?;
        let expected = match verdict {
            MissionOutcome::Success => MissionResult::Succeeded,
            MissionOutcome::Failure => MissionResult::Failed,
        };
        if record.outcome != expected {
            return Err(FlowError::OutcomeMismatch {
                expected,
                recorded: record.outcome,
            });
        }
        let record = self.mission_outcome.take().expect("checked just above");
        let run = self.run.as_mut().ok_or(FlowError::NoCampaign)?;
        let profile = self
            .profile
            .as_mut()
            .ok_or(FlowError::Profile(SessionError::NoProfileSelected))?;
        let applied = run
            .report_outcome(profile, &record)
            .map_err(FlowError::Campaign)?;
        self.applied = Some(applied);
        Ok(())
    }

    /// Hands the flow the construction screen a `CommitBlueprint` commits
    /// through, under the blueprint id the state machine's draft carries.
    ///
    /// # Errors
    ///
    /// [`FlowError::NoCampaign`] when no profile is open to price the draft
    /// against.
    fn attach_construction(
        &mut self,
        blueprint: ContentId,
        screen: ConstructionScreen,
    ) -> Result<(), FlowError> {
        if self.run.is_none() {
            return Err(FlowError::NoCampaign);
        }
        self.construction = Some(screen);
        self.construction_id = Some(blueprint);
        Ok(())
    }

    fn set_mission_outcome(&mut self, record: MissionOutcomeRecord) {
        self.mission_outcome = Some(record);
    }
}

/// The construction, flight check, loading and return flows driven against
/// the real domain: the object an application runs the front end through.
///
/// Every action is **planned first**: [`FrontEnd::plan`] reports what the
/// transition would ask for without touching the machine, the domain runs,
/// and only then does the machine move. That order is what makes a refused
/// campaign save, an unknown airframe or a missing load dependency a
/// *reportable* failure instead of a screen that advanced over a transaction
/// that never happened.
#[derive(Debug)]
pub struct FrontEndFlow {
    session: ScreenSession,
    ledger: ResourceLedger,
    domain: FlowDomain,
    load: LoadFlow,
    exiting: bool,
}

impl FrontEndFlow {
    /// A flow on the install selection with `deck`'s authored screens and
    /// `setup`'s profile, campaign and construction inputs.
    ///
    /// The ledger starts holding what the machine starts holding — the install
    /// selection's own input context and audio scope — fed through the same
    /// acquire path every later effect takes, so the first release a transition
    /// emits has something to release.
    #[must_use]
    pub fn new(deck: ScreenDeck, setup: FlowSetup) -> Self {
        let session = ScreenSession::new(deck);
        let mut ledger = ResourceLedger::default();
        for resource in session.front_end().held() {
            ledger
                .apply(&Effect::Acquire(*resource))
                .expect("the start screen holds each resource at most once");
        }
        Self {
            session,
            ledger,
            domain: FlowDomain::new(setup),
            load: LoadFlow::default(),
            exiting: false,
        }
    }

    /// The presentation behind the flow.
    #[must_use]
    pub fn session(&self) -> &ScreenSession {
        &self.session
    }

    /// The machine behind the presentation.
    #[must_use]
    pub fn front_end(&self) -> &FrontEnd {
        self.session.front_end()
    }

    /// What the application currently holds.
    #[must_use]
    pub fn ledger(&self) -> &ResourceLedger {
        &self.ledger
    }

    /// The domain the requests reach, read-only.
    #[must_use]
    pub fn domain(&self) -> FlowDomainView<'_> {
        FlowDomainView(&self.domain)
    }

    /// The loading screen's load.
    #[must_use]
    pub fn load(&self) -> &LoadFlow {
        &self.load
    }

    /// Whether a `Quit` has been accepted: the application leaves when this
    /// is `true`.
    #[must_use]
    pub fn exiting(&self) -> bool {
        self.exiting
    }

    /// Presses a button (or applies a system result) end to end.
    ///
    /// # Errors
    ///
    /// [`FlowError::Screen`] when the table or the authored screen refuses,
    /// otherwise whatever the domain said — and then the machine has not
    /// moved.
    pub fn press(&mut self, action: Action) -> Result<Outcome, FlowError> {
        let plan = self
            .session
            .front_end()
            .plan(action)
            .map_err(ScreenSessionError::Refused)?;
        self.prepare(&plan)?;
        self.domain.apply(plan.request.as_ref())?;
        let outcome = self.session.press(action)?;
        self.consume(&outcome)?;
        Ok(outcome)
    }

    /// Activates the focused button.
    ///
    /// # Errors
    ///
    /// As [`Self::press`], including no focus at all.
    pub fn activate(&mut self) -> Result<Outcome, FlowError> {
        let plan = self
            .session
            .front_end()
            .plan_focus()
            .map_err(ScreenSessionError::Refused)?;
        self.prepare(&plan)?;
        self.domain.apply(plan.request.as_ref())?;
        let outcome = self.session.activate()?;
        self.consume(&outcome)?;
        Ok(outcome)
    }

    /// Presses the authored button under a surface point.
    ///
    /// # Errors
    ///
    /// As [`Self::press`], including no button at the point.
    pub fn click(&mut self, surface: (u32, u32), x: u32, y: u32) -> Result<Outcome, FlowError> {
        let action = self.session.action_at(surface, x, y)?;
        self.press(action)
    }

    /// Answers an open discard prompt by dropping the draft and carrying on.
    ///
    /// # Errors
    ///
    /// As [`Self::press`], including no prompt being open.
    pub fn confirm_discard(&mut self) -> Result<Outcome, FlowError> {
        let plan = self
            .session
            .front_end()
            .plan_pending()
            .map_err(ScreenSessionError::Refused)?;
        self.prepare(&plan)?;
        self.domain.apply(plan.request.as_ref())?;
        let outcome = self.session.confirm_discard()?;
        self.consume(&outcome)?;
        Ok(outcome)
    }

    /// Answers an open discard prompt by keeping the draft. No transition and
    /// no request, so nothing is planned.
    ///
    /// # Errors
    ///
    /// [`FlowError::Screen`] when no prompt is open.
    pub fn keep_editing(&mut self) -> Result<(), FlowError> {
        self.session.keep_editing().map_err(Into::into)
    }

    /// Reports a failed load the way the producer does: back to the flight
    /// check with the draft intact.
    ///
    /// # Errors
    ///
    /// As [`Self::press`].
    pub fn report_load_failure(&mut self, reason: &str) -> Result<Outcome, FlowError> {
        let plan = self
            .session
            .front_end()
            .plan(Action::LoadFailed)
            .map_err(ScreenSessionError::Refused)?;
        self.prepare(&plan)?;
        self.domain.apply(plan.request.as_ref())?;
        let outcome = self.session.report_load_failure(reason)?;
        self.consume(&outcome)?;
        Ok(outcome)
    }

    /// One bounded step of the loading screen's load: the producer reads the
    /// next dependency, and a load that has settled moves the machine itself
    /// — to the flight when everything was delivered, back to the flight
    /// check when a dependency was missing.
    ///
    /// # Errors
    ///
    /// [`FlowError::Load`] when there is no producer or the driver refused,
    /// otherwise as [`Self::press`] for the transition the step performs.
    pub fn pump_load(&mut self) -> Result<LoadVerdict, FlowError> {
        let (state, running) = self.load.pump()?;
        match state {
            LoadState::Ready => {
                self.load.teardown();
                self.press(Action::LoadSucceeded)?;
                Ok(LoadVerdict::Ready)
            }
            LoadState::Failed => {
                let reason = self
                    .load
                    .screen()
                    .map(|screen| {
                        screen
                            .failures
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join("; ")
                    })
                    .unwrap_or_default();
                self.load.teardown();
                self.report_load_failure(&reason)?;
                Ok(LoadVerdict::Failed { reason })
            }
            _ => match running {
                Some(screen) => Ok(LoadVerdict::Running(Box::new(screen))),
                None => Ok(LoadVerdict::Idle),
            },
        }
    }

    /// Runs the load to its terminal state, pumping one bounded step at a
    /// time, and returns the verdict the last step produced. The bound is the
    /// declared closure plus the validation gate, so a flow that somehow never
    /// settles reports it instead of spinning forever.
    ///
    /// # Errors
    ///
    /// As [`Self::pump_load`].
    pub fn drive_load(&mut self) -> Result<LoadVerdict, FlowError> {
        let budget = self
            .load
            .plan
            .as_ref()
            .map(|plan| plan.items.len() + 2)
            .unwrap_or(1);
        let mut verdict = self.pump_load()?;
        for _ in 0..budget {
            if !matches!(verdict, LoadVerdict::Running(_)) {
                return Ok(verdict);
            }
            verdict = self.pump_load()?;
        }
        Ok(verdict)
    }

    /// Replaces the flight-check selection on the machine (the screen's own
    /// domain-side input, as in F45-B).
    ///
    /// # Errors
    ///
    /// [`FlowError::Screen`] off the flight check or while a prompt is open.
    pub fn select_loadout(&mut self, loadout: Loadout) -> Result<(), FlowError> {
        self.session.select_loadout(loadout).map_err(Into::into)
    }

    /// Opens the machine's construction draft (F45-B's domain-side input).
    ///
    /// # Errors
    ///
    /// [`FlowError::Screen`] off the construction screen or while a prompt is
    /// open.
    pub fn open_construction(&mut self, draft: ConstructionDraft) -> Result<(), FlowError> {
        self.session.open_construction(draft).map_err(Into::into)
    }

    /// Replaces the machine's edited components.
    ///
    /// # Errors
    ///
    /// As [`Self::open_construction`], and with no draft open.
    pub fn edit_construction(&mut self, components: Vec<ContentId>) -> Result<(), FlowError> {
        self.session
            .edit_construction(components)
            .map_err(Into::into)
    }

    /// Hands the flow the construction screen `CommitBlueprint` commits
    /// through, under the blueprint id the machine's draft carries.
    ///
    /// # Errors
    ///
    /// [`FlowError::NoCampaign`] before a profile has been confirmed.
    pub fn attach_construction(
        &mut self,
        blueprint: ContentId,
        screen: ConstructionScreen,
    ) -> Result<(), FlowError> {
        self.domain.attach_construction(blueprint, screen)
    }

    /// Hands the flow the mission's own outcome record, which the results
    /// screen's request applies. The front end only knows Success/Failure; the
    /// record (node, terminal event, score, authority) is the mission's, and a
    /// mismatch between the two is refused rather than resolved here.
    pub fn set_mission_outcome(&mut self, record: MissionOutcomeRecord) {
        self.domain.set_mission_outcome(record);
    }

    /// Declares whether the mission needs a wingmate (the mission data's say).
    pub fn set_wingmate_required(&mut self, required: bool) {
        self.session.set_wingmate_required(required);
    }

    /// The content session the loading screen's reads resolve through.
    pub fn set_content_session(&mut self, content: ContentSession) {
        self.load.set_content_session(content);
    }

    /// The private cache the loading screen's attempts run over.
    pub fn set_cache_store(&mut self, store: CacheStore) {
        self.load.set_cache_store(store);
    }

    /// The mission's declared dependency closure — content data this stage
    /// never reads itself.
    pub fn set_load_plan(&mut self, plan: LoadPlan) {
        self.load.set_plan(plan);
    }

    /// The plan for an action: what it would ask for and where it would go,
    /// with the machine untouched.
    ///
    /// # Errors
    ///
    /// [`FlowError::Screen`] when the machine would refuse the action.
    pub fn plan(&self, action: Action) -> Result<Plan, FlowError> {
        self.session
            .front_end()
            .plan(action)
            .map_err(|refusal| FlowError::Screen(ScreenSessionError::Refused(refusal)))
    }

    /// Everything before the machine moves: the loading screen must be able
    /// to start its load, so a launch that has no closure, no content session
    /// or no cache is refused while the player is still on the flight check.
    fn prepare(&self, plan: &Plan) -> Result<(), FlowError> {
        if plan.to == Screen::Loading && plan.from != Screen::Loading {
            self.load.check_begin()?;
        }
        Ok(())
    }

    /// Takes the effects the machine has already produced. The request in
    /// them is the one [`Self::press`] ran before the transition — it is read
    /// back for the record, never applied twice.
    fn consume(&mut self, outcome: &Outcome) -> Result<(), FlowError> {
        for effect in &outcome.effects {
            if let Effect::Request(_) = effect {
                continue;
            }
            self.ledger.apply(effect)?;
            if let Effect::ExitApplication = effect {
                self.exiting = true;
            }
        }
        match (outcome.from, outcome.to) {
            (from, Screen::Loading) if from != Screen::Loading => {
                self.load.begin().map_err(FlowError::Load)?;
            }
            (Screen::Loading, to) if to != Screen::Loading => self.load.teardown(),
            _ => {}
        }
        Ok(())
    }
}

/// Read-only access to the domain the flow transacts against.
#[derive(Clone, Copy)]
pub struct FlowDomainView<'a>(&'a FlowDomain);

impl FlowDomainView<'_> {
    /// The open profile session, when a profile is open.
    #[must_use]
    pub fn profile(&self) -> Option<&ProfileSession> {
        self.0.profile.as_ref()
    }

    /// The selected profile's id.
    #[must_use]
    pub fn profile_id(&self) -> Option<ProfileId> {
        self.0.profile_id
    }

    /// The live campaign run.
    #[must_use]
    pub fn campaign(&self) -> Option<&CampaignRun> {
        self.0.run.as_ref()
    }

    /// The open construction screen.
    #[must_use]
    pub fn construction(&self) -> Option<&ConstructionScreen> {
        self.0.construction.as_ref()
    }

    /// The loadout the flight check committed.
    #[must_use]
    pub fn committed_loadout(&self) -> Option<&Loadout> {
        self.0.committed.as_ref()
    }

    /// The airframe the committed loadout resolved into.
    #[must_use]
    pub fn launch(&self) -> Option<&ResolvedLaunch> {
        self.0.launch.as_ref()
    }

    /// What the results screen's outcome transaction wrote, if it ran.
    #[must_use]
    pub fn applied_outcome(&self) -> Option<&AppliedOutcome> {
        self.0.applied.as_ref()
    }

    /// What the profile screen's close ended the session with.
    #[must_use]
    pub fn teardown(&self) -> Option<&TeardownReport> {
        self.0.teardown.as_ref()
    }

    /// The mission outcome the results screen is waiting to apply.
    #[must_use]
    pub fn pending_outcome(&self) -> Option<&MissionOutcomeRecord> {
        self.0.mission_outcome.as_ref()
    }
}

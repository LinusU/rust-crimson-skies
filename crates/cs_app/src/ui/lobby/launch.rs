//! The lobby launch consumes the F56 match rules (F56-C2).
//!
//! Spec: `specs/F56-original-multiplayer-scenarios-and-mode-rules.md`, stage
//! `### F56-C`; contract `docs/contracts/UI-NETWORK.md` ("Content/rules
//! mismatch rejects launch before expensive asset loading").
//!
//! [`MatchLauncher::begin`] is the one place a host's launch is judged before
//! the lobby commits it, in this order, each refusal by name:
//!
//! 1. the selected scenario resolves through
//!    [`SlotCatalog::get`] (an id the installation does not ship is
//!    [`LaunchRefusal::UnknownScenario`], never the first slot) and declares
//!    a known objective count ([`LaunchRefusal::ObjectivesUnknown`]);
//! 2. [`RuleDraft::from_lobby`] fills the host's own options, the
//!    application's measured fields are laid over it, and `resolve` either
//!    yields [`MatchRules`] or refuses naming every unknown field
//!    ([`LaunchRefusal::Rules`]);
//! 3. [`LaunchPlan::new`] and [`MatchRules::validate_start`] judge the plan
//!    against the lobby's actual members;
//! 4. only then does the lobby's two-step launch run, and the member
//!    acknowledgments commit exactly one [`MatchSession`] from the plan's
//!    limits, victory label and the slot's possessable objectives.
//!
//! No rule is decided here: unknown rules stay unknown and block the launch.

use cs_content::multiplayer::SlotCatalog;
use cs_net::lobby::{LaunchError, LaunchOrder, LaunchProgress, Loadout, Phase, TeamMode};
use cs_net::rules::{LaunchPlan, MatchRules, ResolveError, RuleDraft, StartError, StartRequest};
use cs_sim::multiplayer::result::{ConfigError, Limits, Roster, ScoreTable, VictoryRule};
use cs_sim::multiplayer::session::{MatchSession, SessionConfig, SessionError};
use cs_types::content::ContentId;
use cs_types::net::{PeerId, SessionId};

use super::link::LobbyLink;

/// What the application supplies to a launch; none of it is invented here.
pub struct LaunchInputs<'a> {
    /// The installation's scenario slots.
    pub catalog: &'a SlotCatalog,
    /// The rule fields the application can answer (for instance
    /// `cs_net::rules::measured::draft`). The lobby's own team grouping and
    /// late-join policy always override the draft's.
    pub measured: RuleDraft,
    /// What each lethal event is worth.
    pub table: ScoreTable,
    /// The session generation this match runs under; a rematch passes a new one.
    pub generation: SessionId,
    /// Whether a loadout is a custom blueprint (for the mode's plane rule).
    pub is_custom: &'a dyn Fn(&Loadout) -> bool,
}

/// Why a launch was refused. A refusal changes nothing and may be retried.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchRefusal {
    /// The selected scenario is not in this installation's catalog.
    UnknownScenario(ContentId),
    /// The scenario's objective records are unknown, so its objective count is.
    ObjectivesUnknown(ContentId),
    /// The rules did not resolve; the error names every unknown field.
    Rules(ResolveError),
    /// The resolved victory rule has no resolver.
    VictoryUnsupported(&'static str),
    /// The plan or the lobby contradicts the mode's rules.
    Start(StartError),
    /// The roster could not be built from the lobby's members.
    Roster(ConfigError),
    /// The lobby refused the launch or the acknowledgment.
    Lobby(LaunchError),
    /// The session refused its configuration.
    Session(SessionError),
    /// A match is already running or a launch is already pending.
    Busy,
}

/// What a launch step did.
#[derive(Debug)]
pub enum LaunchStep {
    /// Still waiting on these members to acknowledge.
    Waiting(Vec<PeerId>),
    /// The launch committed and the match session started.
    Started,
}

struct Pending {
    plan: LaunchPlan,
    objectives: u32,
    config: Config,
}

struct Config {
    table: ScoreTable,
    generation: SessionId,
}

/// Owns the launch in flight and the running match.
#[derive(Default)]
pub struct MatchLauncher {
    pending: Option<Pending>,
    session: Option<MatchSession>,
}

impl MatchLauncher {
    /// A launcher with no launch and no match.
    pub fn new() -> Self {
        Self::default()
    }

    /// The running match, if any.
    pub fn session(&self) -> Option<&MatchSession> {
        self.session.as_ref()
    }

    /// The running match, mutably (events and tick closes go here).
    pub fn session_mut(&mut self) -> Option<&mut MatchSession> {
        self.session.as_mut()
    }

    /// Whether a launch awaits acknowledgments.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The host asks to launch. Everything the rules can refuse is refused
    /// *before* the lobby is asked to commit.
    ///
    /// # Errors
    ///
    /// The first [`LaunchRefusal`]; nothing changed.
    pub fn begin(
        &mut self,
        link: &mut LobbyLink,
        inputs: &LaunchInputs<'_>,
    ) -> Result<LaunchStep, LaunchRefusal> {
        if self.pending.is_some() && link.lobby().phase() != Phase::Launching {
            // The lobby cancelled the launch itself (a member left).
            self.pending = None;
        }
        if self.pending.is_some() || self.session.is_some() {
            return Err(LaunchRefusal::Busy);
        }
        let lobby = link.lobby();
        let rules = lobby.rules().clone();
        let slot = inputs
            .catalog
            .get(&rules.scenario)
            .ok_or_else(|| LaunchRefusal::UnknownScenario(rules.scenario.clone()))?;
        let objectives = slot
            .possessable_objectives()
            .and_then(|n| u32::try_from(n).ok())
            .ok_or_else(|| LaunchRefusal::ObjectivesUnknown(rules.scenario.clone()))?;

        let mut draft = inputs.measured.clone();
        let from_lobby = RuleDraft::from_lobby(&rules);
        draft.teams = from_lobby.teams;
        draft.late_join = from_lobby.late_join;
        let resolved: MatchRules = draft.resolve().map_err(LaunchRefusal::Rules)?;
        let plan = LaunchPlan::new(&rules, resolved).map_err(LaunchRefusal::Start)?;
        VictoryRule::from_label(plan.rules().victory().label()).ok_or(
            LaunchRefusal::VictoryUnsupported(plan.rules().victory().label()),
        )?;

        let members: Vec<_> = lobby.members().collect();
        let request = StartRequest {
            humans: u8::try_from(members.len()).unwrap_or(u8::MAX),
            custom_planes: u8::try_from(
                members
                    .iter()
                    .filter_map(|(_, m)| m.loadout.as_ref())
                    .filter(|l| (inputs.is_custom)(l))
                    .count(),
            )
            .unwrap_or(u8::MAX),
            largest_loadout: members
                .iter()
                .filter_map(|(_, m)| m.loadout.as_ref())
                .map(|l| u16::try_from(l.components.len()).unwrap_or(u16::MAX))
                .max()
                .unwrap_or(0),
        };
        plan.validate_start(&request)
            .map_err(LaunchRefusal::Start)?;

        let host = link.host_peer();
        let progress = link.begin_launch(host).map_err(LaunchRefusal::Lobby)?;
        self.pending = Some(Pending {
            plan,
            objectives,
            config: Config {
                table: inputs.table,
                generation: inputs.generation,
            },
        });
        self.advance(progress)
    }

    /// A member acknowledges the pending launch; the last one starts the match.
    ///
    /// # Errors
    ///
    /// [`LaunchRefusal::Lobby`] from the lobby, or [`LaunchRefusal::Roster`] /
    /// [`LaunchRefusal::Session`] if the committed order cannot start.
    pub fn acknowledge(
        &mut self,
        link: &mut LobbyLink,
        from: PeerId,
    ) -> Result<LaunchStep, LaunchRefusal> {
        let progress = link
            .acknowledge_launch(from)
            .map_err(LaunchRefusal::Lobby)?;
        self.advance(progress)
    }

    /// The host cancels the pending launch; the plan is dropped with it.
    ///
    /// # Errors
    ///
    /// [`LaunchRefusal::Lobby`] when nothing is pending or the sender is not host.
    pub fn cancel(&mut self, link: &mut LobbyLink) -> Result<(), LaunchRefusal> {
        let host = link.host_peer();
        link.cancel_launch(host).map_err(LaunchRefusal::Lobby)?;
        self.pending = None;
        Ok(())
    }

    fn advance(&mut self, progress: LaunchProgress) -> Result<LaunchStep, LaunchRefusal> {
        match progress {
            LaunchProgress::Waiting { pending } => Ok(LaunchStep::Waiting(pending)),
            LaunchProgress::Committed(order) => {
                let pending = self
                    .pending
                    .take()
                    .ok_or(LaunchRefusal::Lobby(LaunchError::NoLaunchPending))?;
                self.session = Some(start_session(&pending, &order)?);
                Ok(LaunchStep::Started)
            }
        }
    }

    /// Closes the finished match: the lobby returns to gathering and the
    /// match is dropped, so the next launch starts a new session generation
    /// with no score, pickup or timer carried over.
    ///
    /// # Errors
    ///
    /// [`LaunchRefusal::Busy`] while the match has no result yet;
    /// [`LaunchRefusal::Lobby`] if the lobby is not in a match.
    pub fn finish(&mut self, link: &mut LobbyLink) -> Result<(), LaunchRefusal> {
        if !self.session.as_ref().is_some_and(MatchSession::is_over) {
            return Err(LaunchRefusal::Busy);
        }
        link.finish_match().map_err(|_| {
            LaunchRefusal::Lobby(LaunchError::WrongPhase {
                phase: link.lobby().phase(),
            })
        })?;
        self.session = None;
        Ok(())
    }
}

fn start_session(pending: &Pending, order: &LaunchOrder) -> Result<MatchSession, LaunchRefusal> {
    let peers: Vec<PeerId> = order.members.keys().copied().collect();
    let roster = match pending.plan.rules().teams() {
        TeamMode::FreeForAll => Roster::free_for_all(&peers),
        TeamMode::Teams { .. } => {
            let held: Vec<(PeerId, u8)> = order
                .members
                .iter()
                .filter_map(|(peer, (_, team, _))| team.map(|t| (*peer, t.0)))
                .collect();
            if held.len() == peers.len() {
                Roster::teams(&held)
            } else {
                // A team match with a pilot on no team is not a roster.
                Err(ConfigError::TooFewTeams)
            }
        }
    }
    .map_err(LaunchRefusal::Roster)?;
    let limits = pending.plan.resolver_limits();
    let victory = VictoryRule::from_label(pending.plan.rules().victory().label()).ok_or(
        LaunchRefusal::VictoryUnsupported(pending.plan.rules().victory().label()),
    )?;
    MatchSession::start(SessionConfig {
        scenario: pending.plan.scenario(),
        session: pending.config.generation,
        roster,
        table: pending.config.table,
        limits: Limits {
            time_limit: limits.time_limit,
            score_limit: limits.score_limit,
        },
        victory,
        objectives: pending.objectives,
    })
    .map_err(LaunchRefusal::Session)
}

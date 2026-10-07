//! The machine that walks [`super::TABLE`] (F45-A).
//!
//! [`FrontEnd`] owns the current screen, focus, held resources and the
//! drafts a screen edits (construction components, the flight-check loadout).
//! It owns **no campaign or profile field**: every change to those is an
//! [`Effect::Request`] for the domain, so a Cancel at any preflight screen
//! cannot have touched cash or ownership — there is nothing here to touch.

use std::collections::BTreeSet;
use std::fmt;

use cs_types::content::{ContentId, ContentKind};

use super::{Action, ActionSource, Guard, RequestKind, Resource, Screen, find, resources, rows_on};

/// Whether the profile is new or existing.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ProfileIntent {
    /// Create a profile.
    New,
    /// Open an existing one.
    Existing,
}

/// How a mission ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MissionOutcome {
    /// The mission was won.
    Success,
    /// The mission was lost.
    Failure,
}

/// The flight-check selection, committed as one request.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Loadout {
    /// The player's aircraft (an `airframe` id).
    pub player: Option<ContentId>,
    /// The wingmate's aircraft (an `airframe` id).
    pub wingmate: Option<ContentId>,
    /// The ammunition (an `ammo` id).
    pub ammunition: Option<ContentId>,
}

/// Why a loadout cannot launch.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadoutProblem {
    /// No player aircraft chosen.
    MissingPlayer,
    /// The mission needs a wingmate and none is chosen.
    MissingWingmate,
    /// No ammunition chosen.
    MissingAmmunition,
    /// A choice is not an id of the kind its slot takes.
    WrongKind {
        /// The slot.
        slot: &'static str,
        /// The offending id.
        id: ContentId,
    },
}

impl Loadout {
    /// Whether anything is selected.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.player.is_none() && self.wingmate.is_none() && self.ammunition.is_none()
    }

    /// Every reason this loadout cannot launch; empty when it can. Whether a
    /// mission needs a wingmate is the caller's declaration, never assumed.
    #[must_use]
    pub fn validate(&self, wingmate_required: bool) -> Vec<LoadoutProblem> {
        let mut problems = Vec::new();
        let slots = [
            (
                "player",
                &self.player,
                ContentKind::Airframe,
                Some(LoadoutProblem::MissingPlayer),
            ),
            (
                "wingmate",
                &self.wingmate,
                ContentKind::Airframe,
                wingmate_required.then_some(LoadoutProblem::MissingWingmate),
            ),
            (
                "ammunition",
                &self.ammunition,
                ContentKind::Ammo,
                Some(LoadoutProblem::MissingAmmunition),
            ),
        ];
        for (slot, choice, kind, missing) in slots {
            match choice {
                None => problems.extend(missing),
                Some(id) if id.kind() != kind => problems.push(LoadoutProblem::WrongKind {
                    slot,
                    id: id.clone(),
                }),
                Some(_) => {}
            }
        }
        problems
    }
}

/// The construction screen's working copy.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConstructionDraft {
    /// The blueprint being edited.
    pub blueprint: ContentId,
    /// The components as last committed.
    pub saved: Vec<ContentId>,
    /// The components as currently edited.
    pub components: Vec<ContentId>,
}

impl ConstructionDraft {
    /// Whether the edit differs from what was committed.
    #[must_use]
    pub fn is_dirty(&self) -> bool {
        self.saved != self.components
    }
}

/// A load failure kept so the selection screen can show it.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    /// Why the load failed.
    pub reason: String,
}

/// A domain transaction a transition asks for.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Create or open the profile.
    OpenProfile(ProfileIntent),
    /// Save and close the profile.
    CloseProfile,
    /// Commit the construction draft.
    CommitBlueprint(ConstructionDraft),
    /// Select the aircraft and ammunition together.
    CommitLoadout(Loadout),
    /// Apply the mission outcome to the campaign.
    ApplyOutcome(MissionOutcome),
    /// Abandon the running mission.
    AbandonMission,
}

/// What a transition does outside the machine.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Effect {
    /// Ask the domain for a transaction.
    Request(Request),
    /// Release a resource (always listed before any acquire).
    Release(Resource),
    /// Acquire a resource.
    Acquire(Resource),
    /// Ask whether to discard the draft; answer with
    /// [`FrontEnd::confirm_discard`] or [`FrontEnd::keep_editing`].
    AskDiscard,
    /// Leave the application.
    ExitApplication,
}

/// The result of an accepted action.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Outcome {
    /// The screen before.
    pub from: Screen,
    /// The screen after (the same when only a prompt or an exit happened).
    pub to: Screen,
    /// What to do outside the machine, in order.
    pub effects: Vec<Effect>,
}

/// What an action would do, decided **before** anything happens.
///
/// [`FrontEnd::plan`] runs the transition on a throwaway copy of the machine
/// and reports the screens it would move between and the domain transaction it
/// would ask for. That is what lets the F45-C wiring run the transaction
/// first: a domain refusal is returned while the real machine is still exactly
/// where it was, instead of after a screen that already advanced over a
/// transaction that never happened.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Plan {
    /// The screen the action was offered on.
    pub from: Screen,
    /// The screen it leads to.
    pub to: Screen,
    /// The domain transaction it asks for, if any.
    pub request: Option<Request>,
}

/// Why an action was refused. A refusal changes nothing.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The screen has no such action.
    NoTransition {
        /// The screen.
        screen: Screen,
        /// The action.
        action: Action,
    },
    /// The loadout cannot launch.
    InvalidLoadout(Vec<LoadoutProblem>),
    /// Retry needs a failed mission.
    NotAFailure,
    /// A discard prompt is open; answer it first.
    ConfirmationPending,
    /// No discard prompt is open.
    NoPendingDiscard,
    /// The call belongs to another screen.
    WrongScreen {
        /// The screen it needs.
        expected: Screen,
        /// The current screen.
        actual: Screen,
    },
    /// The profile screen was entered without a choice of new or existing.
    NoProfileIntent,
    /// Committing needs a construction draft and none is open.
    NoDraft,
    /// Nothing is focused.
    NothingFocused,
}

impl fmt::Display for Refusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NoTransition { screen, action } => {
                write!(f, "{screen:?} has no action {}", action.key())
            }
            Self::InvalidLoadout(problems) => write!(f, "the loadout cannot launch: {problems:?}"),
            Self::NotAFailure => write!(f, "retry needs a failed mission"),
            Self::ConfirmationPending => write!(f, "a discard prompt is open"),
            Self::NoPendingDiscard => write!(f, "no discard prompt is open"),
            Self::WrongScreen { expected, actual } => {
                write!(f, "this belongs to {expected:?}, not {actual:?}")
            }
            Self::NoProfileIntent => write!(f, "the profile screen needs a new/existing choice"),
            Self::NoDraft => write!(f, "there is no construction draft to commit"),
            Self::NothingFocused => write!(f, "no button is focused"),
        }
    }
}

impl std::error::Error for Refusal {}

/// The front-end state machine.
#[derive(Clone, Debug)]
pub struct FrontEnd {
    screen: Screen,
    focus: Option<Action>,
    held: BTreeSet<Resource>,
    intent: Option<ProfileIntent>,
    construction: Option<ConstructionDraft>,
    loadout: Loadout,
    wingmate_required: bool,
    outcome: Option<MissionOutcome>,
    pending_discard: Option<Action>,
    last_failure: Option<LoadFailure>,
}

impl Default for FrontEnd {
    fn default() -> Self {
        Self::new()
    }
}

impl FrontEnd {
    /// A machine on [`Screen::START`], holding that screen's resources.
    #[must_use]
    pub fn new() -> Self {
        let mut machine = Self {
            screen: Screen::START,
            focus: None,
            held: resources(Screen::START),
            intent: None,
            construction: None,
            loadout: Loadout::default(),
            wingmate_required: false,
            outcome: None,
            pending_discard: None,
            last_failure: None,
        };
        machine.reset_focus();
        machine
    }

    /// The current screen.
    #[must_use]
    pub fn screen(&self) -> Screen {
        self.screen
    }

    /// The resources currently held.
    #[must_use]
    pub fn held(&self) -> &BTreeSet<Resource> {
        &self.held
    }

    /// The focused button.
    #[must_use]
    pub fn focus(&self) -> Option<Action> {
        self.focus
    }

    /// The buttons of the current screen in focus order.
    #[must_use]
    pub fn visible_actions(&self) -> Vec<Action> {
        rows_on(self.screen)
            .map(|row| row.action)
            .filter(|action| action.source() == ActionSource::User)
            .collect()
    }

    /// The action waiting on a discard answer.
    #[must_use]
    pub fn pending_discard(&self) -> Option<Action> {
        self.pending_discard
    }

    /// The loadout draft.
    #[must_use]
    pub fn loadout(&self) -> &Loadout {
        &self.loadout
    }

    /// The construction draft.
    #[must_use]
    pub fn construction(&self) -> Option<&ConstructionDraft> {
        self.construction.as_ref()
    }

    /// The last mission outcome, until the cabin or menu is reached.
    #[must_use]
    pub fn outcome(&self) -> Option<MissionOutcome> {
        self.outcome
    }

    /// The last load failure, until the next load starts.
    #[must_use]
    pub fn last_failure(&self) -> Option<&LoadFailure> {
        self.last_failure.as_ref()
    }

    /// Declares whether the mission needs a wingmate (the mission data's say,
    /// never assumed here).
    pub fn set_wingmate_required(&mut self, required: bool) {
        self.wingmate_required = required;
    }

    /// Opens the construction draft on the construction screen.
    ///
    /// # Errors
    ///
    /// [`Refusal::WrongScreen`] off the construction screen,
    /// [`Refusal::ConfirmationPending`] while a prompt is open.
    pub fn open_construction(&mut self, draft: ConstructionDraft) -> Result<(), Refusal> {
        self.expect(Screen::Construction)?;
        self.construction = Some(draft);
        Ok(())
    }

    /// Replaces the edited components.
    ///
    /// # Errors
    ///
    /// As [`Self::open_construction`], and [`Refusal::NoDraft`] with no draft.
    pub fn edit_construction(&mut self, components: Vec<ContentId>) -> Result<(), Refusal> {
        self.expect(Screen::Construction)?;
        let draft = self.construction.as_mut().ok_or(Refusal::NoDraft)?;
        draft.components = components;
        Ok(())
    }

    /// Replaces the flight-check selection.
    ///
    /// # Errors
    ///
    /// [`Refusal::WrongScreen`] off the flight check,
    /// [`Refusal::ConfirmationPending`] while a prompt is open.
    pub fn select_loadout(&mut self, loadout: Loadout) -> Result<(), Refusal> {
        self.expect(Screen::FlightCheck)?;
        self.loadout = loadout;
        Ok(())
    }

    fn expect(&self, screen: Screen) -> Result<(), Refusal> {
        if self.pending_discard.is_some() {
            return Err(Refusal::ConfirmationPending);
        }
        if self.screen == screen {
            Ok(())
        } else {
            Err(Refusal::WrongScreen {
                expected: screen,
                actual: self.screen,
            })
        }
    }

    fn is_draft_dirty(&self) -> bool {
        match self.screen {
            Screen::Construction => self
                .construction
                .as_ref()
                .is_some_and(ConstructionDraft::is_dirty),
            Screen::FlightCheck => !self.loadout.is_empty(),
            _ => false,
        }
    }

    /// Applies an action.
    ///
    /// # Errors
    ///
    /// A [`Refusal`]; the machine is unchanged.
    pub fn apply(&mut self, action: Action) -> Result<Outcome, Refusal> {
        if self.pending_discard.is_some() {
            return Err(Refusal::ConfirmationPending);
        }
        let row = find(self.screen, action).ok_or(Refusal::NoTransition {
            screen: self.screen,
            action,
        })?;
        match row.guard {
            Guard::None => {}
            Guard::ValidLoadout => {
                let problems = self.loadout.validate(self.wingmate_required);
                if !problems.is_empty() {
                    return Err(Refusal::InvalidLoadout(problems));
                }
            }
            Guard::FailedOutcome => {
                if self.outcome != Some(MissionOutcome::Failure) {
                    return Err(Refusal::NotAFailure);
                }
            }
            Guard::DiscardsDraft => {
                if self.is_draft_dirty() {
                    self.pending_discard = Some(action);
                    return Ok(Outcome {
                        from: self.screen,
                        to: self.screen,
                        effects: vec![Effect::AskDiscard],
                    });
                }
            }
        }
        self.perform(action, row.to, row.request)
    }

    /// Answers the discard prompt: drop the draft and carry on.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoPendingDiscard`] with no prompt open.
    pub fn confirm_discard(&mut self) -> Result<Outcome, Refusal> {
        let action = self
            .pending_discard
            .take()
            .ok_or(Refusal::NoPendingDiscard)?;
        let row = find(self.screen, action).ok_or(Refusal::NoTransition {
            screen: self.screen,
            action,
        })?;
        self.perform(action, row.to, row.request)
    }

    /// Answers the discard prompt: keep editing.
    ///
    /// # Errors
    ///
    /// [`Refusal::NoPendingDiscard`] with no prompt open.
    pub fn keep_editing(&mut self) -> Result<(), Refusal> {
        self.pending_discard
            .take()
            .map(|_| ())
            .ok_or(Refusal::NoPendingDiscard)
    }

    /// Reports a failed load: back to the flight check with the draft intact.
    ///
    /// # Errors
    ///
    /// As [`Self::apply`] for [`Action::LoadFailed`].
    pub fn report_load_failure(&mut self, reason: &str) -> Result<Outcome, Refusal> {
        let outcome = self.apply(Action::LoadFailed)?;
        self.last_failure = Some(LoadFailure {
            reason: reason.to_owned(),
        });
        Ok(outcome)
    }

    /// Focuses one specific button of the current screen. The authored
    /// hotspots of a screen declare the order focus visits them (F45-B), so
    /// the presentation layer sets that order here rather than the table's
    /// own row order; focus still resets on entry.
    ///
    /// # Errors
    ///
    /// [`Refusal::ConfirmationPending`] while a discard prompt is open, or
    /// [`Refusal::NoTransition`] when the screen offers no such button for a
    /// person to press (including an application-side result).
    pub fn set_focus(&mut self, action: Action) -> Result<(), Refusal> {
        if self.pending_discard.is_some() {
            return Err(Refusal::ConfirmationPending);
        }
        let row = find(self.screen, action).ok_or(Refusal::NoTransition {
            screen: self.screen,
            action,
        })?;
        if row.action.source() != ActionSource::User {
            return Err(Refusal::NoTransition {
                screen: self.screen,
                action,
            });
        }
        self.focus = Some(action);
        Ok(())
    }

    /// Moves focus to the next (or previous) button, wrapping.
    pub fn move_focus(&mut self, forward: bool) {
        let visible = self.visible_actions();
        let len = visible.len();
        if len == 0 {
            self.focus = None;
            return;
        }
        let at = self
            .focus
            .and_then(|focus| visible.iter().position(|action| *action == focus));
        let next = match (at, forward) {
            (None, true) => 0,
            (None, false) => len - 1,
            (Some(at), true) => (at + 1) % len,
            (Some(at), false) => (at + len - 1) % len,
        };
        self.focus = visible.get(next).copied();
    }

    /// Activates the focused button.
    ///
    /// # Errors
    ///
    /// [`Refusal::NothingFocused`] with no focus, otherwise as [`Self::apply`].
    pub fn activate_focus(&mut self) -> Result<Outcome, Refusal> {
        let action = self.focus.ok_or(Refusal::NothingFocused)?;
        self.apply(action)
    }

    /// What applying `action` would do, with the machine untouched: the
    /// screens it would move between and the request it would ask for.
    ///
    /// The plan is the transition itself, run on a copy, so it cannot drift
    /// from what [`Self::apply`] would really do — including every refusal and
    /// every guard.
    ///
    /// # Errors
    ///
    /// Exactly the [`Refusal`] [`Self::apply`] would return.
    pub fn plan(&self, action: Action) -> Result<Plan, Refusal> {
        let mut probe = self.clone();
        Ok(Self::plan_of(&probe.apply(action)?))
    }

    /// What activating the focused button would do; as [`Self::plan`].
    ///
    /// # Errors
    ///
    /// [`Refusal::NothingFocused`] with no focus, otherwise as [`Self::plan`].
    pub fn plan_focus(&self) -> Result<Plan, Refusal> {
        let mut probe = self.clone();
        Ok(Self::plan_of(&probe.activate_focus()?))
    }

    /// What answering the open discard prompt would do; as [`Self::plan`].
    ///
    /// # Errors
    ///
    /// [`Refusal::NoPendingDiscard`] with no prompt open, otherwise as
    /// [`Self::plan`].
    pub fn plan_pending(&self) -> Result<Plan, Refusal> {
        let mut probe = self.clone();
        Ok(Self::plan_of(&probe.confirm_discard()?))
    }

    fn plan_of(outcome: &Outcome) -> Plan {
        let request = outcome.effects.iter().find_map(|effect| match effect {
            Effect::Request(request) => Some(request.clone()),
            _ => None,
        });
        Plan {
            from: outcome.from,
            to: outcome.to,
            request,
        }
    }

    fn reset_focus(&mut self) {
        self.focus = self.visible_actions().first().copied();
    }

    fn perform(
        &mut self,
        action: Action,
        to: Screen,
        kind: RequestKind,
    ) -> Result<Outcome, Refusal> {
        let from = self.screen;
        if action == Action::Quit {
            return Ok(Outcome {
                from,
                to: from,
                effects: vec![Effect::ExitApplication],
            });
        }
        let mut effects = Vec::new();
        let mut ended = None;
        match kind {
            RequestKind::None => {}
            RequestKind::OpenProfile => {
                let intent = self.intent.ok_or(Refusal::NoProfileIntent)?;
                effects.push(Effect::Request(Request::OpenProfile(intent)));
            }
            RequestKind::CloseProfile => effects.push(Effect::Request(Request::CloseProfile)),
            RequestKind::CommitBlueprint => {
                let draft = self.construction.clone().ok_or(Refusal::NoDraft)?;
                effects.push(Effect::Request(Request::CommitBlueprint(draft)));
            }
            RequestKind::CommitLoadout => {
                effects.push(Effect::Request(Request::CommitLoadout(
                    self.loadout.clone(),
                )));
            }
            RequestKind::ApplyOutcome => {
                let result = if action == Action::MissionSucceeded {
                    MissionOutcome::Success
                } else {
                    MissionOutcome::Failure
                };
                ended = Some(result);
                effects.push(Effect::Request(Request::ApplyOutcome(result)));
            }
            RequestKind::AbandonMission => effects.push(Effect::Request(Request::AbandonMission)),
        }
        // Every fallible step is behind us: from here the machine changes.
        if ended.is_some() {
            self.outcome = ended;
        }
        match action {
            Action::NewProfile => self.intent = Some(ProfileIntent::New),
            Action::ContinueProfile => self.intent = Some(ProfileIntent::Existing),
            _ => {}
        }
        if to == Screen::Loading {
            self.last_failure = None;
        }
        // Reaching the hub drops every working copy; backing out of the flight
        // check drops the selection. A load failure keeps both.
        if matches!(to, Screen::Cabin | Screen::MainMenu) {
            self.construction = None;
            self.loadout = Loadout::default();
            self.outcome = None;
        }
        if from == Screen::FlightCheck && to == Screen::Briefing {
            self.loadout = Loadout::default();
        }
        let wanted = resources(to);
        effects.extend(self.held.difference(&wanted).map(|r| Effect::Release(*r)));
        effects.extend(wanted.difference(&self.held).map(|r| Effect::Acquire(*r)));
        self.held = wanted;
        self.screen = to;
        self.reset_focus();
        Ok(Outcome { from, to, effects })
    }
}

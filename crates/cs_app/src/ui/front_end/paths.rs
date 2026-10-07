//! The navigation review: every path a player can take through the front end,
//! executed on the real machine
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, stage
//! `### F45-D`). Shared contract: `docs/contracts/UI-NETWORK.md` ("Use an
//! explicit state table for every screen and Back/Cancel path").
//!
//! # What this is for
//!
//! F45-A's [`super::validate_table`] proves the table is *structurally*
//! sound — no duplicate row, an escape on every screen, every screen reachable
//! and able to reach the menu — by reading the table. This module **runs** it:
//! it walks the machine from [`Screen::START`], applies every row of every
//! screen it reaches, follows each result (including the discard prompt's own
//! answer), and reports what actually happened. F45-D's minimum scenario is
//! *"capture and review all original front-end screens and navigation paths"*,
//! and this is the navigation half: the review is what tells the capture which
//! screens a player reaches, and which screens this walk could **not** reach
//! and why.
//!
//! # What is **not** claimed
//!
//! * **Not every domain path.** The walk carries no campaign, no profile and no
//!   loader, so a row guarded on one (a valid loadout, a failed mission) comes
//!   back refused. That refusal is *recorded with its code*, never hidden: the
//!   review's completeness rule is that every row of the table is classified
//!   as moved, prompted, exited or refused-by-code, and every screen is either
//!   reached or listed as not reached.
//! * **Not a keyboard or controller.** Nothing here reads a device; F45-D
//!   reviews the *paths*, and device input mapping is F22's boundary. A step
//!   recorded here is the same `Action` a key press or a click would apply
//!   (F45-A/F45-B already pin that equivalence).
//! * **Not the original's screen list.** The screens are F45-A's designed
//!   table; which original artwork belongs to which screen is unread (#742).

use std::collections::{BTreeSet, VecDeque};
use std::fmt;

use super::{Action, Effect, FrontEnd, Refusal, RequestKind, Screen, rows_on};

/// How one applied row ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StepOutcome {
    /// The machine moved to another screen.
    Moved,
    /// The machine stayed on the same screen (a prompt opened or an exit was
    /// requested).
    Stayed,
    /// Leaving asked whether to discard a dirty draft, and the walk answered
    /// it: the row completed after the prompt.
    Prompted,
    /// The row asked the application to leave.
    Exited,
    /// The row was refused; the machine did not move.
    Refused,
}

/// One row of the table, applied once by the walk.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PathStep {
    /// The screen the row was offered on.
    pub from: Screen,
    /// The action applied.
    pub action: Action,
    /// The screen the machine was on afterwards.
    pub to: Screen,
    /// How it ended.
    pub outcome: StepOutcome,
    /// Whether the row asked the domain for a transaction.
    pub request: RequestKind,
    /// The refusal code when [`Self::outcome`] is [`StepOutcome::Refused`],
    /// otherwise empty.
    pub refusal: &'static str,
}

/// Why an applied row was refused, as a stable code.
fn refusal_code(refusal: &Refusal) -> &'static str {
    match refusal {
        Refusal::NoTransition { .. } => "no_transition",
        Refusal::InvalidLoadout(_) => "invalid_loadout",
        Refusal::NotAFailure => "not_a_failure",
        Refusal::ConfirmationPending => "confirmation_pending",
        Refusal::NoPendingDiscard => "no_pending_discard",
        Refusal::WrongScreen { .. } => "wrong_screen",
        Refusal::NoProfileIntent => "no_profile_intent",
        Refusal::NoDraft => "no_draft",
        Refusal::NothingFocused => "nothing_focused",
    }
}

/// The domain inputs a player supplies on screens whose rows are guarded on
/// them. None of these is a table row: they are what the *presentation* hands
/// the machine (F45-B's `ScreenSession::select_loadout` /
/// `open_construction`), and a walk without them stops at the first guarded
/// screen.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct NavigationInputs {
    /// The flight check's loadout: without one, `Launch` is refused
    /// (`invalid_loadout`) and the whole loading/flight/results half of the
    /// table stays behind that refusal.
    pub loadout: Option<super::Loadout>,
    /// The construction screen's open draft: without one, committing is
    /// refused (`no_draft`) and Back/Cancel never asks to discard.
    pub construction: Option<super::ConstructionDraft>,
}

/// One declared input the walk supplied before applying a screen's rows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SuppliedInput {
    /// The screen it was supplied on.
    pub screen: Screen,
    /// Which input, in words (`loadout`, `construction_draft`).
    pub input: &'static str,
}

/// The complete navigation review of the front-end table.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathReview {
    /// Where the walk started.
    pub start: Screen,
    /// Every row the walk applied, in visit order.
    pub steps: Vec<PathStep>,
    /// The screens the walk reached, in discovery order.
    pub reached: Vec<Screen>,
    /// The screens the walk never reached, in [`Screen::ALL`] order.
    pub not_reached: Vec<Screen>,
    /// Rows of the table whose `from` screen the walk reached.
    pub rows_on_reached_screens: usize,
    /// Rows of the table on a screen the walk never reached.
    pub rows_unreached: usize,
    /// The declared domain inputs the walk supplied, in the order it supplied
    /// them.
    pub inputs: Vec<SuppliedInput>,
}

impl PathReview {
    /// Every screen, in discovery order, with the steps that reached it.
    #[must_use]
    pub fn screens(&self) -> &[Screen] {
        &self.reached
    }

    /// The steps that ended refused, by refusal code.
    #[must_use]
    pub fn refusals(&self) -> Vec<(Screen, Action, &'static str)> {
        self.steps
            .iter()
            .filter(|step| step.outcome == StepOutcome::Refused)
            .map(|step| (step.from, step.action, step.refusal))
            .collect()
    }

    /// The steps that asked the domain for a transaction **and were accepted**:
    /// a refused step asked for nothing, however the table declares its row.
    #[must_use]
    pub fn requests(&self) -> Vec<(Screen, Action, RequestKind)> {
        self.steps
            .iter()
            .filter(|step| {
                step.outcome != StepOutcome::Refused && step.request != RequestKind::None
            })
            .map(|step| (step.from, step.action, step.request))
            .collect()
    }

    /// The declared domain inputs the walk supplied before applying a screen's
    /// rows, so a reader can see which paths exist only because a player
    /// selected something first.
    #[must_use]
    pub fn supplied_inputs(&self) -> &[SuppliedInput] {
        &self.inputs
    }

    /// Whether the review is complete: every screen is either reached or named
    /// as not reached, and every row of the table is either applied or on a
    /// screen the review says it never reached.
    #[must_use]
    pub fn is_complete(&self) -> bool {
        let table_rows = super::TABLE.len();
        let classified = self.steps.len() + self.rows_unreached;
        let screens = self.reached.len() + self.not_reached.len();
        classified == table_rows
            && screens == Screen::ALL.len()
            && self.rows_on_reached_screens + self.rows_unreached == table_rows
    }

    /// The review as the artifact this task writes: measured counts plus the
    /// complete row-by-row and screen-by-screen account.
    ///
    /// The result is **parseable JSON**: every screen, action, outcome and
    /// refusal is a JSON string literal, never a bare word, so a reader can
    /// load this with any JSON parser instead of guessing at `START`,
    /// `MainMenu` and friends.
    #[must_use]
    pub fn json(&self) -> String {
        let steps: Vec<String> = self
            .steps
            .iter()
            .map(|step| {
                format!(
                    "{{\"from\":{},\"action\":{},\"to\":{},\"outcome\":{},\"request\":\
                     {},\"refusal\":{}}}",
                    json_string(&format!("{:?}", step.from)),
                    json_string(step.action.key()),
                    json_string(&format!("{:?}", step.to)),
                    json_string(outcome_name(step.outcome)),
                    json_string(request_name(step.request)),
                    json_string(step.refusal)
                )
            })
            .collect();
        let reached: Vec<String> = self
            .reached
            .iter()
            .map(|screen| json_string(&format!("{screen:?}")))
            .collect();
        let missing: Vec<String> = self
            .not_reached
            .iter()
            .map(|screen| json_string(&format!("{screen:?}")))
            .collect();
        let inputs: Vec<String> = self
            .inputs
            .iter()
            .map(|input| {
                format!(
                    "{{\"screen\":{},\"input\":{}}}",
                    json_string(&format!("{:?}", input.screen)),
                    json_string(input.input)
                )
            })
            .collect();
        format!(
            "{{\"start\":{},\"table_rows\":{},\"steps\":{},\"rows_on_reached_screens\":{},\
             \"rows_unreached\":{},\"screens_reached\":[{}],\"screens_not_reached\":[{}],\
             \"supplied_inputs\":[{}],\"complete\":{},\"steps_detail\":[{}]}}",
            json_string(&format!("{:?}", self.start)),
            super::TABLE.len(),
            self.steps.len(),
            self.rows_on_reached_screens,
            self.rows_unreached,
            reached.join(","),
            missing.join(","),
            inputs.join(","),
            self.is_complete(),
            steps.join(",")
        )
    }
}

impl fmt::Display for PathReview {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} rows applied over {} screens, {} screens not reached, {} rows behind them; \
             complete: {}",
            self.steps.len(),
            self.reached.len(),
            self.not_reached.len(),
            self.rows_unreached,
            self.is_complete()
        )
    }
}

fn outcome_name(outcome: StepOutcome) -> &'static str {
    match outcome {
        StepOutcome::Moved => "moved",
        StepOutcome::Stayed => "stayed",
        StepOutcome::Prompted => "prompted",
        StepOutcome::Exited => "exited",
        StepOutcome::Refused => "refused",
    }
}

fn request_name(request: RequestKind) -> &'static str {
    match request {
        RequestKind::None => "none",
        RequestKind::OpenProfile => "open_profile",
        RequestKind::CloseProfile => "close_profile",
        RequestKind::CommitBlueprint => "commit_blueprint",
        RequestKind::CommitLoadout => "commit_loadout",
        RequestKind::ApplyOutcome => "apply_outcome",
        RequestKind::AbandonMission => "abandon_mission",
    }
}

/// A JSON string literal for a fixed vocabulary value.
fn json_string(value: &str) -> String {
    format!("\"{}\"", value.replace('\\', "\\\\").replace('"', "\\\""))
}

/// Walks every navigation path from `start`, applying every row of every
/// screen the walk reaches, **without** supplying any domain input: a player
/// who has selected nothing. [`Screen::FlightCheck`]'s `Launch` is refused
/// (`invalid_loadout`) here, and the half of the table behind it stays
/// unreached and is reported as such.
#[must_use]
pub fn review_navigation(start: Screen) -> PathReview {
    review_navigation_with(start, &NavigationInputs::default())
}

/// As [`review_navigation`], with the declared domain inputs
/// [`NavigationInputs`] a player would have supplied first.
///
/// The walk explores **every distinct machine state** it can reach rather than
/// only the first one per screen: a row refused on the state that first
/// reached a screen is retried when a richer state arrives later, and the step
/// kept for a row is the best outcome any explored state produced. Two states
/// are the same when everything the table's guards read is the same — screen,
/// last outcome, open loadout, open draft, open prompt, last failure — so the
/// walk terminates, never invents state, and never applies a row twice with
/// two different results.
///
/// A row that opens the discard prompt is followed by the prompt's own answer
/// in the same step ([`StepOutcome::Prompted`]), because the path a player
/// takes from a dirty draft is prompt → confirm, not prompt → dead end.
#[must_use]
pub fn review_navigation_with(start: Screen, inputs: &NavigationInputs) -> PathReview {
    let mut machine = FrontEnd::new();
    if start != machine.screen() {
        // Reach `start` by applying the table until the walk's own start screen
        // appears; the caller names the screen to review from, and the table
        // guarantees it is reachable from [`Screen::START`].
        machine = reach(&machine, start).unwrap_or_else(|| {
            panic!(
                "the table cannot reach {start:?} from {:?}, although validate_table requires it",
                Screen::START
            )
        });
    }

    let mut reached: Vec<Screen> = Vec::new();
    let mut supplied: Vec<SuppliedInput> = Vec::new();
    let mut steps: Vec<Option<PathStep>> = vec![None; super::TABLE.len()];
    let mut seen_screens: BTreeSet<Screen> = BTreeSet::new();
    let mut seen_states: BTreeSet<String> = BTreeSet::new();
    let mut queue: VecDeque<FrontEnd> = VecDeque::from(vec![machine]);

    let index_of = |from: Screen, action: Action| -> usize {
        super::TABLE
            .iter()
            .position(|row| row.from == from && row.action == action)
            .expect("`rows_on` only returns rows of this table")
    };

    while let Some(mut state) = queue.pop_front() {
        let screen = state.screen();

        // The declared inputs a player supplies on this screen, before any row
        // of it is applied: the flight check's loadout and the construction
        // screen's open draft are presentation-level inputs (F45-B), never
        // table rows, and the review records every one it used.
        if !seen_screens.contains(&screen) {
            seen_screens.insert(screen);
            reached.push(screen);
            match screen {
                Screen::FlightCheck if state.loadout().is_empty() => {
                    if let Some(loadout) = &inputs.loadout
                        && state.select_loadout(loadout.clone()).is_ok()
                    {
                        supplied.push(SuppliedInput {
                            screen,
                            input: "loadout",
                        });
                    }
                }
                Screen::Construction if state.construction().is_none() => {
                    if let Some(draft) = &inputs.construction
                        && state.open_construction(draft.clone()).is_ok()
                    {
                        supplied.push(SuppliedInput {
                            screen,
                            input: "construction_draft",
                        });
                    }
                }
                _ => {}
            }
        }

        if !seen_states.insert(signature(&state)) {
            // This exact state has already been explored.
            continue;
        }

        let rows: Vec<_> = rows_on(screen).collect();
        for row in rows {
            let action = row.action;
            let slot = &mut steps[index_of(screen, action)];
            if slot.is_some_and(|step| step.outcome != StepOutcome::Refused) {
                // Some state already completed this row.
                continue;
            }
            let mut probe = state.clone();
            match probe.apply(action) {
                Ok(outcome) => {
                    let effects = outcome.effects;
                    let asked = effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::AskDiscard));
                    let exit = effects
                        .iter()
                        .any(|effect| matches!(effect, Effect::ExitApplication));
                    let mut to = outcome.to;
                    let mut final_outcome = if outcome.to == screen {
                        StepOutcome::Stayed
                    } else {
                        StepOutcome::Moved
                    };
                    if asked {
                        // Answer the prompt: the path continues with the draft
                        // dropped, exactly as a player confirming the discard goes.
                        if let Ok(answered) = probe.confirm_discard() {
                            to = answered.to;
                            final_outcome = StepOutcome::Prompted;
                        }
                    }
                    if exit {
                        final_outcome = StepOutcome::Exited;
                    }
                    *slot = Some(PathStep {
                        from: screen,
                        action,
                        to,
                        outcome: final_outcome,
                        request: row.request,
                        refusal: "",
                    });
                    queue.push_back(probe);
                }
                Err(refusal) => {
                    // A refusal only fills an empty slot: a richer state may
                    // still complete the row later, and the refused step stays
                    // until it does.
                    if slot.is_none() {
                        *slot = Some(PathStep {
                            from: screen,
                            action,
                            to: screen,
                            outcome: StepOutcome::Refused,
                            request: row.request,
                            refusal: refusal_code(&refusal),
                        });
                    }
                }
            }
        }
    }

    let steps: Vec<PathStep> = steps.into_iter().flatten().collect();
    let not_reached: Vec<Screen> = Screen::ALL
        .iter()
        .copied()
        .filter(|screen| !seen_screens.contains(screen))
        .collect();
    let rows_on_reached = Screen::ALL
        .iter()
        .filter(|screen| seen_screens.contains(screen))
        .map(|screen| rows_on(*screen).count())
        .sum();
    let rows_unreached = super::TABLE.len() - rows_on_reached;

    PathReview {
        start,
        steps,
        reached,
        not_reached,
        rows_on_reached_screens: rows_on_reached,
        rows_unreached,
        inputs: supplied,
    }
}

/// Everything the table's guards read, as the identity of one explored state.
fn signature(machine: &FrontEnd) -> String {
    format!(
        "{:?}|{:?}|loadout_empty={}|draft={}|prompt={}|failure={}",
        machine.screen(),
        machine.outcome(),
        machine.loadout().is_empty(),
        machine.construction().is_some(),
        machine.pending_discard().is_some(),
        machine.last_failure().is_some(),
    )
}

/// Applies table rows until the machine sits on `target`, or gives up after
/// one pass over every screen. Used only to move the walk's start.
fn reach(from: &FrontEnd, target: Screen) -> Option<FrontEnd> {
    let mut current = from.clone();
    for _ in 0..Screen::ALL.len() {
        if current.screen() == target {
            return Some(current);
        }
        let mut advanced = false;
        for row in rows_on(current.screen()) {
            let mut probe = current.clone();
            if probe.apply(row.action).is_ok() && probe.screen() != current.screen() {
                current = probe;
                advanced = true;
                break;
            }
        }
        if !advanced {
            return None;
        }
    }
    (current.screen() == target).then_some(current)
}

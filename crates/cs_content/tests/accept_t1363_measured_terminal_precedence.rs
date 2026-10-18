//! Task #1363 acceptance: `cs_sim`'s measured terminal precedence agrees with
//! the engine's lost-before-won rule as `cs_content` reads it.
//!
//! The reference is [`MissionEndFlags::end`]; the rule under test is
//! [`TerminalPrecedence::MeasuredLostBeforeWon`]. Both are `static` readings of
//! the engine image's objective pass (finding
//! `docs/findings/2026-10-12-m01-b-obj-3-stage-conditions-and-outcomes.md`);
//! nothing here is `verified_original`.

use std::collections::BTreeSet;

use cs_content::objectives_conditions::{MissionEnd, MissionEndFlags, ObjectiveOutcome};
use cs_sim::objectives::terminal::{TerminalLatch, TerminalOutcome, TerminalPrecedence};
use cs_types::Tick;

fn reference(win: bool, loss: bool) -> Option<MissionEnd> {
    let mut flags = MissionEndFlags::default();
    flags.begin_pass();
    if win {
        flags.completed(ObjectiveOutcome::InstantWin);
    }
    if loss {
        flags.completed(ObjectiveOutcome::InstantLoss);
    }
    flags.end().map(|(end, _)| end)
}

fn measured(win: bool, loss: bool) -> Option<MissionEnd> {
    let mut requested = BTreeSet::new();
    if win {
        requested.insert(TerminalOutcome::Success);
    }
    if loss {
        requested.insert(TerminalOutcome::Failure);
    }
    TerminalPrecedence::MeasuredLostBeforeWon
        .resolve(&requested)
        .map(|outcome| match outcome {
            TerminalOutcome::Failure => MissionEnd::Loss,
            TerminalOutcome::Success => MissionEnd::Win,
            TerminalOutcome::Extraction => unreachable!("not requested"),
        })
}

#[test]
fn accept_t1363_measured_precedence_agrees_with_the_engine_flag_order_for_every_flag_pair() {
    for win in [false, true] {
        for loss in [false, true] {
            assert_eq!(
                measured(win, loss),
                reference(win, loss),
                "win={win} loss={loss}"
            );
        }
    }
    assert_eq!(measured(true, true), Some(MissionEnd::Loss));
}

#[test]
fn accept_t1363_a_same_tick_win_and_loss_latches_failure_and_names_the_superseded_success() {
    let requested = BTreeSet::from([TerminalOutcome::Success, TerminalOutcome::Failure]);
    let mut latch = TerminalLatch::new();
    let resolution = latch.resolve(
        Tick(5),
        &requested,
        TerminalPrecedence::MeasuredLostBeforeWon,
    );
    assert_eq!(latch.outcome(), Some(TerminalOutcome::Failure));
    assert_eq!(
        resolution,
        cs_sim::objectives::terminal::Resolution::Settled {
            outcome: TerminalOutcome::Failure,
            superseded: BTreeSet::from([TerminalOutcome::Success]),
        }
    );
}

#[test]
fn accept_t1363_the_synthetic_policy_is_unchanged_and_the_default() {
    assert_eq!(
        TerminalPrecedence::default(),
        TerminalPrecedence::SyntheticConservative
    );
    assert_eq!(
        TerminalPrecedence::SyntheticConservative.label(),
        "synthetic_conservative"
    );
    assert_eq!(
        TerminalPrecedence::MeasuredLostBeforeWon.label(),
        "measured_lost_before_won"
    );
}

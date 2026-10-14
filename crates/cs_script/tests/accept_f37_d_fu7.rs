//! F37-D-FU7 acceptance (Rally #739): the mission IR can end a mission
//! *without* the instant-outcome marker, so the measured 3.0 s end delay is
//! reachable from a program.
//!
//! The rule (owner note on Rally #589, static analysis of the decrypted
//! executable, sha256 in [`cs_script::runtime::ORIGINAL_IMAGE_SHA256`]) is
//! 0.1 s when an `INSTANTWIN`/`INSTANTLOSS` fired that tick, 3.0 s otherwise.
//! `Action::Finish` keeps meaning the instant keys; `Action::FinishStandard`
//! is the designed-and-unmeasured spelling of "the flag is set, no instant
//! outcome fired" that outcome-class aggregation lowers to.

use std::time::Duration;

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

const SESSION: SessionGeneration = SessionGeneration(23);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn objective(id: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-fu7-obj-{id}")),
        condition: Condition::Const(true),
        actions,
        span: None,
    }
}

fn validated(objectives: Vec<Objective>) -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu7"),
        variables: vec![],
        objectives,
    }
    .validate()
    .unwrap()
}

/// Runs a one-objective program whose only action is `action`, to its end.
fn ending(action: Action) -> (TickResult, MissionEndPresentation) {
    let program = validated(vec![objective(1, vec![action])]);
    let mut state = MissionState::new(&program, SESSION, 1);
    let result = state
        .step(&program, &MissionFacts::default(), Tick(1))
        .unwrap();
    let presentation = state
        .terminal_presentation()
        .expect("the ending tick selects a presentation");
    (result, presentation)
}

#[test]
fn accept_f37_d_fu7_program_driven_standard_ending_takes_the_three_second_delay() {
    let (won, presentation) = ending(Action::FinishStandard(Outcome::Succeeded));
    assert_eq!(won.terminal, TerminalState::Succeeded);
    assert_eq!(presentation.branch, Some(TerminalBranch::Win));
    assert_eq!(presentation.mission_cue, MissionCue::MissionWonSound);
    assert_eq!(presentation.animation, EndAnimation::WinAnim);
    assert_eq!(presentation.end_delay, EndDelay::Standard);
    assert_eq!(presentation.end_delay.seconds(), STANDARD_END_DELAY);
    assert_eq!(STANDARD_END_DELAY, Duration::from_secs(3));

    let (lost, presentation) = ending(Action::FinishStandard(Outcome::Failed));
    assert_eq!(lost.terminal, TerminalState::Failed);
    assert_eq!(presentation.branch, Some(TerminalBranch::Loss));
    assert_eq!(presentation.animation, EndAnimation::LossAnim);
    assert_eq!(presentation.end_delay, EndDelay::Standard);
}

#[test]
fn accept_f37_d_fu7_instant_keys_keep_the_instant_delay() {
    for (outcome, branch) in [
        (Outcome::Succeeded, TerminalBranch::Win),
        (Outcome::Failed, TerminalBranch::Loss),
    ] {
        let (_, presentation) = ending(Action::Finish(outcome));
        assert_eq!(presentation.branch, Some(branch));
        assert_eq!(presentation.end_delay, EndDelay::Instant);
        assert_eq!(presentation.end_delay.seconds(), INSTANT_END_DELAY);
        assert_eq!(INSTANT_END_DELAY, Duration::from_millis(100));
    }
}

#[test]
fn accept_f37_d_fu7_an_instant_outcome_on_the_same_tick_wins_the_delay() {
    // Both requests on one tick: an instant outcome did fire, so 0.1 s, and
    // the branch/cues are selected from the flags as before.
    let program = validated(vec![objective(
        1,
        vec![
            Action::FinishStandard(Outcome::Succeeded),
            Action::Finish(Outcome::Failed),
        ],
    )]);
    let mut state = MissionState::new(&program, SESSION, 1);
    state
        .step(&program, &MissionFacts::default(), Tick(1))
        .unwrap();
    let presentation = state.terminal_presentation().unwrap();
    assert_eq!(presentation.end_delay, EndDelay::Instant);
    assert_eq!(presentation.branch, Some(TerminalBranch::Loss));
    assert_eq!(presentation.mission_cue, MissionCue::MissionWonSound);
}

#[test]
fn accept_f37_d_fu7_standard_ending_is_a_terminal_phase_action_and_round_trips() {
    assert_eq!(
        Action::FinishStandard(Outcome::Succeeded).phase(),
        Phase::Terminal
    );
    let program = validated(vec![objective(
        1,
        vec![Action::FinishStandard(Outcome::Succeeded)],
    )]);
    let mut state = MissionState::new(&program, SESSION, 1);
    state
        .step(&program, &MissionFacts::default(), Tick(1))
        .unwrap();
    let record = state.snapshot(&program);
    let restored = MissionState::restore(&program, record).unwrap();
    assert_eq!(
        restored.terminal_presentation().unwrap().end_delay,
        EndDelay::Standard
    );
}

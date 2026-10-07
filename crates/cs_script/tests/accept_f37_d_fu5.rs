//! F37-D-FU5 acceptance (Rally #731): the measured mission-terminal branch
//! order applied to mission-end presentation.
//!
//! The owner's static analysis of the supplied decrypted executable (owner
//! note on Rally #589, 2026-10-05; image sha256 recorded in
//! [`cs_script::runtime::ORIGINAL_IMAGE_SHA256`]) measured that inside
//! `CZMission::Update` the terminal check tests **LOST `0x46af7a` before WON
//! `0x46afad`** *only* to choose the presentation — the end delay, which
//! `OBJECTIVES_*_SOUND` plays, which `MISSION_*_SOUND` the end call `0x463c30`
//! picks and which animation `0x46ba10` shows — while the recorded result stays
//! **success iff the WON flag is set** (`0x4194e0`). The reads are
//! independent, so the loss branch can run while the result is the success.
//!
//! This is static code evidence, never an original run: the claim is
//! `inferred`, never `verified_original`, and no executable byte, disassembly
//! listing or decompiled code appears in this tree.
//!
//! What the production code is pinned on here:
//!
//! * `..._loss_branch_runs_while_the_recorded_result_is_success` — the branch
//!   runs loss-first while the recorded result is the success, and the mission
//!   sound and animation follow the WON flag rather than the branch.
//! * `..._cues_follow_the_won_flag_not_the_recorded_result` — under the
//!   designed policy, where the recorded result is the failure, the mission
//!   sound and the animation are still the win side, because they follow the
//!   flag and not the result.
//! * `..._win_and_loss_branches_select_their_own_cues` — the single-request
//!   cases: win branch + won cues + success, loss branch + lost cues + failure.
//! * `..._end_delay_follows_the_measured_instant_rule` — 0.1 s when an instant
//!   outcome fired on the terminal tick, 3.0 s when none did, with both halves
//!   reachable on the production path.
//! * `..._presentation_latches_and_survives_save_and_restore` — the terminal
//!   tick's selection is latched, travels in the save record, and a record
//!   whose terminal state and presentation disagree is refused.
//! * `..._terminal_branch_limitation_is_closed_with_the_finding` —
//!   `f37.d.limit.terminal_branch_delay_and_sound` is gone from the
//!   machine-readable limitations, the fact it gated carries none, and the
//!   findings entry that closes it records the addresses, the image and the
//!   closure.

use std::time::Duration;

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// Fixed session generation: the event keys are part of what the probes read.
const SESSION: SessionGeneration = SessionGeneration(17);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

/// A `Condition::Const(true)` objective: it completes on the tick the scan
/// reaches it, so the terminal request is what the probe reads.
fn objective(id: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-fu5-obj-{id}")),
        condition: Condition::Const(true),
        actions,
        span: None,
    }
}

fn validated(objectives: Vec<Objective>) -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu5"),
        variables: vec![],
        objectives,
    }
    .validate()
    .unwrap()
}

/// Advances `state` one tick on an empty fact map.
fn tick(state: &mut MissionState, program: &ValidatedProgram, at: u64) -> TickResult {
    state
        .step(program, &MissionFacts::default(), Tick(at))
        .unwrap()
}

/// **Branch before result, pinned on the production path.**
///
/// Both outcomes are requested on one tick. The measured policy records the
/// success (success iff WON, `0x4194e0`) while the terminal check's **loss
/// branch** is the branch that ran (LOST tested first, `0x46af7a` before
/// `0x46afad`): `OBJECTIVES_LOST_SOUND` is the slot that plays, the end delay
/// is the instant one, and the mission sound and animation are the *win* side
/// because they follow the WON flag — not the branch, and not the result.
#[test]
fn accept_f37_d_fu5_loss_branch_runs_while_the_recorded_result_is_success() {
    let program = validated(vec![
        objective(10, vec![Action::Finish(Outcome::Succeeded)]),
        objective(3, vec![Action::Finish(Outcome::Failed)]),
    ]);
    let mut state = MissionState::new(&program, SESSION);
    assert_eq!(
        state.terminal_presentation(),
        None,
        "a running mission has selected no end presentation yet"
    );

    let result = tick(&mut state, &program, 1);

    // The recorded result is the success: success iff WON.
    assert_eq!(result.terminal, TerminalState::Succeeded);
    let presentation = state
        .terminal_presentation()
        .expect("the terminal tick selects a mission-end presentation");
    assert_eq!(presentation.result, TerminalState::Succeeded);

    // ... and the branch that ran is the loss branch, tested first: this is
    // the half `f37.d.limit.terminal_branch_delay_and_sound` used to say did
    // not exist here.
    assert_eq!(
        presentation.branch,
        Some(TerminalBranch::Loss),
        "LOST is tested before WON, so the loss branch runs even when the \
         result is the success"
    );
    assert_eq!(
        presentation.objectives_cue(),
        Some(ObjectivesCue::ObjectivesLostSound),
        "the loss branch is what selects OBJECTIVES_LOST_SOUND (+0xc74)"
    );

    // The mission sound and the animation follow the WON flag, which is set.
    assert_eq!(presentation.mission_cue, MissionCue::MissionWonSound);
    assert_eq!(presentation.animation, EndAnimation::WinAnim);

    // The terminal request fired on this tick, so the instant delay applies.
    assert_eq!(presentation.end_delay, EndDelay::Instant);
    assert_eq!(presentation.end_delay.seconds(), Duration::from_millis(100));
}

/// **The cues follow the WON flag, not the recorded result.**
///
/// The designed policy is selected through the save record, so this tick
/// records the *failure* while both flags were requested. The mission sound
/// and the animation still answer the WON flag (`0x463c30`, `0x46ba10`): a
/// policy that changes the recorded result cannot repaint the end screen, and
/// the branch still runs loss-first.
#[test]
fn accept_f37_d_fu5_cues_follow_the_won_flag_not_the_recorded_result() {
    let program = validated(vec![
        objective(10, vec![Action::Finish(Outcome::Succeeded)]),
        objective(3, vec![Action::Finish(Outcome::Failed)]),
    ]);
    let mut record = MissionState::new(&program, SESSION).snapshot(&program);
    record.policy = PrecedencePolicy::SyntheticConservative;
    let mut designed = MissionState::restore(&program, record).unwrap();

    let result = tick(&mut designed, &program, 1);
    assert_eq!(
        result.terminal,
        TerminalState::Failed,
        "the designed policy still records the failure"
    );

    let presentation = designed.terminal_presentation().unwrap();
    assert_eq!(presentation.result, TerminalState::Failed);
    assert_eq!(
        presentation.branch,
        Some(TerminalBranch::Loss),
        "the branch reads the flags, not the policy"
    );
    assert_eq!(
        presentation.objectives_cue(),
        Some(ObjectivesCue::ObjectivesLostSound)
    );
    assert_eq!(
        presentation.mission_cue,
        MissionCue::MissionWonSound,
        "the WON flag is set, so the end call picks MISSION_WON_SOUND (+0xc78)"
    );
    assert_eq!(
        presentation.animation,
        EndAnimation::WinAnim,
        "the WON flag is set, so 0x46ba10 picks WIN_ANIM (+0x6e4)"
    );
    assert_eq!(presentation.end_delay, EndDelay::Instant);
}

/// **Both single-request cases select their own cues.**
///
/// Only the win branch and only the loss branch: the branch, the objectives
/// sound, the mission sound, the animation and the result must all agree
/// *within* one request — it is only when both flags are set that they
/// disagree, which the two tests above pin.
#[test]
fn accept_f37_d_fu5_win_and_loss_branches_select_their_own_cues() {
    let win = validated(vec![objective(1, vec![Action::Finish(Outcome::Succeeded)])]);
    let mut state = MissionState::new(&win, SESSION);
    let result = tick(&mut state, &win, 1);
    let presentation = state.terminal_presentation().unwrap();
    assert_eq!(result.terminal, TerminalState::Succeeded);
    assert_eq!(presentation.branch, Some(TerminalBranch::Win));
    assert_eq!(
        presentation.objectives_cue(),
        Some(ObjectivesCue::ObjectivesWonSound),
        "the win branch selects OBJECTIVES_WON_SOUND (+0xc70)"
    );
    assert_eq!(presentation.mission_cue, MissionCue::MissionWonSound);
    assert_eq!(presentation.animation, EndAnimation::WinAnim);
    assert_eq!(presentation.end_delay, EndDelay::Instant);
    assert_eq!(presentation.result, TerminalState::Succeeded);

    let loss = validated(vec![objective(1, vec![Action::Finish(Outcome::Failed)])]);
    let mut state = MissionState::new(&loss, SESSION);
    let result = tick(&mut state, &loss, 1);
    let presentation = state.terminal_presentation().unwrap();
    assert_eq!(result.terminal, TerminalState::Failed);
    assert_eq!(presentation.branch, Some(TerminalBranch::Loss));
    assert_eq!(
        presentation.objectives_cue(),
        Some(ObjectivesCue::ObjectivesLostSound)
    );
    assert_eq!(presentation.mission_cue, MissionCue::MissionLostSound);
    assert_eq!(presentation.animation, EndAnimation::LossAnim);
    assert_eq!(presentation.end_delay, EndDelay::Instant);
    assert_eq!(presentation.result, TerminalState::Failed);
}

/// **The measured delay pair, with both halves reachable.**
///
/// 0.1 s when an `INSTANTWIN`/`INSTANTLOSS` fired that tick — which is what a
/// program's terminal request is in this layer — and 3.0 s when none did. A
/// transition that sets neither flag (the original's countdown-expiry shape,
/// and here a host teardown or an abort request) plays no `OBJECTIVES_*_SOUND`
/// and takes the standard delay, with the mission sound and animation on the
/// loss side because WON is clear.
#[test]
fn accept_f37_d_fu5_end_delay_follows_the_measured_instant_rule() {
    assert_eq!(INSTANT_END_DELAY, Duration::from_millis(100));
    assert_eq!(STANDARD_END_DELAY, Duration::from_secs(3));
    assert_eq!(EndDelay::Instant.seconds(), Duration::from_millis(100));
    assert_eq!(EndDelay::Standard.seconds(), Duration::from_secs(3));

    // Instant: a win/loss terminal request fired on the terminal tick.
    let instant = validated(vec![objective(1, vec![Action::Finish(Outcome::Succeeded)])]);
    let mut state = MissionState::new(&instant, SESSION);
    tick(&mut state, &instant, 1);
    assert_eq!(
        state.terminal_presentation().unwrap().end_delay,
        EndDelay::Instant
    );

    // Standard, program side: an abort request is not an instant win/loss, so
    // no instant outcome fired — the measured "otherwise" half applies.
    let aborted = validated(vec![objective(1, vec![Action::Finish(Outcome::Aborted)])]);
    let mut state = MissionState::new(&aborted, SESSION);
    let result = tick(&mut state, &aborted, 1);
    assert_eq!(result.terminal, TerminalState::Aborted);
    let presentation = state.terminal_presentation().unwrap();
    assert_eq!(presentation.end_delay, EndDelay::Standard);
    assert_eq!(presentation.branch, None, "neither flag is set");
    assert_eq!(presentation.objectives_cue(), None);
    assert_eq!(presentation.mission_cue, MissionCue::MissionLostSound);
    assert_eq!(presentation.animation, EndAnimation::LossAnim);
    assert_eq!(presentation.result, TerminalState::Aborted);

    // Standard, host side: a teardown ends the mission with neither flag set.
    let teardown = validated(vec![objective(1, vec![Action::Finish(Outcome::Succeeded)])]);
    let mut state = MissionState::new(&teardown, SESSION);
    state.abort();
    let presentation = state.terminal_presentation().unwrap();
    assert_eq!(presentation.end_delay, EndDelay::Standard);
    assert_eq!(presentation.branch, None);
    assert_eq!(presentation.objectives_cue(), None);
    assert_eq!(presentation.mission_cue, MissionCue::MissionLostSound);
    assert_eq!(presentation.animation, EndAnimation::LossAnim);
    assert_eq!(presentation.result, TerminalState::Aborted);
    // Aborting twice changes nothing: the presentation belongs to the tick
    // that ended the session.
    state.abort();
    assert_eq!(
        state.terminal_presentation(),
        Some(presentation),
        "the selection is latched with the terminal state"
    );
}

/// **Latched, saved, restored — and refused when forged.**
///
/// Later ticks cannot re-select the presentation, a save record carries it so
/// a restored session answers the host the same way, and a record whose
/// terminal state and presentation disagree is refused as corrupt instead of
/// deciding an end screen from data no tick selected.
#[test]
fn accept_f37_d_fu5_presentation_latches_and_survives_save_and_restore() {
    let program = validated(vec![
        objective(10, vec![Action::Finish(Outcome::Succeeded)]),
        objective(3, vec![Action::Finish(Outcome::Failed)]),
    ]);
    let mut state = MissionState::new(&program, SESSION);
    tick(&mut state, &program, 1);
    let presentation = state.terminal_presentation().unwrap();

    // Latched: a later tick returns early and cannot change the selection.
    let later = tick(&mut state, &program, 2);
    assert_eq!(later.terminal, TerminalState::Succeeded);
    assert_eq!(state.terminal_presentation(), Some(presentation));

    // The record carries it (snapshot version 2 is the record that can)…
    let record = state.snapshot(&program);
    assert_eq!(record.version, SNAPSHOT_VERSION);
    assert_eq!(record.presentation, Some(presentation));
    let restored = MissionState::restore(&program, record.clone()).unwrap();
    assert_eq!(restored.terminal_presentation(), Some(presentation));
    assert_eq!(restored.snapshot(&program), record, "round trip is exact");

    // …and a record that disagrees with itself is refused, twice over.
    let mut missing = record.clone();
    missing.presentation = None;
    assert_eq!(
        MissionState::restore(&program, missing),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::PresentationMismatch
        }),
        "a terminal record without its presentation cannot come from a live session"
    );

    let mut running = record;
    running.terminal = TerminalState::Running;
    assert_eq!(
        MissionState::restore(&program, running),
        Err(RestoreError::Corrupt {
            defect: RestoreDefect::PresentationMismatch
        }),
        "a running record cannot carry the end presentation of a tick it never ran"
    );
}

/// **The limitation is closed by these tests, and the finding says so.**
///
/// `f37.d.limit.terminal_branch_delay_and_sound` recorded that this layer
/// "records the result only — neither the branch, the delay, the sounds nor
/// the animation exists here". It may be removed only together with the
/// behaviour and the findings update (F37-D-FU5), so: it is gone from
/// [`RULE_LIMITATIONS`], the fact it gated carries no limitation any more, and
/// the findings entry the closure cites exists, quotes the image it was
/// measured from and names the addresses of the branch order.
#[test]
fn accept_f37_d_fu5_terminal_branch_limitation_is_closed_with_the_finding() {
    assert!(
        RULE_LIMITATIONS
            .iter()
            .all(|limitation| limitation.id != "f37.d.limit.terminal_branch_delay_and_sound"),
        "the presentation half is implemented and pinned, so its limitation is closed"
    );

    let fact = TERMINAL_PRECEDENCE_RULE
        .facts
        .iter()
        .find(|fact| fact.id == "f37.rule.terminal_precedence.loss_branch_before_win")
        .expect("the branch-order fact is still recorded");
    assert!(
        fact.limitations.is_empty(),
        "a fact this runtime now follows gates no limitation: {:?}",
        fact.limitations
    );

    let findings = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(TERMINAL_PRESENTATION_FINDINGS);
    let text = std::fs::read_to_string(&findings).unwrap_or_else(|error| {
        panic!("the findings entry {TERMINAL_PRESENTATION_FINDINGS} must exist: {error}")
    });
    for needle in [
        "f37.d.limit.terminal_branch_delay_and_sound",
        "0x46af7a",
        "0x46afad",
        "0x4194e0",
        ORIGINAL_IMAGE_SHA256,
    ] {
        assert!(
            text.contains(needle),
            "the findings entry must record {needle}"
        );
    }
}

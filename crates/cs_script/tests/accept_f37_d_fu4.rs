//! F37-D-FU4 acceptance (Rally #730): the mission-countdown expiry
//! **pre-emption** — a countdown that expires on this tick ends the mission
//! as a failure *before* that tick's objectives are considered.
//!
//! The rule is measured (owner note on Rally #589, 2026-10-05: the countdown
//! at `0x71b468` is polled at `0x46c640` inside `CZMission::Update`
//! `0x46a490`, before the objective passes, and expiry ends the mission at
//! once through `0x463c30(1, 3.0)` with neither WON nor LOST — static code
//! analysis of the owner-supplied decrypted executable, sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`; never
//! an original run and never `verified_original`, and no executable byte,
//! disassembly or decompiled code appears in this tree).
//!
//! What the production code is pinned on here:
//!
//! * `accept_f37_d_fu4_expired_countdown_fails_the_mission_before_that_ticks_objectives`
//!   — the acceptance sentence: expiry on this tick ⇒ the mission fails, no
//!   objective of that tick completes and no reward of that tick is granted,
//!   while the same tick without the expiry admits the lowest-declared
//!   objective (one completion per tick,
//!   `f37.rule.terminal_precedence.one_completion_per_tick`), grants its
//!   reward and records the success.
//! * `accept_f37_d_fu4_expiry_grants_no_reward_of_that_tick_including_queued_work`
//!   — deferred work due that tick is not executed either.
//! * `accept_f37_d_fu4_noloss_and_network_games_exclude_the_expiry` — the two
//!   measured exclusions, implemented in the decision rather than recorded.
//! * `accept_f37_d_fu4_the_measured_preemption_is_recorded_and_the_closed_limitation_is_gone`
//!   — `f37.d.limit.mission_countdown_preemption` is closed only together
//!   with this behaviour, and what remains open is recorded with the content
//!   it affects and the task that resolves it.
//! * `accept_f37_d_fu4_plain_step_passes_no_countdown_and_never_preempts` —
//!   the `MissionState::step` path is unchanged: no countdown input, no
//!   pre-emption — the produced input lives in the session layer
//!   (`cs_sim::mission::Countdown`, `accept_f37_d_fu6_*`).

use cs_script::ir::*;
use cs_script::runtime::*;
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// Fixed session generation: the event keys are part of what the probes read.
const SESSION: SessionGeneration = SessionGeneration(17);

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).unwrap()
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn objective(id: u32, condition: Condition, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("synthetic-fu4-obj-{id}")),
        condition,
        actions,
        span: None,
    }
}

/// An objective that is satisfied on every tick: when the countdown does not
/// pre-empt, it fires.
fn always(id: u32, actions: Vec<Action>) -> Objective {
    objective(id, Condition::Const(true), actions)
}

fn program(objectives: Vec<Objective>) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu4"),
        variables: vec![],
        objectives,
    }
}

/// The countdown input of a tick on which the countdown reached zero.
fn expired() -> MissionCountdown {
    MissionCountdown {
        expired: true,
        ..MissionCountdown::NONE
    }
}

fn rewards(events: &[MissionEvent]) -> Vec<ContentId> {
    events
        .iter()
        .filter_map(|event| match &event.kind {
            EventKind::RewardGranted(reward) => Some(reward.clone()),
            _ => None,
        })
        .collect()
}

fn completions(events: &[MissionEvent]) -> Vec<u32> {
    events
        .iter()
        .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
        .map(|event| event.key.source.0)
        .collect()
}

fn terminal_requests(events: &[MissionEvent]) -> Vec<Outcome> {
    events
        .iter()
        .filter_map(|event| match event.kind {
            EventKind::TerminalRequested(outcome) => Some(outcome),
            _ => None,
        })
        .collect()
}

/// **The acceptance sentence, pinned on the production path.**
///
/// One tick, one program whose two objectives are satisfied on it, one of
/// which would also record a success:
///
/// * with [`MissionCountdown::NONE`] the tick does what it always did — the
///   objective the measured scan admits (lowest declaration index,
///   `f37.rule.terminal_precedence.one_completion_per_tick`) completes, its
///   reward is granted and the success is recorded. The second satisfied
///   objective waits for a later tick, and the recorded success ends the
///   mission before it can run;
/// * with a countdown that expired *on this tick* the mission records
///   `Failed`, and that tick completes no objective, grants no reward and
///   requests no outcome. The pre-emption runs before the observe phase, so
///   the objective that would have won the same tick never fires — the
///   measured order (`CZMission::Update` `0x46a490`: countdown, then the
///   objective passes), and the recorded result is a failure because the
///   original's result is success iff the WON flag is set (`0x4194e0`) while
///   an expiry sets neither flag.
#[test]
fn accept_f37_d_fu4_expired_countdown_fails_the_mission_before_that_ticks_objectives() {
    let p = program(vec![
        always(
            1,
            vec![reward("r-first"), Action::Finish(Outcome::Succeeded)],
        ),
        always(2, vec![reward("r-second")]),
    ])
    .validate()
    .unwrap();

    // Control: the same program on the same tick, without an expiry. At most
    // one objective completes per tick — the lowest declaration index — so
    // objective 1 admits, grants and wins here; objective 2, satisfied on the
    // same tick, waits and never runs because the success ends the mission.
    let mut control = MissionState::new(&p, SESSION);
    let tick = control.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(
        completions(&tick.events),
        vec![1],
        "no pre-emption: exactly one completion, the lower declared index"
    );
    assert_eq!(
        rewards(&tick.events),
        [cid(ContentKind::Blueprint, "r-first")],
        "its reward is granted without an expiry"
    );
    assert_eq!(tick.terminal, TerminalState::Succeeded);
    assert!(
        !control.is_completed(SymbolId(2)),
        "the waiting objective never runs: the success ended the mission"
    );

    // The measured pre-emption, on a fresh session of the same program.
    let mut state = MissionState::new(&p, SESSION);
    let tick = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(1), expired())
        .unwrap();
    assert_eq!(
        tick.terminal,
        TerminalState::Failed,
        "an expired countdown is a failure"
    );
    assert_eq!(
        state.terminal(),
        TerminalState::Failed,
        "and it is recorded"
    );
    assert!(
        completions(&tick.events).is_empty(),
        "no objective of that tick completes"
    );
    assert!(
        rewards(&tick.events).is_empty(),
        "no reward of that tick is granted"
    );
    assert!(
        terminal_requests(&tick.events).is_empty(),
        "the expiry is not an objective's outcome request: the original ends with \
         neither WON nor LOST"
    );
    assert!(
        tick.stop.is_none(),
        "the tick stopped by ending, not by a bound"
    );
    assert!(
        !state.is_completed(SymbolId(1)) && !state.is_completed(SymbolId(2)),
        "the objectives of the expiring tick never fired"
    );

    // Latched, like every other terminal path: later ticks change nothing.
    let later = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(2), expired())
        .unwrap();
    assert!(later.events.is_empty());
    assert_eq!(later.terminal, TerminalState::Failed);
    let later = state.step(&p, &MissionFacts::default(), Tick(3)).unwrap();
    assert!(later.events.is_empty());
    assert_eq!(later.terminal, TerminalState::Failed);
}

/// **No reward of that tick is granted — not even one already queued.**
///
/// The first tick schedules a reward one tick ahead (the mission is still
/// running then). On the expiring tick that item is due: the pre-emption
/// returns before the pending queue is drained, so the reward does not run.
/// Like every other terminal path, `step` leaves the queue standing — the
/// consumer tears a terminal session down (`MissionSession::advance` →
/// `finish`) or `MissionState::teardown` drops it — but the queued work is
/// never executed.
#[test]
fn accept_f37_d_fu4_expiry_grants_no_reward_of_that_tick_including_queued_work() {
    let p = program(vec![always(
        1,
        vec![Action::Schedule {
            delay_ticks: 1,
            actions: vec![reward("r-deferred")],
        }],
    )])
    .validate()
    .unwrap();

    let mut state = MissionState::new(&p, SESSION);
    let first = state.step(&p, &MissionFacts::default(), Tick(1)).unwrap();
    assert_eq!(completions(&first.events), vec![1]);
    assert_eq!(
        state.queued_items(),
        1,
        "the deferred reward is queued for tick 2"
    );

    let expiring = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(2), expired())
        .unwrap();
    assert_eq!(expiring.terminal, TerminalState::Failed);
    assert!(
        rewards(&expiring.events).is_empty(),
        "the reward due on the expiring tick is not granted"
    );
    assert_eq!(
        state.queued_items(),
        1,
        "the queue is left standing for the consumer's teardown, never drained"
    );

    // And it never runs: the session is over.
    let later = state.step(&p, &MissionFacts::default(), Tick(3)).unwrap();
    assert!(later.events.is_empty());
    assert_eq!(later.terminal, TerminalState::Failed);
    assert_eq!(state.queued_items(), 1);
}

/// **Both measured exclusions are part of the decision, not of the caller's
/// goodwill.**
///
/// The expiry poll reports **no** expiry when the mission carries `NOLOSS`
/// (`0x46c640` zeroes the remaining time; the flag is set by the exact
/// 7-byte `NOLOSS` child of `MISSION_TIMER`, `0x46c540`), and the owner note
/// records the check is skipped in network games. With either exclusion the
/// tick runs normally — objectives complete, rewards are granted — which is
/// what distinguishes "excluded" from "the countdown did not expire".
#[test]
fn accept_f37_d_fu4_noloss_and_network_games_exclude_the_expiry() {
    // The truth table the decision is: expired alone pre-empts, an exclusion
    // or a live countdown does not.
    assert!(
        MissionCountdown::NONE.eq(&MissionCountdown::default()),
        "the default input is the no-countdown one"
    );
    assert!(
        !MissionCountdown::NONE.preempts(),
        "no expiry, no pre-emption"
    );
    assert!(expired().preempts(), "expired alone pre-empts");
    assert!(
        !MissionCountdown {
            no_loss: true,
            ..expired()
        }
        .preempts(),
        "NOLOSS excludes the expiry"
    );
    assert!(
        !MissionCountdown {
            network_game: true,
            ..expired()
        }
        .preempts(),
        "a network game excludes the expiry"
    );
    assert!(
        !MissionCountdown {
            no_loss: true,
            network_game: true,
            ..MissionCountdown::NONE
        }
        .preempts(),
        "a stopped or live countdown never pre-empts, exclusions or not"
    );

    let p = program(vec![always(1, vec![reward("r-only")])])
        .validate()
        .unwrap();

    for (name, countdown) in [
        (
            "NOLOSS",
            MissionCountdown {
                no_loss: true,
                ..expired()
            },
        ),
        (
            "network game",
            MissionCountdown {
                network_game: true,
                ..expired()
            },
        ),
    ] {
        let mut state = MissionState::new(&p, SESSION);
        let tick = state
            .step_with_countdown(&p, &MissionFacts::default(), Tick(1), countdown)
            .unwrap();
        assert_eq!(
            tick.terminal,
            TerminalState::Running,
            "{name}: the excluded expiry ends nothing"
        );
        assert_eq!(
            completions(&tick.events),
            vec![1],
            "{name}: the tick's objectives run as usual"
        );
        assert_eq!(
            rewards(&tick.events).len(),
            1,
            "{name}: the tick's reward is granted as usual"
        );
        assert!(state.is_completed(SymbolId(1)));
    }

    // The control: the same tick without an exclusion fails the mission.
    let mut state = MissionState::new(&p, SESSION);
    let tick = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(1), expired())
        .unwrap();
    assert_eq!(
        tick.terminal,
        TerminalState::Failed,
        "no exclusion, no tick"
    );
}

/// **The rule is recorded, and the closed limitations cannot come back.**
///
/// `f37.d.limit.mission_countdown_preemption` said "a tick carries no
/// timeout input"; `f37.d.limit.mission_countdown_producer` said "nothing
/// feeds it". F37-D-FU6's `cs_sim::mission::Countdown` makes both false —
/// the entries are gone, and they may only stay gone while the behaviour
/// stands: the same assertion that pins the closures also pins the fact
/// that gates them, the finding that records all three, and the entries
/// that carry what is *still* open (the tick dt, the end-path guards and
/// the spec sourcing) with the content they affect and the tasks that
/// resolve them.
#[test]
fn accept_f37_d_fu4_the_measured_preemption_is_recorded_and_the_closed_limitation_is_gone() {
    for closed in [
        "f37.d.limit.mission_countdown_preemption",
        "f37.d.limit.mission_countdown_producer",
    ] {
        assert!(
            !RULE_LIMITATIONS
                .iter()
                .any(|limitation| limitation.id == closed),
            "the closed entry {closed} must stay closed: what it denied is implemented"
        );
    }
    for (id, task) in [
        ("f37.d.limit.mission_countdown_tick_dt", "F16-F"),
        ("f37.d.limit.mission_countdown_end_guards", "F37-D-FU8"),
        (
            "f37.d.limit.mission_countdown_spec_sourcing",
            "VS-M01-RUNTIME",
        ),
    ] {
        let open = RULE_LIMITATIONS
            .iter()
            .find(|limitation| limitation.id == id)
            .unwrap_or_else(|| panic!("what remains open is still recorded: {id}"));
        assert!(
            !open.affected_content.is_empty(),
            "{id} must name the affected content"
        );
        assert!(
            open.resolving_task.contains(task),
            "{id} must name the task that resolves it: {}",
            open.resolving_task
        );
    }

    // The measured fact they gate still cites the addresses that settle it
    // and now points at the entries that really describe this runtime.
    let fact = TERMINAL_PRECEDENCE_RULE
        .facts
        .iter()
        .find(|fact| fact.id == "f37.rule.terminal_precedence.countdown_preempts")
        .expect("the countdown fact is recorded");
    assert_eq!(
        fact.limitations,
        [
            "f37.d.limit.mission_countdown_tick_dt",
            "f37.d.limit.mission_countdown_end_guards",
            "f37.d.limit.mission_countdown_spec_sourcing",
        ],
        "the fact's divergences must be entries that exist and are open"
    );
    assert!(
        fact.addresses.contains("0x46c640") && fact.addresses.contains("0x4194e0"),
        "the fact must keep citing the expiry check and the result"
    );

    // The findings entry records the closures: same file, same image hash,
    // and it names both closed ids, what replaced them and the producer.
    let findings = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(TERMINAL_RULE_FINDINGS);
    let text = std::fs::read_to_string(&findings).unwrap_or_else(|error| {
        panic!("the findings entry {TERMINAL_RULE_FINDINGS} must exist: {error}")
    });
    assert!(
        text.contains(ORIGINAL_IMAGE_SHA256),
        "the findings entry must quote the image sha256 it was measured from"
    );
    for needle in [
        "f37.d.limit.mission_countdown_preemption",
        "f37.d.limit.mission_countdown_producer",
        "f37.d.limit.mission_countdown_tick_dt",
        "f37.d.limit.mission_countdown_end_guards",
        "f37.d.limit.mission_countdown_spec_sourcing",
        "step_with_countdown",
    ] {
        assert!(
            text.contains(needle),
            "the findings entry must record {needle}: the closures and their \
             replacements travel together"
        );
    }
}

/// **The countdown-less path is untouched: `step` carries no input at all.**
///
/// `MissionState::step` is the entry point for a caller that has no
/// countdown: it still passes [`MissionCountdown::NONE`], so no tick of
/// that path can end by timeout — the produced input lives in the session
/// (`cs_sim::mission::Countdown`), which `accept_f37_d_fu6_*` pins. The two
/// paths differ only in the countdown input: the same program ends
/// `Succeeded` through `step` and `Failed` through the expiring input.
#[test]
fn accept_f37_d_fu4_plain_step_passes_no_countdown_and_never_preempts() {
    let p = program(vec![always(1, vec![reward("r-only")])])
        .validate()
        .unwrap();

    let mut plain = MissionState::new(&p, SESSION);
    let mut granted = 0;
    for tick in 1..=3u64 {
        let result = plain
            .step(&p, &MissionFacts::default(), Tick(tick))
            .unwrap();
        assert_eq!(
            result.terminal,
            TerminalState::Running,
            "tick {tick} of the default path ends nothing"
        );
        granted += rewards(&result.events).len();
    }
    assert_eq!(
        granted, 1,
        "the default path runs the program: one objective, one reward, no timeout"
    );

    // The same program through the countdown input ends at once instead.
    let mut expiring = MissionState::new(&p, SESSION);
    let result = expiring
        .step_with_countdown(&p, &MissionFacts::default(), Tick(1), expired())
        .unwrap();
    assert_eq!(result.terminal, TerminalState::Failed);
    assert!(rewards(&result.events).is_empty());
}

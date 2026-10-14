//! F37-D-FU8 acceptance (Rally #740): the mission-countdown's end-path
//! guards, measured from the owner-supplied decrypted executable (image
//! sha256 `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`,
//! static analysis only — never an original run, `inferred`, never
//! `verified_original`; see
//! `docs/findings/2026-10-15-f37-d-fu8-mission-countdown-end-guards.md`).
//!
//! Two guards of the original's expiry end path were open questions this
//! task settled:
//!
//! * `0x440ad0()`'s global game-state byte is the registered game option
//!   named `Network` (`[[0x64f750]]`, the option's value cell): the end
//!   path runs only while it reads zero (`0x46a4ef`), so a network game's
//!   poll never ends the mission. Modelled since F37-D-FU4 as
//!   `MissionCountdown::network_game`; now sourced in measurement, not
//!   unknown.
//! * The `remaining < -1.0f` skip: the end path additionally requires
//!   `remaining > -1.0f` (constant `0xbf800000` at `0x6034e8`, compared at
//!   `0x46a517` — `fnstsw`/`test ah, 0x41` is below **or equal**, so a
//!   timer at or past -1.0 s no longer ends the mission). Now modelled as
//!   `MissionCountdown::preempts`'s stale-timer guard over the carried
//!   `[+4]` (`MissionCountdown::remaining_seconds`).
//!
//! What the production code is pinned on here:
//!
//! * `accept_f37_d_fu8_the_stale_timer_guard_skips_the_end_path` — the
//!   guard's exact boundary: -1.0 s and beyond skip, the open interval
//!   `(-1.0, 0.0]` ends the mission, NaN skips unordered (the x87 bits a
//!   NaN compare sets).
//! * `accept_f37_d_fu8_step_with_countdown_ignores_a_stale_expiry` — on
//!   the production path: a stale expiry leaves the mission running and
//!   the tick resolves normally, while an in-window expiry ends it.
//! * `accept_f37_d_fu8_the_network_gate_and_stale_guard_are_machine_readable`
//!   — `f37.d.limit.mission_countdown_end_guards` is closed only together
//!   with these tests: the measured fact carries both gates' addresses,
//!   the residuals it does not close stay recorded, and the closed entry
//!   may not come back.

use cs_script::ir::{
    Action, Condition, IR_VERSION, MissionProgram, Objective, SymbolId, ValidatedProgram,
};
use cs_script::runtime::{
    EventKind, MissionCountdown, MissionFacts, MissionState, RULE_LIMITATIONS, SessionGeneration,
    TERMINAL_PRECEDENCE_RULE, TerminalState,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};

/// The session generation every fixture here runs under.
const SESSION: SessionGeneration = SessionGeneration(40);

/// The image every address below was measured from.
const IMAGE_SHA: &str = "43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75";

fn cid(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("a valid content id grammar")
}

fn reward(key: &str) -> Action {
    Action::GrantReward {
        reward: cid(ContentKind::Blueprint, key),
    }
}

fn program(objectives: Vec<Objective>) -> ValidatedProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-f37-d-fu8"),
        variables: vec![],
        objectives,
    }
    .validate()
    .expect("a valid synthetic program")
}

fn always(id: u32, actions: Vec<Action>) -> Objective {
    Objective {
        id: SymbolId(id),
        content: cid(ContentKind::Objective, &format!("fu8-obj-{id}")),
        condition: Condition::Const(true),
        actions,
        span: None,
    }
}

/// The measured expiry input: expired on this tick, no exclusions but the
/// one under test, `[+4]` as the end path would read it.
fn stale_expiry(remaining_seconds: f64) -> MissionCountdown {
    MissionCountdown {
        expired: true,
        remaining_seconds,
        ..MissionCountdown::NONE
    }
}

/// **The stale-timer guard's exact boundary — the measured `<= -1.0f`
/// skip.** The original's end path compares `[+4]` against `-1.0f` with
/// `fnstsw`/`test ah, 0x41` (`0x46a517`): C0 (below) or C3 (equal) skips,
/// so exactly -1.0 s is already too stale, and an unordered compare —
/// which a NaN sets together with both bits — skips too.
#[test]
fn accept_f37_d_fu8_the_stale_timer_guard_skips_the_end_path() {
    // The open interval the end path lives in: the poll reports at
    // `remaining <= 0.0`, and every value above -1.0 ends the mission.
    for remaining in [0.0, -0.5, -0.999] {
        assert!(
            stale_expiry(remaining).preempts(),
            "remaining {remaining} > -1.0f: the end path runs"
        );
    }
    // At -1.0 and beyond the end path is skipped — the mission runs on.
    for remaining in [-1.0, -1.000_000_1, -1.5, -5.0, -1e9] {
        assert!(
            !stale_expiry(remaining).preempts(),
            "remaining {remaining} <= -1.0f: the stale-timer skip"
        );
    }
    // Unordered: x87's NaN compare sets C0, C2 and C3 — the same skip.
    assert!(
        !stale_expiry(f64::NAN).preempts(),
        "an unordered compare skips the end path, as x87's NaN does"
    );
    // The guard never fires without the poll's own observation, and it
    // composes with the two measured exclusions rather than replacing
    // them.
    assert!(
        !MissionCountdown {
            remaining_seconds: 2.0,
            ..MissionCountdown::NONE
        }
        .preempts(),
        "no expiry, no pre-emption, however fresh the timer"
    );
    assert!(
        !MissionCountdown {
            no_loss: true,
            ..stale_expiry(-0.5)
        }
        .preempts(),
        "NOLOSS still excludes the expiry"
    );
    assert!(
        !MissionCountdown {
            network_game: true,
            ..stale_expiry(-0.5)
        }
        .preempts(),
        "a network game still excludes the expiry"
    );
}

/// **On the production path: a stale expiry leaves the mission running and
/// the tick resolves normally — an in-window expiry ends it.**
///
/// The reachable case in this layering is a spelled `TIMER_ADJUST` whose
/// set or add takes `[+4]` past -1.0 in one step (the producer only ever
/// decrements by a tick dt of at most 1.0 s, so its own reports always
/// land in the window): the original's poll reports such an expiry every
/// frame and the stale skip keeps the mission alive — and so does this
/// runtime, where the same input no longer ends the tick.
#[test]
fn accept_f37_d_fu8_step_with_countdown_ignores_a_stale_expiry() {
    let p = program(vec![always(1, vec![reward("r-on")])]);
    let mut state = MissionState::new(&p, SESSION, 1);

    // A stale expiry: the tick resolves as any other — the objective
    // completes and grants its reward, nothing ends.
    let stale = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(1), stale_expiry(-1.5))
        .unwrap();
    assert_eq!(stale.terminal, TerminalState::Running);
    assert_eq!(
        stale
            .events
            .iter()
            .filter(|event| matches!(event.kind, EventKind::ObjectiveCompleted))
            .count(),
        1,
        "the stale expiry ends nothing: the tick's objective completes"
    );
    assert!(
        state.terminal_presentation().is_none(),
        "a skipped end path selects no end presentation"
    );

    // Still stale, still running.
    let still = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(2), stale_expiry(-2.75))
        .unwrap();
    assert_eq!(still.terminal, TerminalState::Running);

    // An in-window expiry ends the mission the way the poll's first report
    // always did — the guard gates staleness, not expiry.
    let fresh = state
        .step_with_countdown(&p, &MissionFacts::default(), Tick(3), stale_expiry(-0.5))
        .unwrap();
    assert_eq!(fresh.terminal, TerminalState::Failed);
    assert!(
        fresh.events.is_empty(),
        "the in-window expiry pre-empts the tick's objectives"
    );
}

/// **The closure is machine-readable — and only together with these
/// tests.** `f37.d.limit.mission_countdown_end_guards` may not come back
/// while `MissionCountdown::preempts` carries all three measured guards;
/// the fact records both gates this task settled with their addresses, the
/// residuals it does not close stay recorded with their affected content
/// and resolving task, and the rule's evidence names this measurement.
#[test]
fn accept_f37_d_fu8_the_network_gate_and_stale_guard_are_machine_readable() {
    assert!(
        !RULE_LIMITATIONS
            .iter()
            .any(|limitation| limitation.id == "f37.d.limit.mission_countdown_end_guards"),
        "both guards are measured and modelled: the closed entry must not come back"
    );
    for id in [
        "f37.d.limit.mission_countdown_tick_dt",
        "f37.d.limit.mission_countdown_spec_sourcing",
    ] {
        let open = RULE_LIMITATIONS
            .iter()
            .find(|limitation| limitation.id == id)
            .unwrap_or_else(|| panic!("the residual {id} stays recorded"));
        assert!(
            !open.affected_content.is_empty() && !open.resolving_task.is_empty(),
            "{id} must keep naming its affected content and resolving task"
        );
    }

    let fact = TERMINAL_PRECEDENCE_RULE
        .facts
        .iter()
        .find(|fact| fact.id == "f37.rule.terminal_precedence.countdown_preempts")
        .expect("the countdown fact is recorded");
    assert_eq!(
        fact.limitations,
        [
            "f37.d.limit.mission_countdown_tick_dt",
            "f37.d.limit.mission_countdown_spec_sourcing",
        ],
        "the fact's residuals are exactly the ones this task did not close"
    );
    for needle in [
        // The game-state byte, by name and by read site: the `Network`
        // option's value cell, the helper that reads it, the gate test and
        // the constant.
        "Network", "0x440ad0", "0x46a4ef",
        // The stale-timer skip: the compare site and -1.0f's constant.
        "0x46a517", "0x6034e8",
    ] {
        assert!(
            fact.addresses.contains(needle),
            "the fact must record the gate at {needle}: {}",
            fact.addresses
        );
    }
    assert!(
        TERMINAL_PRECEDENCE_RULE
            .evidence
            .contains("docs/findings/2026-10-15-f37-d-fu8-mission-countdown-end-guards.md"),
        "the rule's evidence names this measurement"
    );
    assert!(
        TERMINAL_PRECEDENCE_RULE.evidence.contains(IMAGE_SHA),
        "the rule's evidence pins the image this task measured"
    );

    // The findings entry records the measurement and the closure: same
    // image hash, both guards by their names and sites, the closed id.
    let findings = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("docs/findings/2026-10-15-f37-d-fu8-mission-countdown-end-guards.md");
    let text = std::fs::read_to_string(&findings)
        .unwrap_or_else(|error| panic!("the findings entry must exist: {error}"));
    assert!(
        text.contains(IMAGE_SHA),
        "the findings entry quotes the image it was measured from"
    );
    for needle in [
        "f37.d.limit.mission_countdown_end_guards",
        "Network",
        "0x440ad0",
        "0x46a4ef",
        "0x46a517",
        "0x6034e8",
        "step_with_countdown",
    ] {
        assert!(
            text.contains(needle),
            "the findings entry must record {needle}: the measurement and its \
             closure travel together"
        );
    }
}

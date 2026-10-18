//! Acceptance suite M01-B-WAKE (#1357): the world facts that wake M01's
//! dormant `Finish` blocks.
//!
//! **Layout correction (M01-ARCHIVE-DECISION):** the archive this suite
//! measured, `zbd/c1c/m01`, is bound to work order **M06** under the
//! corrected content-derived campaign layout, so the launch below resolves
//! it through `label("M06")`; every constant names bytes of that unchanged
//! archive and the M01-B-WAKE names stay as provenance.
//!
//! M01's two terminal blocks are `OBJECTIVE24` (`INSTANTWIN`, zero-based
//! index 23) and `OBJECTIVE40` (`INSTANTLOSS`, index 39). Both start dormant
//! behind `BEGIN_DORMANT -1`, which the measured timer model never wakes on
//! its own, so each is reached only through the record's own effect graph:
//!
//! * **23 (Succeeded)** — `OBJECTIVE24` is woken by `OBJECTIVE18`'s
//!   `WAKE_OBJECTIVE_WHEN_I_COMPLETE`, at the end of a chain that starts at
//!   `OBJECTIVE6` (one of `workersvoyagezep`'s 18 engine-part chains out of
//!   play), climbs `OBJECTIVE8` (five) and `OBJECTIVE9` (nine), crosses the
//!   `wv_drop_copilot`/`wv_pickup_copilot`/`hooked_to_klondike`
//!   `ANIM_STATE` gates, both `DEDG` group depletions (groups 1 and 2) and
//!   the `NAP` re-wake timers (15 + 5 + 15 + 15 + 30 + 30 + 3 + 10 = 123
//!   seconds of mission clock) the record spells between them.
//! * **39 (Failed)** — `OBJECTIVE40` is re-woken 15 s after `OBJECTIVE39`
//!   completes, at the end of a chain that starts at `OBJECTIVE36` (two of
//!   `piratezep`'s 12 engine-part chains out of play) and climbs
//!   `OBJECTIVE37` (six) and `OBJECTIVE38` (ten) through three more 15 s
//!   `NAP` re-wakes; `OBJECTIVE39` itself needs all twelve.
//!
//! Every edge of both chains is one of the four measured mechanisms this
//! task wires into `MissionHost::step`:
//!
//! * the recovered [`EngineObjectives`] lifecycle model
//!   ([`cs_app::mission_session::ObjectiveLifecycles`]) — `BEGIN_DORMANT`
//!   timers, `TICK_DEPENDS_ON_OBJ`, and the `WAKE`/`NAP`/`KILL`/`SLEEP`
//!   completion effects, advanced once per host step before the control
//!   program evaluates and completed once per `ObjectiveCompleted` event;
//! * the [`MissionWorldObservation`] port — the world's own per-step report
//!   of member presence, group living counts and animation states, consumed
//!   each step so a report describes exactly one tick;
//! * the record player's animation rows, folded under each record's own
//!   startup identity;
//! * the measured `+0x5c8` state projection (`Dormant`/`Awake`/`Napping`/
//!   `Asleep` → `Dormant`/`Awake`/`Napping`/`Done`), with a `KILL`ed
//!   objective publishing no row at all.
//!
//! A driven run reaches each terminal through those providers and only
//! through them: report no member rows and the `INACTIVE` ladders never
//! start; remove the engine model and the completion effects never apply;
//! report eleven of the twelve `piratezep` parts and the chain stalls awake
//! at the threshold it could not meet — each leg named in a test below.
//! Nothing here asks for an outcome; `INSTANTWIN`/`INSTANTLOSS` arrive only
//! through the program's own `Action::Finish`.

use std::time::Duration;

use bevy::prelude::App;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostRefusal, MissionHostTick, MissionStage, MissionTerminal,
    MissionTerminalSource, MissionWorldObservation, build_headless, stage_for, teardown,
};
use cs_app::physics::{BASELINE_FIXED_HZ, PhysicsTickLedger};
use cs_app::world_facts::{MemberObservation, WorldObservation, member_chain};
use cs_content::objectives_engine::{
    DormantTimers, EngineDeclaration, EngineObjective, EngineObjectives, NapDeclaration, TargetList,
};
use cs_script::ir::{
    Action, AnimationState, Condition, IR_VERSION, MemberName, MissionProgram, Objective, Outcome,
    SymbolId,
};
use cs_script::runtime::ObjectiveLifecycle;
use cs_sim::objectives::terminal::TerminalOutcome;
use cs_types::content::ContentKind;

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The fixed timestep one composed step covers — `Time<Fixed>`'s own
/// timestep on the baseline clock, handed to `MissionHost::step` exactly
/// the way `mission_host_tick` hands it.
const FIXED_DT: Duration = Duration::from_nanos(1_000_000_000 / BASELINE_FIXED_HZ as u64);

/// The elapsed a driven retail step covers: one second of mission clock per
/// host step. `MissionHost::step` accepts the frame's elapsed as a parameter
/// and the engine's measured timer pass consumes whatever delta it is
/// handed (`mission_clock += dt`, then the timer cases), so this compresses
/// the run's wall-clock cost — M01's `NAP` re-wakes spell 123 s of delays
/// on the win chain alone, which is 14 760 fixed ticks — without changing a
/// single gate: the `INACTIVE` ladders still count the same observed
/// members, the naps still accumulate the same seconds, and the script
/// still advances exactly one tick per step. Nothing in the lowered program
/// reads the tick-to-second ratio, so the compressed clock changes only
/// *how fast* the measured states arrive, never which ones do.
const DRIVEN_STEP: Duration = Duration::from_secs(1);

/// `OBJECTIVE24`, the `INSTANTWIN` block — zero-based index 23.
const WIN_BLOCK: u32 = 23;
/// `OBJECTIVE40`, the `INSTANTLOSS` block — zero-based index 39.
const LOSS_BLOCK: u32 = 39;

/// `workersvoyagezep`'s engine-part chains, in the order the record's
/// `INACTIVE<n>` stages spell them: nine right engines, nine left.
fn workers_voyage_chains() -> Vec<MemberName> {
    (1..=9)
        .flat_map(|n| {
            [
                member_chain(["workersvoyagezep", &format!("reng{n}"), "healthy"]),
                member_chain(["workersvoyagezep", &format!("leng{n}"), "healthy"]),
            ]
        })
        .collect()
}

/// `piratezep`'s engine-part chains: six right, six left.
fn pirate_chains() -> Vec<MemberName> {
    ["reng11", "reng12", "reng21", "reng22", "reng31", "reng32"]
        .into_iter()
        .chain(["leng11", "leng12", "leng21", "leng22", "leng31", "leng32"])
        .map(|part| member_chain(["piratezep", part, "healthy"]))
        .collect()
}

/// The world report a run where `workersvoyagezep` is dead files every
/// step: all eighteen engine parts out of play, groups 1 and 2 living
/// nobody, `wv_drop_copilot` running and `wv_pickup_copilot` +
/// `hooked_to_klondike` executed — the world-side rows the win chain's
/// `INACTIVE`/`DEDG`/`ANIM_STATE` gates read. `piratezep` is never
/// reported, so the loss chain's `INACTIVE` ladders count zero cleared
/// parts and stay unarmed.
fn win_observation() -> WorldObservation {
    let mut observation = WorldObservation::new();
    for chain in workers_voyage_chains() {
        observation = observation.member(chain, MemberObservation::no_longer_in_play([0.0; 3]));
    }
    observation
        .group(1, 0)
        .group(2, 0)
        .animation("wv_drop_copilot", AnimationState::Running.code())
        .animation("wv_pickup_copilot", AnimationState::Executed.code())
        .animation("hooked_to_klondike", AnimationState::Executed.code())
}

/// The world report a run where `piratezep` is dead files every step: the
/// twelve engine parts out of play — every row the loss chain's four
/// `INACTIVE` ladders count. `workersvoyagezep`'s parts are never
/// reported, so the win chain stays unarmed.
fn loss_observation() -> WorldObservation {
    let mut observation = WorldObservation::new();
    for chain in pirate_chains() {
        observation = observation.member(chain, MemberObservation::no_longer_in_play([0.0; 3]));
    }
    observation
}

/// One driven step of the production host: the world's own report for the
/// step is written through the [`MissionWorldObservation`] port, the
/// world's own tick ledger commits the tick (the role the fixed schedule's
/// `record_tick_boundary` plays in a composed run), and the host steps
/// exactly the way `mission_host_tick` steps it — same seam, same settle.
///
/// `elapsed` is the gameplay time the step covers: [`FIXED_DT`] for the
/// synthetic stage, [`DRIVEN_STEP`] for retail drives whose record-spelled
/// nap delays are minutes of mission clock.
///
/// Returns the step's own answers. The caller reads the terminal off the
/// host itself, exactly as the composed entry leaves it.
fn step_host(app: &mut App, observation: WorldObservation, elapsed: Duration) -> MissionHostTick {
    app.world_mut()
        .insert_resource(MissionWorldObservation { observation });
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks += 1;
    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composed app carries the host");
    let produced = host
        .step(app.world_mut(), elapsed)
        .expect("the driven step answers, not a refusal");
    host.settle(app.world_mut(), &produced);
    app.world_mut().insert_resource(host);
    produced
}

/// Drives the production host step by step until it settles or `budget`
/// steps ran without a terminal, answering with the terminal when one
/// lands. Every step re-files `observation`, the way a world that keeps
/// holding the reported state reports it every tick.
fn drive_until_terminal(
    app: &mut App,
    observation: impl Fn() -> WorldObservation,
    budget: usize,
) -> Option<MissionTerminal> {
    for _ in 0..budget {
        step_host(app, observation(), DRIVEN_STEP);
        let host = app.world().resource::<MissionHost>();
        if let Some(terminal) = host.terminal() {
            return Some(terminal.clone());
        }
    }
    None
}

/// The production stage for retail M01 — the one whose seed carries the
/// recovered engine lifecycle model beside the declared table.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// **M01's stage carries the recovered engine lifecycle model, and the
/// terminals start dormant behind the `-1` sentinel.**
///
/// The provider this task adds is the recovered [`EngineObjectives`]: it
/// must be on the seed, it must have loaded M01's 58 objectives, and the
/// two `Finish` blocks must load dormant — the state the whole wake graph
/// exists to leave.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_retail_m01_carries_the_recovered_lifecycle_model() {
    let stage = retail_stage();
    let engine = stage
        .host
        .engine_objectives
        .as_ref()
        .expect("the recovered engine lifecycle model is on the seed");
    assert_eq!(
        (0usize..)
            .take_while(|index| engine.get(*index).is_some())
            .count(),
        58,
        "the engine model loads all 58 of M01's objectives"
    );
    assert!(
        stage.host.refusals.iter().all(|refusal| !matches!(
            refusal,
            MissionHostRefusal::BlockLifecycles { .. }
                | MissionHostRefusal::EngineObjectives { .. }
        )),
        "a stage that carries both lifecycle models raises neither refusal"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let host = app.world().resource::<MissionHost>();
    assert_eq!(host.lifecycles().len(), 58);
    for index in [WIN_BLOCK, LOSS_BLOCK] {
        assert_eq!(
            host.lifecycles().state(index),
            Some(ObjectiveLifecycle::Dormant),
            "terminal block {index} starts dormant behind the -1 sentinel"
        );
    }
    assert_eq!(
        host.lifecycles().mission_clock(),
        0.0,
        "the mission clock starts at zero"
    );
    teardown(&mut app);
}

/// **A driven run reaches `INSTANTWIN` through the measured facts and the
/// record's own effect graph — and through nothing else.**
///
/// The world reports `workersvoyagezep` dead every step; the recovered
/// lifecycle model applies every `NAP` re-wake and `WAKE` the record
/// spells along the chain `OBJECTIVE6 → 8 → 9 → 10 → 49 → 50 → 11 → 12 →
/// 13 → 14 → 15 → 16 → 17 → 18 → 24`. The terminal that lands is the
/// control program's own `Action::Finish(Succeeded)`, requested by the
/// block the program says asked — index 23 — not a test shortcut.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_a_driven_run_reaches_instantwin_through_measured_facts() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // The chain needs ~123 s of mission clock across its NAP re-wakes plus
    // one evaluation hop per block; at one second per step, 400 steps is
    // generous headroom.
    let terminal = drive_until_terminal(&mut app, win_observation, 400)
        .expect("a world that reports the win facts settles the run");

    assert_eq!(
        terminal.outcome,
        TerminalOutcome::Success,
        "the win chain ends on the record's own INSTANTWIN"
    );
    assert_eq!(
        terminal.source,
        MissionTerminalSource::ControlProgram,
        "the control program's own terminal is this run's source"
    );
    assert_eq!(
        terminal.requested_by,
        Some(SymbolId(WIN_BLOCK)),
        "the block whose Finish asked is OBJECTIVE24"
    );
    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.lifecycles().state(WIN_BLOCK),
        Some(ObjectiveLifecycle::Done),
        "the terminal block completed: the engine's Asleep projects to the vocabulary's Done"
    );
    assert!(
        host.terminal().is_some(),
        "the settled terminal stays on the host"
    );

    teardown(&mut app);
}

/// **A driven run reaches `INSTANTLOSS` through the same machinery the
/// other way.**
///
/// `piratezep` is dead; the `OBJECTIVE36 → 37 → 38 → 39 → 40` chain climbs
/// its four thresholds (2, 6, 10, 12 of the twelve parts) and three-plus-one
/// 15 s `NAP` re-wakes to the loss terminal. `workersvoyagezep` is never
/// reported, so the win chain never starts — the outcome is the record's
/// own `INSTANTLOSS`, not a race.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_a_driven_run_reaches_instantloss_through_measured_facts() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    let terminal = drive_until_terminal(&mut app, loss_observation, 400)
        .expect("a world that reports the loss facts settles the run");

    assert_eq!(
        terminal.outcome,
        TerminalOutcome::Failure,
        "the loss chain ends on the record's own INSTANTLOSS"
    );
    assert_eq!(
        terminal.source,
        MissionTerminalSource::ControlProgram,
        "the control program's own terminal is this run's source"
    );
    assert_eq!(
        terminal.requested_by,
        Some(SymbolId(LOSS_BLOCK)),
        "the block whose Finish asked is OBJECTIVE40"
    );

    teardown(&mut app);
}

/// **Report no world facts and neither terminal can wake.**
///
/// The composed entry itself runs here — no `MissionWorldObservation` is
/// ever written — so every `INACTIVE` ladder counts zero cleared members
/// and both chains stay asleep at the root. The lifecycle model still
/// runs: `OBJECTIVE1`'s `BEGIN_DORMANT 2` self-wake fires on the mission
/// clock this test's steps advance, which is the positive control that the
/// run is alive while it cannot end.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_an_unobserved_world_reaches_no_terminal() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // ~5 s of mission clock: OBJECTIVE1's 2 s self-wake must have fired,
    // nothing else may have.
    for _ in 0..310 {
        app.update();
    }
    let host = app.world().resource::<MissionHost>();
    assert!(
        host.lifecycles().mission_clock() > 2.0,
        "the mission clock advanced past OBJECTIVE1's wake slot: {}",
        host.lifecycles().mission_clock()
    );
    assert!(
        matches!(
            host.lifecycles().state(0),
            Some(ObjectiveLifecycle::Awake) | Some(ObjectiveLifecycle::Done)
        ),
        "OBJECTIVE1 self-woke on its own timer (and may already have completed): {:?}",
        host.lifecycles().state(0)
    );
    for index in [WIN_BLOCK, LOSS_BLOCK] {
        assert_eq!(
            host.lifecycles().state(index),
            Some(ObjectiveLifecycle::Dormant),
            "terminal block {index} stays dormant: every wake path needs a world fact"
        );
    }
    assert!(
        host.terminal().is_none(),
        "no terminal: the unobserved world reports no INACTIVE member at all"
    );

    teardown(&mut app);
}

/// **Eleven parts is not twelve: the chain stalls exactly at the threshold
/// the missing fact could not meet.**
///
/// This is the fail-closed leg for the member-presence provider. The first
/// three rungs of the loss chain complete on the eleven reported parts —
/// `OBJECTIVE36`, `37` and `38` each emit their `ObjectiveCompleted` event —
/// `OBJECTIVE39` wakes and then *stays awake forever*: its threshold is
/// twelve and the twelfth part was never reported out of play. The run
/// neither settles nor pretends the chain finished.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_a_missing_member_stalls_the_chain_at_its_threshold() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    // OBJECTIVE39 (index 38) needs all twelve piratezep parts; report
    // eleven so it wakes and can never complete. Three 15 s naps plus the
    // hops place its wake inside the first minute of mission clock; 200
    // steps holds it there long past the point any inherited behavior
    // could fake.
    let mut completed = Vec::new();
    for _ in 0..200 {
        let produced = step_host(
            &mut app,
            {
                let mut observation = WorldObservation::new();
                for chain in pirate_chains().into_iter().take(11) {
                    observation =
                        observation.member(chain, MemberObservation::no_longer_in_play([0.0; 3]));
                }
                observation
            },
            DRIVEN_STEP,
        );
        completed.extend(produced.lifecycles.completed.iter().copied());
        if app.world().resource::<MissionHost>().terminal().is_some() {
            panic!("the run must not settle: the twelfth part was never reported");
        }
    }
    for index in [35u32, 36, 37] {
        assert!(
            completed.contains(&index),
            "OBJECTIVE{} completed on the eleven reported parts",
            index + 1
        );
    }
    assert!(
        !completed.contains(&38u32),
        "OBJECTIVE39 never completes: eleven of twelve is below its threshold of twelve"
    );
    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.lifecycles().state(38),
        Some(ObjectiveLifecycle::Awake),
        "OBJECTIVE39 woke on the chain and stays awake, honestly incomplete"
    );
    assert_eq!(
        host.lifecycles().state(LOSS_BLOCK),
        Some(ObjectiveLifecycle::Dormant),
        "OBJECTIVE40 never wakes while its predecessor cannot complete"
    );

    teardown(&mut app);
}

/// **Remove the lifecycle provider and the completion effects never
/// land.**
///
/// The same driven win report, on a stage whose `engine_objectives` was
/// removed: the declared dormancy table still answers `ObjectiveAwake`, so
/// `OBJECTIVE6` really does complete — the event is real — but its
/// `WAKE`/`NAP`/`KILL`/`SLEEP` effects have no model to apply to, so
/// `OBJECTIVE8` stays dormant forever and the chain dies at the first
/// hop. This is the provider-removal leg for the lifecycle model itself.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_b_wake_removing_the_lifecycle_model_blocks_the_effects() {
    let mut stage = retail_stage();
    stage.host.engine_objectives = None;
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");

    let mut completed = Vec::new();
    for _ in 0..200 {
        let produced = step_host(&mut app, win_observation(), DRIVEN_STEP);
        completed.extend(produced.lifecycles.completed.iter().copied());
    }
    assert!(
        completed.contains(&5u32),
        "OBJECTIVE6 completed: the declared table's awake row and the observed parts are real"
    );
    let host = app.world().resource::<MissionHost>();
    assert_eq!(
        host.lifecycles().state(7),
        Some(ObjectiveLifecycle::Dormant),
        "OBJECTIVE8 stays dormant: no model applied OBJECTIVE6's WAKE effect"
    );
    assert!(
        host.terminal().is_none(),
        "no terminal: a chain whose effects never apply can climb nowhere"
    );

    teardown(&mut app);
}

// ---------------------------------------------------------------------------
// The CI half: the mechanism itself, on the synthetic stage
// ---------------------------------------------------------------------------

/// A three-block synthetic program whose only terminal is the record's own
/// `Action::Finish(Succeeded)` on block 2: block 0 is awake and completes on
/// its first tick, its `NAP` re-wakes block 1 a sixth of a second later, and
/// block 1's `WAKE` opens block 2. Every gate is the record's own
/// `ObjectiveAwake` spelling — nothing here is a shortcut around the
/// lifecycle model, which is exactly what the assertions check.
fn wake_chain_program() -> MissionProgram {
    let gated = |index: u32| {
        Condition::All(vec![
            Condition::ObjectiveAwake { index },
            Condition::Const(true),
        ])
    };
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-m01-b-wake"),
        variables: Vec::new(),
        objectives: vec![
            Objective {
                id: SymbolId(0),
                content: cid(ContentKind::Objective, "synthetic-m01-b-wake.objective.1"),
                condition: gated(0),
                actions: Vec::new(),
                span: None,
            },
            Objective {
                id: SymbolId(1),
                content: cid(ContentKind::Objective, "synthetic-m01-b-wake.objective.2"),
                condition: gated(1),
                actions: Vec::new(),
                span: None,
            },
            Objective {
                id: SymbolId(2),
                content: cid(ContentKind::Objective, "synthetic-m01-b-wake.objective.3"),
                condition: gated(2),
                actions: vec![Action::Finish(Outcome::Succeeded)],
                span: None,
            },
        ],
    }
}

/// The engine declarations matching that program: block 0 naps block 1 for
/// a sixth of a second on completion; block 1 wakes block 2 on completion;
/// both downstream blocks start dormant behind the `-1` sentinel, exactly
/// as M01's real terminal blocks do.
fn wake_chain_engine() -> EngineObjectives {
    let dormant = DormantTimers {
        wake_at: -1.0,
        nap_after: -1.0,
        rewake_after: -1.0,
        sleep_at: -1.0,
    };
    let first = EngineDeclaration {
        nap: Some(NapDeclaration {
            target: Some(1),
            time: 0.2,
        }),
        ..EngineDeclaration::default()
    };
    let second = EngineDeclaration {
        begin_dormant: Some(dormant),
        wake: TargetList::from_record(&[3]).expect("one target fits"),
        ..EngineDeclaration::default()
    };
    let third = EngineDeclaration {
        begin_dormant: Some(dormant),
        ..EngineDeclaration::default()
    };
    EngineObjectives::new(vec![
        EngineObjective::loaded(first),
        EngineObjective::loaded(second),
        EngineObjective::loaded(third),
    ])
    .expect("every target is in range")
}

/// **The synthetic chain reaches its terminal only through the engine
/// lifecycle model the host now runs.**
///
/// CI leg, no installation: the stage carries the engine model, block 0
/// completes on tick 1, its `NAP` re-wakes block 1, block 1's `WAKE` opens
/// block 2 and the run settles `Success` on the program's own
/// `Action::Finish`. The tick report's lifecycle section is the measured
/// record of each hop: `completed` names the block, and the nap re-wake
/// names it in `woken` the step it returns.
#[test]
fn accept_m01_b_wake_synthetic_chain_reaches_terminal_through_lifecycle_effects() {
    let scratch = Scratch::new("m01-b-wake-chain");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.control = wake_chain_program();
    stage.host.engine_objectives = Some(wake_chain_engine());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    assert!(
        app.world()
            .resource::<MissionHost>()
            .refusals()
            .iter()
            .all(|refusal| !matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
        "a stage carrying the engine model raises no undeclared-lifecycles refusal"
    );
    assert_eq!(
        app.world().resource::<MissionHost>().lifecycles().state(1),
        Some(ObjectiveLifecycle::Dormant),
        "block 1 starts dormant behind the -1 sentinel"
    );

    let mut completed = Vec::new();
    let mut woken = Vec::new();
    let mut terminal = None;
    for _ in 0..600 {
        let produced = step_host(&mut app, WorldObservation::new(), FIXED_DT);
        completed.extend(produced.lifecycles.completed.iter().copied());
        woken.extend(produced.lifecycles.woken.iter().copied());
        let host = app.world().resource::<MissionHost>();
        if let Some(done) = host.terminal() {
            terminal = Some(done.clone());
            break;
        }
    }
    let terminal = terminal.expect("the synthetic chain settles inside 600 steps");
    assert_eq!(
        completed.first().copied(),
        Some(0),
        "block 0 completes first"
    );
    assert!(
        woken.contains(&1),
        "block 1 re-woke through block 0's NAP timer, not a default: {woken:?}"
    );
    assert!(
        completed.contains(&1) && completed.contains(&2),
        "blocks 1 and 2 completed through the chain: {completed:?}"
    );
    assert_eq!(terminal.outcome, TerminalOutcome::Success);
    assert_eq!(terminal.requested_by, Some(SymbolId(2)));
}

/// **The same synthetic chain with the provider removed dies at the first
/// dormant block.**
///
/// The fail-closed leg in CI: the seed's `engine_objectives` is `None` and
/// the declared table is empty (the synthetic stage's documented default),
/// so `Condition::ObjectiveAwake` on blocks 1 and 2 answers `false`
/// forever — block 0's `NAP` has no model to write to either — and no
/// terminal lands.
#[test]
fn accept_m01_b_wake_synthetic_chain_without_the_model_stays_dormant() {
    let scratch = Scratch::new("m01-b-wake-chain-blocked");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.control = wake_chain_program();
    stage.host.engine_objectives = None;
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    assert_eq!(
        app.world().resource::<MissionHost>().lifecycles().state(1),
        None,
        "an empty lifecycle model holds no row for block 1"
    );
    let mut completed = Vec::new();
    for _ in 0..120 {
        let produced = step_host(&mut app, WorldObservation::new(), FIXED_DT);
        completed.extend(produced.lifecycles.completed.iter().copied());
    }
    assert!(
        !completed.contains(&1) && !completed.contains(&2),
        "no dormant block ever woke: {completed:?}"
    );
    assert!(
        app.world().resource::<MissionHost>().terminal().is_none(),
        "no terminal: every wake path needed the removed provider"
    );

    teardown(&mut app);
}

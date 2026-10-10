//! Acceptance stage VS-M01-RT-MISSION-HOST.01: the mission host's records on
//! the stage, driven through one composed per-tick entry (Rally #1278).
//!
//! [`cs_app::mission_session::build_headless`] composes the same systems the
//! `--mission` window runs; the composed entry
//! ([`cs_app::mission_session::mission_host_tick`]) is what this suite holds
//! to the stage. Three things have to be proved:
//!
//! * **Every record answers for itself.** The synthetic stage composes and
//!   steps, and the report the composed entry writes carries what each record
//!   produced — the environment's committed ticks, the world-actor tick, the
//!   animation record player's `TickReport`, the marker consumer's
//!   `MarkerDelivery`, the objective session's `SessionTick` and the control
//!   program's `MissionTick`. None of those values is built by the test, and
//!   none is a mock: the control program's half is checked twice, once
//!   against the objective it really completed and once against an
//!   independent replay of the same program over the same fact fold. This
//!   member runs in CI.
//! * **M01's real program runs through the same entry.** Against the owner's
//!   installation, M01's stage composes headlessly and the composed step
//!   answers with the produced state above plus the named F39 refusal — the
//!   objective declarations no original mission yields today. It also holds
//!   the host to the join's startup rows, the one thing only a stage that
//!   carries an animation join exercises. It is
//!   `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`, so CI skips it
//!   and the implementing and reviewing agents run it with
//!   `--include-ignored`.
//! * **Every absence is reachable.** The first member holds the host to the
//!   missing-join refusal of a stage that carries no animation join; a third
//!   member withholds the world-actor program too, and holds it to the
//!   missing-program refusal while the other three records keep answering for
//!   the same tick. Both members run in CI.
//!
//! Every member fails if the composed entry is removed: the report resource
//! is written by that entry and by nothing else, so deleting the step or the
//! system it is installed from leaves no report to assert on.
//!
//! What is **not** claimed: no original executable ran, nothing here is
//! `verified_original`, M01 is not asserted to reach a terminal state
//! headlessly (its two `Finish` blocks start dormant and every wake path
//! needs world facts a headless run does not have — a measured property of
//! the content, not a defect), and neither the terminal mapping nor the
//! restart exists yet (VS-M01-RT-MISSION-HOST `.02` and `.03`).

use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHost, MissionHostRefusal, MissionHostReport, MissionHostTick, build_headless, stage_for,
    teardown,
};
use cs_app::world_facts::compose_mission_facts;
use cs_script::ir::SymbolId;
use cs_script::runtime::TerminalState;
use cs_sim::mission::MissionSession;
use cs_types::Tick;

use crate::common::label;
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The composed entry's answer, or a panic naming exactly what is missing.
///
/// The report resource is written by [`cs_app::mission_session::mission_host_tick`]
/// and by nothing else, so a world without it is a world whose composed entry
/// was never installed.
fn report_of<'a>(app: &'a bevy::prelude::App, stage_name: &str) -> &'a MissionHostReport {
    app.world()
        .get_resource::<MissionHostReport>()
        .unwrap_or_else(|| {
            panic!(
                "{stage_name}: the composed entry wrote no MissionHostReport, so the fixed-tick \
             system ran no step"
            )
        })
}

/// Steps one composed app through a few fixed ticks and answers with the
/// report the composed entry last wrote.
fn step(app: &mut bevy::prelude::App, stage_name: &str) {
    // One headless frame is 1/60 s over a 120 Hz fixed loop, so two frames
    // are four committed ticks — enough for every record's clock to leave
    // zero whichever way the first frame is counted.
    app.update();
    app.update();
    let _ = report_of(app, stage_name);
}

/// **The synthetic stage's records are all driven by the composed entry, and
/// every answer in the report is the record's own produced state.**
#[test]
fn accept_vs_m01_runtime_host_01_synthetic_stage_drives_every_record_through_one_composed_tick() {
    let scratch = Scratch::new("host");
    let stage = synthetic_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    step(&mut app, "the synthetic composition");

    let report = report_of(&app, "the synthetic composition");
    let host = app.world().resource::<MissionHost>();
    let produced: &MissionHostTick = report.answer.as_ref().unwrap_or_else(|error| {
        panic!("the synthetic composed step refused: {error}");
    });

    // The tick the entry was asked for is the world's own committed fixed
    // tick, and every record stepped it.
    assert!(
        report.tick.0 >= 1,
        "the composed entry must step a committed tick, got {:?}",
        report.tick
    );
    assert_eq!(
        produced.tick, report.tick,
        "every record's answer belongs to the tick the composed entry stepped"
    );

    // 1. Environment: the clock left tick zero and committed real ticks for
    //    this step's elapsed.
    assert!(
        host.environment().clock().tick().0 > 0,
        "the environment clock must advance past tick zero, it is at {:?}",
        host.environment().clock().tick()
    );
    assert!(
        produced.environment_ticks > 0,
        "the composed step must report the ticks the environment committed, got {}",
        produced.environment_ticks
    );

    // 2. World actors: the session reached the host tick through the
    //    production `WorldActorSession::step`.
    let world_actors = produced
        .world_actors
        .as_ref()
        .expect("the synthetic stage lowers a world-actor program, so one answers");
    assert_eq!(
        world_actors.tick, report.tick,
        "the world-actor session must be stepped to the host tick"
    );
    assert_eq!(
        host.world_actors()
            .expect("the synthetic stage lowers a world-actor program")
            .tick(),
        report.tick,
        "the live world-actor session must stand at the tick the report carries"
    );
    assert_eq!(world_actors.session, host.generation());

    // 3a. Animation: the record player's own TickReport, at the player's own
    //     advanced-through tick.
    assert_eq!(
        produced.animation.tick(),
        report.tick,
        "the animation TickReport must belong to the composed tick"
    );
    assert_eq!(
        host.animation().advanced_through(),
        Some(report.tick),
        "the record player must have advanced through the composed tick"
    );
    assert_eq!(host.animation().served(), host.served());

    // 3b. Markers: the delivery the composed step returned is the consumer's
    //     own — its raised markers are exactly the activations the consumer
    //     now holds for this session.
    assert_eq!(
        produced.markers.raised().len(),
        host.markers().applied().len(),
        "the MarkerDelivery in the report must be the consumer's own batch"
    );
    assert_eq!(host.markers().served(), host.served());

    // 3c. Objectives: the session stepped its own tick.
    assert_eq!(
        produced.objectives.tick.tick, report.tick,
        "the objective SessionTick must be for the composed tick"
    );
    assert_eq!(
        host.objectives().runtime().last_tick(),
        Some(report.tick),
        "the live objective runtime must stand at the composed tick"
    );
    assert_eq!(host.objectives().session(), host.generation());

    // 4. Script host: the program's own answer, not a canned one. The host's
    //    session completed the objective its declared condition admits, which
    //    only a real evaluation can do.
    assert_eq!(
        produced.script.tick, report.tick,
        "the control program's MissionTick must be for the composed tick"
    );
    assert_eq!(
        produced.script.terminal,
        TerminalState::Running,
        "the synthetic program declares no terminal directive, so it keeps running"
    );
    assert!(
        host.script().state().is_completed(SymbolId(0)),
        "the composed entry must have run the program far enough to complete its objective"
    );

    // The same answer, recomputed independently: the program the stage
    // declares, launched again over the same fact fold and stepped over the
    // same ticks, must say exactly what the composed entry reported. Nothing
    // here constructs an expected value — production code produces both.
    let mut replay = MissionSession::launch(stage.host.control.clone(), host.generation(), [])
        .expect("the stage's own control program launches");
    let mut answer = None;
    for tick in 1..=report.tick.0 {
        let facts = compose_mission_facts(
            &replay,
            host.blocks(),
            host.world_facts(),
            host.world_operands(),
        );
        answer = Some(
            replay
                .advance(&facts, Tick(tick))
                .expect("the replayed program steps the same ticks"),
        );
    }
    assert_eq!(
        answer.as_ref(),
        Some(&produced.script),
        "the composed entry's MissionTick must be this program's own answer for this tick"
    );

    // The absences the host runs over are named, not silently filled in.
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::ObjectiveDeclarations { .. })),
        "the stage carries no declared objectives and must say so"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::WorldObservation { .. })),
        "the unobserved world fact fold must be named"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
        "the undeclared block lifecycles must be named"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::AnimationJoin { .. })),
        "this stage carries no animation join, so the host must say that no startup row was \
         offered rather than silently stepping an empty player"
    );
    assert_eq!(
        (
            host.animation().running_count(),
            host.animation().finished_count()
        ),
        (0, 0),
        "a stage with no animation join must have started no record"
    );
    assert!(
        host.objectives().program().objectives.is_empty(),
        "a synthetic stage with no declared objectives must never be handed synthesized ones"
    );

    teardown(&mut app);
}

/// **A scope that lowered no world-actor program is named, and the composed
/// entry still answers for every other record.**
///
/// This is the one absence on the host's list with no other seat: neither
/// acceptance stage above lacks a world-actor program, so without this member
/// [`MissionHostRefusal::WorldActors`] and the `None` half of
/// [`MissionHostTick::world_actors`] could only be reached by reading the
/// code. The stage is the very one `vs_m01_rt_window.rs` builds, with only the
/// seed's world-actor program withheld.
#[test]
fn accept_vs_m01_runtime_host_01_a_scope_without_a_world_actor_program_is_named() {
    let scratch = Scratch::new("host-no-actors");
    let mut stage = synthetic_stage(scratch.path());
    stage.host.world_actors = None;
    let mut app = build_headless(&stage).expect("the synthetic stage composes without actors");
    step(&mut app, "the no-world-actor composition");

    let report = report_of(&app, "the no-world-actor composition");
    let host = app.world().resource::<MissionHost>();
    let produced: &MissionHostTick = report.answer.as_ref().unwrap_or_else(|error| {
        panic!("the composed step refused: {error}");
    });

    assert!(
        produced.world_actors.is_none(),
        "a scope that lowered no world-actor program must answer with no world-actor tick"
    );
    assert!(
        host.world_actors().is_none(),
        "no world-actor session may be launched from a scope that lowered no program"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::WorldActors { .. })),
        "the missing world-actor program must be named, not silently skipped"
    );

    // The absence of one record is not a step that did nothing: the other
    // three still answered for this composed tick.
    assert!(
        produced.environment_ticks > 0,
        "the environment must still commit ticks, got {}",
        produced.environment_ticks
    );
    assert_eq!(produced.animation.tick(), report.tick);
    assert_eq!(produced.objectives.tick.tick, report.tick);
    assert_eq!(produced.script.tick, report.tick);

    teardown(&mut app);
}

/// **Against the owner's installation, M01's real control program runs
/// through the composed entry and the F39 refusal is present and named.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_host_01_retail_m01_runs_its_real_program_through_the_composed_entry() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    drop(plan);

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    step(&mut app, "M01's composition");

    let report = report_of(&app, "M01's composition");
    let host = app.world().resource::<MissionHost>();
    let produced: &MissionHostTick = report.answer.as_ref().unwrap_or_else(|error| {
        panic!("M01's composed step refused: {error}");
    });

    assert!(report.tick.0 >= 1, "the composed entry must step a tick");
    assert_eq!(produced.tick, report.tick);

    assert!(
        host.environment().clock().tick().0 > 0,
        "M01's environment clock must advance past tick zero, it is at {:?}",
        host.environment().clock().tick()
    );
    assert!(produced.environment_ticks > 0);

    let world_actors = produced
        .world_actors
        .as_ref()
        .expect("M01's scope lowers a world-actor program, so one answers");
    assert_eq!(world_actors.tick, report.tick);
    assert_eq!(
        host.world_actors()
            .expect("M01's scope lowers a world-actor program")
            .tick(),
        report.tick,
        "M01's world-actor session must reach the host tick"
    );

    assert_eq!(produced.animation.tick(), report.tick);
    assert_eq!(host.animation().advanced_through(), Some(report.tick));
    // The join's startup rows were offered to the record player by the
    // composed entry itself, grouped by event and at the host's own rate.
    // `plan.launchable()` above proved that at least one of M01's
    // mission-carrier rows is playable by this very consumer, so a player that
    // holds neither a running nor a finished record was offered nothing.
    assert!(
        host.animation().running_count() + host.animation().finished_count() > 0,
        "the composed entry must offer M01's startup rows to its record player: {} running, {} \
         finished, {} refused",
        host.animation().running_count(),
        host.animation().finished_count(),
        host.animation().refused_count()
    );
    assert!(
        host.refusals()
            .iter()
            .all(|refusal| !matches!(refusal, MissionHostRefusal::AnimationJoin { .. })),
        "M01's stage carries an animation join, so no missing-join refusal may be raised"
    );
    assert_eq!(
        produced.markers.raised().len(),
        host.markers().applied().len(),
        "the MarkerDelivery in the report must be the consumer's own batch"
    );

    assert_eq!(produced.objectives.tick.tick, report.tick);
    assert_eq!(
        host.objectives().runtime().last_tick(),
        Some(report.tick),
        "M01's objective runtime must stand at the composed tick"
    );
    assert!(
        host.objectives().program().objectives.is_empty(),
        "no original mission yields a declared objective program today, so the session must \
         launch the empty one"
    );

    // M01's own 58-block program, stepped by the composed entry: still
    // running, because both `Finish` blocks start dormant behind the `-1`
    // sentinel and every wake path needs world facts a headless run does not
    // have. That is a measured property of the content (#1278's fact 7), not
    // a step that did nothing.
    assert_eq!(produced.script.tick, report.tick);
    assert_eq!(
        produced.script.terminal,
        TerminalState::Running,
        "M01 cannot reach a terminal state headlessly; a terminal here would mean a block woke \
         without the facts that wake it"
    );

    // The F39 refusal, quoted from the reader's own message.
    let detail = host
        .refusals()
        .iter()
        .find_map(|refusal| match refusal {
            MissionHostRefusal::ObjectiveDeclarations { detail } => Some(detail),
            _ => None,
        })
        .expect("the F39 objective-declaration refusal must be present");
    assert!(
        detail.contains("no declared objective program can be recovered"),
        "the refusal must quote the reader's own message, got: {detail}"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::WorldObservation { .. })),
        "the unobserved world fact fold must be named"
    );
    assert!(
        host.refusals()
            .iter()
            .any(|refusal| matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
        "the undeclared block lifecycles must be named"
    );

    teardown(&mut app);
}

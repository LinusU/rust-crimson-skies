//! Acceptance stage VS-M01-RT-MISSION-HOST.03: a restart rebuilds M01's
//! authored initial state with no leftover entity, cue or session id (Rally
//! #1280).
//!
//! [`cs_app::mission_session::MissionHost::restart`] is the production
//! rebuild this suite holds to the stage; the composed trigger
//! ([`cs_app::mission_session::mission_host_restart`], installed in
//! `PostUpdate` beside the per-tick entry) is what the windowed composition
//! runs. Four things have to be proved:
//!
//! * **The meta reset rebuilds everything, twice.** The synthetic stage
//!   composes and steps until its environment clock and world-actor tick
//!   have moved and its objective session holds a pending dialogue cue; the
//!   playtest's own `R` seam (`PlaytestRequests` → `PlaytestState.resets`)
//!   then triggers the composed restart, and every record answers with the
//!   authored initial state again: the environment clock at tick zero, the
//!   world-actor session at its initial tick, the record player fresh, the
//!   marker ledger released, the objective session on the new generation
//!   with **no** stale cue, the control program `Running` again, the host's
//!   own tick record cleared, the world resident again with the same
//!   authored objects and exactly one player body at the stage's start pose.
//!   The [`TeardownReport`](cs_app::objectives::TeardownReport) the restart
//!   stored names the generation it tore down and the cue that will never
//!   play. A **second** reset advances both identities again. This member
//!   runs in CI.
//! * **A restart requested after a settled terminal clears it.** The very
//!   same stage carries a control program whose own `Finish(Succeeded)`
//!   directive settles a terminal on its first evaluated tick (the designed
//!   synthetic terminal path — M01 itself cannot reach one headlessly, a
//!   measured property of the content). A [`MissionHostRestartRequest`] —
//!   the seam a terminal handler latches through — consumes and rebuilds,
//!   and the fresh control session is `Running` again before its next step
//!   and settles its authored terminal again after it. This member runs in
//!   CI.
//! * **A restart that reuses the live generation fails, in place.** Asked
//!   for the generation that is still served,
//!   [`MissionHost::restart`](cs_app::mission_session::MissionHost::restart)
//!   refuses by name before rebuilding anything
//!   (`IDENTITY-CONTENT`: no cross-session id reuse), and the host keeps
//!   serving the live generation untouched. This member runs in CI.
//! * **M01's real composition restarts.** Against the owner's installation,
//!   M01's stage composes headlessly, steps, and is restarted through the
//!   composed request seam: the same initial state is asserted on the real
//!   world, the real world-actor program, the real start pose and the real
//!   objective-session refusal. It is
//!   `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`, so CI skips
//!   it and the implementing and reviewing agents run it with
//!   `--include-ignored`.
//!
//! Every member fails if the restart is removed: the
//! [`MissionHostRestartReport`](cs_app::mission_session::MissionHostRestartReport)
//! is written by the restart and by nothing else, and the generation-reuse
//! member calls the restart itself.
//!
//! What is **not** claimed: no original executable ran, nothing here is
//! `verified_original`, and the world's fixed ledger (`PhysicsTickLedger`)
//! keeps counting across a restart — it is the physics timeline, not this
//! host's clock; "the host tick is back at zero" means the host's own tick
//! record is cleared and every session it drives stands at its authored
//! initial tick again. The announced load is not re-run by a restart, so
//! the composition's one delivered binding stays exactly one. The terminal
//! *exit mapping* is VS-M01-RT-MISSION-HOST `.02`, not this stage.

use avian3d::prelude::{Position, Rotation};
use bevy::prelude::{Entity, Quat, With};
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHost, MissionHostRefusal, MissionHostReport, MissionHostRestartError,
    MissionHostRestartReport, MissionHostRestartRequest, MissionPlayerBody, MissionStage,
    build_headless, stage_for, teardown,
};
use cs_app::objectives::{LoweredObjectives, lower_program};
use cs_app::playtest::PlaytestRequests;
use cs_app::playtest::scene::PlaytestOriginalFlight;
use cs_app::world::residency;
use cs_content::objectives::{
    DeclaredObjectiveProgram, DeclaredTimer, DeclaredTimerStart, SYNTHETIC_RADIO,
    declared_synthetic_objectives,
};
use cs_content::world::WorldObjectId;
use cs_script::ir::{Action, Condition, IR_VERSION, MissionProgram, Objective, Outcome, SymbolId};
use cs_script::runtime::TerminalState;
use cs_types::Tick;
use cs_types::content::ContentKind;

use crate::common::{cid, label};
use crate::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The composed entry's answer, or a panic naming exactly what is missing.
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

/// The stored restart report, or a panic naming exactly what is missing.
fn restart_report_of<'a>(
    app: &'a bevy::prelude::App,
    stage_name: &str,
) -> &'a MissionHostRestartReport {
    app.world()
        .get_resource::<MissionHostRestartReport>()
        .unwrap_or_else(|| {
            panic!(
                "{stage_name}: no MissionHostRestartReport, so the composed restart either never \
                 ran or was removed"
            )
        })
}

/// The stage the restart suite drives: the very synthetic stage
/// `vs_m01_rt_window.rs` builds, with two of its seed records swapped for
/// ones this suite authors (both synthetic, both lowered/built through
/// production constructors):
///
/// * a **settling** control program — one objective whose condition is
///   `true` and whose action is the IR's own terminal request
///   [`Action::Finish`]`(`[`Outcome::Succeeded]`)`, so the composed entry
///   drives the program to a real terminal state headlessly;
/// * a **cue-emitting** objective program — the F39-C fixture
///   ([`declared_synthetic_objectives`], production-lowered through
///   [`lower_program`]) with its radio timer re-declared to auto-arm at tick
///   one ([`DeclaredTimerStart::AtTick`] instead of `OnArm`), so driving a
///   few committed ticks through the composed entry leaves a dialogue cue
///   pending in the objective session — the teardown must name it.
///
/// The seed's own `ObjectiveDeclarations` refusal described the **empty**
/// objective program this variant no longer carries, so it is not carried
/// into the report; the two absences every host names (the unobserved world
/// fact fold and the undeclared block lifecycles) are appended by the
/// launch itself, as everywhere else.
fn restart_stage(scratch: &std::path::Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    stage.host.control = settling_control_program();
    stage.host.objectives = cue_emitting_objectives();
    stage.host.refusals = Vec::new();
    stage
}

/// The synthetic stage's settling control program: one objective that is
/// true from the first evaluated tick and whose own action requests the
/// terminal outcome. Authored synthetic content through the production IR —
/// never a reading of any original mission's record.
fn settling_control_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-mission-host-restart"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                ContentKind::Objective,
                "synthetic-mission-host-restart-objective",
            ),
            condition: Condition::Const(true),
            actions: vec![Action::Finish(Outcome::Succeeded)],
            span: None,
        }],
    }
}

/// The F39-C fixture objective program with its radio cue timer re-declared
/// to auto-arm at tick one, so ordinary composed ticks emit exactly one
/// pending cue. Everything else — objectives, conditions, the other timers,
/// the trigger and the spawn group — is the fixture's own, lowered through
/// [`lower_program`].
fn cue_emitting_objectives() -> LoweredObjectives {
    let base = declared_synthetic_objectives();
    let timers: Vec<DeclaredTimer> = base
        .timers()
        .iter()
        .map(|timer| {
            if timer.symbol == SYNTHETIC_RADIO {
                DeclaredTimer {
                    start: DeclaredTimerStart::AtTick(1),
                    ..timer.clone()
                }
            } else {
                timer.clone()
            }
        })
        .collect();
    let program = DeclaredObjectiveProgram::try_new(
        base.subject().clone(),
        base.origin().clone(),
        base.provenance().clone(),
        base.precedence().clone(),
        base.objectives().to_vec(),
        base.conditions().to_vec(),
        timers,
        base.triggers().to_vec(),
        base.spawn_groups().to_vec(),
    )
    .expect("the fixture with an auto-armed radio timer is a legal declared program");
    lower_program(&program).expect("the cue-emitting fixture lowers")
}

/// How many delivered load-item bindings the composition holds.
fn binding_count(app: &mut bevy::prelude::App) -> usize {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>();
    query.iter(world).count()
}

/// How many player bodies exist.
fn player_count(app: &mut bevy::prelude::App) -> usize {
    let world = app.world_mut();
    let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
    query.iter(world).count()
}

/// The world's resident objects, sorted — the authored population a reload
/// must bring back identically.
fn resident_objects(app: &mut bevy::prelude::App) -> Vec<WorldObjectId> {
    let residency = residency(app.world()).expect("the stage's world is resident");
    let mut objects: Vec<WorldObjectId> = residency
        .resident()
        .present_objects()
        .into_iter()
        .cloned()
        .collect();
    objects.sort();
    objects
}

/// **The playtest's own meta reset rebuilds the authored initial state, and a
/// second one advances both identities again.**
#[test]
fn accept_vs_m01_runtime_host_03_a_meta_reset_rebuilds_the_authored_initial_state() {
    let scratch = Scratch::new("host-restart");
    let stage = restart_stage(scratch.path());
    let pose = stage.start;
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // The mutate phase: drive enough committed ticks that the environment
    // clock and the world-actor tick have moved and the auto-armed radio
    // timer has left a dialogue cue pending in the objective session.
    app.update();
    app.update();
    app.update();

    let (
        old_generation,
        old_served,
        environment_tick,
        world_actor_tick,
        old_step_tick,
        pending_before,
    ) = {
        let host = app.world().resource::<MissionHost>();
        (
            host.generation(),
            host.served(),
            host.environment().clock().tick(),
            host.world_actors()
                .expect("the synthetic stage lowers a world-actor program")
                .tick(),
            report_of(&app, "the pre-restart composition").tick,
            host.objectives().pending_cues(),
        )
    };
    assert!(
        environment_tick.0 > 0,
        "the environment clock must have moved"
    );
    assert!(
        world_actor_tick.0 > 0,
        "the world-actor tick must have moved"
    );
    assert!(old_step_tick.0 > 0, "the composed entry must have stepped");
    assert!(
        pending_before >= 1,
        "the auto-armed radio timer must have left a cue pending before the restart"
    );
    let objects_before = resident_objects(&mut app);
    assert!(!objects_before.is_empty(), "the harbor world has objects");
    let bindings_before = binding_count(&mut app);
    assert_eq!(bindings_before, 1, "one announced item was delivered");

    // The trigger: the playtest's own meta reset (`R`), observed by the
    // composed restart through `PlaytestState::resets`.
    app.world_mut().resource_mut::<PlaytestRequests>().reset = true;
    app.update();

    let report = restart_report_of(&app, "the restarted composition").clone();
    assert_eq!(
        report.torn_down, old_generation,
        "the teardown report must name the generation it tore down"
    );
    assert!(
        report.started.0 > report.torn_down.0,
        "SessionGeneration must strictly advance on restart, got {:?} then {:?}",
        report.torn_down,
        report.started
    );
    assert_ne!(
        report.served, old_served,
        "SessionId must advance with the generation, never be reused"
    );
    assert_eq!(report.objectives.session, old_generation);
    assert_eq!(
        report.objectives.cues.len(),
        pending_before,
        "the teardown must name every cue that will now never play, got {:?}",
        report.objectives.cues
    );
    assert_eq!(
        report.markers.served, report.served,
        "the marker ledger was released onto the new session id"
    );
    assert!(
        !report.world_despawned.is_empty(),
        "the unload must name the world objects it despawned"
    );
    let (mut despawned, mut respawned) = (
        report.world_despawned.clone(),
        report.world_respawned.clone(),
    );
    despawned.sort();
    respawned.sort();
    assert_eq!(
        despawned, respawned,
        "the reload must respawn exactly the objects the unload despawned"
    );

    // The rebuilt state, every record's own:
    {
        let host = app.world().resource::<MissionHost>();
        assert_eq!(host.generation(), report.started);
        assert_eq!(host.served(), report.served);
        assert!(
            host.last_tick().is_none(),
            "the host's tick record is back at zero: it has stepped nothing in the new generation"
        );
        assert_eq!(
            host.environment().clock().tick(),
            Tick(0),
            "the environment clock must be back at the authored tick zero"
        );
        assert_eq!(
            host.world_actors()
                .expect("the synthetic stage lowers a world-actor program")
                .tick(),
            Tick(0),
            "the world-actor session must be back at its initial tick"
        );
        assert_eq!(host.objectives().session(), host.generation());
        assert!(
            host.objectives().runtime().last_tick().is_none(),
            "the fresh objective runtime has stepped nothing"
        );
        assert_eq!(
            host.objectives().pending_cues(),
            0,
            "the previous generation's pending cue must not survive its teardown"
        );
        assert_eq!(host.markers().served(), host.served());
        assert!(
            host.markers().applied().is_empty(),
            "the marker ledger starts empty on the new session"
        );
        assert_eq!(
            host.script().state().terminal(),
            TerminalState::Running,
            "the fresh control session is Running again"
        );
        assert!(host.script().state().last_tick().is_none());
        assert_eq!(host.animation().served(), host.served());
        assert!(
            host.animation().advanced_through().is_none(),
            "the fresh record player has advanced through nothing"
        );
    }

    // The world half: resident again, the same authored objects, and no
    // leftover of the previous load — the restart re-runs no announced
    // load, so the composition's one binding is unchanged.
    assert_eq!(
        resident_objects(&mut app),
        objects_before,
        "the reload must respawn the same authored population"
    );
    assert_eq!(
        binding_count(&mut app),
        bindings_before,
        "a restart re-runs no announced load: the composition's one binding is neither doubled \
         nor left behind"
    );

    // The player body: exactly one, at the stage's start pose.
    assert_eq!(
        player_count(&mut app),
        1,
        "exactly one player body exists after the restart"
    );
    {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
        let player = query
            .iter(world)
            .next()
            .expect("the respawned player body exists");
        let position = world.get::<Position>(player).expect("it has a position");
        for axis in 0..3 {
            assert!(
                (position.0.to_array()[axis] - pose.position[axis]).abs() < 2.0,
                "the respawned body must sit at the stage's start pose {:?}, got {:?}",
                pose.position,
                position.0.to_array()
            );
        }
        let rotation = world.get::<Rotation>(player).expect("it has a rotation");
        let expected = Quat::from_rotation_y(pose.heading);
        assert!(
            rotation.0.angle_between(expected) < 1.0e-2,
            "the respawned body must face the stage's start heading {}",
            pose.heading
        );
        assert!(
            world.get::<PlaytestOriginalFlight>(player).is_some(),
            "the respawned body flies the original-law record again"
        );
    }

    // The previous composed step's report described the torn-down
    // generation; it is gone until the next step writes this one's.
    assert!(
        app.world().get_resource::<MissionHostReport>().is_none(),
        "the stale composed-step report must not survive the restart"
    );

    // The fresh host steps again through the same composed entry, and the
    // settled program replays its authored terminal from zero.
    app.update();
    {
        let report = report_of(&app, "the restarted composition");
        let produced = report.answer.as_ref().unwrap_or_else(|error| {
            panic!("the fresh host must step cleanly after the restart: {error}");
        });
        assert_eq!(produced.script.terminal, TerminalState::Succeeded);
        assert!(report.tick.0 > old_step_tick.0);
    }

    // A second reset must advance both identities again — a restart that
    // reused a generation would fail instead (the third member proves the
    // refusal).
    app.world_mut().resource_mut::<PlaytestRequests>().reset = true;
    app.update();
    let second = restart_report_of(&app, "the twice-restarted composition").clone();
    assert_eq!(second.torn_down, report.started);
    assert!(
        second.started.0 > report.started.0,
        "the second restart must advance SessionGeneration again"
    );
    assert_ne!(
        second.served, report.served,
        "the second restart must advance SessionId again"
    );
    {
        let host = app.world().resource::<MissionHost>();
        assert_eq!(host.environment().clock().tick(), Tick(0));
        assert!(host.last_tick().is_none());
        assert_eq!(host.objectives().pending_cues(), 0);
    }
    assert_eq!(player_count(&mut app), 1);

    teardown(&mut app);
}

/// **A restart requested after a settled terminal clears it: the fresh
/// control session is Running again before its next step and settles its
/// authored terminal again after it.**
#[test]
fn accept_vs_m01_runtime_host_03_a_requested_restart_clears_the_settled_terminal() {
    let scratch = Scratch::new("host-restart-terminal");
    let stage = restart_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");

    // Drive until the control program's own Finish directive settles the
    // terminal through the composed entry.
    let mut settled = None;
    for _ in 0..8 {
        app.update();
        let Ok(produced) = report_of(&app, "the settling composition").answer.as_ref() else {
            continue;
        };
        if produced.script.terminal != TerminalState::Running {
            settled = Some(produced.script.terminal);
            break;
        }
    }
    assert_eq!(
        settled,
        Some(TerminalState::Succeeded),
        "the synthetic program's own Finish must settle a terminal through the composed entry"
    );
    let old_generation = app.world().resource::<MissionHost>().generation();

    // The trigger: a restart requested after the terminal — the seam a
    // terminal handler latches through.
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();

    assert!(
        app.world()
            .get_resource::<MissionHostRestartRequest>()
            .is_none(),
        "the composed restart consumes the request exactly once"
    );
    let report = restart_report_of(&app, "the restarted composition").clone();
    assert_eq!(report.torn_down, old_generation);
    {
        let host = app.world().resource::<MissionHost>();
        assert_eq!(
            host.script().state().terminal(),
            TerminalState::Running,
            "the previous generation's terminal must be cleared with the session that carried it"
        );
        assert!(host.script().state().last_tick().is_none());
        assert!(host.last_tick().is_none());
        assert_eq!(host.environment().clock().tick(), Tick(0));
    }
    assert!(
        app.world().get_resource::<MissionHostReport>().is_none(),
        "the terminal generation's composed-step report must not survive the restart"
    );

    // The fresh session runs its authored program again from zero and
    // settles the very same terminal — proof the rebuild is the authored
    // initial state, not an empty one.
    app.update();
    {
        let report = report_of(&app, "the restarted composition");
        let produced = report.answer.as_ref().unwrap_or_else(|error| {
            panic!("the fresh host must step cleanly after the restart: {error}");
        });
        assert_eq!(produced.script.terminal, TerminalState::Succeeded);
    }

    teardown(&mut app);
}

/// **Asked for the live generation, the restart fails by name and rebuilds
/// nothing.**
#[test]
fn accept_vs_m01_runtime_host_03_a_restart_that_reuses_the_live_generation_fails() {
    let scratch = Scratch::new("host-restart-same-gen");
    let stage = restart_stage(scratch.path());
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    app.update();
    app.update();

    let (generation, served, environment_tick, stepped) = {
        let host = app.world().resource::<MissionHost>();
        (
            host.generation(),
            host.served(),
            host.environment().clock().tick(),
            host.last_tick(),
        )
    };
    assert!(environment_tick.0 > 0, "the session has run");

    let mut host = app
        .world_mut()
        .remove_resource::<MissionHost>()
        .expect("the composition owns its host");
    let stage = app.world().resource::<MissionStage>().clone();
    let error = host
        .restart(app.world_mut(), &stage, generation)
        .expect_err("a restart asked for the live generation must fail, never rebuild in place");
    assert!(
        matches!(
            error,
            MissionHostRestartError::SameGeneration { session } if session == generation
        ),
        "the refusal must name the live generation, got: {error}"
    );

    // Nothing was rebuilt: the live sessions keep serving the live
    // generation, its clock and its tick record.
    assert_eq!(host.generation(), generation);
    assert_eq!(host.served(), served);
    assert_eq!(host.environment().clock().tick(), environment_tick);
    assert_eq!(host.last_tick(), stepped);
    app.world_mut().insert_resource(host);

    // The composition keeps running on the untouched host.
    app.update();
    let report = report_of(&app, "the untouched composition");
    assert!(
        report.answer.is_ok(),
        "the live host must keep stepping after the refused restart"
    );

    teardown(&mut app);
}

/// **Against the owner's installation, M01's real composition restarts into
/// the same authored initial state with no leftover entity, cue or session
/// id.**
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_vs_m01_runtime_host_03_retail_m01_restart_rebuilds_its_authored_initial_state() {
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
    let pose = stage.start;
    let mission_dir = plan.mission_dir.clone();
    drop(plan);

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();
    app.update();

    let (old_generation, old_served) = {
        let host = app.world().resource::<MissionHost>();
        assert!(
            host.environment().clock().tick().0 > 0,
            "M01's environment clock must have moved before the restart"
        );
        assert!(
            host.world_actors()
                .expect("M01's scope lowers a world-actor program")
                .tick()
                .0
                > 0,
            "M01's world-actor session must have moved before the restart"
        );
        (host.generation(), host.served())
    };
    let old_step_tick = report_of(&app, "M01's composition").tick;
    assert!(old_step_tick.0 > 0, "the composed entry must have stepped");
    let objects_before = resident_objects(&mut app);
    assert!(!objects_before.is_empty(), "M01's world has objects");
    let bindings_before = binding_count(&mut app);
    assert_eq!(bindings_before, 1, "one announced item was delivered");

    // The trigger: the composed request seam (the window's terminal handler
    // latches the same resource once .02 wires it).
    app.world_mut().insert_resource(MissionHostRestartRequest);
    app.update();

    let report = restart_report_of(&app, "M01's restarted composition").clone();
    assert_eq!(
        report.torn_down, old_generation,
        "the teardown report must name M01's torn-down generation"
    );
    assert!(report.started.0 > report.torn_down.0);
    assert_ne!(report.served, old_served);
    assert_eq!(report.objectives.session, old_generation);
    assert!(
        report.objectives.cues.is_empty(),
        "no original mission yields a declared objective program today, so no cue was ever \
         pending — the report names that by being empty, never by inventing one"
    );
    assert!(!report.world_despawned.is_empty());
    let (mut despawned, mut respawned) = (
        report.world_despawned.clone(),
        report.world_respawned.clone(),
    );
    despawned.sort();
    respawned.sort();
    assert_eq!(despawned, respawned);

    {
        let host = app.world().resource::<MissionHost>();
        assert_eq!(host.generation(), report.started);
        assert_eq!(host.served(), report.served);
        assert!(host.last_tick().is_none());
        assert_eq!(
            host.environment().clock().tick(),
            Tick(0),
            "M01's environment clock must be back at the authored tick zero"
        );
        assert_eq!(
            host.world_actors()
                .expect("M01's scope lowers a world-actor program")
                .tick(),
            Tick(0),
            "M01's world-actor session must be back at its initial tick"
        );
        assert_eq!(host.objectives().session(), host.generation());
        assert!(host.objectives().runtime().last_tick().is_none());
        assert!(
            host.objectives().program().objectives.is_empty(),
            "a restart may never hand an original mission synthesized objectives"
        );
        assert_eq!(host.markers().served(), host.served());
        assert!(host.markers().applied().is_empty());
        assert_eq!(host.script().state().terminal(), TerminalState::Running);
        assert!(host.script().state().last_tick().is_none());
        assert_eq!(host.animation().served(), host.served());
        assert_eq!(host.animation().advanced_through(), None);
        assert!(
            host.animation().running_count() + host.animation().finished_count() > 0,
            "M01's join startup rows must be offered to the fresh record player too: {} running, \
             {} finished, {} refused",
            host.animation().running_count(),
            host.animation().finished_count(),
            host.animation().refused_count()
        );
        assert!(
            host.refusals()
                .iter()
                .any(|refusal| matches!(refusal, MissionHostRefusal::ObjectiveDeclarations { .. })),
            "the restart must report the F39 absence the launch reports"
        );
        assert!(
            host.refusals()
                .iter()
                .any(|refusal| matches!(refusal, MissionHostRefusal::WorldObservation { .. })),
            "the restart must report the unobserved world fact fold"
        );
        assert!(
            host.refusals()
                .iter()
                .any(|refusal| matches!(refusal, MissionHostRefusal::BlockLifecycles { .. })),
            "the restart must report the undeclared block lifecycles"
        );
    }

    assert_eq!(
        resident_objects(&mut app),
        objects_before,
        "M01's reload must respawn the same authored population"
    );
    assert_eq!(
        binding_count(&mut app),
        bindings_before,
        "a restart re-runs no announced load: M01's one binding is neither doubled nor left behind"
    );

    // Exactly one player body, at M01's measured start pose.
    assert_eq!(player_count(&mut app), 1);
    {
        let world = app.world_mut();
        let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
        let player = query
            .iter(world)
            .next()
            .expect("M01's respawned player body exists");
        let position = world
            .get::<Position>(player)
            .expect("the body has a position");
        for axis in 0..3 {
            assert!(
                (position.0.to_array()[axis] - pose.position[axis]).abs() < 2.0,
                "M01's respawned body must sit at the measured start pose {:?}, got {:?} \
                 ({mission_dir})",
                pose.position,
                position.0.to_array()
            );
        }
        let rotation = world
            .get::<Rotation>(player)
            .expect("the body has a rotation");
        let expected = Quat::from_rotation_y(pose.heading);
        assert!(
            rotation.0.angle_between(expected) < 1.0e-2,
            "M01's respawned body must face the measured start heading {}",
            pose.heading
        );
        assert!(
            world.get::<PlaytestOriginalFlight>(player).is_some(),
            "M01's respawned body flies the pdevastator law again"
        );
    }

    // The composed entry keeps stepping the fresh host: M01 still cannot
    // reach a terminal headlessly — a measured property of the content, not
    // of the restart.
    app.update();
    {
        let report = report_of(&app, "M01's restarted composition");
        let produced = report.answer.as_ref().unwrap_or_else(|error| {
            panic!("M01's fresh host must step cleanly after the restart: {error}");
        });
        assert_eq!(
            produced.script.terminal,
            TerminalState::Running,
            "M01 cannot reach a terminal state headlessly; a terminal here would mean a block \
             woke without the facts that wake it"
        );
    }

    teardown(&mut app);
    assert!(
        residency(app.world()).is_none(),
        "teardown must leave no WorldResidency"
    );
    let (players, flights, bound) = {
        let world = app.world_mut();
        let mut players = world.query_filtered::<Entity, With<MissionPlayerBody>>();
        let players = players.iter(world).count();
        let mut flights = world.query_filtered::<Entity, With<PlaytestOriginalFlight>>();
        let flights = flights.iter(world).count();
        let mut bound = world.query_filtered::<Entity, With<cs_app::loading::LoadedItemBinding>>();
        let bound = bound.iter(world).count();
        (players, flights, bound)
    };
    assert_eq!(
        (players, flights, bound),
        (0, 0, 0),
        "teardown must leave nothing of {mission_dir} behind"
    );
}

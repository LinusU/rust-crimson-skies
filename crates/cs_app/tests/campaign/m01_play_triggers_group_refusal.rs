//! Acceptance suite M01-PLAY-TRIGGERS-GROUP-REFUSAL (#1512): a `DEDG` gate
//! can never complete on a group whose enabled records did not all spawn.
//!
//! The original's group counter `0x4658d0` counts vehicle-registry members
//! and every *enabled* `aiv.zrd` record enters that registry at spawn
//! (docs/findings/2026-10-15-m01-play-triggers-group-membership.md). So a
//! group whose records are all enabled is only *observable* when every such
//! record spawned: one the composition refused leaves the registry count
//! unknown, and a count of the fraction that did spawn is an undercount —
//! the same fail-open family as the invented zero #1483 closed for
//! skipped-only groups. This suite pins all three measured rules:
//!
//! * **A mixed group stays unobserved.** One spawning enabled record plus
//!   one refused enabled record of the same group leaves it unobserved and
//!   its `DEDG` gate never completes — CI leg.
//! * **The other rules are unchanged.** A skipped-only group still counts
//!   `0` and completes; a fully spawned group reports its live count and
//!   still depletes when its members are removed — CI leg.
//! * **Retail M01 is unaffected.** Every enabled record of M01 spawns, so
//!   all four of its measured groups are observed exactly as before
//!   (`#[ignore = "requires CS_GAME_DIR"]`).
//!
//! Nothing here is `verified_original`: the registry-count rule is measured
//! from the executable's own counter; no original run exists.

use std::collections::BTreeMap;

use bevy::prelude::{App, Entity, FixedPostUpdate, IntoScheduleConfigs, Resource, World};
use cs_app::mission_actors::ScriptedAircraftActor;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostReport, MissionStage, build_headless, mission_host_tick, stage_for,
    teardown,
};
use cs_app::mission_start::{ScriptedAircraft, StoredStartPose, spawn_dispatch};
use cs_script::ir::{Condition, IR_VERSION, MissionProgram, Objective, SymbolId};
use cs_types::content::Provenance;
use cs_types::evidence::ClaimId;

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// Every block index that completed, across all host ticks.
#[derive(Resource, Default)]
struct Completed(Vec<u32>);

fn record_completions(world: &mut World) {
    let Some(report) = world.get_resource::<MissionHostReport>() else {
        return;
    };
    let Ok(answer) = &report.answer else {
        return;
    };
    let done = answer.lifecycles.completed.clone();
    world.resource_mut::<Completed>().0.extend(done);
}

fn arm(app: &mut App) {
    app.insert_resource(Completed::default());
    app.add_systems(FixedPostUpdate, record_completions.after(mission_host_tick));
}

fn completed(app: &App, block: u32) -> bool {
    app.world().resource::<Completed>().0.contains(&block)
}

fn observed_groups(app: &App) -> BTreeMap<i32, u32> {
    app.world()
        .resource::<MissionHost>()
        .world_facts()
        .observation()
        .groups()
        .clone()
}

fn provenance(name: &str) -> Provenance {
    Provenance::designed(
        ClaimId::new(&format!("test.m01-play-triggers-group-refusal.{name}"))
            .expect("the claim id is valid"),
    )
}

/// An **enabled** record the composition spawns: the measured stored pose
/// every M01 record carries, the `enabled` flag set, asleep so no airframe
/// table is needed.
fn spawnable(name: &str, group: u32) -> ScriptedAircraft {
    ScriptedAircraft {
        name: name.to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: Some(StoredStartPose {
            position: [40.0, 20.0, -60.0],
            heading: 0.0,
        }),
        team: Some(2),
        group: Some(group),
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: Some(true),
        dispatch: spawn_dispatch(name),
        provenance: provenance(name),
    }
}

/// An **enabled** record the composition refuses to spawn: it carries no
/// stored pose, so [`cs_app::mission_actors::spawn_mission_actors`] records a
/// refusal instead of an entity. The original's registry would have held it.
fn refused(name: &str, group: u32) -> ScriptedAircraft {
    let mut record = spawnable(name, group);
    record.stored_pose = None;
    record
}

/// A record the original's own spawn loop skips (`enabled == 0`): it never
/// enters the vehicle registry, so a group whose only records are these
/// counts `0` (#1483's measured answer).
fn skipped(name: &str, group: u32) -> ScriptedAircraft {
    let mut record = spawnable(name, group);
    record.enabled = Some(false);
    record
}

/// One `DEDG [group, remaining]` objective per `(group, remaining)` pair.
///
/// Group 5's `remaining` is `1`, not `0`: its true registry count is 2, so
/// the original's evaluator (`living <= remaining`) never fires — but the
/// undercount of 1 this task removes satisfies `1 <= 1` and completes the
/// gate immediately. It is the sharpest pin of the acceptance criterion:
/// under the old rule the gate completes, under this rule the group is
/// unobserved and it never can.
fn depletion_program(gates: &[(i32, i32)]) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(
            cs_types::content::ContentKind::Mission,
            "synthetic-m01-group-refusal",
        ),
        variables: Vec::new(),
        objectives: gates
            .iter()
            .enumerate()
            .map(|(index, (group, remaining))| Objective {
                id: SymbolId(index as u32),
                content: cid(
                    cs_types::content::ContentKind::Objective,
                    &format!("synthetic-m01-group-refusal.objective.{}", index + 1),
                ),
                condition: Condition::EnemyGroupDepletion {
                    group: *group,
                    remaining: *remaining,
                    generator: None,
                },
                actions: Vec::new(),
                span: None,
            })
            .collect(),
    }
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// **A group whose enabled records only partially spawned is unobserved —
/// no count, no `DEDG` completion — while a skipped-only group still counts
/// `0` and a fully spawned group still counts its live members.**
///
/// The synthetic stage carries three groups: `5` mixes one record that
/// spawns with one the composition refuses (no stored pose — the refusal
/// family the task names); `6` is a single `enabled == 0` skip; `7` holds
/// two records that both spawn. After the run settles:
#[test]
fn accept_m01_play_triggers_group_refusal_a_partial_spawn_stays_unobserved_and_never_completes() {
    let scratch = Scratch::new("m01-play-group-refusal");
    let mut stage = synthetic_stage(scratch.path());
    stage.aircraft = vec![
        spawnable("rusdevastator_5a", 5),
        refused("bsfury_5b", 5),
        skipped("bsfury_6", 6),
        spawnable("rusdevastator_7a", 7),
        spawnable("rusdevastator_7b", 7),
    ];
    stage.host.control = depletion_program(&[(5, 1), (6, 0), (7, 0)]);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    arm(&mut app);
    for _ in 0..30 {
        app.update();
    }

    let groups = observed_groups(&app);
    assert!(
        !groups.contains_key(&5),
        "one refused enabled record leaves group 5's true count unknown — unobserved, never an \
         undercount of 1: {groups:?}"
    );
    assert_eq!(
        groups.get(&6),
        Some(&0),
        "the skipped-only group still counts 0: {groups:?}"
    );
    assert_eq!(
        groups.get(&7),
        Some(&2),
        "the fully spawned group counts its two live members: {groups:?}"
    );

    // Objectives are ordered [5, 6, 7]: index 0 must never complete — its
    // `remaining: 1` is exactly what the undercount of 1 would satisfy —
    // index 1 completes on the skip's 0, index 2 waits on two live members.
    assert!(
        !completed(&app, 0),
        "the mixed group's DEDG [5, 1] gate must never complete on the undercount of 1"
    );
    assert!(
        completed(&app, 1),
        "the skipped-only group's gate completes on 0"
    );
    assert!(
        !completed(&app, 2),
        "the live group's gate waits while its members fly"
    );

    // Removing both spawned members through the world still depletes the
    // fully observed group — the count is real, not latched.
    let members: Vec<Entity> = {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
        query
            .iter(world)
            .filter(|(_, actor)| actor.record.group == Some(7))
            .map(|(entity, _)| entity)
            .collect()
    };
    assert_eq!(members.len(), 2, "group 7 spawned both of its records");
    for entity in members {
        app.world_mut().despawn(entity);
    }
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(
        observed_groups(&app).get(&7),
        Some(&0),
        "the fully spawned group's count follows the world to 0"
    );
    assert!(completed(&app, 2), "the emptied group's gate completes");
    assert!(
        !completed(&app, 0),
        "the mixed group stays unobserved and its gate never completes"
    );

    teardown(&mut app);
}

/// **On retail M01 every enabled record spawns, so the new rule changes
/// nothing: all four measured groups are observed exactly as before.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_group_refusal_retail_m01s_groups_are_all_observed() {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let groups = observed_groups(&app);
    assert_eq!(
        groups,
        BTreeMap::from([(1, 4), (2, 0), (3, 1), (4, 0), (5, 4)]),
        "M01's enabled records all spawn, so every group is observed — groups 2 and 4 on the \
         measured skip-zero, 1, 3 and the player record's group 5 on live counts: {groups:?}"
    );

    teardown(&mut app);
}

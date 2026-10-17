//! Acceptance suite M01-PLAY-TRIGGERS-DEPLETION (#1483): every group id M01's
//! eight `DEDG` sites name (1, 2, 3, 4) has a measured live count.
//!
//! Measured (docs/findings/2026-10-15-m01-play-triggers-group-membership.md):
//! the original's group counter `0x4658d0` counts vehicle-registry members
//! whose group field (`+0x388`) matches. Groups 2 and 4 have exactly one
//! `aiv.zrd` record each and both are the `enabled == 0` records the spawn
//! loop skips (`0x474d10`), so the registry holds no member of them and the
//! counter answers 0 — the block completes without any invented content.
//! Nothing here is `verified_original`.

use std::collections::BTreeMap;

use bevy::prelude::{App, Entity, FixedPostUpdate, IntoScheduleConfigs, Resource, World};
use cs_app::mission_actors::{MissionSkippedAircraft, ScriptedAircraftActor};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionHost, MissionHostReport, MissionStage, build_headless, mission_host_tick, stage_for,
    teardown,
};
use cs_app::mission_start::{ScriptedAircraft, spawn_dispatch};
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

fn skipped(name: &str, group: u32) -> ScriptedAircraft {
    ScriptedAircraft {
        name: name.to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: None,
        team: None,
        group: Some(group),
        enabled: Some(false),
        primary_target: None,
        title: None,
        starts_asleep: None,
        dispatch: spawn_dispatch(name),
        provenance: Provenance::designed(
            ClaimId::new("test.m01-play-triggers-depletion.record").expect("claim id"),
        ),
    }
}

/// An **enabled** record the composition refuses to spawn: it carries no
/// stored pose, so [`cs_app::mission_actors::spawn_mission_actors`] records a
/// refusal instead of an entity. The original's registry would have held it,
/// so its group's count is unknown here — never a skip's `0`.
fn unspawnable(name: &str, group: u32) -> ScriptedAircraft {
    let mut record = skipped(name, group);
    record.enabled = Some(true);
    record
}

/// One `DEDG [group, 0]` objective per group.
fn depletion_program(groups: &[i32]) -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(
            cs_types::content::ContentKind::Mission,
            "synthetic-m01-depletion",
        ),
        variables: Vec::new(),
        objectives: groups
            .iter()
            .enumerate()
            .map(|(index, group)| Objective {
                id: SymbolId(index as u32),
                content: cid(
                    cs_types::content::ContentKind::Objective,
                    &format!("synthetic-m01-depletion.objective.{}", index + 1),
                ),
                condition: Condition::EnemyGroupDepletion {
                    group: *group,
                    remaining: 0,
                    generator: None,
                },
                actions: Vec::new(),
                span: None,
            })
            .collect(),
    }
}

/// **A group whose only records the spawn loop skips counts 0 and completes
/// its gate; a group no record carries stays unobserved and never does — CI
/// leg.**
#[test]
fn accept_m01_play_triggers_depletion_skipped_records_count_zero_and_unknown_groups_stay_unobserved()
 {
    let scratch = Scratch::new("m01-play-depletion");
    let mut stage = synthetic_stage(scratch.path());
    stage.aircraft = vec![skipped("rusdevastator_3", 2), skipped("bsfury_2", 4)];
    stage.host.control = depletion_program(&[2, 4, 7]);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    arm(&mut app);
    assert_eq!(
        app.world().resource::<MissionSkippedAircraft>().0.len(),
        2,
        "the composition keeps the two skipped records"
    );
    for _ in 0..30 {
        app.update();
    }

    let groups = observed_groups(&app);
    assert_eq!(groups.get(&2), Some(&0), "group 2 counts 0: {groups:?}");
    assert_eq!(groups.get(&4), Some(&0), "group 4 counts 0: {groups:?}");
    assert!(
        !groups.contains_key(&7),
        "a group no record carries is unobserved, never 0: {groups:?}"
    );
    assert!(completed(&app, 0), "the group-2 gate completes on its 0");
    assert!(completed(&app, 1), "the group-4 gate completes on its 0");
    assert!(
        !completed(&app, 2),
        "the wrong (unobserved) group never satisfies a gate"
    );

    teardown(&mut app);
}

/// **A skip's zero is never borrowed by a group an enabled record also
/// carries: a refused spawn leaves that group unknown — CI leg.**
///
/// Group 6 carries two records: one the original's spawn loop skips
/// (`enabled == 0`) and one *enabled* record this composition refuses to
/// spawn (no stored pose, so the spawn path records a refusal). The original's
/// registry would have held the enabled record, so the group's count is not
/// the skip's `0` — it is unknown, and the `DEDG [6, 0]` gate must never
/// complete on it.
#[test]
fn accept_m01_play_triggers_depletion_a_refused_enabled_record_keeps_its_group_unknown() {
    let scratch = Scratch::new("m01-play-depletion-mixed");
    let mut stage = synthetic_stage(scratch.path());
    stage.aircraft = vec![
        skipped("rusdevastator_3", 2),
        unspawnable("rusdevastator_6", 6),
        skipped("bsfury_6", 6),
    ];
    stage.host.control = depletion_program(&[2, 6]);
    let mut app = build_headless(&stage).expect("the synthetic stage composes");
    arm(&mut app);
    for _ in 0..30 {
        app.update();
    }

    let groups = observed_groups(&app);
    assert_eq!(groups.get(&2), Some(&0), "group 2 counts 0: {groups:?}");
    assert!(
        !groups.contains_key(&6),
        "a group whose only spawned-capable record was refused stays unobserved, never the \
         skip's 0: {groups:?}"
    );
    assert!(completed(&app, 0), "the group-2 gate completes on its 0");
    assert!(
        !completed(&app, 1),
        "the mixed group's gate never completes on an unknown count"
    );

    teardown(&mut app);
}

/// **Every DEDG group of retail M01 has a measured count, and removing a
/// spawned member through the world drives its count down — retail leg.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_depletion_all_four_m01_groups_are_observed() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage: MissionStage = stage_for(&root, &plan).expect("M01's stage reads");
    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    arm(&mut app);
    app.update();

    let groups = observed_groups(&app);
    assert_eq!(groups.get(&1), Some(&4), "{groups:?}");
    assert_eq!(groups.get(&2), Some(&0), "{groups:?}");
    assert_eq!(groups.get(&3), Some(&1), "{groups:?}");
    assert_eq!(groups.get(&4), Some(&0), "{groups:?}");

    // Group 3's one member (bsfury_1): remove it and the count follows.
    let member: Vec<Entity> = {
        let world = app.world_mut();
        let mut query = world.query::<(Entity, &ScriptedAircraftActor)>();
        query
            .iter(world)
            .filter(|(_, actor)| actor.record.group == Some(3))
            .map(|(entity, _)| entity)
            .collect()
    };
    assert_eq!(member.len(), 1, "group 3 has exactly bsfury_1");
    for entity in member {
        app.world_mut().despawn(entity);
    }
    for _ in 0..30 {
        app.update();
    }
    assert_eq!(observed_groups(&app).get(&3), Some(&0));
    let host = app.world().resource::<MissionHost>();
    let facts = host.world_facts().facts(host.world_operands());
    assert_eq!(
        facts.groups.get(&3),
        Some(&0),
        "the fold the DEDG evaluator reads carries the emptied group"
    );

    teardown(&mut app);
}

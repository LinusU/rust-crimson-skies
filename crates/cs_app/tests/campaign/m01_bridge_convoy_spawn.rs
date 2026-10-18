//! Acceptance suite M01-BRIDGE-CONVOY-SPAWN (Rally #1566): M01's bridge convoy,
//! camps and wreck are bound as damageable targets of the composition.
//!
//! `docs/findings/2026-10-17-m01-ground-units-locate.md` (#1549) measured where
//! M01 "The Lost Treasure" keeps its convoy, its camps and its wreck, and
//! nothing bound them: the world import makes an object only for a record the
//! world node owns directly, so a bridge child (`t_truck01`, `bridge_truck01`)
//! and a town child (`u_camp1`) had no entity, and no consumer read the
//! mission's `targets.zrd` or the world group's authored damage coupling. This
//! suite pins the wiring that closes that:
//!
//! 1. **Retail — the mission declares its targets.** `read_mission_targets`
//!    reads `ZBD/C3/M01/zrdr.zbd`'s `targets.zrd` and every declared node
//!    resolves to a container record at its composed world position.
//! 2. **Retail — the world group's damage set is read.** `read_damage_couplings`
//!    reads the group's `zrdr.zbd` members and states which nodes the authored
//!    `HEALTH`/`DAMAGE_SEQUENCE`/`PROXIMITY_DAMAGE` couple.
//! 3. **Retail — a composed M01 carries them as damageable actors.** Every
//!    bound target's class is read back from `SessionDamageResolver`'s
//!    registered graph, never from the record that planned it.
//! 4. **Synthetic (CI) — the row reader and the damage node.** A hand-authored
//!    `targets.zrd` document reads into the measured pair shape, and a bound
//!    target's damage node is the labelled designed one, so a reader or a node
//!    that stopped consuming production code fails with no installation.
//!
//! Nothing here is `verified_original`: the pools are the designed single unit
//! of `mission_targets::DESIGNED_TARGET_INTEGRITY`, the coupling's meaning is
//! explicitly unmeasured (`f20-anim.damage-coupling-unmeasured`), and no
//! original executable ran.

use std::path::PathBuf;

use bevy::prelude::{App, Entity, With};
use cs_app::mission_actors::{
    MissionActorSpawnReport, MissionGroundTarget, MissionGroundTargetReport,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionStage, build_headless, stage_for, teardown};
use cs_app::mission_targets::{
    BoundTarget, DESIGNED_TARGET_INTEGRITY, DamageCoupling, MissionTargetBinding, TARGETS_MEMBER,
    bind_mission_targets, read_damage_couplings, read_mission_targets, read_target_rows,
    target_damage_node,
};
use cs_app::weapons::SessionDamageResolver;
use cs_content::stunts::ZrdValue;
use cs_sim::damage::{DamageNodeKey, DamageNodeKind};
use cs_types::content::Resolved;
use cs_types::evidence::ClaimStatus;

use crate::campaign::common::label;

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The production stage for retail M01 (`zbd/c3/m01`, #1567).
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// One node key from a node's spelling.
fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a damage node key")
}

/// The registered class of one live ground target's damage node, read back
/// from the session damage authority the composition installs.
fn live_node(app: &App, node: &str) -> (DamageNodeKind, bool) {
    let report = app
        .world()
        .get_resource::<MissionGroundTargetReport>()
        .expect("the ground-target report is installed");
    let bound = report
        .target(node)
        .unwrap_or_else(|| panic!("the composition spawned a ground target for {node}"));
    let resolver = app.world().resource::<SessionDamageResolver>().resolver();
    let graph = resolver
        .graph(&bound.actor)
        .unwrap_or_else(|| panic!("{node}'s damage actor is registered"));
    let registered = graph
        .node(&key(node))
        .unwrap_or_else(|| panic!("{node} is a node of its own actor's graph"));
    (registered.kind(), registered.is_lethal())
}

/// The entity one bound target spawned on.
fn component_entity(app: &mut App, node: &str) -> Entity {
    let mut query = app
        .world_mut()
        .query_filtered::<(Entity, &MissionGroundTarget), With<MissionGroundTarget>>();
    query
        .iter(app.world())
        .find(|(_, target)| target.node == node)
        .map(|(entity, _)| entity)
        .unwrap_or_else(|| panic!("{node} has an entity"))
}

/// **The mission's `targets.zrd` declares the convoy, the wreck and the rest,
/// and every declared node resolves at its composed world position.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_bridge_convoy_spawn_declared_targets_resolve() {
    let root = game_dir();
    let table = read_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    assert_eq!(
        table.carrier.as_deref(),
        Some("zbd/c3/m01/zrdr.zbd"),
        "the mission's own archive carries {TARGETS_MEMBER}"
    );
    assert_eq!(table.refusal, None, "the member decodes");
    assert_eq!(table.rows.len(), 11, "the member's own record count");
    let convoy: Vec<&str> = table
        .rows
        .iter()
        .filter(|row| row.description.as_deref() == Some("MSG_OBJ_BRITCONVOY"))
        .flat_map(|row| row.chains.iter().flatten().map(String::as_str))
        .collect();
    assert_eq!(
        convoy,
        vec!["t_truck01", "t_truck02", "t_truck03", "bridge_truck01"],
        "the four British-convoy rows, in stored order"
    );
    assert!(
        table
            .rows
            .iter()
            .any(|row| row.chains == vec![vec!["shipwreck".to_owned()]]),
        "the wreck is a declared objective site"
    );

    let binding = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    assert_eq!(
        binding.declared,
        (0..11).collect::<Vec<_>>(),
        "every declared row bound at least one node"
    );
    for (node, expected) in [
        ("t_truck01", [-5459.328, 118.62919, -5339.184]),
        ("t_truck02", [-5461.536, 118.62919, -5375.9116]),
        ("t_truck03", [-5461.384, 118.62919, -5391.7837]),
        ("bridge_truck01", [-5458.0996, 118.40865, -5366.4116]),
        ("shipwreck", [-8891.403, 5.0, -4685.451]),
    ] {
        let target = binding
            .target(node)
            .unwrap_or_else(|| panic!("{node} is bound"));
        for (axis, want) in target.position_m.iter().zip(expected) {
            assert!(
                (axis - want).abs() < 1.0e-3,
                "{node}'s composed position is {want}, found {axis}"
            );
        }
        assert_eq!(
            target.position_provenance().class,
            ClaimStatus::ObservedTool,
            "{node}'s position is an installation read"
        );
    }
    // The camp rows are not declared by the mission; the world group's damage
    // set is what binds them.
    assert!(binding.target("u_camp1").is_some(), "u_camp1 is bound");
    // The census: 11 declared nodes plus the world group's damage set
    // (airdock2, leasure1..5, craterlake, hydrogentank1..4, susp_bridge,
    // g_tower1..7, trucker1..5, u_camp1..3/5..8, unit01..42, shipwreck), with
    // the three nodes both halves name counted once.
    assert_eq!(
        binding.targets.len(),
        69,
        "the binding's own census: declared targets plus the world damage set"
    );
    assert_eq!(
        binding.declared_targets().count(),
        11,
        "every declared row binds at least one node"
    );
}

/// **The world group's damage set states the authored coupling for the convoy,
/// the camps and the wreck, and the gun trucks carry the destroy layout.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_bridge_convoy_spawn_damage_coupling_is_read() {
    let root = game_dir();
    let couplings = read_damage_couplings(&root, "zbd/c3");
    assert_eq!(couplings.carrier.as_deref(), Some("zbd/c3/zrdr.zbd"));
    assert_eq!(couplings.refusal, None, "every damage member decodes");
    assert!(
        couplings.definitions.len() >= 11,
        "the group's damage members, found {}",
        couplings.definitions.len()
    );

    let binding = bind_mission_targets(&root, "zbd/c3/m01", "zbd/c3");
    // The fuel truck's HEALTH comes from fueltruck.zrd; the camps' HEALTH,
    // DAMAGE_SEQUENCE and PROXIMITY_DAMAGE from ucamp_dest.zrd; the bridge's
    // HEALTH from susp_bridge.zrd.
    let fuel = binding
        .target("bridge_truck01")
        .expect("the fuel truck is bound");
    assert_eq!(fuel.coupling.fields, vec!["HEALTH".to_owned()]);
    assert_eq!(fuel.coupling.members, vec!["fueltruck.zrd".to_owned()]);

    let camp = binding.target("u_camp1").expect("u_camp1 is bound");
    assert_eq!(
        camp.coupling.fields,
        vec![
            "HEALTH".to_owned(),
            "DAMAGE_SEQUENCE".to_owned(),
            "PROXIMITY_DAMAGE".to_owned()
        ],
        "the camp's three authored coupling fields"
    );
    assert_eq!(camp.coupling.members, vec!["ucamp_dest.zrd".to_owned()]);
    assert_eq!(
        (camp.healthy.as_deref(), camp.destroyed.as_deref()),
        (Some("healthy"), Some("destroyed")),
        "the camp's authored healthy/destroyed layout"
    );

    let bridge = binding.target("susp_bridge").expect("the bridge is bound");
    assert_eq!(bridge.coupling.fields, vec!["HEALTH".to_owned()]);
    assert_eq!(bridge.coupling.members, vec!["susp_bridge.zrd".to_owned()]);

    for node in ["t_truck01", "t_truck02", "t_truck03"] {
        let truck = binding.target(node).expect("the gun truck is bound");
        assert!(
            truck.has_destroy_layout(),
            "{node} carries the authored healthy/destroyed pair"
        );
    }
    assert!(
        binding.refusals.is_empty(),
        "no selector is left unmeasured: {:?}",
        binding.refusals
    );
}

/// **A composed M01 registers every bound ground target as a live, lethal
/// internal-structure damage node.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_bridge_convoy_spawn_composed_targets_are_damageable() {
    let stage = retail_stage();
    assert!(
        !stage.host.targets.is_empty(),
        "the stage bound the mission's ground targets"
    );
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let session = app
        .world()
        .get_resource::<MissionActorSpawnReport>()
        .and_then(|report| report.session)
        .expect("the composition's spawn report names its session");
    let report = app
        .world()
        .get_resource::<MissionGroundTargetReport>()
        .expect("the ground-target report is installed")
        .clone();
    assert_eq!(
        report.session,
        Some(session),
        "the targets registered under the composition's own session"
    );
    for node in [
        "t_truck01",
        "t_truck02",
        "t_truck03",
        "bridge_truck01",
        "susp_bridge",
        "shipwreck",
        "u_camp1",
        "u_camp2",
        "u_camp3",
        "u_camp5",
        "u_camp6",
        "u_camp7",
        "u_camp8",
    ] {
        assert_eq!(
            live_node(&app, node),
            (DamageNodeKind::InternalStructure, true),
            "{node} is a live lethal internal-structure node"
        );
    }
    // The bound target's own entity carries the layout the container authored.
    let camp = report.target("u_camp1").expect("u_camp1 spawned");
    let entity = component_entity(&mut app, "u_camp1");
    let component = app
        .world()
        .entity(entity)
        .get::<MissionGroundTarget>()
        .expect("the entity carries the ground-target component");
    assert_eq!(component.healthy.as_deref(), Some("healthy"));
    assert_eq!(component.destroyed.as_deref(), Some("destroyed"));
    assert_eq!(component.damage_actor, camp.actor);
    assert!(
        component.declared.is_none(),
        "u_camp1 is not a declared row"
    );
    teardown(&mut app);
}

/// **A `targets.zrd` document reads into the measured pair shape, and a bound
/// target's damage node is the labelled designed one (CI, no installation).**
#[test]
fn accept_m01_bridge_convoy_spawn_rows_and_damage_node_are_production() {
    let document = ZrdValue::List(vec![ZrdValue::List(vec![
        ZrdValue::List(vec![
            ZrdValue::Text("description".to_owned()),
            ZrdValue::Text("MSG_OBJ_BRITCONVOY".to_owned()),
        ]),
        ZrdValue::List(vec![
            ZrdValue::Text("nodes".to_owned()),
            ZrdValue::List(vec![ZrdValue::Text("t_truck01".to_owned())]),
        ]),
        ZrdValue::List(vec![ZrdValue::Text("help_label".to_owned())]),
    ])]);
    let rows = read_target_rows(&document);
    assert_eq!(rows.len(), 1, "one record");
    let row = &rows[0];
    assert_eq!(row.description.as_deref(), Some("MSG_OBJ_BRITCONVOY"));
    assert_eq!(row.help_label, None, "a bare marker key carries no value");
    assert_eq!(row.chains, vec![vec!["t_truck01".to_owned()]]);
    assert!(!row.objective && !row.other_target, "no marker key present");
    assert_eq!(row.provenance().class, ClaimStatus::ObservedTool);

    let target = BoundTarget {
        declared: Some(0),
        chain: vec!["t_truck01".to_owned()],
        node: "t_truck01".to_owned(),
        index: 3029,
        path: "world1.susp_bridge.t_truck01".to_owned(),
        position_m: [-5459.328, 118.62919, -5339.184],
        healthy: Some("healthy".to_owned()),
        destroyed: Some("destroyed".to_owned()),
        coupling: DamageCoupling {
            fields: vec!["HEALTH".to_owned()],
            members: vec!["fueltruck.zrd".to_owned()],
        },
        objective: false,
        other_target: false,
    };
    assert!(target.has_destroy_layout());
    let node = target_damage_node(&target).expect("the designed node builds");
    assert_eq!(node.key().as_str(), "t_truck01");
    assert_eq!(node.kind(), DamageNodeKind::InternalStructure);
    assert!(node.is_lethal());
    match node.integrity() {
        Resolved::Known(known) => {
            assert_eq!(known.value, DESIGNED_TARGET_INTEGRITY);
            assert_eq!(
                known.provenance.class,
                ClaimStatus::Designed,
                "the pool is a labelled design, never an authored number"
            );
        }
        other => panic!("the designed pool is known, found {other:?}"),
    }
    // A binding with no targets is empty and reports nothing.
    assert!(MissionTargetBinding::default().is_empty());
}

//! Acceptance suite M01-PLAY-TRIGGERS-TURRET-WIRE (Rally #1499): the `ai.zrd`
//! `TURRET` table the stage reads is the table the **live** damage graph is
//! built from.
//!
//! #1491 measured what M01's nine `workersvoyagezep` turret nodes (`utur1`…
//! `utur4`, `ctur1`…`ctur4`, `maingun9`) are and gave `mission_session::parts`
//! the classifier for them (`classed_part_node`), but nothing in the
//! composition called it: `mission_actors::designed_actor_graph` built every
//! part node as unclassed internal structure, so a destroyed turret kept its
//! `Weapon` system firing and every F29 consumer of those nine nodes read a
//! class no record declares. This suite pins the wiring that closes that:
//!
//! 1. **Retail — the nine mounts are live.** `stage_for` reads the scope's own
//!    `ai.zrd` through the production reader (mission → group → shared mount
//!    order; on this installation the shared `zbd/zrdr.zbd` carries it), and a
//!    headless composition of M01 registers exactly those nine chains as
//!    [`DamageNodeKind::WeaponMount`] nodes whose destruction disables
//!    [`SystemKind::Weapon`] — the F29 edge, kept per node — while the 30
//!    engine chains stay [`DamageNodeKind::InternalStructure`] with no system
//!    edge.
//! 2. **Synthetic — the stage's table decides (CI).** The same composition
//!    over a synthetic stage whose table declares one of its two parts classes
//!    exactly that part and leaves the other alone, so a spawn host that
//!    ignored the stage's table fails here with no installation at all.
//! 3. **Synthetic — an unread table is reported (CI).** A stage whose table
//!    could not be read says why on the parts report instead of passing that
//!    silence off as a measured absence, and nothing is classed from it.
//!
//! Nothing here is `verified_original`: the pools stay the designed single
//! unit of `parts::DESIGNED_PART_INTEGRITY`, the `Weapon` edge is F29's own
//! AC03 vocabulary (`designed`, not measured), and no original executable ran.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use bevy::prelude::App;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionPartReport, MissionStage, TurretRecord, build_headless, stage_for, teardown,
};
use cs_app::weapons::SessionDamageResolver;
use cs_app::world_actors::LoweredWorldActors;
use cs_app::world_facts::MemberResolver;
use cs_script::ir::{
    Action, ActorId as WorldActorId, Condition, IR_VERSION, MemberName, MissionProgram, Objective,
    Outcome, SymbolId,
};
use cs_sim::damage::{DamageNodeKey, DamageNodeKind, SystemKind};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_sim::world_actors::runtime::{ActorMotion, WorldActorKind, WorldActorSpec};
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The nine chains the shared `TURRET` table declares under
/// `workersvoyagezep` (#1491): records 31, 28 and 27.
const MOUNTS: [&str; 9] = [
    "utur1", "utur2", "utur3", "utur4", "ctur1", "ctur2", "ctur3", "ctur4", "maingun9",
];

/// The mount actor every `MOUNTS` entry resolved to — the `NODES` mount
/// pattern that matched it.
const MOUNT_ACTOR: &str = "workersvoyagezep";

/// The two part chains the synthetic stage binds.
const SYNTHETIC_ACTOR: &str = "syntheticzep";
const SYNTHETIC_PARTS: [&str; 2] = ["engine1", "engine2"];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The production stage for retail M01.
fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

/// One member chain out of its spellings.
fn member(parts: &[&str]) -> MemberName {
    parts.iter().map(|part| (*part).to_owned()).collect()
}

/// One node key from a node's spelling.
fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a damage node key")
}

/// The class one bound part's **own registered graph** carries, read back from
/// the session damage authority the composition installs — never from the
/// record that planned the part.
fn graph_class(
    app: &App,
    chain: &MemberName,
    node: &DamageNodeKey,
) -> (DamageNodeKind, Option<SystemKind>) {
    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed");
    let part = report
        .parts
        .iter()
        .find(|part| part.chain == *chain)
        .unwrap_or_else(|| panic!("the composition bound a part for {}", chain.join("/")));
    let actor = part.actor;
    let resolver = app.world().resource::<SessionDamageResolver>().resolver();
    let graph = resolver
        .graph(&actor)
        .unwrap_or_else(|| panic!("the owning actor {actor:?} is registered"));
    let registered = graph
        .node(node)
        .unwrap_or_else(|| panic!("{} is a node of its own actor's graph", chain.join("/")));
    (registered.kind(), registered.disables())
}

/// **The live graph of a composed M01 carries the nine turret chains as weapon
/// mounts and the 30 engine chains as internal structure.**
///
/// Every class is read back from `SessionDamageResolver`'s registered graphs,
/// so this fails for a composition that reads the table but never builds from
/// it, for one that drops the `disables` edge, and for one that classifies a
/// part no record declares.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_turret_wire_the_live_graph_classes_the_nine_mounts() {
    let stage = retail_stage();

    // The stage read the table itself, through the production scope read: the
    // records, the carrier they came from, and no refusal.
    assert_eq!(
        stage.turret_carrier.as_deref(),
        Some("zbd/zrdr.zbd"),
        "the mission/group/shared mount order reaches the shared carrier, where #1491 \
         measured the member"
    );
    assert_eq!(
        stage.turret_refusal, None,
        "every carrier of this installation could be checked"
    );
    assert_eq!(
        stage.turret_records.len(),
        26,
        "the TURRET list's NODES-bearing records (0..=15 are standalone)"
    );
    assert_eq!(
        stage.turret_records.first().map(|record| record.index),
        Some(16),
        "the records are the shared list's own, in list order"
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed")
        .clone();
    assert!(
        report.refusals.is_empty(),
        "the binding refused nothing: {:?}",
        report.refusals
    );
    assert_eq!(
        report.parts.len(),
        39,
        "M01's 42 operands spell 39 part-state chains (27 workersvoyagezep + 12 piratezep)"
    );
    assert_eq!(
        report
            .parts
            .iter()
            .filter(|part| MOUNTS.contains(&part.node.as_str()))
            .count(),
        9,
        "exactly the nine chains the table declares are bound parts"
    );

    // The nine mounts: weapon mounts under the actor the table names, each
    // disabling the F29 `Weapon` system on its own node — so a destroyed
    // turret silences exactly that mount and never its eight siblings.
    for node in MOUNTS {
        let chain = member(&[MOUNT_ACTOR, node, "healthy"]);
        assert_eq!(
            graph_class(&app, &chain, &key(node)),
            (DamageNodeKind::WeaponMount, Some(SystemKind::Weapon)),
            "{node} is declared by a TURRET record, so its own graph node is a weapon \
             mount whose destruction disables its own weapon"
        );
    }

    // The 30 engine chains stay unclassed structure with no system edge: no
    // `NODES` entry of any record declares them (#1491 measured that too).
    let mut structures = 0;
    for part in &report.parts {
        if MOUNTS.contains(&part.node.as_str()) {
            continue;
        }
        assert_eq!(
            graph_class(&app, &part.chain, &part.node),
            (DamageNodeKind::InternalStructure, None),
            "{} is structure: no TURRET record declares it",
            part.chain.join("/")
        );
        structures += 1;
    }
    assert_eq!(
        structures, 30,
        "the 18 + 12 engine chains are the parts no mount record declares"
    );

    teardown(&mut app);
}

// ------------------------------------------------------------ synthetic ---

/// A synthetic stage whose host carries one bound world actor and two
/// part-state chains, so the production binder and the graph builder run with
/// no installation: the world-actor program registers one held actor whose
/// subject is `scene_node/…syntheticzep`, and the resolver pins
/// `syntheticzep` → `engine1`/`engine2` → `healthy`.
fn synthetic_wire_stage(scratch: &Path) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    let program = MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-m01-turret-wire"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                ContentKind::Objective,
                "synthetic-m01-turret-wire.objective.1",
            ),
            condition: Condition::InactiveMembers {
                members: SYNTHETIC_PARTS
                    .iter()
                    .map(|part| member(&[SYNTHETIC_ACTOR, part, "healthy"]))
                    .collect(),
                threshold: 1,
            },
            actions: vec![Action::Finish(Outcome::Succeeded)],
            span: None,
        }],
    };
    let mut blocks = BlockLifecycleTable::new();
    for index in 0..program.objectives.len() {
        blocks
            .declare(index as u32, LifecycleDecl::awake())
            .expect("each synthetic block is declared once");
    }
    stage.host.block_lifecycles = blocks;
    stage.host.control = program;
    stage.host.resolver = MemberResolver::from_hierarchy([
        (SYNTHETIC_ACTOR.to_owned(), None),
        (SYNTHETIC_PARTS[0].to_owned(), Some(0)),
        (SYNTHETIC_PARTS[1].to_owned(), Some(0)),
        ("healthy".to_owned(), Some(1)),
        ("healthy".to_owned(), Some(2)),
    ]);
    stage.host.world_actors = Some(LoweredWorldActors {
        ticks_per_second: 30,
        actors: vec![WorldActorSpec {
            actor: WorldActorId(1),
            kind: WorldActorKind::Kinematic,
            faction: cid(ContentKind::Faction, "synthetic.faction"),
            objective: None,
            motion: ActorMotion::Held {
                position_m: [1_000.0, 100.0, -2_000.0],
                orientation: cs_sim::world_actors::Quat::IDENTITY,
            },
        }],
        subjects: BTreeMap::from([(WorldActorId(1), synthetic_subject())]),
        sockets: BTreeMap::new(),
        support: Vec::new(),
        pickups: Vec::new(),
        transitions: Vec::new(),
    });
    stage
}

/// The synthetic actor's joined subject, spelled the way the composition's own
/// scene-node subjects are.
fn synthetic_subject() -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("container.synthetic.window.{SYNTHETIC_ACTOR}"),
    )
    .expect("the synthetic subject id is valid")
}

/// The one `TURRET` record the declaring synthetic stage carries: its `NODES`
/// entry names the synthetic mount exactly the way a retail one does.
fn synthetic_turret() -> TurretRecord {
    TurretRecord {
        index: 0,
        nodes: vec![vec![
            SYNTHETIC_ACTOR.to_owned(),
            SYNTHETIC_PARTS[0].to_owned(),
        ]],
        title: Some("MSG_TUR_SYNTH".to_owned()),
        weapon: Some("wep_synthetic".to_owned()),
        parts: vec!["turret".to_owned(), "gun".to_owned()],
        health: Some(7.5),
    }
}

/// **The stage's own table decides the class of a live synthetic graph.**
///
/// One record declares `engine1`; `engine2` is declared by nothing. Both parts
/// bind, and only the declared one comes back a weapon mount with the F29
/// edge — so a spawn host that builds its nodes from anything other than the
/// stage's table fails here, in CI, with no installation present.
#[test]
fn accept_m01_play_triggers_turret_wire_the_synthetic_stage_classes_exactly_its_declared_part() {
    let scratch = Scratch::new("m01-play-turret-wire");
    let mut stage = synthetic_wire_stage(scratch.path());
    stage.turret_records = vec![synthetic_turret()];
    stage.turret_carrier = Some("synthetic/ai.zrd".to_owned());

    let mut app = build_headless(&stage).expect("the synthetic wire stage composes");
    app.update();

    let declared = member(&[SYNTHETIC_ACTOR, SYNTHETIC_PARTS[0], "healthy"]);
    let other = member(&[SYNTHETIC_ACTOR, SYNTHETIC_PARTS[1], "healthy"]);
    assert_eq!(
        graph_class(&app, &declared, &key(SYNTHETIC_PARTS[0])),
        (DamageNodeKind::WeaponMount, Some(SystemKind::Weapon)),
        "the record's own `NODES` entry classes its part in the registered graph"
    );
    assert_eq!(
        graph_class(&app, &other, &key(SYNTHETIC_PARTS[1])),
        (DamageNodeKind::InternalStructure, None),
        "a part no record declares keeps the unclassed node and disables nothing"
    );

    teardown(&mut app);
}

/// **A table that could not be read is reported, never mistaken for one that
/// declares no turret.**
///
/// The reader's own refusal arm lives in `parts::read_scope_turrets` and is
/// unit-tested beside it; this pins the other half — that the composition
/// carries the reason verbatim onto the parts report and classes nothing from
/// a table it could not read.
#[test]
fn accept_m01_play_triggers_turret_wire_an_unread_table_is_named_on_the_parts_report() {
    let scratch = Scratch::new("m01-play-turret-wire-refused");
    let mut stage = synthetic_wire_stage(scratch.path());
    // The refusal arrives as the reader's own reason; the records stay empty
    // because nothing could be read out of a carrier nobody could open.
    stage.turret_records = Vec::new();
    stage.turret_carrier = None;
    stage.turret_refusal = Some("zbd/zrdr.zbd is not in the manifest".to_owned());

    let mut app = build_headless(&stage).expect("the synthetic wire stage composes");
    app.update();

    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed")
        .clone();
    assert!(
        report
            .refusals
            .iter()
            .any(|refusal| refusal.contains("ai.zrd")
                && refusal.contains("is not in the manifest")),
        "the parts report names the unread carrier verbatim: {:?}",
        report.refusals
    );

    for part in SYNTHETIC_PARTS {
        let chain = member(&[SYNTHETIC_ACTOR, part, "healthy"]);
        assert_eq!(
            graph_class(&app, &chain, &key(part)),
            (DamageNodeKind::InternalStructure, None),
            "no class is guessed from a table that could not be read"
        );
    }

    teardown(&mut app);
}

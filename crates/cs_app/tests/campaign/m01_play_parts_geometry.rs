//! Acceptance suite M01-PLAY-PARTS-GEOMETRY (#1492): M01's zeppelin parts
//! carry the **swept-box geometry** and the **declared allegiance** a fired
//! round needs, so the player's own guns can take a part out of play.
//!
//! M01-PLAY-TRIGGERS-PARTS (#1481) made the 39 part-state chains observable and
//! destroyable *through the damage authority*, but nothing in the composed run
//! could destroy a part **geometrically**:
//!
//! * the parts carried no [`PartSweptBox`], no
//!   [`NodeVisualTransform`](cs_app::scene::NodeVisualTransform) and no
//!   `LinearVelocity`, so the F27 `part_sweep_candidates` read had nothing to
//!   sweep;
//! * M01's `zeppelins.zrd` records state no `team` the gun's pair check could
//!   read, so nothing declared the zeppelins hostile and
//!   `FriendlyFireRule::HostileOnly` refused every shot at them by name.
//!
//! This suite pins both halves:
//!
//! * **the geometry** — every bound part entity carries a box at the owner's
//!   measured bind pose, and the box's provenance is the labelled **designed**
//!   value it is (the container's own per-node box has no decoded source and is
//!   carried as an explicit unknown);
//! * **the allegiance** — every bound world actor is rostered under the
//!   **measured** faction its record's loader resolved (#1155/#1177), and the
//!   relation the pair check reads comes from the designed reading of the two
//!   measured teams (`#1436`'s role split, applied to the world-actor program);
//! * **the path** — the player's own fire key sweeps a bound part and takes it
//!   out of play, and the `INACTIVE` ladder that counts it advances; while an
//!   actor on the player's **own** team is refused, because the allegiance is
//!   load-bearing.
//!
//! Nothing here is `verified_original`: no original executable ran, and every
//! measured value is read through the production readers on this run. The box
//! is designed and labelled, never presented as the original's geometry.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use avian3d::prelude::LinearVelocity;
use bevy::prelude::{App, Entity};
use cs_app::mission_actors::{
    M01_ZEPPELIN_ALLEGIANCE_CLAIM, MissionActorSpawnReport, MissionZeppelinActor,
    MissionZeppelinAllegianceReport, declared_player_allegiance, measured_team, target_class_of,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    DESIGNED_PART_HALF_EXTENTS_M, M01_PART_GEOMETRY_CLAIM, M01_PART_GEOMETRY_UNKNOWN_CLAIM,
    MISSION_FACTION_PLAYER, MissionCockpitActions, MissionHost, MissionPartReport,
    MissionPartState, MissionPartSweep, MissionStage, MissionWeapons, build_headless, stage_for,
    teardown,
};
use cs_app::scene::NodeVisualTransform;
use cs_app::targeting::TargetingSession;
use cs_app::weapons::{PartSweptBox, SessionDamageResolver, part_sweep_candidates};
use cs_app::world_actors::LoweredWorldActors;
use cs_app::world_facts::MemberResolver;
use cs_script::ir::{
    Action as ScriptAction, ActorId as WorldActorId, Condition, IR_VERSION, MemberName,
    MissionProgram, Objective, Outcome, SymbolId,
};
use cs_sim::damage::{ActorId, PartState};
use cs_sim::mission::{BlockLifecycleTable, LifecycleDecl};
use cs_sim::targeting::{Allegiance, TargetClass};
use cs_sim::world_actors::runtime::{ActorMotion, WorldActorKind, WorldActorSpec};
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimStatus;
use cs_types::input::{Action, FlightCommand};

use crate::campaign::common::{cid, label};
use crate::campaign::vs_m01_rt_window::{Scratch, synthetic_stage};

/// The synthetic world actor's node name.
const SYNTHETIC_ACTOR: &str = "syntheticzep";
/// The synthetic part node under it.
const SYNTHETIC_PART: &str = "engine1";
/// The player's measured team: `aiv.zrd`'s `player` record (#1453/#1436).
const PLAYER_TEAM: u32 = 1;
/// How far ahead of the player's start pose the synthetic actor sits, in
/// metres. Inside one round's first segment at the designed muzzle velocity.
const ACTOR_AHEAD_M: f32 = 10.0;
/// The most rendered frames a leg waits for a kill before failing.
const MAX_FRAMES: usize = 400;
/// How long a leg fires at a target the allegiance must refuse.
const REFUSAL_FRAMES: usize = 120;

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

/// The synthetic chain the CI legs bind and count.
fn chain() -> MemberName {
    member(&[SYNTHETIC_ACTOR, SYNTHETIC_PART, "healthy"])
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

/// A synthetic program with one `InactiveMembers` ladder over the synthetic
/// chain: the part leaving play completes block `0`.
fn ladder_program() -> MissionProgram {
    MissionProgram {
        version: IR_VERSION,
        mission: cid(ContentKind::Mission, "synthetic-m01-parts-geometry"),
        variables: Vec::new(),
        objectives: vec![Objective {
            id: SymbolId(0),
            content: cid(
                ContentKind::Objective,
                "synthetic-m01-parts-geometry.objective.1",
            ),
            condition: Condition::InactiveMembers {
                members: vec![chain()],
                threshold: 1,
            },
            actions: vec![ScriptAction::Finish(Outcome::Succeeded)],
            span: None,
        }],
    }
}

/// The player's own `aiv.zrd` record, fabricated: only its **measured**
/// `team` matters here, and it is what the designed allegiance reading
/// compares a world actor's faction team against.
fn player_record(team: u32) -> cs_app::mission_start::ScriptedAircraft {
    cs_app::mission_start::ScriptedAircraft {
        name: "player".to_owned(),
        index: 0,
        field_count: 81,
        netids: None,
        stored_pose: None,
        team: Some(team),
        group: None,
        enabled: Some(true),
        primary_target: None,
        title: None,
        starts_asleep: None,
        dispatch: cs_app::mission_start::SpawnDispatch::Player,
        provenance: cs_types::content::Provenance::designed(
            cs_types::evidence::ClaimId::new("test.m01-parts-geometry.player")
                .expect("the claim id is valid"),
        ),
    }
}

/// A synthetic composed stage with one bound world actor (a zeppelin-kind
/// actor of `faction`), one part chain and a player record carrying `team`.
///
/// The actor sits [`ACTOR_AHEAD_M`] metres down the player's own start-forward,
/// so the production gun step's own swept path crosses its part's box.
fn synthetic_geometry_stage(
    scratch: &Path,
    faction: &str,
    team: Option<u32>,
    ahead_m: f32,
) -> MissionStage {
    let mut stage = synthetic_stage(scratch);
    if let Some(team) = team {
        stage.aircraft.push(player_record(team));
    }
    let mut blocks = BlockLifecycleTable::new();
    for index in 0..ladder_program().objectives.len() {
        blocks
            .declare(index as u32, LifecycleDecl::awake())
            .expect("each synthetic block is declared once");
    }
    stage.host.block_lifecycles = blocks;
    stage.host.control = ladder_program();
    stage.host.resolver = MemberResolver::from_hierarchy([
        (SYNTHETIC_ACTOR.to_owned(), None),
        (SYNTHETIC_PART.to_owned(), Some(0)),
        ("healthy".to_owned(), Some(1)),
    ]);
    let start = stage.start;
    let forward = [-start.heading.sin(), 0.0, -start.heading.cos()];
    let position_m = [
        f64::from(start.position[0] + forward[0] * ahead_m),
        f64::from(start.position[1] + forward[1] * ahead_m),
        f64::from(start.position[2] + forward[2] * ahead_m),
    ];
    stage.host.world_actors = Some(LoweredWorldActors {
        ticks_per_second: 30,
        actors: vec![WorldActorSpec {
            actor: WorldActorId(1),
            // The measured `zeppelins.zrd` record family lowers to an airship.
            kind: WorldActorKind::Airship,
            faction: cid(ContentKind::Faction, faction),
            objective: None,
            motion: ActorMotion::Held {
                position_m,
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

/// The part entity the production binder spawned for `chain`, with its owner.
fn part_of(app: &mut App, chain: &MemberName) -> (Entity, ActorId) {
    app.world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed")
        .parts
        .iter()
        .find(|part| &part.chain == chain)
        .map(|part| (part.entity, part.actor))
        .unwrap_or_else(|| panic!("the composition binds a part for {chain:?}"))
}

/// Whether the session's damage authority reports `chain`'s part out of play.
fn part_out_of_play(app: &App, chain: &MemberName) -> bool {
    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed");
    let part = report
        .parts
        .iter()
        .find(|part| &part.chain == chain)
        .expect("the chain is bound");
    app.world()
        .resource::<SessionDamageResolver>()
        .resolver()
        .part_state(&part.actor, &part.node)
        == Some(PartState::Destroyed)
}

/// Whether the host's lifecycle model projects `block` as completed.
fn completed(app: &App, block: u32) -> bool {
    app.world()
        .resource::<MissionHost>()
        .lifecycles()
        .state(block)
        == Some(cs_script::runtime::ObjectiveLifecycle::Done)
}

/// Publishes one frame's delivered cockpit edge and runs the composed ticks —
/// the real fire key's delivered action.
fn fire(app: &mut App, command: FlightCommand) {
    app.world_mut().insert_resource(MissionCockpitActions {
        actions: vec![Action::Flight(command)],
    });
    app.update();
}

/// The player's actor in the composed session.
fn player_actor(app: &App) -> ActorId {
    app.world().resource::<MissionWeapons>().player()
}

/// The relation closure the production gun step builds: both actors must be
/// rostered, and the pair's declared allegiance is what the shot reads.
fn relation(app: &App, from: ActorId, to: ActorId) -> Option<Allegiance> {
    let targeting = app.world().resource::<TargetingSession>();
    let store = targeting.store();
    let (from, to) = (store.record(&from)?, store.record(&to)?);
    store.allegiance(&from.faction, &to.faction)
}

// ------------------------------------------------------------ synthetic ---

/// **The player's own fire key sweeps a bound part, takes it out of play and
/// advances the `INACTIVE` ladder that counts it.**
///
/// The whole path is the production one: the composed mission opens the weapon
/// session, the spawn host binds the parts and the roster, the publisher reads
/// the delivered fire edge, the gun step sweeps the part's box under the
/// declared relation, and the damage authority applies the hit the damage
/// schedule scores. The final row is the production observer's.
#[test]
fn accept_m01_play_parts_geometry_the_fire_key_destroys_a_bound_part_and_advances_its_ladder() {
    let scratch = Scratch::new("m01-parts-geometry-fire");
    let stage = synthetic_geometry_stage(scratch.path(), "enemy", Some(PLAYER_TEAM), ACTOR_AHEAD_M);
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    // The part entity carries the whole geometric face the sweep reads.
    let (entity, actor) = part_of(&mut app, &chain());
    let world = app.world();
    let box_record = world
        .get::<PartSweptBox>(entity)
        .expect("the part carries a swept box");
    assert_eq!(
        box_record.half_extents_m, DESIGNED_PART_HALF_EXTENTS_M,
        "the box half extents are the labelled designed value"
    );
    assert_eq!(box_record.actor, actor, "the box names its owner");
    assert!(
        world.get::<NodeVisualTransform>(entity).is_some(),
        "the part's box centre comes from a composed NodeVisualTransform"
    );
    assert_eq!(
        world.get::<LinearVelocity>(entity).map(|v| v.0),
        Some(bevy::prelude::Vec3::ZERO),
        "a Held world actor's part has no motion"
    );
    let sweep = world
        .get::<MissionPartSweep>(entity)
        .expect("the part records where its box came from");
    assert_eq!(
        sweep.provenance.class,
        ClaimStatus::Designed,
        "the box is a labelled designed value"
    );
    assert_eq!(sweep.provenance.claim_id.as_str(), M01_PART_GEOMETRY_CLAIM);
    assert!(
        matches!(
            &sweep.unmeasured,
            cs_types::content::Resolved::Unknown { claim_id, .. }
                if claim_id.as_str() == M01_PART_GEOMETRY_UNKNOWN_CLAIM
        ),
        "the container's own per-node box is an explicit unknown: {sweep:?}"
    );

    // The bounded world actor is rostered under its measured faction, and the
    // pair's relation is the designed reading of the two measured teams.
    let roster = app
        .world()
        .resource::<MissionZeppelinAllegianceReport>()
        .clone();
    assert_eq!(
        roster.rostered.len(),
        1,
        "the bound world actor is rostered: {roster:?}"
    );
    let (rostered_actor, faction, allegiance) = roster.rostered[0].clone();
    assert_eq!(rostered_actor, actor);
    assert_eq!(faction, cid(ContentKind::Faction, "enemy"));
    assert_eq!(allegiance, Some(Allegiance::Hostile));
    let player = player_actor(&app);
    assert_eq!(
        relation(&app, player, actor),
        Some(Allegiance::Hostile),
        "the gun step's own pair check reads the declared hostile relation"
    );

    // Fire the player's own fire key until the part leaves play.
    let mut killed = false;
    for _ in 0..MAX_FRAMES {
        fire(&mut app, FlightCommand::FirePrimary);
        if part_out_of_play(&app, &chain()) {
            killed = true;
            break;
        }
    }
    assert!(
        killed,
        "the player's fire key destroyed the bound part: weapons={:?}",
        app.world()
            .get_resource::<cs_app::weapons::WeaponScheduleReport>(),
    );

    app.update();
    assert!(
        completed(&app, 0),
        "the ladder that counts the cleared chain advanced on the live world's own state"
    );

    teardown(&mut app);
}

/// **A world actor on the player's own team is refused by the pair check.**
///
/// The allegiance is load-bearing: the same composed run, the same gun, the
/// same box, and only the actor's measured faction differs (`ally`, the
/// player's own team `1`) — so the designed `FriendlyFireRule::HostileOnly`
/// refuses every contact and the part never leaves play. Removing the roster or
/// the relation would make the gun refuse too, so this leg alone cannot tell
/// "refused for the right reason"; the leg above is what distinguishes them,
/// and this one pins that the relation is not a blanket hostility grant.
#[test]
fn accept_m01_play_parts_geometry_a_world_actor_on_the_players_team_is_refused() {
    let scratch = Scratch::new("m01-parts-geometry-friendly");
    let stage = synthetic_geometry_stage(scratch.path(), "ally", Some(PLAYER_TEAM), ACTOR_AHEAD_M);
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let (_, actor) = part_of(&mut app, &chain());
    let roster = app
        .world()
        .resource::<MissionZeppelinAllegianceReport>()
        .clone();
    assert_eq!(
        roster.rostered,
        vec![(
            actor,
            cid(ContentKind::Faction, "ally"),
            Some(Allegiance::Friendly)
        )],
        "an actor on the player's own team reads friendly: {roster:?}"
    );
    let player = player_actor(&app);
    assert_eq!(relation(&app, player, actor), Some(Allegiance::Friendly));

    for _ in 0..REFUSAL_FRAMES {
        fire(&mut app, FlightCommand::FirePrimary);
    }
    assert!(
        !part_out_of_play(&app, &chain()),
        "the declared non-hostile relation refuses every contact, so the part stays in play"
    );
    assert!(
        !completed(&app, 0),
        "an uncleared member never satisfies the ladder"
    );

    teardown(&mut app);
}

/// **A stage whose player team was never read states no side.**
///
/// The fail-closed half of the designed reading: with no measured player team
/// there is no relation to declare, so the actor is rostered but nothing grants
/// a shot — the honest state, never a guessed hostility.
#[test]
fn accept_m01_play_parts_geometry_an_unread_player_team_declares_no_side() {
    let scratch = Scratch::new("m01-parts-geometry-no-team");
    let stage = synthetic_geometry_stage(scratch.path(), "enemy", None, ACTOR_AHEAD_M);
    let mut app = build_headless(&stage).expect("the synthetic parts stage composes");
    app.update();

    let (_, actor) = part_of(&mut app, &chain());
    let roster = app
        .world()
        .resource::<MissionZeppelinAllegianceReport>()
        .clone();
    assert_eq!(
        roster.rostered,
        vec![(actor, cid(ContentKind::Faction, "enemy"), None)],
        "an unread player team states no side: {roster:?}"
    );
    assert_eq!(roster.unresolved, Vec::<String>::new());
    for _ in 0..REFUSAL_FRAMES {
        fire(&mut app, FlightCommand::FirePrimary);
    }
    assert!(
        !part_out_of_play(&app, &chain()),
        "with no declared relation the shot is refused, not invented into a hit"
    );

    teardown(&mut app);
}

/// **The designed allegiance reading, on the measured vocabulary.**
///
/// A pure leg over the two functions the wiring uses, so the reading is pinned
/// without an installation: the team map is the loader's own (#1155), a record
/// on a different team is opposed, a record on the same team is the player's
/// flight, and a faction or a team that was never read states no side.
#[test]
fn accept_m01_play_parts_geometry_the_allegiance_reading_is_the_measured_team_split() {
    let enemy = cid(ContentKind::Faction, "enemy");
    let ally = cid(ContentKind::Faction, "ally");
    let neutral = cid(ContentKind::Faction, "neutral");
    let stranger = cid(ContentKind::Faction, "a.faction.no.record.states");
    assert_eq!(measured_team(&enemy), Some(2));
    assert_eq!(measured_team(&ally), Some(1));
    assert_eq!(measured_team(&neutral), Some(0));
    assert_eq!(measured_team(&stranger), None);

    assert_eq!(
        declared_player_allegiance(&enemy, Some(1)),
        Some(Allegiance::Hostile)
    );
    assert_eq!(
        declared_player_allegiance(&ally, Some(1)),
        Some(Allegiance::Friendly)
    );
    assert_eq!(
        declared_player_allegiance(&neutral, Some(1)),
        Some(Allegiance::Hostile)
    );
    assert_eq!(declared_player_allegiance(&enemy, None), None);
    assert_eq!(declared_player_allegiance(&stranger, Some(1)), None);
    assert_eq!(
        M01_ZEPPELIN_ALLEGIANCE_CLAIM,
        "m01-play-parts-geometry.designed-player-allegiance"
    );

    assert_eq!(
        target_class_of(WorldActorKind::Airship),
        TargetClass::CapitalShip
    );
    for kind in [
        WorldActorKind::Rail,
        WorldActorKind::Road,
        WorldActorKind::Water,
        WorldActorKind::Kinematic,
    ] {
        assert_eq!(target_class_of(kind), TargetClass::WorldObject);
    }
}

// --------------------------------------------------------------- retail ---

/// **M01's 39 bound parts carry a labelled box at their measured bind pose,
/// and every bound world actor is rostered under its measured faction.**
///
/// The measured half is the allegiance and the pose: `workersvoyagezep`'s
/// loader-resolved faction is `enemy` and `piratezep`'s is `ally` (#1155/#1177),
/// the player's own team is `1` (`aiv.zrd`), and the box centre is the session's
/// canonical actor pose. The designed half is the box, and it says so: its
/// provenance is a designed claim and the container's own per-node box it
/// stands in for is an explicit unknown under its own claim.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_parts_geometry_m01_binds_the_measured_allegiance_and_a_labelled_box_on_every_part()
 {
    let stage = retail_stage();
    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    app.update();

    // The measured player team the designed reading compares against.
    let player_team = stage
        .aircraft
        .iter()
        .find(|record| record.dispatch == cs_app::mission_start::SpawnDispatch::Player)
        .and_then(|record| record.team);
    assert_eq!(
        player_team,
        Some(PLAYER_TEAM),
        "M01's `player` record carries the measured team 1"
    );

    let report = app
        .world()
        .get_resource::<MissionPartReport>()
        .expect("the parts report is installed")
        .clone();
    assert_eq!(report.parts.len(), 39, "M01 binds its 39 part-state chains");

    // Every part entity carries the entire geometric face, at its owner's
    // measured bind pose, with the labelled designed box and its unknown.
    let spawn = app
        .world()
        .get_resource::<MissionActorSpawnReport>()
        .expect("the spawn report is installed")
        .clone();
    let mut owner_pose: BTreeMap<ActorId, [f64; 3]> = BTreeMap::new();
    for bound in &spawn.zeppelins {
        let zeppelin = app
            .world()
            .get::<MissionZeppelinActor>(bound.entity)
            .expect("the bound zeppelin carries its component")
            .clone();
        assert!(
            zeppelin.roster.is_some(),
            "the actor carries its roster record"
        );
        owner_pose.insert(bound.actor, zeppelin.position_m);
    }
    let world = app.world_mut();
    for part in &report.parts {
        assert!(
            world.get::<MissionPartState>(part.entity).is_some(),
            "{} is a live part-state object",
            part.chain.join("/")
        );
        let box_record = world
            .get::<PartSweptBox>(part.entity)
            .unwrap_or_else(|| panic!("{} carries a swept box", part.chain.join("/")));
        assert_eq!(box_record.actor, part.actor);
        assert_eq!(&box_record.node, &part.node);
        assert_eq!(
            box_record.half_extents_m,
            DESIGNED_PART_HALF_EXTENTS_M,
            "{} carries the labelled designed box",
            part.chain.join("/")
        );
        let visual = world
            .get::<NodeVisualTransform>(part.entity)
            .unwrap_or_else(|| panic!("{} carries a composed pose", part.chain.join("/")));
        let pose = owner_pose
            .get(&part.actor)
            .unwrap_or_else(|| panic!("{}'s owner was bound", part.chain.join("/")));
        let translation = visual.global().translation();
        for (axis, expected) in pose.iter().enumerate() {
            assert!(
                (f64::from(translation.to_array()[axis]) - expected).abs() < 1.0e-3,
                "{}'s box centre is its owner's measured bind pose",
                part.chain.join("/")
            );
        }
        assert_eq!(
            world.get::<LinearVelocity>(part.entity).map(|v| v.0),
            Some(bevy::prelude::Vec3::ZERO),
            "a Held world actor's part has no motion"
        );
        let sweep = world
            .get::<MissionPartSweep>(part.entity)
            .unwrap_or_else(|| panic!("{} records its box provenance", part.chain.join("/")));
        assert_eq!(
            sweep.provenance.class,
            ClaimStatus::Designed,
            "{}'s box is designed, never measured",
            part.chain.join("/")
        );
        assert_eq!(sweep.provenance.claim_id.as_str(), M01_PART_GEOMETRY_CLAIM);
        assert!(
            matches!(
                &sweep.unmeasured,
                Resolved::Unknown { claim_id, reason }
                    if claim_id.as_str() == M01_PART_GEOMETRY_UNKNOWN_CLAIM && !reason.is_empty()
            ),
            "{}'s container box is an explicit unknown with a reason: {sweep:?}",
            part.chain.join("/")
        );
    }

    // The actor-level measured faction: `workersvoyagezep` enemy,
    // `piratezep` ally (#1155/#1177) — and every rostered actor's relation is
    // the designed reading of its own measured faction and the player's team.
    let roster = app
        .world()
        .resource::<MissionZeppelinAllegianceReport>()
        .clone();
    assert_eq!(
        roster.rostered.len(),
        spawn.zeppelins.len(),
        "every bound world actor is rostered: {roster:?}"
    );
    assert!(
        roster.unresolved.is_empty(),
        "every bound actor was rostered: {:?}",
        roster.unresolved
    );
    let factions: BTreeMap<ActorId, (ContentId, Option<Allegiance>)> = roster
        .rostered
        .iter()
        .map(|(actor, faction, allegiance)| (*actor, (faction.clone(), *allegiance)))
        .collect();
    let mut measured_by_name: BTreeMap<String, (ContentId, Allegiance)> = BTreeMap::new();
    for bound in &spawn.zeppelins {
        let name = cs_app::mission_session::subject_node_name(&bound.subject)
            .unwrap_or_else(|| panic!("{:?} spells a node name", bound.subject))
            .to_owned();
        let (faction, allegiance) = factions
            .get(&bound.actor)
            .cloned()
            .unwrap_or_else(|| panic!("{name} is rostered"));
        assert_eq!(
            allegiance,
            declared_player_allegiance(&faction, player_team),
            "{name}'s relation is the designed reading of its measured faction"
        );
        assert!(
            measured_team(&faction).is_some(),
            "{name}'s faction is inside the measured loader vocabulary"
        );
        measured_by_name.insert(name, (faction, allegiance.expect("a declared side")));
    }
    assert_eq!(
        measured_by_name.get("workersvoyagezep"),
        Some(&(cid(ContentKind::Faction, "enemy"), Allegiance::Hostile)),
        "the loader resolves workersvoyagezep as enemy (#1155/#1177)"
    );
    assert_eq!(
        measured_by_name.get("piratezep"),
        Some(&(cid(ContentKind::Faction, "ally"), Allegiance::Friendly)),
        "the loader resolves piratezep as ally (#1155/#1177)"
    );

    // Every part's owner is one of those rostered actors, and the two zeppelins
    // the program's chains name are both present.
    let by_actor: BTreeMap<ActorId, Vec<&MemberName>> = {
        let mut map: BTreeMap<ActorId, Vec<&MemberName>> = BTreeMap::new();
        for part in &report.parts {
            map.entry(part.actor).or_default().push(&part.chain);
        }
        map
    };
    assert_eq!(by_actor.len(), 2, "the 39 chains span two zeppelins");
    let mut seen_workers = false;
    let mut seen_pirates = false;
    for (actor, chains) in &by_actor {
        assert!(factions.contains_key(actor), "the owner is rostered");
        if chains
            .iter()
            .any(|chain| chain.first().map(String::as_str) == Some("workersvoyagezep"))
        {
            seen_workers = true;
        }
        if chains
            .iter()
            .any(|chain| chain.first().map(String::as_str) == Some("piratezep"))
        {
            seen_pirates = true;
        }
    }
    assert!(seen_workers && seen_pirates, "both zeppelins are bound");

    // The pair the gun step reads: the player's own faction toward each actor.
    let player = player_actor(&app);
    for (actor, (_, allegiance)) in &factions {
        assert_eq!(
            relation(&app, player, *actor),
            *allegiance,
            "the production pair check reads the declared relation"
        );
    }
    let player_faction = ContentId::from_source(ContentKind::Faction, MISSION_FACTION_PLAYER)
        .expect("the designed player faction id is valid");
    {
        let targeting = app.world().resource::<TargetingSession>();
        let store = targeting.store();
        for (actor, (faction, allegiance)) in &factions {
            assert_eq!(
                store.record(actor).map(|record| &record.faction),
                Some(faction),
                "the actor is rostered under its measured faction"
            );
            assert_eq!(
                store.allegiance(&player_faction, faction),
                *allegiance,
                "the declared relation is the designed reading"
            );
            assert_eq!(
                store.record(actor).map(|record| record.class),
                Some(TargetClass::CapitalShip),
                "a zeppelin-kind world actor is a capital ship"
            );
        }
    }

    // The production sweep read admits the hostile parts under the relation
    // the gun step binds, and news the same parts under the ally's.
    let candidates = {
        let world = app.world();
        let targeting = world.resource::<TargetingSession>();
        let store = targeting.store();
        let player_faction = player_faction.clone();
        part_sweep_candidates(world, 1.0 / 30.0, |target| {
            let record = store.record(&target)?;
            store.allegiance(&player_faction, &record.faction)
        })
    };
    assert_eq!(
        candidates.refused.len(),
        0,
        "every part read its pose, box and velocity: {:?}",
        candidates.refused
    );
    assert_eq!(
        candidates.candidates.len(),
        39,
        "every bound part is a sweep candidate"
    );
    for candidate in &candidates.candidates {
        let hostile = by_actor.iter().any(|(actor, chains)| {
            *actor == candidate.target.actor
                && chains
                    .iter()
                    .any(|chain| chain.first().map(String::as_str) == Some("workersvoyagezep"))
        });
        assert_eq!(
            candidate.relation,
            Some(if hostile {
                Allegiance::Hostile
            } else {
                Allegiance::Friendly
            }),
            "the candidate's relation is the declared pair"
        );
    }

    teardown(&mut app);
}

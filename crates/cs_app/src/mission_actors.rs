//! Scripted mission actors for the mission composition (M01-PLAY-ACTORS,
//! #1435): the enabled non-player `aiv.zrd` aircraft and the
//! `zeppelins.zrd` world actors, bound into the world the composition
//! builds.
//!
//! What is measured here and what is not:
//!
//! * **The records** come from [`crate::mission_start::read_aircraft_records`]
//!   (#1453) and [`crate::mission_world_actors::bind_mission_world_actors`]
//!   (#574/#772): every field on [`ScriptedAircraftActor::record`] and every
//!   zeppelin row is `ObservedTool`-provenanced retail data.
//! * **Who spawns** is measured: the original's spawn loop skips a record
//!   whose `enabled` field is `0` (`0x474d10`), builds the `player` record
//!   through the engine-state path instead of this one
//!   ([`SpawnDispatch::Player`]), and sleeps a record whose `deactivated`
//!   field is non-zero (`0x47ca14` → `0x4b0f40`, #681).
//! * **The pose** is measured: `stored_pose.position_metres()` (#436) and
//!   `stored_heading_radians` composed through the production
//!   [`object3d_orientation`] `Ry`, exactly as the player's own spawn does.
//! * **The airframe/model selection is designed.** #1453 measured that the
//!   original resolves the record's name-truncated prefix against
//!   `vehicle.zrd`; which record each prefix binds and what model it draws
//!   is unmeasured, so the entity carries [`SpawnDispatch::VehicleRecord`]'s
//!   measured prefix verbatim and nothing more.
//! * **The flight model is the recovered original law, flown by the designed
//!   AI (#1436)**: a record that is not asleep and whose measured
//!   name-prefix resolved an imported `vehicle.zrd` airframe spawns a
//!   **dynamic** body carrying
//!   [`PlaytestOriginalFlight`](crate::playtest::scene::PlaytestOriginalFlight)
//!   with `is_player: false` — the #796 law's own AI branch — and the
//!   [`MissionAircraftAi`] component the designed pursuit-and-fire law
//!   ([`crate::mission_ai`]) commands each fixed tick. A record that starts
//!   asleep (#681) or whose airframe would not import keeps the kinematic
//!   held body it has always had, because a body with no law to fly it must
//!   never be dropped into the world by gravity alone.
//! * **The damage model is designed**: every actor registers a single-node
//!   graph (`airframe`, internal structure, lethal) whose integrity is
//!   [`Resolved::Unknown`], so a routed hit blocks visibly in the
//!   `SessionDamageResolver` rather than being absorbed by an invented
//!   pool — no M01 damage table has been measured (#668).
//! * **Zeppelins** already exist as launched [`WorldActorSession`] actors
//!   (#772); this module binds each one an ECS entity carrying
//!   [`WorldActorBinding`] and [`DamageActorBinding`] so the actor is a
//!   world-side identity the damage consumers and teardown can name. No
//!   pose, route or faction is invented: the binding's pose comes from the
//!   session's own canonical read.
//!
//! # The bound world actor's allegiance (M01-PLAY-PARTS-GEOMETRY, #1492)
//!
//! A shot the player fires is admitted by the pair `(shooter, target)`: the
//! F27 gun step's declared `FriendlyFireRule::HostileOnly` refuses an
//! **undeclared** pair by name, and M01's `zeppelins.zrd` records state no
//! `team` a naive read could compare (that was #1481's finding). The loader
//! still resolves an allegiance for each record — the record's own `team`
//! spelling or the engine's marked-node/turret resolver (#1155, #1177) — and
//! the world-actor program carries it as the actor's `faction`. This module
//! binds that measured faction onto the actor ([`WorldActorRoster`]) and
//! [`roster_mission_zeppelins`] puts the actor and the designed relation into
//! the session's targeting store, so the gun step's own pair check reads a
//! declared relation instead of nothing.
//!
//! The **reading** that turns a measured faction and the player record's own
//! measured team into a relation is designed
//! ([`declared_player_allegiance`], [`M01_ZEPPELIN_ALLEGIANCE_CLAIM`]): the same
//! team split `#1436`'s role split uses. A record whose team was never read
//! states no side, and no hostility is invented for it.
//!
//! Teardown: every entity spawned here carries [`ScriptedAircraftActor`] or
//! [`MissionZeppelinActor`]; `MissionHost::restart` despawns both kinds and
//! respawns them through the same function, and `teardown` removes them
//! with the rest of the composition.

use std::collections::BTreeMap;

use avian3d::prelude::{NoAutoMass, Rotation};
use bevy::prelude::{
    App, Component, Entity, FixedPreUpdate, Plugin, Quat, Resource, Transform, Vec3, World,
};
use cs_script::ir::{ActorId as WorldActorId, MemberName};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::damage::{
    AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
    DamageResolver,
};
use cs_sim::targeting::{Allegiance, TargetClass, TargetRecord};
use cs_sim::world_actors::runtime::WorldActorKind;
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::{ActorAllocator, ActorId, SessionId};
use cs_types::space::WorldPosition;

use crate::damage::DamageActorBinding;
use crate::mission_ai::{
    MissionAiFireOrders, MissionAiReport, MissionAircraftAi, ai_flight_record, spawn_mass_kg,
};
use crate::mission_session::{MISSION_FACTION_PLAYER, MissionHost, MissionStage, parts};
use crate::mission_start::{ScriptedAircraft, SpawnDispatch, stored_heading_radians};
use crate::mission_world_actors::{AllegianceBinding, object3d_orientation};
use crate::physics::{BodyMode, BodySpec, spawn_body};
use crate::scene::SceneGenerations;
use crate::targeting::TargetingSession;
use crate::weapons::SessionDamageResolver;
use crate::world_actors::WorldActorBinding;

/// The producer serial the mission's damage resolver stamps — designed:
/// the session has no measured producer table, so the spawn path owns this
/// one.
const MISSION_DAMAGE_PRODUCER: u32 = 0;

/// The designed single node every scripted actor's damage graph carries:
/// a lethal internal-structure part with an unresolved pool.
const SCRIPTED_DAMAGE_NODE: &str = "airframe";

/// Resource: the scripted-aircraft records the composition spawns — every
/// `aiv.zrd` record except the `player` one (the composition's own
/// `MissionPlayerBody` is that record's runtime face), `enabled == 0`
/// excluded exactly as the original's spawn loop skips it.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionAircraftRoster(pub Vec<ScriptedAircraft>);

impl MissionAircraftRoster {
    /// The roster a stage's record list becomes: every record except the
    /// `player` one (the composition's `MissionPlayerBody` is its runtime
    /// face) and the ones `enabled` drops, exactly the original's `== 0`
    /// skip (`0x474d10`).
    #[must_use]
    pub fn from_records(records: &[ScriptedAircraft]) -> Self {
        Self(
            records
                .iter()
                .filter(|record| {
                    record.enabled != Some(false) && record.dispatch != SpawnDispatch::Player
                })
                .cloned()
                .collect(),
        )
    }
}

/// Resource: the `aiv.zrd` records the original's spawn loop skips
/// (`enabled == 0`, `0x474d10`). They never enter the vehicle registry, so
/// the group counter `0x4658d0` — a pure registry count — finds none of them:
/// a group whose only records are these counts **0**, which is the measured
/// answer `DEDG` reads (docs/findings/2026-10-15-m01-play-triggers-group-membership.md).
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionSkippedAircraft(pub Vec<ScriptedAircraft>);

impl MissionSkippedAircraft {
    /// The non-player records `enabled == 0` drops from a stage's list.
    #[must_use]
    pub fn from_records(records: &[ScriptedAircraft]) -> Self {
        Self(
            records
                .iter()
                .filter(|record| {
                    record.enabled == Some(false) && record.dispatch != SpawnDispatch::Player
                })
                .cloned()
                .collect(),
        )
    }
}

/// One record the spawn left an entity for.
#[derive(Clone, Debug, PartialEq)]
pub struct SpawnedAircraft {
    /// The record name, as `aiv.zrd` spells it.
    pub record: String,
    /// The spawned entity.
    pub entity: Entity,
    /// The session damage actor registered for it.
    pub actor: ActorId,
    /// The measured `deactivated` state the record spawned under.
    pub asleep: bool,
}

/// One zeppelin world actor an entity was bound to.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundZeppelin {
    /// The runtime actor of the world-actor session.
    pub world_actor: cs_script::ir::ActorId,
    /// The joined scene-node subject.
    pub subject: ContentId,
    /// The spawned binding entity.
    pub entity: Entity,
    /// The session damage actor registered for it.
    pub actor: ActorId,
}

/// One bound ground target an entity was spawned for.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundGroundTarget {
    /// The container record's stored display name.
    pub node: String,
    /// The `targets.zrd` row that declared it, when one did.
    pub declared: Option<usize>,
    /// The composed world position in metres the target spawned at.
    pub position_m: [f64; 3],
    /// The spawned entity.
    pub entity: Entity,
    /// The session damage actor registered for it.
    pub actor: ActorId,
}

/// Component: one bound ground target's ECS face — the entity the damage
/// consumers and teardown name for a world-container record the mission's
/// `targets.zrd` declares or the world group's damage set couples
/// (M01-BRIDGE-CONVOY-SPAWN, #1566).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MissionGroundTarget {
    /// The container record's stored display name — the damage node key the
    /// target is addressable by.
    pub node: String,
    /// The `targets.zrd` row that declared it, when one did.
    pub declared: Option<usize>,
    /// The session damage actor this entity's [`DamageActorBinding`]
    /// registers.
    pub damage_actor: ActorId,
    /// The scene generation the entity spawned under.
    pub generation: crate::scene::SceneGeneration,
    /// The composed world position in metres.
    pub position_m: [f64; 3],
    /// The node's authored `healthy` child, when it has one.
    pub healthy: Option<String>,
    /// The node's authored `destroyed` child, when it has one.
    pub destroyed: Option<String>,
    /// The damage-coupling field names the world group states for the node,
    /// in [`crate::mission_targets::DAMAGE_COUPLING_FIELDS`] order — read,
    /// never interpreted.
    pub coupling: Vec<String>,
    /// Whether the declaring row carried the `objective` marker.
    pub objective: bool,
    /// Whether the declaring row carried the `other_target` marker.
    pub other_target: bool,
    /// The target's targeting roster record (M01-TARGET-ROSTER, #1592): the
    /// faction the engine's node resolver answers for the node's subtree,
    /// and the designed relation the player's flight holds toward it.
    /// `None` when the resolver could not settle a team — the reason is in
    /// [`Self::allegiance_open`], and no faction is invented.
    pub roster: Option<WorldActorRoster>,
    /// Why no roster record exists, verbatim, when [`Self::roster`] is `None`.
    pub allegiance_open: Option<String>,
}

/// Resource: what the ground-target binding spawned — the entities, their
/// registered damage actors and every refusal by name. Replaced on each spawn,
/// so a restart's report is the fresh generation's own.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionGroundTargetReport {
    /// The session the targets were registered under.
    pub session: Option<SessionId>,
    /// The bound ground targets, in binding order.
    pub targets: Vec<BoundGroundTarget>,
    /// Every refusal, verbatim.
    pub refusals: Vec<String>,
}

impl MissionGroundTargetReport {
    /// The report entry for a container node, when one spawned.
    #[must_use]
    pub fn target(&self, node: &str) -> Option<&BoundGroundTarget> {
        self.targets.iter().find(|target| target.node == node)
    }
}

/// Resource: what [`spawn_mission_actors`] did — the entities spawned, the
/// zeppelin bindings made, and every refusal by name. Replaced on each
/// spawn, so a restart's report is the fresh generation's own.
#[derive(Resource, Clone, Debug, Default)]
pub struct MissionActorSpawnReport {
    /// The session the actors were registered under.
    pub session: Option<SessionId>,
    /// The scripted aircraft that spawned, in roster order.
    pub aircraft: Vec<SpawnedAircraft>,
    /// The zeppelin world actors bound, in program order.
    pub zeppelins: Vec<BoundZeppelin>,
    /// Every refusal, verbatim.
    pub refusals: Vec<String>,
}

/// Component: an `aiv.zrd` scripted aircraft's runtime binding. The
/// entity's body is the kinematic `Aircraft` body [`spawn_mission_actors`]
/// gives it; everything else the record measured rides here.
#[derive(Component, Clone, Debug)]
pub struct ScriptedAircraftActor {
    /// The measured record (`ObservedTool` provenance).
    pub record: ScriptedAircraft,
    /// The `vehicle.zrd` record the measured dispatch resolves — the
    /// name-truncated registry prefix for the generic branch, and the player
    /// airframe row's `w` pointer for `wingman_1` (`0x426d60`). **Designed**
    /// as an airframe/model selection: #1453 measured the name-prefix lookup,
    /// not which record the prefix binds.
    pub vehicle_record: String,
    /// The session damage actor this entity's [`DamageActorBinding`]
    /// registers.
    pub damage_actor: ActorId,
    /// The scene generation the entity spawned under.
    pub generation: crate::scene::SceneGeneration,
    /// The measured `deactivated` state: the original calls its sleep
    /// routine on a non-zero field (#681), and the mission program's
    /// `WAKEUP_ENEMIES` is the measured wake.
    pub asleep: bool,
}

/// Component: one `zeppelins.zrd` world actor's ECS face — the entity the
/// damage consumers and teardown name for a launched [`WorldActorSession`]
/// actor. The zeppelin's drawn world object is the residency's own; this
/// entity binds the actor identity to it by subject, never by a guessed
/// pose. The world observation reads [`Self::damage_actor`] beside the
/// binding so the actor row can follow the damage authority's destruction
/// ledger out of play (M01-PLAY-TRIGGERS-ACTOR-BIT, #1490).
#[derive(Component, Clone, Debug)]
pub struct MissionZeppelinActor {
    /// The runtime actor of the world-actor session.
    pub world_actor: cs_script::ir::ActorId,
    /// The session damage actor this entity's [`DamageActorBinding`]
    /// registers.
    pub damage_actor: ActorId,
    /// The scene generation the entity spawned under.
    pub generation: crate::scene::SceneGeneration,
    /// The session's canonical pose at bind time — the world's own read,
    /// not a stored-field interpretation.
    pub position_m: [f64; 3],
    /// The canonical orientation at bind time.
    pub orientation: [f64; 4],
    /// The targeting roster record this actor presents
    /// (M01-PLAY-PARTS-GEOMETRY): its measured faction and kind, whether the
    /// program flags it an objective, and the designed relation the player's
    /// flight holds toward it. Decided once at spawn from the launched
    /// world-actor program, re-applied to each generation's targeting store by
    /// [`roster_mission_zeppelins`].
    ///
    /// `None` when the launched program declares the subject without a
    /// registration record: a subject with no declared actor states no faction,
    /// so no roster record and no relation are invented for it.
    pub roster: Option<WorldActorRoster>,
}

/// One bound world actor's targeting roster record, decided at spawn from the
/// measured world-actor program (M01-PLAY-PARTS-GEOMETRY).
#[derive(Clone, Debug, PartialEq)]
pub struct WorldActorRoster {
    /// The actor's faction, verbatim from the lowered program: the
    /// loader-resolved `ally`/`enemy`/`neutral` the record's own `team` key or
    /// the engine's allegiance resolver measured (#1155, #1177).
    pub faction: ContentId,
    /// The target class the actor's own measured
    /// [`WorldActorKind`] maps onto.
    pub class: TargetClass,
    /// Whether the mission program flags the actor an objective
    /// (`WorldActorSpec::objective`), verbatim.
    pub objective: bool,
    /// The designed relation of the player's flight toward this actor — the
    /// reading [`declared_player_allegiance`] derives from the measured teams.
    /// `None` when the actor's faction or the player's own team was never
    /// read: a missing team states no side, and no hostility is invented.
    pub player_allegiance: Option<Allegiance>,
}

/// The target class a measured [`WorldActorKind`] presents to targeting.
///
/// The mapping is designed; the input is the actor's own declared kind
/// (`zeppelins.zrd`'s measured `zeppelin` record family lowers to
/// [`WorldActorKind::Airship`]).
#[must_use]
pub const fn target_class_of(kind: WorldActorKind) -> TargetClass {
    match kind {
        WorldActorKind::Airship => TargetClass::CapitalShip,
        WorldActorKind::Rail
        | WorldActorKind::Road
        | WorldActorKind::Water
        | WorldActorKind::Kinematic => TargetClass::WorldObject,
    }
}

/// The measured team a loader-resolved faction names.
///
/// The vocabulary is the loader's own (#1155): `neutral` 0, `ally` 1, `enemy`
/// 2 — and the same three ints are what its `team` key writes and its
/// allegiance resolver returns. A faction whose key is none of those three
/// states no team, and none is guessed.
#[must_use]
pub fn measured_team(faction: &ContentId) -> Option<u32> {
    match faction.key() {
        "neutral" => Some(0),
        "ally" => Some(1),
        "enemy" => Some(2),
        _ => None,
    }
}

/// A bound ground target's roster record and the reason it has none
/// (M01-TARGET-ROSTER, #1592).
///
/// The faction is the engine node resolver's own answer for the node's
/// subtree (`0x4bf080` → `0x4bef90`, measured by #1155/#1177), spelled in the
/// loader's vocabulary. The relation is the designed reading of
/// [`declared_player_allegiance`], except that a `neutral` (team 0) node is
/// rostered with **no** relation: nothing measured says the player's flight is
/// opposed to a neutral node, and hostility is not invented for one.
pub fn ground_target_roster(
    target: &crate::mission_targets::BoundTarget,
    player_team: Option<u32>,
) -> (Option<WorldActorRoster>, Option<String>) {
    let team = match &target.allegiance {
        AllegianceBinding::Measured { team, .. } => *team,
        AllegianceBinding::Open(reason) => return (None, Some(reason.clone())),
    };
    let spelling = match team {
        0 => "neutral",
        1 => "ally",
        2 => "enemy",
        other => {
            return (
                None,
                Some(format!(
                    "the resolver answered team {other}, outside the measured neutral/ally/enemy \
                     vocabulary, so no faction is guessed"
                )),
            );
        }
    };
    let faction = match ContentId::from_source(ContentKind::Faction, spelling) {
        Ok(faction) => faction,
        Err(error) => {
            return (
                None,
                Some(format!("the faction cannot be spelled: {error}")),
            );
        }
    };
    let player_allegiance = if team == 0 {
        None
    } else {
        declared_player_allegiance(&faction, player_team)
    };
    (
        Some(WorldActorRoster {
            faction,
            class: TargetClass::WorldObject,
            objective: target.objective,
            player_allegiance,
        }),
        None,
    )
}

/// The designed relation of the player's flight toward a world actor, read
/// from the two measured teams — `#1436`'s role split applied to the
/// world-actor program.
///
/// The inputs are measured: the actor's loader-resolved faction (its team int,
/// #1155/#1177) and the player record's own `team` (`aiv.zrd`, #1453). The
/// **reading** is designed, exactly as `#1436`'s is — a record on the player's
/// own team is the player's flight, any other measured team is opposed — and
/// it is labelled [`M01_ZEPPELIN_ALLEGIANCE_CLAIM`]. A faction outside the
/// measured vocabulary, or a player team that was never read, answers `None`:
/// no side, and therefore no relation, is invented.
#[must_use]
pub fn declared_player_allegiance(
    faction: &ContentId,
    player_team: Option<u32>,
) -> Option<Allegiance> {
    let team = measured_team(faction)?;
    let player = player_team?;
    Some(if team == player {
        Allegiance::Friendly
    } else {
        Allegiance::Hostile
    })
}

/// The claim the designed player↔world-actor allegiance reading is filed
/// under.
pub const M01_ZEPPELIN_ALLEGIANCE_CLAIM: &str =
    "m01-play-parts-geometry.designed-player-allegiance";

/// The record's spawn pose in the simulation frame: measured position in
/// metres (#436) and the measured heading conversion composed through the
/// production `object3d_orientation` `Ry` — the same compose the player's
/// own spawn uses.
fn measured_spawn_pose(record: &ScriptedAircraft) -> Option<(Vec3, Quat)> {
    let stored = record.stored_pose?;
    let [x, y, z, w] =
        object3d_orientation(0.0, f64::from(stored_heading_radians(stored.heading)), 0.0);
    Some((
        Vec3::from_array(stored.position_metres()),
        Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32),
    ))
}

/// The designed damage graph one scripted actor registers: one lethal
/// internal-structure node whose pool is unresolved — a hit routes in and
/// blocks by name rather than draining an invented count — plus, for an actor
/// whose parts this composition binds, one node per part.
///
/// The part nodes are what makes an engine addressable
/// (`super::mission_session::parts`): M01's `INACTIVE` ladders count an engine
/// leaving play, and a hit has to be able to name it. `parts` is empty for
/// every scripted aircraft. Each part node is classed through
/// `mission_session::parts::classed_part_node` against the stage's
/// `ai.zrd` `TURRET` table: a part that table declares is a weapon mount whose
/// destruction disables its own `Weapon` system, anything else stays the
/// unclassed internal-structure node.
fn designed_damage_graph(
    spelling: &str,
    parts: &[DamageNodeKey],
    turrets: &[parts::TurretRecord],
    actor: &str,
) -> Result<(ContentId, DamageGraph, DamagePolicy), String> {
    let (subject, graph, _) = designed_actor_graph(spelling, parts, turrets, actor)?;
    let policy = DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    };
    Ok((subject, graph, policy))
}

/// The graph itself, split out so the part-node composition is checkable on its
/// own: the frame node first, then one node per part, duplicates skipped (two
/// chains of one actor may name one part).
///
/// `actor` is the name the `TURRET` table's `NODES` mount patterns match
/// against: for a zeppelin, the bound subject's own node name (the first
/// element of every chain in its part plan); for a scripted aircraft, the
/// record's name. It is consulted only for a part node, so an actor with no
/// parts never queries the table.
fn designed_actor_graph(
    spelling: &str,
    parts: &[DamageNodeKey],
    turrets: &[parts::TurretRecord],
    actor: &str,
) -> Result<(ContentId, DamageGraph, Vec<DamageNodeKey>), String> {
    let (subject, frame) = designed_frame_node(spelling)?;
    let mut nodes = vec![frame];
    let mut added = Vec::new();
    for key in parts {
        if nodes.iter().any(|node| node.key() == key) {
            continue;
        }
        nodes.push(crate::mission_session::classed_part_node(
            turrets, actor, key,
        ));
        added.push(key.clone());
    }
    let graph = DamageGraph::try_new(subject.clone(), nodes)
        .map_err(|error| format!("the designed damage graph refuses: {error}"))?;
    Ok((subject, graph, added))
}

/// The one lethal internal-structure frame node every scripted actor carries
/// and the graph subject its spelling names.
fn designed_frame_node(spelling: &str) -> Result<(ContentId, DamageNode), String> {
    let subject = ContentId::from_source(ContentKind::Blueprint, spelling)
        .map_err(|error| error.to_string())?;
    let node_key = DamageNodeKey::new(SCRIPTED_DAMAGE_NODE)
        .map_err(|error| format!("the damage node key refuses: {error}"))?;
    let claim = ClaimId::new("m01-actors.damage-pool")
        .map_err(|error| format!("the damage claim id refuses: {error}"))?;
    let integrity = Resolved::unknown(
        claim,
        "no M01 damage table is measured (#668): the scripted actor's integrity pool \
         is unresolved so a routed hit is reported unresolved instead of being absorbed \
         by an invented capacity",
    )
    .map_err(|error| format!("the unresolved pool refuses: {error}"))?;
    let node =
        DamageNode::new(node_key, DamageNodeKind::InternalStructure, integrity).with_lethal(true);
    Ok((subject, node))
}

/// The session's damage authority: a resolver of this generation's own,
/// installed or replaced so registration never lands on a torn-down
/// generation's ledger (a restart installs a new resolver for the new
/// generation, exactly as the weapons session contract spells).
fn session_damage(world: &mut World, session: SessionId) -> &mut DamageResolver {
    if world
        .get_resource::<SessionDamageResolver>()
        .is_none_or(|resolver| resolver.resolver().session() != session)
    {
        world.insert_resource(SessionDamageResolver::new(DamageResolver::new(
            session,
            MISSION_DAMAGE_PRODUCER,
        )));
    }
    world
        .get_resource_mut::<SessionDamageResolver>()
        .expect("the resolver was just installed")
        .into_inner()
        .resolver_mut()
}

/// The host state the spawn reads, lifted out of the resource so the two
/// callers see the same thing: the Startup system whose host sits in the
/// world, and `MissionHost::restart` whose host is a `&mut self` mid-call
/// — freshly relaunched for the new generation — and is not yet a
/// resource again.
///
/// The player's measured team travels here too: the composed restart entry
/// holds the [`MissionStage`] resource out of the world for the whole
/// rebuild, so a spawn that re-read it there would state no side for any
/// record. The stage's own tables travel in [`StageBinding`] instead
/// (M01-PLAY-AI-RESTART, #1509).
struct HostView {
    /// The session the host serves.
    served: SessionId,
    /// The world-actor program's actor/subject pairs, in program order.
    zeppelins: Vec<(cs_script::ir::ActorId, ContentId)>,
    /// Each launched actor's canonical session pose.
    poses: Vec<(
        cs_script::ir::ActorId,
        Option<cs_sim::world_actors::trajectory::Pose>,
    )>,
    /// Each declared actor's targeting fields, in program order: its measured
    /// faction, its measured kind and the objective the program flags it with.
    ///
    /// Read from the lowered program's own registration records (never a
    /// guess), so a bound actor's targeting roster record carries the values
    /// the record states.
    actors: Vec<(cs_script::ir::ActorId, ContentId, WorldActorKind, bool)>,
    /// The player record's own measured `team` (`aiv.zrd`), which the designed
    /// allegiance reading compares a world actor's faction team against. The
    /// caller supplies it, because on the composed restart path the stage is
    /// held out of the world and reading the resource here would lose it.
    player_team: Option<u32>,
}

fn host_view(host: &MissionHost, player_team: Option<u32>) -> HostView {
    let world_actors = host.world_actors();
    HostView {
        served: host.served(),
        zeppelins: world_actors
            .map(|actors| {
                actors
                    .program()
                    .subjects
                    .iter()
                    .map(|(actor, subject)| (*actor, subject.clone()))
                    .collect()
            })
            .unwrap_or_default(),
        poses: world_actors
            .map(|actors| {
                actors
                    .set()
                    .actors()
                    .map(|actor| (actor, actors.set().pose(actor).ok()))
                    .collect()
            })
            .unwrap_or_default(),
        actors: world_actors
            .map(|actors| {
                actors
                    .program()
                    .actors
                    .iter()
                    .map(|spec| {
                        (
                            spec.actor,
                            spec.faction.clone(),
                            spec.kind,
                            spec.objective.is_some(),
                        )
                    })
                    .collect()
            })
            .unwrap_or_default(),
        player_team,
    }
}

/// Spawns the roster's scripted aircraft and binds the launched zeppelin
/// world actors, under the host's served session — the composition's
/// Startup seam, where the host is a resource.
///
/// Every entity is marked so the next teardown or restart takes exactly
/// what this function made. The report it builds is left as
/// [`MissionActorSpawnReport`] for the tests and the evidence record to
/// read.
pub fn spawn_mission_actors(world: &mut World) -> MissionActorSpawnReport {
    let player_team = player_team_of(world.get_resource::<MissionStage>());
    let view = world
        .get_resource::<MissionHost>()
        .map(|host| host_view(host, player_team));
    let Some(view) = view else {
        let mut report = MissionActorSpawnReport::default();
        report
            .refusals
            .push("no MissionHost resource: the composition did not install the host".to_owned());
        world.insert_resource(report.clone());
        return report;
    };
    // The stage is a world resource on this path; the restart hands it in
    // directly (below), because the composed restart entry holds it out of the
    // world for the whole rebuild. The binding carries every stage-derived
    // table the spawn reads, including the AI airframe table that splits
    // flying from held records (M01-PLAY-AI-RESTART, #1509).
    let binding = stage_binding(world.get_resource::<MissionStage>());
    spawn_mission_actors_under(world, view, &binding)
}

/// `MissionHost::restart`'s entry: the host is mid-restart — already
/// serving the fresh session and relaunched world actors — and not a
/// world resource, so the caller hands it by reference. The stage comes in
/// the same way: the composed restart entry (`mission_host_restart`) removes
/// the [`MissionStage`] resource before it rebuilds, so a binder that read the
/// resource here would silently rebind no part-state chain after a retry and
/// every `INACTIVE` ladder over them would stall for the rest of the mission.
/// The same holds for the `TURRET` table: read from the argument, a restarted
/// composition classes its turrets exactly as the first spawn did. And for the
/// AI airframe table: read from the argument, every record that flew before
/// the restart flies after it, with the side its measured team selects
/// (M01-PLAY-AI-RESTART, #1509).
pub fn respawn_mission_actors(
    world: &mut World,
    host: &MissionHost,
    stage: &MissionStage,
) -> MissionActorSpawnReport {
    spawn_mission_actors_under(
        world,
        host_view(host, player_team_of(Some(stage))),
        &stage_binding(Some(stage)),
    )
}

/// The stage records the spawn reads through the one seam its two callers
/// share (M01-PLAY-TRIGGERS-PARTS, #1481; M01-PLAY-TRIGGERS-TURRET-WIRE,
/// #1499): which part-state chains a bound world actor's graph carries, and
/// which `ai.zrd` `TURRET` records class them.
#[derive(Default)]
struct StageBinding {
    /// The part-state chains of every bound world actor, by world actor id.
    parts: BTreeMap<WorldActorId, Vec<(MemberName, DamageNodeKey)>>,
    /// The scope's `TURRET` records, in record order, or empty when no carrier
    /// held the member.
    turrets: Vec<parts::TurretRecord>,
    /// Why no table could be read, when one could not — surfaced on the parts
    /// report instead of being folded into the empty record list above.
    turret_refusal: Option<String>,
    /// The AI airframe table the stage imported for every name-prefix dispatch
    /// ([`crate::mission_ai::MissionAiAirframes`]), or `None` when the
    /// composition carried no stage at all (M01-PLAY-AI-RESTART, #1509).
    ///
    /// The spawn must never re-read this from the world: the composed restart
    /// holds the [`MissionStage`] resource out of the world while it rebuilds,
    /// and a table read from the world there would be empty — every
    /// non-asleep scripted aircraft would spawn the kinematic held body, with
    /// no law and no side, for the rest of the run.
    ai_airframes: Option<crate::mission_ai::MissionAiAirframes>,
    /// The `vehicle.zrd` record a `wingman_1` record's measured
    /// [`SpawnDispatch::WingmanOne`] resolves through — the player airframe
    /// row's `w` pointer (`0x426d60`, finding §1) — carried on the stage's
    /// [`crate::mission_session::MissionFlight`], for the same restart reason
    /// as the airframe table. `None` when the composition has no stage.
    wingman_one_record: Option<String>,
    /// The mission's declared ground targets and the world group's authored
    /// damage set, bound at stage build (M01-BRIDGE-CONVOY-SPAWN, #1566).
    targets: crate::mission_targets::MissionTargetBinding,
}

/// The binding of a stage, or the empty one when the composition has no stage
/// — the world resource is absent on the Startup path before it is installed
/// and on every synthetic composition that declares neither records nor a
/// table.
fn stage_binding(stage: Option<&MissionStage>) -> StageBinding {
    stage.map_or_else(StageBinding::default, |stage| StageBinding {
        parts: parts::plan_part_states(stage),
        turrets: stage.turret_records.clone(),
        turret_refusal: stage.turret_refusal.clone(),
        ai_airframes: Some(stage.ai_airframes.clone()),
        wingman_one_record: Some(stage.flight.wingman_record.clone()),
        targets: stage.host.targets.clone(),
    })
}

/// The player record's own measured `team`, or `None` when the stage carries
/// no player record or its team was never read.
///
/// The value is what the designed allegiance reading
/// ([`declared_player_allegiance`]) compares a world actor's faction team
/// against; a stage without one states no player side.
fn player_team_of(stage: Option<&MissionStage>) -> Option<u32> {
    stage.and_then(|stage| {
        stage
            .aircraft
            .iter()
            .find(|record| record.dispatch == SpawnDispatch::Player)
            .and_then(|record| record.team)
    })
}

fn spawn_mission_actors_under(
    world: &mut World,
    view: HostView,
    binding: &StageBinding,
) -> MissionActorSpawnReport {
    let mut report = MissionActorSpawnReport::default();
    let roster = world
        .get_resource::<MissionAircraftRoster>()
        .cloned()
        .unwrap_or_default();
    let session = view.served;
    report.session = Some(session);
    let zeppelins = view.zeppelins;
    let zeppelin_poses = view.poses;
    let roster_actors = view.actors;
    // The player's own measured team, for the designed allegiance reading
    // (M01-PLAY-PARTS-GEOMETRY) and the AI half's flown/held split
    // (M01-PLAY-AI). It travels in the view so the composed restart — which
    // holds the stage out of the world — keeps it.
    let player_team_of_player = view.player_team;
    let generation = world
        .get_resource::<SceneGenerations>()
        .map(SceneGenerations::latest)
        .unwrap_or_default();
    // The part-state chains of every bound world actor and the `TURRET` table
    // that classes them, read from the stage's own records by the caller —
    // the only place that has the stage: on the Startup path as a world
    // resource, on the restart path as an argument (M01-PLAY-TRIGGERS-PARTS,
    // #1481; M01-PLAY-TRIGGERS-TURRET-WIRE, #1499). A stage whose host carries
    // no world-actor program plans none, and so does every synthetic stage.
    let mut part_report = parts::MissionPartReport::default();
    if let Some(reason) = &binding.turret_refusal {
        part_report.refusals.push(format!(
            "{} cannot supply the TURRET table ({reason}), so no part of this composition is \
             classed a weapon mount: every part keeps its unclassed node",
            crate::mission_world_actors::TURRET_MEMBER
        ));
    }
    let mut allocator = ActorAllocator::new(session);

    // The AI half of the spawn (M01-PLAY-AI, #1436): the airframe table the
    // stage imported for every name-prefix dispatch, and the measured team
    // of the stage's `player` record, which is what splits the roster into
    // the player's flight and the aircraft opposed to it. The table travels
    // in the binding and the team in the view, both from the caller that holds
    // the stage — the Startup path as a world resource, the restart path as
    // its argument — never read from the world here: the composed restart
    // holds the [`MissionStage`] resource out of the world for the whole
    // rebuild, and a table read from the world there would be empty, holding
    // every scripted aircraft for the rest of the run (M01-PLAY-AI-RESTART,
    // #1509).
    let stageless = binding.ai_airframes.is_none();
    let airframes = binding.ai_airframes.clone().unwrap_or_default();
    // The same measured team the allegiance reading below compares against:
    // one reading of the view, never a second opinion.
    let player_team = player_team_of_player;
    let mut ai_flown: Vec<String> = Vec::new();
    let mut ai_held: Vec<String> = Vec::new();
    let mut ai_refusals: Vec<String> = Vec::new();
    if stageless {
        ai_refusals.push(
            "no MissionStage resource: the composition left no AI airframe table, so every \
             scripted aircraft spawns held"
                .to_owned(),
        );
    }
    ai_refusals.extend(airframes.refusals.iter().cloned());

    for record in &roster.0 {
        let name = record.name.clone();
        let Some((position, orientation)) = measured_spawn_pose(record) else {
            report.refusals.push(format!(
                "{name}: the record carries no stored pose, so no spawn pose exists to apply"
            ));
            continue;
        };
        let vehicle_record = match &record.dispatch {
            SpawnDispatch::VehicleRecord(prefix) => prefix.clone(),
            // The `wingman_1` special dispatch resolves through the player
            // airframe row's `w` pointer (measured `0x426d60`, finding §1) —
            // the stage carries it on `MissionFlight::wingman_record`.
            SpawnDispatch::WingmanOne => match &binding.wingman_one_record {
                Some(resolved) => resolved.clone(),
                None => {
                    report.refusals.push(format!(
                        "{name}: the measured `wingman_1` dispatch resolves through the player \
                         airframe row's `w` pointer (`0x426d60`), and this stage carries no \
                         player airframe record to resolve it against"
                    ));
                    continue;
                }
            },
            dispatch => {
                report.refusals.push(format!(
                    "{name}: dispatch {dispatch:?} is not a vehicle-record spawn and this seam \
                     does not build it"
                ));
                continue;
            }
        };
        let asleep = record.starts_asleep.unwrap_or(false);
        // Which records fly (M01-PLAY-AI, #1436): not asleep, and carrying
        // an airframe the stage imported for this record's measured name
        // prefix. Everything else keeps the kinematic held body — a body no
        // law steps must never be released into the world to fall out of
        // the sky under gravity.
        let airframe = if asleep {
            None
        } else {
            airframes.models.get(&vehicle_record)
        };
        if airframe.is_none() {
            let reason = if asleep {
                "the measured `deactivated` field starts the record asleep (#681) and the \
                 mission's own WAKEUP_ENEMIES wake has not released it yet, so it holds its \
                 spawn pose"
            } else {
                "the stage carries no imported airframe for this record's name prefix"
            };
            ai_held.push(format!("{name}: {reason}"));
        }
        let mut spec = BodySpec {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            mode: BodyMode::Kinematic,
            // No declared mass: a kinematic scripted actor integrates
            // nothing, and a later release must declare the measured mass
            // rather than fly a collider-density default.
            mass_kg: 0.0,
            // The playtest's declared development box: no scripted-aircraft
            // collider extent is measured.
            half_extents_m: crate::playtest::scene::AIRCRAFT_HALF_EXTENTS_M,
            position_m: position.to_array(),
            linear_velocity_m_s: [0.0, 0.0, 0.0],
        };
        if let Some(frame) = airframe {
            // A flown aircraft is a dynamic body: the law's one force request
            // must reach an integrator, and the declared mass is the imported
            // weight over the law's own force scale (`W / 9.82`).
            spec.mode = BodyMode::Dynamic;
            spec.mass_kg = spawn_mass_kg(frame);
        }
        let entity = match spawn_body(world, &spec) {
            Ok(entity) => entity,
            Err(error) => {
                report
                    .refusals
                    .push(format!("{name}: the body spec refuses: {error}"));
                continue;
            }
        };
        let actor = match allocator.allocate() {
            Ok(actor) => actor,
            Err(error) => {
                let _ = world.despawn(entity);
                report
                    .refusals
                    .push(format!("{name}: the actor allocator refuses: {error}"));
                continue;
            }
        };
        let graph_spelling = format!(
            "damage-graph.scripted-aircraft.{}",
            name.to_ascii_lowercase()
        );
        match designed_damage_graph(&graph_spelling, &[], &binding.turrets, &name) {
            Ok((subject, graph, policy)) => {
                match session_damage(world, session).register_actor(actor, graph, policy) {
                    Ok(()) => {
                        world.entity_mut(entity).insert(DamageActorBinding {
                            actor,
                            graph: subject,
                            generation,
                        });
                    }
                    Err(error) => report
                        .refusals
                        .push(format!("{name}: damage registration refuses: {error}")),
                }
            }
            Err(reason) => report.refusals.push(format!("{name}: {reason}")),
        }
        world.entity_mut(entity).insert((
            Rotation(orientation),
            Transform::from_translation(position).with_rotation(orientation),
            ScriptedAircraftActor {
                record: record.clone(),
                vehicle_record,
                damage_actor: actor,
                generation,
                asleep,
            },
            bevy::prelude::Visibility::Inherited,
        ));
        if let Some(frame) = airframe {
            // The flown record: the law's declared mass binding, the
            // recovered law on the AI seat, and the designed behaviour the
            // record's measured fields select. The decision system then
            // commands it every fixed tick (`crate::mission_ai`).
            world.entity_mut(entity).insert((
                NoAutoMass,
                ai_flight_record(frame, position, orientation),
                MissionAircraftAi::for_record(record, player_team),
            ));
            ai_flown.push(name.clone());
        }
        report.aircraft.push(SpawnedAircraft {
            record: name,
            entity,
            actor,
            asleep,
        });
    }

    for (world_actor, subject) in &zeppelins {
        let actor = match allocator.allocate() {
            Ok(actor) => actor,
            Err(error) => {
                let refusal =
                    format!("zeppelin {world_actor:?}: the actor allocator refuses: {error}");
                // A bound actor that never spawns binds no part-state chain
                // either, so the parts report names the same refusal.
                part_report.refusals.push(refusal.clone());
                report.refusals.push(refusal);
                continue;
            }
        };
        let pose = zeppelin_poses
            .iter()
            .find(|(candidate, _)| candidate == world_actor)
            .and_then(|(_, pose)| *pose);
        let Some(pose) = pose else {
            let refusal =
                format!("zeppelin {world_actor:?}: the session holds no pose for the actor");
            part_report.refusals.push(refusal.clone());
            report.refusals.push(refusal);
            continue;
        };
        // The targeting roster record this actor presents
        // (M01-PLAY-PARTS-GEOMETRY): its faction and kind verbatim from the
        // lowered program, its objective from the program's own symbol, and
        // the designed relation the player's flight holds toward it, read from
        // the two measured teams. A program that declares no faction for the
        // actor binds none — no relation is invented.
        let roster = roster_actors
            .iter()
            .find(|(candidate, ..)| candidate == world_actor)
            .map(|(_, faction, kind, objective)| WorldActorRoster {
                faction: faction.clone(),
                class: target_class_of(*kind),
                objective: *objective,
                player_allegiance: declared_player_allegiance(faction, player_team_of_player),
            });
        let (position_m, orientation) = (pose.position_m, pose.orientation.0);
        let entity = world
            .spawn((
                Transform::from_translation(Vec3::new(
                    position_m[0] as f32,
                    position_m[1] as f32,
                    position_m[2] as f32,
                ))
                // The session's canonical orientation, not an assumed one:
                // the same `pose.orientation` the component records.
                .with_rotation(Quat::from_xyzw(
                    orientation[0] as f32,
                    orientation[1] as f32,
                    orientation[2] as f32,
                    orientation[3] as f32,
                )),
                bevy::prelude::Visibility::Inherited,
                WorldActorBinding {
                    actor: *world_actor,
                    subject: subject.clone(),
                    generation,
                },
                MissionZeppelinActor {
                    world_actor: *world_actor,
                    damage_actor: actor,
                    generation,
                    position_m,
                    orientation,
                    roster,
                },
            ))
            .id();
        let graph_spelling = format!("damage-graph.zeppelin.{}", subject.key());
        // The part-state chains this actor's own record spells
        // (M01-PLAY-TRIGGERS-PARTS, #1481): their part elements become nodes of
        // this actor's graph, so a hit can name an engine. Each node is classed
        // against the stage's `TURRET` table by the graph builder, under the
        // name the table's `NODES` mount patterns match — the bound subject's
        // node name, which is the first element of every planned chain. A
        // planned chain always has that element, so the empty fallback below is
        // only reached by an actor with no parts, whose nodes never consult the
        // table.
        let planned = binding.parts.get(world_actor).cloned().unwrap_or_default();
        let part_keys: Vec<DamageNodeKey> = planned.iter().map(|(_, node)| node.clone()).collect();
        let actor_name = planned
            .first()
            .and_then(|(chain, _)| chain.first().map(String::as_str))
            .unwrap_or_default();
        match designed_damage_graph(&graph_spelling, &part_keys, &binding.turrets, actor_name) {
            Ok((graph_subject, graph, policy)) => {
                let part_nodes: Vec<DamageNodeKey> = graph
                    .nodes()
                    .map(|node| node.key().clone())
                    .filter(|key| part_keys.contains(key))
                    .collect();
                match session_damage(world, session).register_actor(actor, graph, policy) {
                    Ok(()) => {
                        world.entity_mut(entity).insert(DamageActorBinding {
                            actor,
                            graph: graph_subject,
                            generation,
                        });
                        for (chain, node) in &planned {
                            let part_entity = parts::spawn_part_state(
                                world,
                                actor,
                                subject.clone(),
                                generation,
                                chain.clone(),
                                node.clone(),
                                position_m,
                            );
                            part_report.parts.push(parts::BoundPart {
                                chain: chain.clone(),
                                node: node.clone(),
                                actor,
                                entity: part_entity,
                            });
                        }
                        if !part_nodes.is_empty() {
                            part_report.nodes.insert(actor, part_nodes);
                        }
                    }
                    Err(error) => {
                        let refusal = format!(
                            "zeppelin {world_actor:?}: damage registration refuses: {error}"
                        );
                        part_report.refusals.push(refusal.clone());
                        report.refusals.push(refusal);
                    }
                }
            }
            Err(reason) => {
                let refusal = format!("zeppelin {world_actor:?}: {reason}");
                part_report.refusals.push(refusal.clone());
                report.refusals.push(refusal);
            }
        }
        report.zeppelins.push(BoundZeppelin {
            world_actor: *world_actor,
            subject: subject.clone(),
            entity,
            actor,
        });
    }

    // The mission's declared ground targets and the world group's authored
    // damage set (M01-BRIDGE-CONVOY-SPAWN, #1566): one damageable actor per
    // bound target, at the composed world position the binding read. The node
    // name is the damage node key a hit addresses the target by, so a gun
    // round or a mission effect can name it; the authored `healthy`/
    // `destroyed` layout and coupling fields ride on the component, read and
    // never interpreted.
    let mut target_report = MissionGroundTargetReport {
        session: Some(session),
        ..MissionGroundTargetReport::default()
    };
    for target in &binding.targets.targets {
        let node = match crate::mission_targets::target_damage_node(target) {
            Ok(node) => node,
            Err(reason) => {
                target_report
                    .refusals
                    .push(format!("{}: {reason}", target.node));
                continue;
            }
        };
        if target.damage_key().is_err() {
            target_report.refusals.push(format!(
                "{}: the node name is not a damage node key",
                target.node
            ));
            continue;
        }
        let subject = match ContentId::from_source(
            ContentKind::Blueprint,
            &format!("world-target.{}", target.node),
        ) {
            Ok(subject) => subject,
            Err(error) => {
                target_report
                    .refusals
                    .push(format!("{}: {error}", target.node));
                continue;
            }
        };
        let actor = match allocator.allocate() {
            Ok(actor) => actor,
            Err(error) => {
                target_report.refusals.push(format!(
                    "{}: the actor allocator refuses: {error}",
                    target.node
                ));
                continue;
            }
        };
        let graph = match DamageGraph::try_new(subject.clone(), vec![node]) {
            Ok(graph) => graph,
            Err(error) => {
                target_report.refusals.push(format!(
                    "{}: the damage graph refuses: {error}",
                    target.node
                ));
                continue;
            }
        };
        let policy = DamagePolicy {
            attribution: AttributionRule::FirstLethalHit,
        };
        match session_damage(world, session).register_actor(actor, graph, policy) {
            Ok(()) => {}
            Err(error) => {
                target_report.refusals.push(format!(
                    "{}: damage registration refuses: {error}",
                    target.node
                ));
                continue;
            }
        }
        let (roster, allegiance_open) = ground_target_roster(target, player_team_of_player);
        let [x, y, z] = target.position_m;
        let entity = world
            .spawn((
                Transform::from_translation(Vec3::new(x as f32, y as f32, z as f32)),
                bevy::prelude::Visibility::Inherited,
                DamageActorBinding {
                    actor,
                    graph: subject,
                    generation,
                },
                MissionGroundTarget {
                    node: target.node.clone(),
                    declared: target.declared,
                    damage_actor: actor,
                    generation,
                    position_m: target.position_m,
                    healthy: target.healthy.clone(),
                    destroyed: target.destroyed.clone(),
                    coupling: target.coupling.fields.clone(),
                    objective: target.objective,
                    other_target: target.other_target,
                    roster,
                    allegiance_open,
                },
            ))
            .id();
        target_report.targets.push(BoundGroundTarget {
            node: target.node.clone(),
            declared: target.declared,
            position_m: target.position_m,
            entity,
            actor,
        });
    }
    // Every refusal the binding itself recorded (a selector the container
    // could not answer, a source it could not read) reaches this report by
    // name, so a target nobody bound is visible instead of absent.
    target_report
        .refusals
        .extend(binding.targets.refusals.iter().cloned());
    world.insert_resource(target_report);

    // The AI spawn report: which records fly the designed law, which are
    // held and why, and every refusal on this path. It replaces any report a
    // previous generation left, so a restart reports its own spawn.
    world.insert_resource(MissionAiReport {
        flown: ai_flown,
        held: ai_held,
        woken: Vec::new(),
        refusals: ai_refusals,
        armed: Vec::new(),
        decisions: 0,
        no_target: 0,
        fire_orders: 0,
    });

    // The pending fire intents go with the generation they were decided
    // for: a restart's respawn must not publish the previous generation's
    // asks into the fresh session's first tick, where the actors they name
    // no longer exist. The plugin initialises the resource only when it is
    // missing, so a respawn has to replace it the way it replaces the
    // report above.
    world.insert_resource(MissionAiFireOrders::default());

    world.insert_resource(report.clone());
    // The part-state binding's own report: which chains got a live entity and
    // which damage nodes their actors' graphs carry (M01-PLAY-TRIGGERS-PARTS).
    // Its refusals are the bound-actor ones above, pushed at their own sites:
    // a refusal that stopped a scripted aircraft has nothing to say about the
    // part-state chains, so it is deliberately not copied here.
    world.insert_resource(part_report);
    report
}

/// The composed Startup entry: [`spawn_mission_actors`] as a system.
pub fn mission_actor_spawn_system(world: &mut World) {
    spawn_mission_actors(world);
}

/// Despawns every entity [`spawn_mission_actors`] made — the restart and
/// teardown share this list.
pub fn despawn_mission_actors(world: &mut World) {
    let entities: Vec<Entity> = {
        use bevy::prelude::With;
        let mut query = world.query_filtered::<Entity, bevy::prelude::Or<(
            With<ScriptedAircraftActor>,
            With<MissionZeppelinActor>,
            With<MissionGroundTarget>,
            With<parts::MissionPartState>,
        )>>();
        query.iter(world).collect()
    };
    for entity in entities {
        let _ = world.despawn(entity);
    }
    world.remove_resource::<parts::MissionPartReport>();
    world.remove_resource::<MissionGroundTargetReport>();
    world.remove_resource::<MissionZeppelinAllegianceReport>();
}

/// Resource: what [`roster_mission_zeppelins`] decided on its last pass.
///
/// A report the acceptance tests read: which bound world actors the player's
/// targeting store holds, under which measured faction, and under which
/// designed relation — and every actor whose side could not be read, by name.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionZeppelinAllegianceReport {
    /// Every rostered world actor, in ascending session-actor order: the
    /// session damage actor, its measured faction and the designed relation
    /// the player's flight holds toward it (`None` when no side could be read).
    pub rostered: Vec<(ActorId, ContentId, Option<Allegiance>)>,
    /// Every bound world actor the pass could not settle, with why — verbatim.
    pub unresolved: Vec<String>,
}

impl MissionZeppelinAllegianceReport {
    /// The relation the player's flight holds toward `actor`, per the last
    /// pass, or `None` when the actor is not rostered.
    #[must_use]
    pub fn allegiance_toward(&self, actor: ActorId) -> Option<Option<Allegiance>> {
        self.rostered
            .iter()
            .find(|(candidate, ..)| *candidate == actor)
            .map(|(_, _, allegiance)| *allegiance)
    }
}

/// Rosters every bound world actor with the session's targeting store, under
/// its measured faction and the designed relation
/// (M01-PLAY-PARTS-GEOMETRY).
///
/// M01's zeppelins declare no `team` the player's gun could compare against
/// (#1481's finding), so nothing rostered them and the F27 gun step's own
/// `(shooter, target)` relation check answered `None` for every part — an
/// **undeclared pair**, which the designed `FriendlyFireRule::HostileOnly`
/// refuses by name. This pass closes that: each bound actor's `TargetRecord`
/// (its measured faction, class and objective flag, at its bind pose) joins the
/// session's roster, and the relation the pair check reads is declared from the
/// designed reading of the two measured teams
/// ([`declared_player_allegiance`]).
///
/// Idempotent: a pass over a store that already holds the actor re-declares the
/// same relation and adds no second record, so it runs every fixed tick and a
/// restart (which opens a fresh store and re-spawns the actors) re-rosters
/// exactly the new generation. A world with no targeting store yet — the
/// composition opens one at `PostStartup` — does nothing: there is no roster to
/// write into, and the next tick has one.
pub fn roster_mission_zeppelins(world: &mut World) {
    let Some(targeting) = world.get_resource::<TargetingSession>() else {
        return;
    };
    let generation = targeting.generation();

    let mut candidates: Vec<(ActorId, WorldActorRoster, [f64; 3])> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    {
        let mut query = world.query::<(Entity, &MissionZeppelinActor)>();
        for (_, bound) in query.iter(world) {
            if bound.generation != generation {
                continue;
            }
            match &bound.roster {
                Some(roster) => {
                    candidates.push((bound.damage_actor, roster.clone(), bound.position_m));
                }
                None => unresolved.push(format!(
                    "world actor {:?}: the launched program declares no registration record, so \
                     its faction is unread and no relation is invented",
                    bound.world_actor
                )),
            }
        }
    }
    candidates.sort_by_key(|(actor, ..)| *actor);

    let Ok(player_faction) = ContentId::from_source(ContentKind::Faction, MISSION_FACTION_PLAYER)
    else {
        return;
    };
    let mut report = MissionZeppelinAllegianceReport {
        rostered: Vec::new(),
        unresolved,
    };
    {
        let mut targeting = world.resource_mut::<TargetingSession>();
        let store = targeting.store_mut();
        for (actor, roster, position_m) in candidates {
            if let Some(allegiance) = roster.player_allegiance {
                // The pair the gun step's relation check reads is
                // `(shooter, target)`, so the player's side of the relation is
                // the load-bearing one; the reverse is declared with it so the
                // two directions cannot disagree.
                store.set_allegiance(player_faction.clone(), roster.faction.clone(), allegiance);
                store.set_allegiance(roster.faction.clone(), player_faction.clone(), allegiance);
            }
            if !store.is_registered(&actor) {
                let Ok(position) = WorldPosition::try_new(position_m) else {
                    report.unresolved.push(format!(
                        "session actor {actor:?}: the bind pose is not a finite world position"
                    ));
                    continue;
                };
                let record = TargetRecord {
                    actor,
                    faction: roster.faction.clone(),
                    class: roster.class,
                    objective: roster.objective,
                    revealed: true,
                    phase_eligible: true,
                    position,
                };
                if let Err(error) = store.register(record) {
                    report.unresolved.push(format!(
                        "session actor {actor:?}: the targeting store refuses the record: {error}"
                    ));
                    continue;
                }
            }
            report
                .rostered
                .push((actor, roster.faction.clone(), roster.player_allegiance));
        }
    }
    world.insert_resource(report);
}

/// Resource: what [`roster_mission_ground_targets`] decided on its last pass
/// (M01-TARGET-ROSTER, #1592).
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionGroundTargetRosterReport {
    /// Every rostered ground target, in ascending session-actor order: the
    /// session damage actor, its measured faction and the designed relation
    /// the player's flight holds toward it (`None` for a `neutral` node, or
    /// when the player's own team was never read).
    pub rostered: Vec<(ActorId, ContentId, Option<Allegiance>)>,
    /// Every bound ground target the pass could not roster, with why.
    pub unresolved: Vec<String>,
}

impl MissionGroundTargetRosterReport {
    /// The faction `actor` is rostered under, per the last pass.
    #[must_use]
    pub fn faction_of(&self, actor: ActorId) -> Option<&ContentId> {
        self.rostered
            .iter()
            .find(|(candidate, ..)| *candidate == actor)
            .map(|(_, faction, _)| faction)
    }
}

/// Rosters every bound ground target with the session's targeting store under
/// the faction the engine's node resolver answers for it
/// (M01-TARGET-ROSTER, #1592), so the player's reticle and selection can name
/// a convoy truck.
///
/// A target whose resolver answer is open is reported by name and **not**
/// rostered. Idempotent and generation-scoped exactly like
/// [`roster_mission_zeppelins`]; a world with no targeting store yet does
/// nothing.
pub fn roster_mission_ground_targets(world: &mut World) {
    let Some(targeting) = world.get_resource::<TargetingSession>() else {
        return;
    };
    let generation = targeting.generation();
    let mut candidates: Vec<(ActorId, WorldActorRoster, [f64; 3])> = Vec::new();
    let mut unresolved: Vec<String> = Vec::new();
    {
        let mut query = world.query::<&MissionGroundTarget>();
        for target in query.iter(world) {
            if target.generation != generation {
                continue;
            }
            match &target.roster {
                Some(roster) => {
                    candidates.push((target.damage_actor, roster.clone(), target.position_m));
                }
                None => unresolved.push(format!(
                    "ground target {}: {}",
                    target.node,
                    target
                        .allegiance_open
                        .as_deref()
                        .unwrap_or("no roster record was decided")
                )),
            }
        }
    }
    candidates.sort_by_key(|(actor, ..)| *actor);
    let Ok(player_faction) = ContentId::from_source(ContentKind::Faction, MISSION_FACTION_PLAYER)
    else {
        return;
    };
    let mut report = MissionGroundTargetRosterReport {
        rostered: Vec::new(),
        unresolved,
    };
    {
        let mut targeting = world.resource_mut::<TargetingSession>();
        let store = targeting.store_mut();
        for (actor, roster, position_m) in candidates {
            if let Some(allegiance) = roster.player_allegiance {
                store.set_allegiance(player_faction.clone(), roster.faction.clone(), allegiance);
                store.set_allegiance(roster.faction.clone(), player_faction.clone(), allegiance);
            }
            if !store.is_registered(&actor) {
                let Ok(position) = WorldPosition::try_new(position_m) else {
                    report.unresolved.push(format!(
                        "session actor {actor:?}: the bind pose is not a finite world position"
                    ));
                    continue;
                };
                let record = TargetRecord {
                    actor,
                    faction: roster.faction.clone(),
                    class: roster.class,
                    objective: roster.objective,
                    revealed: true,
                    phase_eligible: true,
                    position,
                };
                if let Err(error) = store.register(record) {
                    report.unresolved.push(format!(
                        "session actor {actor:?}: the targeting store refuses the record: {error}"
                    ));
                    continue;
                }
            }
            report
                .rostered
                .push((actor, roster.faction.clone(), roster.player_allegiance));
        }
    }
    world.insert_resource(report);
}

/// Installs [`roster_mission_zeppelins`] on the fixed tick, so a bound world
/// actor is in the targeting store before the same tick's weapon step reads
/// the `(shooter, target)` relation.
#[derive(Clone, Copy, Debug, Default)]
pub struct MissionZeppelinAllegiancePlugin;

impl Plugin for MissionZeppelinAllegiancePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MissionZeppelinAllegianceReport>()
            .init_resource::<MissionGroundTargetRosterReport>()
            .add_systems(
                FixedPreUpdate,
                (roster_mission_zeppelins, roster_mission_ground_targets),
            );
    }
}

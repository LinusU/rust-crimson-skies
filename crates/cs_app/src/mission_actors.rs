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
//! Teardown: every entity spawned here carries [`ScriptedAircraftActor`] or
//! [`MissionZeppelinActor`]; `MissionHost::restart` despawns both kinds and
//! respawns them through the same function, and `teardown` removes them
//! with the rest of the composition.

use std::collections::BTreeMap;

use avian3d::prelude::{NoAutoMass, Rotation};
use bevy::prelude::{Component, Entity, Quat, Resource, Transform, Vec3, World};
use cs_script::ir::{ActorId as WorldActorId, MemberName};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::damage::{
    AttributionRule, DamageGraph, DamageNode, DamageNodeKey, DamageNodeKind, DamagePolicy,
    DamageResolver,
};
use cs_types::content::{ContentId, ContentKind, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::{ActorAllocator, ActorId, SessionId};

use crate::damage::DamageActorBinding;
use crate::mission_ai::{MissionAiReport, MissionAircraftAi, ai_flight_record, spawn_mass_kg};
use crate::mission_session::{MissionHost, MissionStage, parts};
use crate::mission_start::{ScriptedAircraft, SpawnDispatch, stored_heading_radians};
use crate::mission_world_actors::object3d_orientation;
use crate::physics::{BodyMode, BodySpec, spawn_body};
use crate::scene::SceneGenerations;
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
    /// The `vehicle.zrd` registry prefix the measured dispatch names —
    /// **designed** as an airframe/model selection: #1453 measured the
    /// name-prefix lookup, not which record the prefix binds.
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
/// pose.
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
}

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
/// whose parts this composition binds, one designed node per part.
///
/// The part nodes are what makes an engine addressable
/// (`super::mission_session::parts`): M01's `INACTIVE` ladders count an engine
/// leaving play, and a hit has to be able to name it. `parts` is empty for
/// every scripted aircraft.
fn designed_damage_graph(
    spelling: &str,
    parts: &[DamageNodeKey],
) -> Result<(ContentId, DamageGraph, DamagePolicy), String> {
    let (subject, graph, _) = designed_actor_graph(spelling, parts)?;
    let policy = DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    };
    Ok((subject, graph, policy))
}

/// The graph itself, split out so the part-node composition is checkable on its
/// own: the frame node first, then one designed node per part, duplicates
/// skipped (two chains of one actor may name one part).
fn designed_actor_graph(
    spelling: &str,
    parts: &[DamageNodeKey],
) -> Result<(ContentId, DamageGraph, Vec<DamageNodeKey>), String> {
    let (subject, frame) = designed_frame_node(spelling)?;
    let mut nodes = vec![frame];
    let mut added = Vec::new();
    for key in parts {
        if nodes.iter().any(|node| node.key() == key) {
            continue;
        }
        nodes.push(crate::mission_session::designed_part_node(key));
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
}

fn host_view(host: &MissionHost) -> HostView {
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
    let view = world.get_resource::<MissionHost>().map(host_view);
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
    // world for the whole rebuild.
    let part_plan = part_plan_of(world.get_resource::<MissionStage>());
    spawn_mission_actors_under(world, view, &part_plan)
}

/// `MissionHost::restart`'s entry: the host is mid-restart — already
/// serving the fresh session and relaunched world actors — and not a
/// world resource, so the caller hands it by reference. The stage comes in
/// the same way: the composed restart entry (`mission_host_restart`) removes
/// the [`MissionStage`] resource before it rebuilds, so a binder that read the
/// resource here would silently rebind no part-state chain after a retry and
/// every `INACTIVE` ladder over them would stall for the rest of the mission.
pub fn respawn_mission_actors(
    world: &mut World,
    host: &MissionHost,
    stage: &MissionStage,
) -> MissionActorSpawnReport {
    spawn_mission_actors_under(world, host_view(host), &parts::plan_part_states(stage))
}

/// The part-state plan of a stage, or the empty plan when the composition has
/// no stage — or the stage's host carries no bound world-actor program.
fn part_plan_of(
    stage: Option<&MissionStage>,
) -> BTreeMap<WorldActorId, Vec<(MemberName, DamageNodeKey)>> {
    stage.map(parts::plan_part_states).unwrap_or_default()
}

fn spawn_mission_actors_under(
    world: &mut World,
    view: HostView,
    part_plan: &BTreeMap<WorldActorId, Vec<(MemberName, DamageNodeKey)>>,
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
    let generation = world
        .get_resource::<SceneGenerations>()
        .map(SceneGenerations::latest)
        .unwrap_or_default();
    // The part-state chains of every bound world actor, read from the stage's
    // own records through the container resolver (M01-PLAY-TRIGGERS-PARTS,
    // #1481) by the caller, which is the only place that has the stage: on the
    // Startup path as a world resource, on the restart path as an argument. A
    // stage whose host carries no world-actor program plans none, and so does
    // every synthetic stage.
    let mut part_report = parts::MissionPartReport::default();
    let mut allocator = ActorAllocator::new(session);

    // The AI half of the spawn (M01-PLAY-AI, #1436): the airframe table the
    // stage imported for every name-prefix dispatch, and the measured team
    // of the stage's `player` record, which is what splits the roster into
    // the player's flight and the aircraft opposed to it.
    let stage = world.get_resource::<MissionStage>();
    let airframes = stage
        .map(|stage| stage.ai_airframes.clone())
        .unwrap_or_default();
    let player_team = stage.and_then(|stage| {
        stage
            .aircraft
            .iter()
            .find(|record| record.dispatch == SpawnDispatch::Player)
            .and_then(|record| record.team)
    });
    let mut ai_flown: Vec<String> = Vec::new();
    let mut ai_held: Vec<String> = Vec::new();
    let mut ai_refusals: Vec<String> = Vec::new();
    if stage.is_none() {
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
                "the measured `deactivated` field starts the record asleep (#681) and no \
                 WAKEUP_ENEMIES consumer exists yet, so it holds its spawn pose"
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
        match designed_damage_graph(&graph_spelling, &[]) {
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
                },
            ))
            .id();
        let graph_spelling = format!("damage-graph.zeppelin.{}", subject.key());
        // The part-state chains this actor's own record spells
        // (M01-PLAY-TRIGGERS-PARTS, #1481): their part elements become nodes of
        // this actor's graph, so a hit can name an engine.
        let planned = part_plan.get(world_actor).cloned().unwrap_or_default();
        let part_keys: Vec<DamageNodeKey> = planned.iter().map(|(_, node)| node.clone()).collect();
        match designed_damage_graph(&graph_spelling, &part_keys) {
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

    // The AI spawn report: which records fly the designed law, which are
    // held and why, and every refusal on this path. It replaces any report a
    // previous generation left, so a restart reports its own spawn.
    world.insert_resource(MissionAiReport {
        flown: ai_flown,
        held: ai_held,
        refusals: ai_refusals,
        armed: Vec::new(),
        decisions: 0,
        no_target: 0,
        fire_orders: 0,
    });

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
            With<parts::MissionPartState>,
        )>>();
        query.iter(world).collect()
    };
    for entity in entities {
        let _ = world.despawn(entity);
    }
    world.remove_resource::<parts::MissionPartReport>();
}

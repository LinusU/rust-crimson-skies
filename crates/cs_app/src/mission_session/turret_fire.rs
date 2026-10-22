//! Turret fire: the woken turrets' rounds in the weapon runtime (Rally #1882,
//! FLAK-B3).
//!
//! FLAK-B2's [`TurretSystem`] decides *that* a turret fires and in which
//! direction ([`TurretFireIntent`]); this module turns the intent into a real
//! shot:
//!
//! * **Cannon turrets** (`t_truck**` name `wep_29`, a record with `CANNON`)
//!   are registered with the session's [`WeaponSession`] as weapon actors of
//!   their own, with a one-gun loadout built from the retail record
//!   (`VELOCITY`, `RANGE`, `FIRE_RATE`, `ARMOR_DAMAGE`, `HEALTH_DAMAGE`,
//!   `AMMO_LIMIT`, exactly as the player's `wep_50` is by
//!   [`selected_gun_loadout`]). Each intent aims the turret's mount at the
//!   intent's direction and mints a [`WeaponOrder::Fire`] that rides the
//!   mission's own weapon publication
//!   ([`append_turret_fire_orders`]). The round is the F27 step's, and its
//!   hit on the player's swept hull box resolves through the session's one
//!   [`SessionDamageResolver`](crate::weapons::SessionDamageResolver).
//! * **Rocket turrets** (`aagun**`/`maingun*` name `wep_27`) fly a
//!   [`FlakRound`] from the retail record. Its burst is recorded; applying the
//!   splash to the player is *not* modelled (the falloff and the
//!   `entity+0x678` factor are unmeasured), and no M01 program wakes such a
//!   turret.
//!
//! The line of sight is a physics ray against the static world
//! ([`SightAnswer`]); the muzzle's own enclosing static colliders and the
//! player are excluded. Everything the law leaves unknown stays in
//! [`TurretFireReport::unresolved`].

use std::collections::BTreeMap;
use std::path::Path;

use avian3d::prelude::{LinearVelocity, Position, RigidBody, SpatialQuery, SpatialQueryFilter};
use bevy::prelude::{
    ChildOf, Component, Dir3, Entity, GlobalTransform, Query, Res, ResMut, Resource, Transform,
    Vec3, With, World,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_sim::damage::ActorId;
use cs_sim::targeting::{TargetClass, TargetRecord};
use cs_sim::weapons::{FireIntent, FireIntentId};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
use cs_types::space::WorldPosition;

use super::flak_round::{FlakRecord, FlakRound, FlakStep, OrdnanceBurst};
use super::gun_sound::{READER_ARCHIVE, WEAPONS_MEMBER};
use super::host::MissionHost;
use super::launch_sound::reader_member;
use super::player_victim::PlayerVictim;
use super::weapon_counts::{
    OriginalGunRecord, ammo_limit_from_document, float_from_document, gun_record_from_document,
    record_has_key,
};
use super::weapons::{MISSION_FACTION_HOSTILE, MissionWeapons, selected_gun_loadout};
use super::{MissionPlayerBody, MissionStage};
use crate::mission_turret_update::{
    MountPose, SightAnswer, TargetCandidate, TargetKind, TurretConfig, TurretFireIntent,
    TurretFrame, TurretSystem,
};
use crate::mission_turrets::{TurretEntry, TurretSpec};
use crate::physics::PhysicsTickLedger;
use crate::scene::NodeVisualTransform;
use crate::targeting::TargetingSession;
use crate::weapons::{
    MountPoseBinding, SessionDamageResolver, WeaponActorBinding, WeaponOrder, WeaponSession,
    WeaponTickInput,
};

/// The producer serial of the turrets' fire intents (the AI's is 205).
pub const TURRET_FIRE_PRODUCER: u32 = 206;

/// The first serial of the turrets' weapon actors. The scripted actors
/// allocate upwards from 1 and the player holds `u64::MAX`; a turret takes
/// `BASE + its position in the turret list`.
pub const TURRET_ACTOR_SERIAL_BASE: u64 = 1 << 40;

/// The id the player carries as a [`TargetCandidate`].
pub const PLAYER_CANDIDATE_ID: u64 = 1;

/// The mount key a cannon turret's gun hangs on (the selected gun's nose
/// mount, [`super::MISSION_GUN_MOUNT_NOSE`]).
const TURRET_MOUNT_KEY: &str = super::weapons::MISSION_GUN_MOUNT_NOSE;

/// What the turret's `WEAPON` record is, read from `weapons.zrd`.
#[derive(Clone, Debug, PartialEq)]
pub enum TurretWeaponRecord {
    /// A record with `CANNON` (`wep_29`): fires through the gun runtime.
    Cannon(TurretCannon),
    /// A record with `ROCKET` (`wep_27`): flies as a [`FlakRound`].
    Rocket(FlakRecord),
}

/// The retail numbers of a cannon record.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretCannon {
    /// The record's name.
    pub record: String,
    /// `FIRE_RATE`, `VELOCITY`, `RANGE` and the damage pair.
    pub gun: OriginalGunRecord,
    /// `AMMO_LIMIT`: the rounds the turret's gun starts with.
    pub ammo_limit: u64,
    /// `CANNON_SPREAD` as the record states it (the `-cos` of an angle in the
    /// engine); read, not applied: its unit and whether it is a half angle
    /// are unmeasured.
    pub cannon_spread: Option<f64>,
}

/// Reads every `WEAPON` the specs name from a decoded `weapons.zrd`, and the
/// reason each could not be classified or read.
#[must_use]
pub fn turret_weapons_from_document(
    weapons: &ZrdValue,
    specs: &[TurretSpec],
) -> (BTreeMap<String, TurretWeaponRecord>, Vec<String>) {
    let mut records = BTreeMap::new();
    let mut refusals = Vec::new();
    for name in specs.iter().filter_map(|spec| spec.weapon.as_deref()) {
        if records.contains_key(name) {
            continue;
        }
        let record = if record_has_key(weapons, name, "CANNON") {
            gun_record_from_document(weapons, name)
                .and_then(|gun| {
                    Ok(TurretCannon {
                        record: name.to_owned(),
                        gun,
                        ammo_limit: ammo_limit_from_document(weapons, name)?,
                        cannon_spread: float_from_document(weapons, name, "CANNON_SPREAD").ok(),
                    })
                })
                .map(TurretWeaponRecord::Cannon)
                .map_err(|error| error.to_string())
        } else if record_has_key(weapons, name, "ROCKET") {
            FlakRecord::from_document_record(weapons, name)
                .map(TurretWeaponRecord::Rocket)
                .map_err(|error| error.to_string())
        } else {
            Err(format!("{name} has neither CANNON nor ROCKET"))
        };
        match record {
            Ok(record) => {
                records.insert(name.to_owned(), record);
            }
            Err(reason) => refusals.push(format!("turret weapon {name}: {reason}")),
        }
    }
    (records, refusals)
}

/// [`turret_weapons_from_document`] over the installation's `weapons.zrd`.
#[must_use]
pub fn read_turret_weapons(
    install_root: &Path,
    specs: &[TurretSpec],
) -> (BTreeMap<String, TurretWeaponRecord>, Vec<String>) {
    let read = reader_member(install_root, READER_ARCHIVE, WEAPONS_MEMBER)
        .map_err(|reason| reason.to_string())
        .and_then(|bytes| decode_zrd(&bytes).map_err(|error| format!("{WEAPONS_MEMBER}: {error}")));
    match read {
        Ok(weapons) => turret_weapons_from_document(&weapons, specs),
        Err(reason) => (
            BTreeMap::new(),
            vec![format!("turret weapons unreadable: {reason}")],
        ),
    }
}

/// Component: a cannon turret's weapon-actor body. Its transform is the
/// muzzle, and it looks along the last fire direction.
#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct TurretBody {
    /// The turret entry's name.
    pub name: String,
}

/// One shot a turret took.
#[derive(Clone, Debug, PartialEq)]
pub struct TurretShot {
    /// The tick of the intent.
    pub tick: u64,
    /// The turret's name.
    pub name: String,
    /// The weapon record fired.
    pub weapon: String,
    /// The muzzle, metres.
    pub muzzle_m: [f64; 3],
    /// The unit direction.
    pub direction: [f64; 3],
}

/// What the turret fire did, and what it could not.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TurretFireReport {
    /// The turrets built into the runtime.
    pub built: Vec<String>,
    /// Every entry or record the runtime could not build, verbatim.
    pub refusals: Vec<String>,
    /// Every shot taken, in order.
    pub shots: Vec<TurretShot>,
    /// Fire orders handed to the weapon publication.
    pub cannon_orders: u64,
    /// Rocket rounds fired.
    pub rockets_fired: u64,
    /// Bursts of rocket rounds, with no splash applied.
    pub rocket_bursts: Vec<OrdnanceBurst>,
    /// Ticks the player's line of sight was asked and the ray answered.
    pub sight_queries: u64,
    /// Ticks a ray found the player's line blocked.
    pub sight_blocked: u64,
    /// The unknowns the shots still depend on.
    pub unresolved: Vec<&'static str>,
}

/// The unknowns every shot depends on.
pub const UNRESOLVED: [&str; 5] = [
    "PARTS firepoint offsets: the muzzle is the mount node's origin",
    "the mount's yaw/frame convention and the PITCH/YAW travel: the aim is the intent's direction",
    "weapon flag 0x40 (lead aiming): the direction is target minus muzzle plus the INACCURACY cone",
    "CANNON_SPREAD unit and shape: the gun keeps the mission's designed spread cone",
    "the HEALTH unit and rocket splash falloff: rocket bursts apply no damage",
];

struct Slot {
    ledger_index: usize,
    name: String,
    weapon: SlotWeapon,
}

enum SlotWeapon {
    Cannon {
        actor: ActorId,
        entity: Entity,
        record: String,
    },
    Rocket {
        record: FlakRecord,
        name: String,
    },
}

/// Resource: the turret runtime of one mission session.
#[derive(Resource)]
pub struct MissionTurretFire {
    session: SessionId,
    generation: crate::scene::SceneGeneration,
    system: TurretSystem,
    slots: Vec<Slot>,
    rounds: Vec<FlakRound>,
    orders: Vec<ActorId>,
    sequence: u32,
    report: TurretFireReport,
}

impl MissionTurretFire {
    /// The report so far.
    #[must_use]
    pub const fn report(&self) -> &TurretFireReport {
        &self.report
    }

    /// The turret system, for its runtimes' states.
    #[must_use]
    pub const fn system(&self) -> &TurretSystem {
        &self.system
    }

    /// Fire orders waiting for the weapon publication.
    #[must_use]
    pub fn pending_orders(&self) -> &[ActorId] {
        &self.orders
    }
}

fn player_team(stage: &MissionStage) -> Option<u32> {
    stage
        .aircraft
        .iter()
        .find(|record| record.dispatch == crate::mission_start::SpawnDispatch::Player)
        .and_then(|record| record.team)
}

/// Builds the turret runtime once the weapon session, the player's body and
/// the host exist, and again for a new session generation.
pub fn build_mission_turrets(world: &mut World) {
    let Some(mission) = world.get_resource::<MissionWeapons>().cloned() else {
        return;
    };
    if world.get_resource::<WeaponSession>().is_none()
        || world.get_resource::<TargetingSession>().is_none()
    {
        return;
    }
    if world
        .get_resource::<MissionTurretFire>()
        .is_some_and(|fire| {
            fire.session == mission.session() && fire.generation == mission.generation()
        })
    {
        return;
    }
    let Some(ticks) = world
        .get_resource::<PhysicsTickLedger>()
        .map(|ledger| ledger.ticks)
    else {
        return;
    };
    let (Some(host), Some(stage)) = (
        world.get_resource::<MissionHost>(),
        world.get_resource::<MissionStage>(),
    ) else {
        return;
    };
    let entries: Vec<TurretEntry> = host.wake_turrets().entries().to_vec();
    let bound = stage.host.turrets.clone();
    let mission_seed = host.environment().seeds().mission_seed();
    let mut report = TurretFireReport {
        unresolved: UNRESOLVED.to_vec(),
        ..TurretFireReport::default()
    };
    report.refusals.extend(bound.refusals.iter().cloned());
    let hz = crate::mission_world_actors::SESSION_TICKS_PER_SECOND;

    // Stale bodies of a previous generation go first.
    let stale: Vec<Entity> = world
        .query_filtered::<Entity, With<TurretBody>>()
        .iter(world)
        .collect();
    for entity in stale {
        if let Ok(target) = world.get_entity_mut(entity) {
            target.despawn();
        }
    }

    let mut built: Vec<(TurretEntry, TurretConfig)> = Vec::new();
    let mut slots: Vec<Slot> = Vec::new();
    for (ledger_index, entry) in entries.iter().enumerate() {
        let Some(position) = entry.position_m else {
            report.refusals.push(format!(
                "{}: a zeppelin part's mount moves with its aircraft and is not bound",
                entry.name
            ));
            continue;
        };
        let Some(spec) = bound.specs.iter().find(|spec| spec.index == entry.record) else {
            report
                .refusals
                .push(format!("{}: its TURRET record is not carried", entry.name));
            continue;
        };
        let Some(weapon_name) = spec.weapon.as_deref() else {
            report
                .refusals
                .push(format!("{}: its record names no WEAPON", entry.name));
            continue;
        };
        let Some(record) = bound.weapons.get(weapon_name) else {
            report.refusals.push(format!(
                "{}: weapon {weapon_name} was not read (see the weapon refusals)",
                entry.name
            ));
            continue;
        };
        let config = match TurretConfig::from_spec(spec, None, vec![[0.0, 0.0, 0.0]]) {
            Ok(config) => config,
            Err(refusals) => {
                for refusal in refusals {
                    report
                        .refusals
                        .push(format!("{}: {}", entry.name, refusal.0));
                }
                continue;
            }
        };
        let slot_index = slots.len() as u64;
        let weapon = match record {
            TurretWeaponRecord::Cannon(cannon) => {
                match spawn_cannon(
                    world,
                    &mission,
                    &entry.name,
                    position,
                    slot_index,
                    cannon,
                    hz,
                ) {
                    Ok((actor, entity)) => SlotWeapon::Cannon {
                        actor,
                        entity,
                        record: cannon.record.clone(),
                    },
                    Err(reason) => {
                        report.refusals.push(format!("{}: {reason}", entry.name));
                        continue;
                    }
                }
            }
            TurretWeaponRecord::Rocket(rocket) => SlotWeapon::Rocket {
                record: rocket.clone(),
                name: weapon_name.to_owned(),
            },
        };
        report.built.push(entry.name.clone());
        slots.push(Slot {
            ledger_index,
            name: entry.name.clone(),
            weapon,
        });
        built.push((entry.clone(), config));
    }
    let system = TurretSystem::new(mission_seed, hz, ticks, built);
    world.insert_resource(MissionTurretFire {
        session: mission.session(),
        generation: mission.generation(),
        system,
        slots,
        rounds: Vec::new(),
        orders: Vec::new(),
        sequence: 0,
        report,
    });
}

/// Registers one cannon turret as a weapon actor and spawns its body.
fn spawn_cannon(
    world: &mut World,
    mission: &MissionWeapons,
    name: &str,
    position_m: [f64; 3],
    slot_index: u64,
    cannon: &TurretCannon,
    hz: u32,
) -> Result<(ActorId, Entity), String> {
    let actor = ActorId {
        session: mission.session(),
        serial: TURRET_ACTOR_SERIAL_BASE + slot_index,
    };
    let loadout = selected_gun_loadout(&cannon.gun, hz);
    world
        .get_resource_mut::<WeaponSession>()
        .ok_or("no weapon session")?
        .register(
            actor,
            &loadout.guns,
            loadout.bank.clone(),
            cannon.ammo_limit,
        )
        .map_err(|error| format!("the weapon session refuses the guns: {error}"))?;
    let faction = ContentId::from_source(ContentKind::Faction, MISSION_FACTION_HOSTILE)
        .map_err(|error| format!("the faction id refuses: {error}"))?;
    let world_position = WorldPosition::try_new(position_m)
        .map_err(|error| format!("the mount is not a world position: {error}"))?;
    world
        .get_resource_mut::<TargetingSession>()
        .ok_or("no targeting session")?
        .store_mut()
        .register(TargetRecord {
            actor,
            faction,
            class: TargetClass::WorldObject,
            objective: false,
            // A turret is a shooter here, not a selectable target: nothing
            // in the player's target list changes.
            revealed: false,
            phase_eligible: false,
            position: world_position,
        })
        .map_err(|error| format!("the targeting store refuses the turret: {error}"))?;

    let at = Vec3::new(
        position_m[0] as f32,
        position_m[1] as f32,
        position_m[2] as f32,
    );
    let transform = Transform::from_translation(at);
    let body = world
        .spawn((
            TurretBody {
                name: name.to_owned(),
            },
            transform,
            GlobalTransform::from(transform),
            LinearVelocity(Vec3::ZERO),
            WeaponActorBinding {
                actor,
                guns: loadout.guns.iter().map(|gun| gun.gun().clone()).collect(),
                loadout: loadout.ids.loadout.clone(),
                generation: mission.generation(),
            },
        ))
        .id();
    let mount = cs_content::damage::DamageNodeKey::new(TURRET_MOUNT_KEY)
        .map_err(|error| error.to_string())?;
    world.spawn((
        MountPoseBinding {
            mount,
            generation: mission.generation(),
        },
        NodeVisualTransform(GlobalTransform::from(transform)),
        Transform::IDENTITY,
        ChildOf(body),
    ));
    Ok((actor, body))
}

/// One fixed tick of the turrets: the update law over the live bytes the
/// wake ledger holds, the player as the only target, and the physics ray as
/// the line of sight. Cannon intents aim their body and queue a fire order;
/// rocket intents fire a [`FlakRound`]; rocket rounds fly.
#[allow(clippy::too_many_arguments)]
pub fn step_mission_turrets(
    fire: Option<ResMut<MissionTurretFire>>,
    host: Option<Res<MissionHost>>,
    stage: Option<Res<MissionStage>>,
    ledger: Option<Res<PhysicsTickLedger>>,
    spatial: SpatialQuery,
    players: Query<(Entity, &Position), With<MissionPlayerBody>>,
    bodies: Query<&RigidBody>,
    mut turret_bodies: Query<(&mut Transform, &mut GlobalTransform), With<TurretBody>>,
    mut mounts: Query<(&ChildOf, &mut NodeVisualTransform)>,
    damage: Option<Res<SessionDamageResolver>>,
    victim: Option<Res<PlayerVictim>>,
) {
    let (Some(mut fire), Some(host), Some(stage), Some(ledger)) = (fire, host, stage, ledger)
    else {
        return;
    };
    let Some((player_entity, player_position)) = players.iter().next() else {
        return;
    };
    let Some(team) = player_team(&stage) else {
        if !fire
            .report
            .refusals
            .iter()
            .any(|r| r.starts_with("player team"))
        {
            fire.report.refusals.push(
                "player team: the stage's player record states no TEAM, so no turret has a \
                 hostile side to engage"
                    .to_owned(),
            );
        }
        return;
    };
    let tick = ledger.ticks;
    let player_m = [
        f64::from(player_position.0.x),
        f64::from(player_position.0.y),
        f64::from(player_position.0.z),
    ];
    let player_alive = match (damage.as_deref(), victim.as_deref()) {
        (Some(damage), Some(victim)) => !damage.resolver().is_destroyed(&victim.actor),
        _ => true,
    };

    // The live bytes the wake ledger holds.
    let entries = host.wake_turrets().entries();
    let live: Vec<(usize, bool)> = fire
        .slots
        .iter()
        .enumerate()
        .map(|(index, slot)| {
            (
                index,
                entries
                    .get(slot.ledger_index)
                    .is_some_and(|entry| entry.live),
            )
        })
        .collect();
    for (index, live) in live {
        fire.system.set_live(index, live);
    }

    let mount_poses: Vec<MountPose> = fire
        .slots
        .iter()
        .map(|slot| {
            let entry = &entries[slot.ledger_index];
            MountPose {
                position_m: entry.position_m.unwrap_or([0.0; 3]),
                yaw_rad: 0.0,
                active: true,
            }
        })
        .collect();
    let candidates = [TargetCandidate {
        id: PLAYER_CANDIDATE_ID,
        kind: TargetKind::Vehicle,
        team,
        position_m: player_m,
        is_player: true,
        vehicle_class4: false,
        accepts: player_alive,
    }];
    let frame = TurretFrame {
        tick,
        candidates: &candidates,
        player_team: team,
        include_structs_flag: false,
        player_flag: false,
        freeze_timers: false,
    };

    let mut queries = 0u64;
    let mut blocked = 0u64;
    let mut sight = |from: [f64; 3], to: [f64; 3]| {
        queries += 1;
        let answer = line_of_sight(&spatial, &bodies, player_entity, from, to);
        if answer == SightAnswer::Blocked {
            blocked += 1;
        }
        answer
    };
    let intents = fire.system.update(&frame, &mount_poses, &mut sight);
    fire.report.sight_queries += queries;
    fire.report.sight_blocked += blocked;

    for intent in intents {
        take_shot(
            &mut fire,
            &intent,
            &mut turret_bodies,
            &mut mounts,
            player_m,
        );
    }

    // Rocket rounds fly.
    let dt = 1.0 / f64::from(crate::mission_world_actors::SESSION_TICKS_PER_SECOND);
    let mut bursts = Vec::new();
    fire.rounds
        .retain_mut(|round| match round.step(Tick(tick), dt, Some(player_m)) {
            FlakStep::Flying => true,
            FlakStep::Burst(burst) => {
                bursts.push(burst);
                false
            }
            FlakStep::SilentEnd => false,
        });
    fire.report.rocket_bursts.extend(bursts);
}

fn take_shot(
    fire: &mut MissionTurretFire,
    intent: &TurretFireIntent,
    turret_bodies: &mut Query<(&mut Transform, &mut GlobalTransform), With<TurretBody>>,
    mounts: &mut Query<(&ChildOf, &mut NodeVisualTransform)>,
    _player_m: [f64; 3],
) {
    let Some(slot) = fire.slots.get(intent.turret) else {
        return;
    };
    let muzzle = Vec3::new(
        intent.muzzle_m[0] as f32,
        intent.muzzle_m[1] as f32,
        intent.muzzle_m[2] as f32,
    );
    let direction = Vec3::new(
        intent.direction[0] as f32,
        intent.direction[1] as f32,
        intent.direction[2] as f32,
    );
    let name = slot.name.clone();
    match &slot.weapon {
        SlotWeapon::Cannon {
            actor,
            entity,
            record,
        } => {
            let (actor, entity, record) = (*actor, *entity, record.clone());
            let Ok(forward) = Dir3::new(direction) else {
                fire.report
                    .refusals
                    .push(format!("{name}: the fire direction is not a direction"));
                return;
            };
            if let Ok((mut transform, mut global)) = turret_bodies.get_mut(entity) {
                *transform = Transform::from_translation(muzzle).looking_to(forward, Vec3::Y);
                *global = GlobalTransform::from(*transform);
                for (parent, mut node) in mounts.iter_mut() {
                    if parent.parent() == entity {
                        node.0 = *global;
                    }
                }
            }
            fire.orders.push(actor);
            fire.report.cannon_orders += 1;
            fire.report.shots.push(TurretShot {
                tick: intent.tick,
                name,
                weapon: record,
                muzzle_m: intent.muzzle_m,
                direction: intent.direction,
            });
        }
        SlotWeapon::Rocket {
            record,
            name: weapon,
        } => {
            let weapon = weapon.clone();
            match FlakRound::fire(record, intent.muzzle_m, intent.direction) {
                Ok(round) => {
                    fire.rounds.push(round);
                    fire.report.rockets_fired += 1;
                    fire.report.shots.push(TurretShot {
                        tick: intent.tick,
                        name,
                        weapon,
                        muzzle_m: intent.muzzle_m,
                        direction: intent.direction,
                    });
                }
                Err(refusal) => fire
                    .report
                    .refusals
                    .push(format!("{name}: the rocket refuses to fire: {refusal:?}")),
            }
        }
    }
}

/// The ray from a turret's muzzle to the player: blocked by a static body
/// that does not enclose the muzzle.
fn line_of_sight(
    spatial: &SpatialQuery,
    bodies: &Query<&RigidBody>,
    player: Entity,
    from: [f64; 3],
    to: [f64; 3],
) -> SightAnswer {
    let origin = Vec3::new(from[0] as f32, from[1] as f32, from[2] as f32);
    let target = Vec3::new(to[0] as f32, to[1] as f32, to[2] as f32);
    let Ok(direction) = Dir3::new(target - origin) else {
        return SightAnswer::Clear;
    };
    let length = (target - origin).length();
    let mut excluded = vec![player];
    excluded.extend(spatial.point_intersections(origin, &SpatialQueryFilter::default()));
    let filter = SpatialQueryFilter::default().with_excluded_entities(excluded);
    let is_static = |entity: Entity| bodies.get(entity).is_ok_and(|body| body.is_static());
    if spatial
        .cast_ray_predicate(origin, direction, length, true, &filter, &is_static)
        .is_some()
    {
        SightAnswer::Blocked
    } else {
        SightAnswer::Clear
    }
}

/// Joins this tick's turret fire to the mission's own weapon publication, the
/// way [`crate::mission_ai::append_ai_fire_orders`] does for the aircraft.
pub fn append_turret_fire_orders(world: &mut World, input: &mut WeaponTickInput) {
    let Some(mut fire) = world.get_resource_mut::<MissionTurretFire>() else {
        return;
    };
    if fire.orders.is_empty() {
        return;
    }
    let pending = std::mem::take(&mut fire.orders);
    let first = fire.sequence;
    fire.sequence = fire.sequence.wrapping_add(pending.len() as u32);
    for (offset, shooter) in pending.into_iter().enumerate() {
        input.orders.push(WeaponOrder::Fire(FireIntent {
            id: FireIntentId {
                session: input.session,
                tick: input.at,
                producer: TURRET_FIRE_PRODUCER,
                sequence: first + offset as u32,
            },
            shooter,
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::SceneGeneration;

    fn text(s: &str) -> ZrdValue {
        ZrdValue::Text(s.to_owned())
    }

    fn number(key: &str, value: f32) -> [ZrdValue; 2] {
        [text(key), ZrdValue::List(vec![ZrdValue::Float(value)])]
    }

    fn flag(key: &str) -> [ZrdValue; 2] {
        [text(key), ZrdValue::List(Vec::new())]
    }

    fn record(name: &str, numbers: &[(&str, f32)], flags: &[&str]) -> Vec<ZrdValue> {
        let mut body = Vec::new();
        for (key, value) in numbers {
            body.extend(number(key, *value));
        }
        for key in flags {
            body.extend(flag(key));
        }
        vec![text(name), ZrdValue::List(body)]
    }

    fn document() -> ZrdValue {
        let shared = [
            ("FIRE_RATE", 3.0),
            ("VELOCITY", 850.0),
            ("RANGE", 900.0),
            ("ARMOR_DAMAGE", 10.0),
            ("HEALTH_DAMAGE", 10.0),
        ];
        let mut cannon = shared.to_vec();
        cannon.push(("CANNON_SPREAD", 6.0));
        let mut rocket = shared.to_vec();
        rocket.extend([("DETONATION_DISTANCE", 20.0), ("IMPACT_PROXIMITY", 22.0)]);
        let mut all = Vec::new();
        let mut wep_29 = record("wep_29", &cannon, &["CANNON"]);
        if let Some(ZrdValue::List(body)) = wep_29.last_mut() {
            body.extend([text("AMMO_LIMIT"), ZrdValue::List(vec![ZrdValue::Int(400)])]);
        }
        all.extend(wep_29);
        all.extend(record("wep_27", &rocket, &["ROCKET", "LOCK_ON"]));
        all.extend(record("wep_28", &shared, &[]));
        ZrdValue::List(vec![ZrdValue::List(all)])
    }

    fn spec(index: usize, weapon: &str) -> TurretSpec {
        TurretSpec {
            index,
            nodes: Vec::new(),
            activated: None,
            team: None,
            detection_range_m: None,
            shoot_up_only: None,
            fire_rate: None,
            fire_limits: None,
            inaccuracy: None,
            attack_interval: None,
            bored_interval: None,
            pitch: None,
            yaw: None,
            effect: None,
            ammo: None,
            weapon: Some(weapon.to_owned()),
            other_keys: Vec::new(),
        }
    }

    #[test]
    fn accept_flak_b3_weapon_records_classify_cannon_from_rocket_and_refuse_the_rest() {
        let specs = [spec(0, "wep_29"), spec(1, "wep_27"), spec(2, "wep_28")];
        let (records, refusals) = turret_weapons_from_document(&document(), &specs);
        let Some(TurretWeaponRecord::Cannon(cannon)) = records.get("wep_29") else {
            panic!("wep_29 is a cannon: {records:?}");
        };
        assert_eq!(cannon.gun.velocity_mps, 850.0);
        assert_eq!(cannon.gun.health_damage, 10.0);
        assert_eq!(cannon.ammo_limit, 400);
        assert_eq!(cannon.cannon_spread, Some(6.0));
        let Some(TurretWeaponRecord::Rocket(rocket)) = records.get("wep_27") else {
            panic!("wep_27 is a rocket: {records:?}");
        };
        assert_eq!(rocket.detonation_distance_m, 20.0);
        // A record that is neither is refused by name, not guessed.
        assert!(!records.contains_key("wep_28"));
        assert_eq!(refusals.len(), 1, "{refusals:?}");
        assert!(refusals[0].contains("wep_28"), "{refusals:?}");
    }

    fn runtime_with_orders(orders: Vec<ActorId>) -> MissionTurretFire {
        MissionTurretFire {
            session: SessionId::new(7).expect("nonzero"),
            generation: SceneGeneration(1),
            system: TurretSystem::new(1, 60, 0, Vec::new()),
            slots: Vec::new(),
            rounds: Vec::new(),
            orders,
            sequence: 0,
            report: TurretFireReport::default(),
        }
    }

    #[test]
    fn accept_flak_b3_fire_orders_ride_the_mission_publication_under_the_turret_producer() {
        let session = SessionId::new(7).expect("nonzero");
        let actor = |serial| ActorId {
            session,
            serial: TURRET_ACTOR_SERIAL_BASE + serial,
        };
        let mut world = World::new();
        world.insert_resource(runtime_with_orders(vec![actor(0), actor(3)]));
        let mut input = super::super::weapons::mission_tick_input(
            session,
            Tick(5),
            ActorId {
                session,
                serial: u64::MAX,
            },
            [0.0; 3],
            Vec::new(),
        );
        append_turret_fire_orders(&mut world, &mut input);
        let fired: Vec<(u32, u32, ActorId)> = input
            .orders
            .iter()
            .map(|order| match order {
                WeaponOrder::Fire(intent) => {
                    (intent.id.producer, intent.id.sequence, intent.shooter)
                }
                other => panic!("only fire orders: {other:?}"),
            })
            .collect();
        assert_eq!(
            fired,
            vec![
                (TURRET_FIRE_PRODUCER, 0, actor(0)),
                (TURRET_FIRE_PRODUCER, 1, actor(3))
            ]
        );
        // Drained: a second publication carries none, and numbering goes on.
        assert!(
            world
                .resource::<MissionTurretFire>()
                .pending_orders()
                .is_empty()
        );
        world
            .resource_mut::<MissionTurretFire>()
            .orders
            .push(actor(1));
        let mut next = super::super::weapons::mission_tick_input(
            session,
            Tick(6),
            ActorId {
                session,
                serial: u64::MAX,
            },
            [0.0; 3],
            Vec::new(),
        );
        append_turret_fire_orders(&mut world, &mut next);
        assert_eq!(next.orders.len(), 1);
        match &next.orders[0] {
            WeaponOrder::Fire(intent) => assert_eq!(intent.id.sequence, 2),
            other => panic!("only fire orders: {other:?}"),
        }
    }
}

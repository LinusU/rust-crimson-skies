//! The world's own observation of the live composition (M01-PLAY-TRIGGERS-
//! OBSERVE, Rally #1480): the **production writer** for
//! [`MissionWorldObservation`].
//!
//! [`super::host::MissionHost::step`] already folds a report from that port in
//! stage 4 — the port has existed since VS-M01-RT-MISSION-HOST — but no stage
//! ever wrote one, so every real run folded only the record player's animation
//! rows and [`MissionHostRefusal::WorldObservation`] stood over an empty
//! world half. This module is the observation system the port was waiting
//! for: [`mission_world_observation`] reads the **live ECS world** once per
//! fixed tick and writes the report the host consumes on the same tick. It is
//! installed beside [`super::host::install_mission_host`] in
//! [`FixedPostUpdate`](bevy::prelude::FixedPostUpdate), after the physics step
//! of the tick and before [`super::host::mission_host_tick`], so a row always
//! describes the pose that tick's integration just committed.
//!
//! # What is observed, and from where
//!
//! | Row | Source | Why that source |
//! | --- | --- | --- |
//! | `["player"]` | the [`MissionPlayerBody`] entity's [`Position`] | the body's physics pose is the authoritative position the trigger has to measure from; nothing else in the composition is `player` |
//! | `["<node>"]` per bound world actor | the [`WorldActorBinding`] entity's [`Transform`] | the binding entity is not a physics body, so its `Transform` is its live pose; its node name comes from the measured subject spelling (below) |
//! | one row per group id | live [`ScriptedAircraftActor`] entities, keyed by the groups the composition **actually spawned** | a count is only reportable for a group whose members were spawned and can now be counted |
//! | one row per bound part-state chain | the [`MissionPartState`] entity's own pose, with the presence the session's damage authority reports | a part-state chain now has a live entity (M01-PLAY-TRIGGERS-PARTS, below); its in-play bit is the one the `INACTIVE` evaluator reads |
//!
//! M01's 39 part-state chains (`["workersvoyagezep", "reng1", "healthy"]`, …)
//! get their entities from [`super::parts`] through the spawn host — the
//! composition binds one per chain beside the bound world actor that owns it —
//! and their presence from the
//! session's damage authority: `Intact`/`Damaged` is in play, `Destroyed` is the
//! `OutOfPlay` an `INACTIVE` ladder counts, and an unresolved pool or an
//! unregistered actor is **no row at all** (the fail-closed read
//! [`super::parts`] documents).
//!
//! # Fail-closed, row by row
//!
//! * **The player** is reported only when exactly one [`MissionPlayerBody`]
//!   exists *and* it carries a [`Position`]. Zero bodies, two bodies or a body
//!   the physics has not given a pose yet is no `player` row at all — a
//!   condition reading `["player"]` then answers `false` rather than reading a
//!   position from a world that did not describe one.
//! * **A bound actor** is reported only when its subject is a `scene_node`
//!   id whose last component is a name [`escape_scene_node_name`] could have
//!   written verbatim (see [`subject_node_name`]). A subject this observer
//!   cannot read back into a node name yields no row.
//! * **A group** is reported only for a group id the composition spawned
//!   members for (`MissionActorSpawnReport` × [`MissionAircraftRoster`]), and
//!   its count is the live entity count of that group — `0` when every member
//!   is gone, which is a measured answer and not a default. A group nothing
//!   spawned (M01's `2` and `4`) gets **no row**, never `0`:
//!   [`WorldFactTable::facts`] then reports no key and
//!   `Condition::EnemyGroupDepletion` answers `false` ("an unrecorded group is
//!   unknown, not an empty one").
//!
//! # What is not claimed
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original`. The binding entity's
//! `Transform` is the pose [`super::mission_actors`] bound it at, and nothing
//! in this composition moves it afterwards — M01's `zeppelins.zrd` records
//! carry no route (`DeclaredMotion::Held`, `MOTION_RESIDUE`), so session pose
//! and binding pose agree; whether a driven world actor's binding follows its
//! session is unmeasured and is named in
//! `docs/findings/2026-10-15-m01-play-triggers-observation.md`.

use std::collections::BTreeMap;

use avian3d::prelude::Position;
use bevy::prelude::{Entity, Transform, With, World};
use cs_content::scene::escape_scene_node_name;
use cs_types::content::{ContentId, ContentKind};

use super::compose::MissionPlayerBody;
use super::host::{MissionHost, MissionWorldObservation};
use crate::mission_actors::{
    MissionActorSpawnReport, MissionAircraftRoster, ScriptedAircraftActor,
};
use crate::weapons::SessionDamageResolver;
use crate::world_actors::WorldActorBinding;
use crate::world_facts::{MemberObservation, WorldObservation, member_chain};

use super::parts::{MissionPartState, part_in_play};

/// The member chain the record spells for the player's own body.
///
/// This is the measured spelling: M01's `TRAVELERS` subject is `["player"]`,
/// `zeppelins.zrd`'s `targets` name the `player` too, and `aiv.zrd` names the
/// player's record `player` (`SpawnDispatch::Player`). It is **not** a world
/// node — no node of that name exists anywhere in `C1C`, which is why
/// `MemberResolver` leaves the chain unresolved and the block that reads it
/// only ever answered through an observation (`docs/findings/2026-10-08-m01-lc-world-facts.md`).
pub const PLAYER_MEMBER: &str = "player";

/// Reads the node name a `scene_node` subject spells, or `None` when it
/// cannot be read back.
///
/// The subject is `<container key>.<escaped name-path>`
/// ([`cs_content::scene::SceneNodeId`]), so the node's own name is the last
/// `.`-separated component of the key — the escaping never writes a `.`.
/// The component is then checked against the escaping itself: it is accepted
/// only when [`escape_scene_node_name`] maps it onto itself, which is
/// exactly the condition for the component to *be* an authored name rather
/// than an escape sequence this module would have to decode. This module
/// therefore never re-implements the reversible rule — it only reads names
/// the rule guarantees are already plain, and refuses everything else.
///
/// `None` (no row, fail-closed) covers: a subject outside the `scene_node`
/// namespace, an empty key, and a name that was escaped (`-2e` for `.`, …)
/// — a name this observer cannot spell back verbatim is not guessed.
#[must_use]
pub fn subject_node_name(subject: &ContentId) -> Option<&str> {
    if subject.kind() != ContentKind::SceneNode {
        return None;
    }
    let key = subject.key();
    if key.is_empty() {
        return None;
    }
    let last = key.rsplit('.').next()?;
    (escape_scene_node_name(last) == last).then_some(last)
}

/// The one composed observation of this tick: what the live ECS world holds
/// for the player, for every bound world actor, and for every group the
/// composition spawned members for.
///
/// Installed beside [`super::host::install_mission_host`] in
/// [`FixedPostUpdate`](bevy::prelude::FixedPostUpdate) after
/// `PhysicsSystems::StepSimulation` and before
/// [`super::host::mission_host_tick`], so the report it writes is consumed by
/// that very tick's host step. Each run **replaces** the port's content
/// (never merges into it), so a report always describes one tick — the port's
/// own rule.
///
/// A world that carries no [`MissionHost`] runs nothing: there is no host to
/// consume the report, and a report nobody consumes is a stale one. The same
/// "no session, no host" rule the rest of the composition follows.
pub fn mission_world_observation(world: &mut World) {
    if !world.contains_resource::<MissionHost>() {
        return;
    }
    let mut observation = WorldObservation::new();

    // 1. The player: exactly one body, exactly one pose.
    let bodies: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
        query.iter(world).collect()
    };
    if let [body] = bodies.as_slice()
        && let Some(position) = world.get::<Position>(*body)
    {
        let [x, y, z] = position.0.to_array();
        observation = observation.member(
            member_chain([PLAYER_MEMBER]),
            MemberObservation::held_in_play([f64::from(x), f64::from(y), f64::from(z)]),
        );
    }

    // 2. Every bound world actor, at its live pose, under the node name its
    //    measured subject spells.
    let bindings: Vec<(ContentId, [f64; 3])> = {
        let mut query = world.query::<(&WorldActorBinding, &Transform)>();
        query
            .iter(world)
            .map(|(binding, transform)| {
                let [x, y, z] = transform.translation.to_array();
                (
                    binding.subject.clone(),
                    [f64::from(x), f64::from(y), f64::from(z)],
                )
            })
            .collect()
    };
    for (subject, position) in bindings {
        if let Some(name) = subject_node_name(&subject) {
            observation = observation.member(
                member_chain([name]),
                MemberObservation::held_in_play(position),
            );
        }
    }

    // 2b. Every bound part-state chain, at its own entity's pose, with the
    //     presence the session's damage authority reports. An unresolved pool
    //     is not a presence statement, so such a chain gets no row at all.
    let parts: Vec<(MissionPartState, [f64; 3])> = {
        let mut query = world.query::<(&MissionPartState, &Transform)>();
        query
            .iter(world)
            .map(|(part, transform)| {
                let [x, y, z] = transform.translation.to_array();
                (part.clone(), [f64::from(x), f64::from(y), f64::from(z)])
            })
            .collect()
    };
    if !parts.is_empty()
        && let Some(damage) = world.get_resource::<SessionDamageResolver>()
    {
        let resolver = damage.resolver();
        for (part, position) in parts {
            match part_in_play(resolver, &part) {
                Some(true) => {
                    observation = observation.member(
                        part.chain.clone(),
                        MemberObservation::held_in_play(position),
                    );
                }
                Some(false) => {
                    observation = observation.member(
                        part.chain.clone(),
                        MemberObservation::no_longer_in_play(position),
                    );
                }
                // An unresolved pool is not a statement about presence: the
                // chain stays unobserved, fail-closed.
                None => {}
            }
        }
    }

    // 3. Groups: which ids the composition actually spawned members for, and
    //    how many of them are still in the world.
    let spawned: Vec<String> = world
        .get_resource::<MissionActorSpawnReport>()
        .map(|report| {
            report
                .aircraft
                .iter()
                .map(|row| row.record.clone())
                .collect()
        })
        .unwrap_or_default();
    if !spawned.is_empty() {
        let roster = world
            .get_resource::<MissionAircraftRoster>()
            .map(|roster| roster.0.clone())
            .unwrap_or_default();
        let mut living: BTreeMap<i32, u32> = BTreeMap::new();
        for record in &roster {
            if !spawned.contains(&record.name) {
                // The record never spawned, so its group was never observed:
                // an unobservable group stays unobserved, never 0.
                continue;
            }
            if let Some(group) = record.group
                && let Ok(id) = i32::try_from(group)
            {
                living.entry(id).or_insert(0);
            }
        }
        let mut query = world.query::<&ScriptedAircraftActor>();
        for actor in query.iter(world) {
            if let Some(group) = actor.record.group
                && let Ok(id) = i32::try_from(group)
                && let Some(count) = living.get_mut(&id)
            {
                *count += 1;
            }
        }
        for (id, count) in living {
            observation = observation.group(id, count);
        }
    }

    world.insert_resource(MissionWorldObservation { observation });
}

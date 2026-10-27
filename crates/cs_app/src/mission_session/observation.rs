//! The world's own observation of the live composition (M01-PLAY-TRIGGERS-
//! OBSERVE, Rally #1480): the **production writer** for
//! [`MissionWorldObservation`].
//!
//! [`super::host::MissionHost::step`] already folds a report from that port in
//! stage 4 — the port has existed since VS-M01-RT-MISSION-HOST — but no stage
//! ever wrote one, so every real run folded only the record player's animation
//! rows and [`MissionHostRefusal::WorldObservation`](super::host::MissionHostRefusal::WorldObservation) stood over an empty
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
//! | `["<node>"]` per bound world actor | the [`WorldActorBinding`] entity's [`Transform`], with the presence the world's own state reports (M01-PLAY-TRIGGERS-ACTOR-BIT, below) | the binding entity is not a physics body, so its `Transform` is its live pose; its node name comes from the measured subject spelling (below), and whether it is *in play* is the world-actor session's lifecycle × the damage authority's destruction ledger — never a default |
//! | one row per group id | live [`ScriptedAircraftActor`] entities, keyed by the groups the composition **fully spawned** | a count is only reportable when every enabled record of a group spawned; a partial spawn is an unknown count, not a smaller one |
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
//! # The bound actor's in-play bit (M01-PLAY-TRIGGERS-ACTOR-BIT, #1490)
//!
//! M01's two actor-level `INACTIVE` ladders — block indexes 53 and 55,
//! `OBJECTIVE54`/`OBJECTIVE56`, threshold 1 over the **one-element** chain
//! `["piratezep"]` — count the zeppelin's *own* node leaving play, not a
//! part's. Before this slice the actor row was written `held_in_play` from the
//! binding alone and nothing could ever take it out, so both ladders stalled
//! forever. [`world_actor_in_play`] is the measured read that replaces the
//! default, and it consults two fact families, **never the actor's damage
//! pool** (an unresolved pool is not a presence statement):
//!
//! * the **world-actor session's lifecycle** — `Presence::Intact` is the
//!   world's own statement the actor is still in the world; `Presence::Destroyed`
//!   (monotonic, written only by the session's `Destroy` command) is the
//!   measured destruction;
//! * the **session damage authority's destruction ledger** — `Destroyed`
//!   recorded there, by a lethal hit or by the resolver's own scripted port
//!   `DamageResolver::record_lifecycle`, is the authoritative record of an
//!   actor's destruction (F36-C.1) and takes the row out of play even while
//!   the session presence still says `Intact`.
//!
//! A binding neither family speaks to gets **no row at all** — the fail-closed
//! half, and the reason the row can never claim an actor is in play that the
//! world does not hold. See
//! `docs/findings/2026-10-15-m01-play-triggers-actor-bit.md` for the
//! measurement behind both families and what stays unmeasured.
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
//!   written verbatim (see [`subject_node_name`]), **and** the world's own
//!   state holds the actor: the world-actor session must carry its lifecycle
//!   presence ([`world_actor_in_play`]). A subject this observer cannot read
//!   back into a node name — or a binding the world's lifecycle state does
//!   not back — yields no row.
//! * **A group** is reported only once the actor spawn has run, for a group id
//!   some `aiv.zrd` record carries, and its count is the live entity count of
//!   that group — `0` when every member is gone. A group whose only records
//!   are the original's `enabled == 0` skips (M01's `2` and `4`) is `0` too:
//!   the original's counter `0x4658d0` counts registry members and a skipped
//!   record never enters the registry
//!   (`docs/findings/2026-10-15-m01-play-triggers-group-membership.md`). A
//!   group whose records are all enabled is observed only when **every** such
//!   record spawned: one the composition refused leaves the registry count
//!   unknown, and a fraction of the group's members is an undercount, not a
//!   count (`docs/findings/2026-10-16-m01-play-triggers-group-refusal.md`).
//!   A group no record carries gets **no row**, and
//!   `Condition::EnemyGroupDepletion` over it answers `false` ("an unrecorded
//!   group is unknown, not an empty one").
//!
//! # What is not claimed
//!
//! No original executable ran; `retail` is read access to the owner's
//! installation. Nothing here is `verified_original`. The binding entity's
//! `Transform` is the pose [`crate::mission_actors`] bound it at, and nothing
//! in this composition moves it afterwards — M01's `zeppelins.zrd` records
//! carry no route (`DeclaredMotion::Held`, `MOTION_RESIDUE`), so session pose
//! and binding pose agree; whether a driven world actor's binding follows its
//! session is unmeasured and is named in
//! `docs/findings/2026-10-15-m01-play-triggers-observation.md`.

use std::collections::{BTreeMap, BTreeSet};

use avian3d::prelude::Position;
use bevy::prelude::{Entity, Transform, With, World};
use cs_content::scene::escape_scene_node_name;
use cs_script::ir::ActorId as WorldActorId;
use cs_sim::damage::DamageResolver;
use cs_sim::world_actors::graph::Presence;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::ActorId;

use super::compose::MissionPlayerBody;
use super::host::{MissionHost, MissionWorldObservation};
use super::landings::MissionLandings;
use crate::mission_actors::{
    MissionActorSpawnReport, MissionAircraftRoster, MissionGroundTarget, MissionSkippedAircraft,
    MissionZeppelinActor, ScriptedAircraftActor,
};
use crate::weapons::SessionDamageResolver;
use crate::world_actors::{WorldActorBinding, WorldActorSession};
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

/// The presence the world's own state reports for one **bound world actor**,
/// or `None` when it says nothing about it — the fail-closed read behind the
/// actor rows of [`mission_world_observation`] (M01-PLAY-TRIGGERS-ACTOR-BIT,
/// #1490).
///
/// Two measured fact families decide, and **neither is the actor's damage
/// pool** — an unresolved pool is not a presence statement:
///
/// * the **world-actor session's lifecycle**: [`WorldActorSet::presence`] is
///   the world's own read. `Presence::Intact` says the actor is still in the
///   world; `Presence::Destroyed` — monotonic, written only by the session's
///   measured `Destroy` command (`WorldActorCommand::Destroy`, F34) — says it
///   left. The composed host steps the session with no commands today
///   (`MissionHost::step` stage 2 schedules nothing on its behalf), so that
///   arm currently answers only through F34's own command surface; the
///   measurement is recorded in
///   `docs/findings/2026-10-15-m01-play-triggers-actor-bit.md`.
/// * the **session damage authority's destruction ledger**: `Destroyed`
///   recorded there — by a lethal hit or by the resolver's own scripted port
///   [`DamageResolver::record_lifecycle`] — is the authoritative record of an
///   actor's destruction (F36-C.1, the ledger [`DamageActorDestroyed`]
///   publishes from) and takes the row out of play even while the session
///   presence still says `Intact`. A missing resolver resource leaves the
///   ledger unread; the session's presence still stands.
///
/// `None` (no row at all) covers every case the world's own state does not
/// speak to: no world-actor session in the host, or a session that does not
/// hold the actor. A binding the composition cannot back with the world's
/// own lifecycle state is never reported in play — the fail-closed half,
/// which also keeps an unresolved pool from ever *becoming* a presence
/// statement by default.
///
/// [`WorldActorSet::presence`]: cs_sim::world_actors::runtime::WorldActorSet::presence
/// [`DamageActorDestroyed`]: crate::damage::DamageActorDestroyed
#[must_use]
pub fn world_actor_in_play(
    session: Option<&WorldActorSession>,
    world_actor: WorldActorId,
    damage: Option<&DamageResolver>,
    damage_actor: Option<ActorId>,
) -> Option<bool> {
    match session?.set().presence(world_actor) {
        Some(Presence::Destroyed) => Some(false),
        Some(Presence::Intact) => Some(
            !damage_actor
                .is_some_and(|actor| damage.is_some_and(|resolver| resolver.is_destroyed(&actor))),
        ),
        None => None,
    }
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
    // The rows steps 1 and 2 write, kept by name so step 4 can anchor a
    // landing node on them without reading the observation back.
    let mut anchors: BTreeMap<String, [f64; 3]> = BTreeMap::new();

    // 1. The player: exactly one body, exactly one pose.
    let bodies: Vec<Entity> = {
        let mut query = world.query_filtered::<Entity, With<MissionPlayerBody>>();
        query.iter(world).collect()
    };
    if let [body] = bodies.as_slice()
        && let Some(position) = world.get::<Position>(*body)
    {
        let [x, y, z] = position.0.to_array();
        let position = [f64::from(x), f64::from(y), f64::from(z)];
        anchors.insert(PLAYER_MEMBER.to_owned(), position);
        observation = observation.member(
            member_chain([PLAYER_MEMBER]),
            MemberObservation::held_in_play(position),
        );
    }

    // 2. Every bound world actor, at its live pose, under the node name its
    //    measured subject spells — with the presence the world's own state
    //    reports (M01-PLAY-TRIGGERS-ACTOR-BIT): the world-actor session's
    //    lifecycle crossed with the damage authority's destruction ledger.
    //    Neither family speaks → no row at all, fail-closed.
    let bindings: Vec<(ContentId, WorldActorId, Option<ActorId>, [f64; 3])> = {
        let mut query = world.query::<(
            &WorldActorBinding,
            &Transform,
            Option<&MissionZeppelinActor>,
        )>();
        query
            .iter(world)
            .map(|(binding, transform, zeppelin)| {
                let [x, y, z] = transform.translation.to_array();
                (
                    binding.subject.clone(),
                    binding.actor,
                    zeppelin.map(|actor| actor.damage_actor),
                    [f64::from(x), f64::from(y), f64::from(z)],
                )
            })
            .collect()
    };
    // Every bound actor's subject node name, in play or not, so step 4 can
    // tell a static container anchor from an actor whose row is absent.
    let mut bound_names: BTreeSet<String> = BTreeSet::new();
    let session = world
        .get_resource::<MissionHost>()
        .and_then(MissionHost::world_actors);
    let damage = world
        .get_resource::<SessionDamageResolver>()
        .map(|shared| shared.resolver());
    for (subject, world_actor, damage_actor, position) in bindings {
        let Some(name) = subject_node_name(&subject) else {
            continue;
        };
        bound_names.insert(name.to_owned());
        match world_actor_in_play(session, world_actor, damage, damage_actor) {
            Some(true) => {
                // An in-play actor anchors the landing nodes that hang from
                // its subject node (M01-PLAY-TRIGGERS-ANIMS-LAND, #1498): a
                // landing node composes its position from this row only, so
                // an anchor the world reports out of play — or says nothing
                // about — anchors nothing and the node stays unobserved.
                anchors.insert(name.to_owned(), position);
                observation = observation.member(
                    member_chain([name]),
                    MemberObservation::held_in_play(position),
                );
            }
            Some(false) => {
                observation = observation.member(
                    member_chain([name]),
                    MemberObservation::no_longer_in_play(position),
                );
            }
            // The world's own state says nothing about this actor: the chain
            // stays unobserved, fail-closed.
            None => {}
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

    // 2c. Every ground target the mission declares, at its node's composed
    //     origin (M01-EXAMINE-OBJECTIVE, #1754): `TRAVELERS` anchors on a
    //     named world node, and the original reads that node's world
    //     position through `0x4cf200`, the translation of the node's
    //     composed transform — the origin, not the targeting record's
    //     bounding-box centre (`docs/findings/2026-10-21-m01-examine-
    //     objective.md`). A name a bound world actor already answered keeps
    //     that row.
    let grounds: Vec<(String, [f64; 3], ActorId)> = {
        let mut query = world.query::<&MissionGroundTarget>();
        query
            .iter(world)
            .map(|target| (target.node.clone(), target.position_m, target.damage_actor))
            .collect()
    };
    let damage = world.get_resource::<SessionDamageResolver>();
    for (name, position, actor) in grounds {
        if anchors.contains_key(&name) {
            continue;
        }
        // A destroyed target leaves play the way the original's counters
        // read it: the convoy's `INACTIVE` ladders complete on the member
        // row turning, not on the entity despawning — the wreck stays in
        // the world.
        let member = if damage.is_some_and(|damage| damage.resolver().is_destroyed(&actor)) {
            MemberObservation::no_longer_in_play(position)
        } else {
            MemberObservation::held_in_play(position)
        };
        observation = observation.member(member_chain([name.as_str()]), member);
    }

    // 3. Groups: which ids the composition actually spawned members for, and
    //    how many of them are still in the world.
    let spawn_ran = world
        .get_resource::<MissionActorSpawnReport>()
        .is_some_and(|report| report.session.is_some());
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
    if spawn_ran {
        let roster = world
            .get_resource::<MissionAircraftRoster>()
            .map(|roster| roster.0.clone())
            .unwrap_or_default();
        let mut living: BTreeMap<i32, u32> = BTreeMap::new();
        // A group's count is only a count when the composition can account
        // for **every** enabled record carrying it: the original's counter
        // (`0x4658d0`) counts vehicle-registry members and every enabled
        // record enters that registry, so a record of this group the spawn
        // refused leaves the true count unknown — observed only when all of
        // the group's enabled records spawned, never on the fraction that
        // did (the undercount a `DEDG` gate could complete early on).
        let mut roster_groups: BTreeSet<u32> = BTreeSet::new();
        // Each spawned row accounts for exactly one roster record: two
        // same-named records cannot both stand on a single spawn.
        let mut spawn_rows: BTreeMap<&str, usize> = BTreeMap::new();
        for name in &spawned {
            *spawn_rows.entry(name.as_str()).or_default() += 1;
        }
        let mut expected: BTreeMap<u32, usize> = BTreeMap::new();
        let mut admitted: BTreeMap<u32, usize> = BTreeMap::new();
        for record in &roster {
            let Some(group) = record.group else {
                continue;
            };
            roster_groups.insert(group);
            *expected.entry(group).or_default() += 1;
            if let Some(available) = spawn_rows.get_mut(record.name.as_str())
                && *available > 0
            {
                *available -= 1;
                *admitted.entry(group).or_default() += 1;
            }
        }
        for (group, total) in &expected {
            if admitted.get(group).copied().unwrap_or(0) == *total
                && let Ok(id) = i32::try_from(*group)
            {
                living.insert(id, 0);
            }
        }
        // Records the original's spawn loop skips (`enabled == 0`) are never
        // in its registry, so their group counts 0 (`0x4658d0`) — but only
        // when no enabled record carries that group. A group the composition
        // could have spawned a member for stays unobserved rather than
        // borrowed to a skip's zero: an unrecorded group is unknown, not an
        // empty one.
        if let Some(skipped) = world.get_resource::<MissionSkippedAircraft>() {
            for record in &skipped.0 {
                if let Some(group) = record.group
                    && !roster_groups.contains(&group)
                    && let Ok(id) = i32::try_from(group)
                {
                    living.entry(id).or_insert(0);
                }
            }
        }
        // The counting `TRAVELERS` tests the awake members' positions
        // (`0x4659b0` skips a vehicle whose sleep byte is set), reported for
        // exactly the groups whose count is accounted for above.
        let mut positions: BTreeMap<i32, Vec<[f64; 3]>> = BTreeMap::new();
        let mut query = world.query::<(&ScriptedAircraftActor, Option<&Transform>)>();
        let resolver = world.get_resource::<SessionDamageResolver>();
        for (actor, transform) in query.iter(world) {
            // A destroyed aircraft leaves its group's count (#2100): the
            // wreck stays in the world, as a destroyed ground target does.
            if resolver.is_some_and(|damage| damage.resolver().is_destroyed(&actor.damage_actor)) {
                continue;
            }
            if let Some(group) = actor.record.group
                && let Ok(id) = i32::try_from(group)
                && let Some(count) = living.get_mut(&id)
            {
                *count += 1;
                let members = positions.entry(id).or_default();
                if !actor.asleep
                    && let Some(transform) = transform
                {
                    let [x, y, z] = transform.translation.to_array();
                    members.push([f64::from(x), f64::from(y), f64::from(z)]);
                }
            }
        }
        for (id, count) in living {
            observation = observation.group(id, count);
            observation = observation.group_members(id, positions.remove(&id).unwrap_or_default());
        }
    }

    // 4. The landing nodes the table names (M01-PLAY-TRIGGERS-ANIMS-LAND,
    //    #1498): a node's world position is the observed position of its
    //    anchor — the container node a bound actor's subject spells, its
    //    ancestor directly under the world root — plus the container's
    //    measured offset from that anchor
    //    (`LANDING_NODE_POSITION_CLAIM`). An anchor this tick's observation
    //    has no row for gives **no node row**: the landing consumer then
    //    finds no position there and runs nothing, because a position
    //    nobody observed is not a position this writer may invent. Node
    //    entries the reader refused (unknown or ambiguous node) are absent
    //    from the table's `nodes` and are simply not looked up here.
    if let Some(table) = world.get_resource::<MissionLandings>() {
        for entry in &table.nodes {
            // A bound actor anchors on its observed row only. An anchor that
            // is no bound actor and not the player is a static container
            // node: the container's own position for it is its position.
            let anchor = anchors.get(&entry.anchor).or_else(|| {
                (entry.anchor != PLAYER_MEMBER && !bound_names.contains(&entry.anchor))
                    .then_some(&entry.anchor_rest)
            });
            if let Some(anchor) = anchor {
                let [ax, ay, az] = *anchor;
                let [ox, oy, oz] = entry.offset;
                observation = observation.member(
                    member_chain([entry.node.as_str()]),
                    MemberObservation::held_in_play([ax + ox, ay + oy, az + oz]),
                );
            }
        }
    }

    world.insert_resource(MissionWorldObservation { observation });
}

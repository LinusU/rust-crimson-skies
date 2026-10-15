//! The live part-state objects of a mission's bound world actors
//! (M01-PLAY-TRIGGERS-PARTS, Rally #1481).
//!
//! # The defect this module repairs
//!
//! [`super::observation::mission_world_observation`] (M01-PLAY-TRIGGERS-OBSERVE,
//! #1480) writes a row for every chain the **composed world** can see: the
//! player's body, each bound world actor under its measured subject name and
//! the living count of every group the composition really spawned members for.
//! M01's 39 part-state chains — `["workersvoyagezep", "reng1", "healthy"]`,
//! `["piratezep", "reng32", "healthy"]`, … — are **not** among them: the world
//! import makes an object only for an `object3d` record the world node owns
//! directly, and an engine's `healthy` state node is a grandchild of the
//! zeppelin's own object. So those chains had no entity anywhere, the
//! `INACTIVE` ladders that count them could never complete and the win/loss
//! chain they feed could not start.
//!
//! This module gives each of those chains a **live part-state object**: one ECS
//! entity, spawned beside the bound world actor that owns it, carrying the chain
//! exactly as the record spells it and the damage identity the part is
//! addressable by. The observer then reports each one's presence, and the ladder
//! that reads it advances.
//!
//! # What the installation measures, and what it does not
//!
//! * **The chains resolve.** `MemberResolver` pins each `[actor, part,
//!   part-state]` chain to exactly one node of the C1C hierarchy: the leaf is the
//!   part's `healthy` state node, a child of the part node the chain's middle
//!   element names (`reng1`…`reng9`/`leng1`…`leng9` under `workersvoyagezep`,
//!   `reng11`…`leng32` under `piratezep`).
//! * **The part names are the record's own.** M01's `zeppelins.zrd` records
//!   declare `engines` — `workersvoyagezep`'s 18 and `piratezep`'s 12 node names
//!   — which are exactly the middle elements of the 18 and 12 chains
//!   `OBJECTIVE6/8/9/55` and `OBJECTIVE36`–`39` count. The `healthy` state node
//!   is the part's **intact** subtree (its siblings are `spin`/`counterspin`/
//!   `propstill`/`supports`); the wreck each part becomes is a separate
//!   root-level record (`wvreng_destroyed.flt`, `pzleng_destroyed.flt`, …), the
//!   same authored destruction-state shape `planes.zbd` gives an airframe's
//!   `healthy`/`destroyed` siblings.
//! * **The in-play bit is the node's own active flag.** The `INACTIVE` evaluator
//!   counts the listed nodes whose `+0x24` bit 4 is **clear**, and that bit is
//!   written by `gwNodeSetActive` (VA `0x4cca30`) — measured in #1360 — whose
//!   producer set (spawn, despawn, damage, ownership, scripts) is untraced
//!   ([`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`]).
//! * **No per-engine integrity pool is stated anywhere.** The record states
//!   floats for its **gasbags** (`150.0` for all nine of `workersvoyagezep`'s,
//!   `120.0` for `piratezep`'s six) and, on the instant-action records,
//!   `cannon_health` rows, but nothing for an engine node. So what *destroys* an
//!   engine — the writer behind the bit — is not recoverable from data this tree
//!   reads, and this module does not invent one: it makes the parts
//!   **addressable** (one named damage node each, the seam the F27/F29 damage
//!   authority already routes a hit through) and reports the pool it applies.
//!
//! # The designed value, and why there is one
//!
//! A damage node whose pool is `Resolved::Unknown` **blocks** every hit
//! (`DamageResolver::resolve`, the `HitBlocked` arm), so a mission whose parts
//! have no measured pool cannot show a part leaving play at all. This slice
//! gives each part a labelled **designed** single-unit pool
//! ([`DESIGNED_PART_INTEGRITY`], claim [`M01_PART_STATE_CLAIM`]) so the
//! destruction path (#1437's landed weapon session, #1436's AI fire, and the
//! damage authority they both publish into) can actually take a part out of
//! play — exactly the shape M01-PLAY-WEAPONS (#1437) used for its designed
//! loadout. It is **not** a claim about the original's engine hit points, which
//! stay unmeasured and are named as a residue in
//! `docs/findings/2026-10-15-m01-play-triggers-part-state-presence.md`; F27-E's
//! measured damage table replaces it.
//!
//! # What is observed, and the fail-closed rule
//!
//! A part entity's presence is read from the session's damage authority, never
//! defaulted:
//!
//! | Part state | Row |
//! | --- | --- |
//! | `Intact` / `Damaged` | the chain is **in play**, at the owning actor's pose |
//! | `Destroyed` | the chain is **out of play** — the `INACTIVE` read |
//! | `Unknown` pool, or no registered actor | **no row at all** |
//!
//! The last line is the fail-closed half: an unmeasured pool is not a statement
//! that the part is in play, so the chain stays unobserved and the ladder that
//! counts it stays where it is.

use std::collections::BTreeMap;

use bevy::prelude::{Component, Entity, Resource, Transform, Vec3, World};
use cs_script::ir::{ActorId as WorldActorId, MemberName};
use cs_sim::damage::{DamageNode, DamageNodeKey, DamageNodeKind, DamageResolver, PartState};
use cs_types::content::{ContentId, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::ActorId;

use crate::scene::SceneGeneration;
use crate::world_facts::{MemberResolver, WorldOperands};

use super::compose::MissionStage;
use super::observation::subject_node_name;

/// The claim every value this slice designed is filed under.
pub const M01_PART_STATE_CLAIM: &str = "m01-play-triggers-parts.designed-part-integrity";

/// The designed integrity pool of one named part: **one damage unit**.
///
/// Not a measurement and not a table: the original states no engine pool
/// (finding `2026-10-15-m01-play-triggers-part-state-presence.md`), and a
/// single unit is the least a pool can be while still letting *any* damage
/// destroy the part — which is what the destruction path has to be able to do
/// before the `INACTIVE` ladders can advance. Every part gets the same value,
/// so this is explicitly not a per-part tuning table.
pub const DESIGNED_PART_INTEGRITY: f64 = 1.0;

/// Component: one live part-state object of a bound world actor.
///
/// The entity *is* the engine's part-state node in this composition: the chain
/// it was spawned for, the actor that owns it and the damage node a hit
/// addresses it by. Its pose is the owning actor's own bind-time pose — the
/// per-node authored transform of a container record the world import does not
/// make an object for is not decoded, so no offset is invented.
#[derive(Component, Clone, Debug, PartialEq)]
pub struct MissionPartState {
    /// The member chain, exactly as the record spells it.
    pub chain: MemberName,
    /// The part's damage node on the owning actor's graph — the chain's part
    /// element, which is the node name the actor's own record declares under
    /// `engines`.
    pub node: DamageNodeKey,
    /// The bound world actor's measured subject.
    pub subject: ContentId,
    /// The session damage actor the part's node lives on.
    pub actor: ActorId,
    /// The scene generation the entity spawned under.
    pub generation: SceneGeneration,
}

/// One part-state chain that was given a live entity.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundPart {
    /// The chain, as spelled.
    pub chain: MemberName,
    /// The part's damage node.
    pub node: DamageNodeKey,
    /// The world actor that owns it.
    pub actor: ActorId,
    /// The spawned entity.
    pub entity: Entity,
}

/// Resource: what the part-state binding did — the parts it bound, the damage
/// nodes they added to their actors' graphs and every refusal by name.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct MissionPartReport {
    /// The parts that were bound, in (actor, chain) order.
    pub parts: Vec<BoundPart>,
    /// The part damage nodes this binding added, per session damage actor, in
    /// stable order. This is exactly what the owning actor's graph carries
    /// beyond the designed airframe node.
    pub nodes: BTreeMap<ActorId, Vec<DamageNodeKey>>,
    /// Every chain that could not be bound, with why, verbatim.
    pub refusals: Vec<String>,
}

impl MissionPartReport {
    /// The chain of every bound part of one actor, in stable order.
    #[must_use]
    pub fn chains_of(&self, actor: ActorId) -> Vec<&MemberName> {
        self.parts
            .iter()
            .filter(|part| part.actor == actor)
            .map(|part| &part.chain)
            .collect()
    }

    /// Whether nothing was bound at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.parts.is_empty()
    }
}

/// The part a member chain names, as a damage node key — `None` for a chain
/// that is not a part-state chain.
///
/// A chain is `[actor]` or `[actor, part, …]` (the measured one-, two- and
/// three-element arities); only the second and later elements name a part, so a
/// one-element chain — M01's bare `["piratezep"]`, its `TRAVELERS` subject and
/// anchor — names the actor itself and is never a part.
#[must_use]
pub fn part_state_node(chain: &MemberName) -> Option<DamageNodeKey> {
    let part = chain.get(1)?;
    DamageNodeKey::new(part).ok()
}

/// The designed damage node of one part.
///
/// The class is [`DamageNodeKind::InternalStructure`] for every part: 18 of
/// M01's chains name nodes the record's own `engines` key declares, but the
/// nine turret chains (`utur*`/`ctur*`/`maingun9`) name nodes **no** record key
/// declares, so a class read from the name would be a guess for those. No
/// `disables` edge is declared either: the zeppelin's motion is `Held` and the
/// original's part→system rule is unmeasured, so binding a propulsion or weapon
/// system here would be an invented rule.
#[must_use]
pub fn designed_part_node(key: &DamageNodeKey) -> DamageNode {
    DamageNode::new(
        key.clone(),
        DamageNodeKind::InternalStructure,
        Resolved::Known(Known::new(
            DESIGNED_PART_INTEGRITY,
            Provenance::designed(claim()),
        )),
    )
}

/// The claim id [`M01_PART_STATE_CLAIM`] parsed.
fn claim() -> ClaimId {
    ClaimId::new(M01_PART_STATE_CLAIM).expect("a claim id this module declares")
}

/// The part-state chains of one world actor, in (chain, node) order.
///
/// A chain qualifies when it resolves against the container hierarchy, has a
/// part element and names `subject_name` — the node name the bound actor's
/// measured subject spells ([`subject_node_name`]) — as its actor.
#[must_use]
pub fn part_state_chains(
    resolver: &MemberResolver,
    operands: &WorldOperands,
    subject_name: &str,
) -> Vec<(MemberName, DamageNodeKey)> {
    let mut planned = Vec::new();
    for chain in &operands.members {
        if chain.first().map(String::as_str) != Some(subject_name) {
            continue;
        }
        let Some(node) = part_state_node(chain) else {
            continue;
        };
        if resolver.resolve(chain).is_none() {
            continue;
        }
        planned.push((chain.clone(), node));
    }
    planned
}

/// Every part plan of a stage, keyed by the **world actor id** the program gave
/// the actor: what each bound actor's parts are, and which damage nodes its
/// graph must carry.
///
/// The plan is read from the stage's own records — the mission program's world
/// operands through the container resolver over the very container the stage
/// imported — so the composition binds exactly the chains the record spells and
/// nothing else.
#[must_use]
pub fn plan_part_states(
    stage: &MissionStage,
) -> BTreeMap<WorldActorId, Vec<(MemberName, DamageNodeKey)>> {
    let operands = WorldOperands::of(&stage.host.control);
    let mut plan = BTreeMap::new();
    let Some(actors) = stage.host.world_actors.as_ref() else {
        return plan;
    };
    for (actor, subject) in &actors.subjects {
        let Some(name) = subject_node_name(subject) else {
            continue;
        };
        let chains = part_state_chains(&stage.host.resolver, &operands, name);
        if !chains.is_empty() {
            plan.insert(*actor, chains);
        }
    }
    plan
}

/// Reports what the session's damage authority says about one part, or `None`
/// when it says nothing this observer may report.
///
/// `Some(true)` is in play, `Some(false)` is out of play, and `None` is the
/// fail-closed read: an unregistered actor or an unresolved pool is not a
/// statement about presence.
#[must_use]
pub fn part_in_play(resolver: &DamageResolver, part: &MissionPartState) -> Option<bool> {
    match resolver.part_state(&part.actor, &part.node) {
        Some(PartState::Destroyed) => Some(false),
        Some(PartState::Intact | PartState::Damaged) => Some(true),
        Some(PartState::Unknown) | None => None,
    }
}

/// Spawns the live entity of one part-state chain beside its bound world actor.
#[must_use]
pub fn spawn_part_state(
    world: &mut World,
    actor: ActorId,
    subject: ContentId,
    generation: SceneGeneration,
    chain: MemberName,
    node: DamageNodeKey,
    pose: [f64; 3],
) -> Entity {
    let transform =
        Transform::from_translation(Vec3::new(pose[0] as f32, pose[1] as f32, pose[2] as f32));
    world
        .spawn((
            transform,
            MissionPartState {
                chain,
                node,
                subject,
                actor,
                generation,
            },
        ))
        .id()
}

/// Every part entity the world holds, in spawn order, for teardown and for the
/// observer.
#[must_use]
pub fn part_entities(world: &mut World) -> Vec<Entity> {
    use bevy::prelude::With;
    let mut query = world.query_filtered::<Entity, With<MissionPartState>>();
    query.iter(world).collect()
}

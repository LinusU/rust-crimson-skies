//! Acceptance scenarios F29-C.2: the damage → debris consumer.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-C`, task F29-C.2 "Spawn authored debris when a damage part is
//! destroyed". Task test prefix: `accept_f29_c_debris_` (the stage prefix
//! `accept_f29_c_` selects these too). Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The four scenarios the task names, each read off production code:
//!
//! * destroying a part spawns its debris once — the declared graph is lowered
//!   by [`cs_app::damage::lower_graph`] / [`lower_policy`], the real
//!   [`DamageResolver`] resolves the hit, and [`apply_debris_state`] spawns
//!   the instance the part's authored [`PartDebrisBinding`] names;
//! * a second pass spawns none — the pass is convergent, and a duplicate
//!   instance left in the world is collapsed back to one;
//! * a repair/reload removes it — the authority no longer calling the part
//!   destroyed despawns the instance, and [`release_debris`] is the reload
//!   teardown entry;
//! * an unresolved binding is refused by name — with its claim, and with
//!   nothing spawned.
//!
//! Nothing is observed through a test-only bridge: the instance is read back
//! off the [`SpawnedDebris`] entity the production pass spawned, so a pass
//! that recorded a decision without spawning would fail the entity count.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data. Whether the original spawned authored debris per part, which
//! object it used and how many it spawned are unmeasured (F29 "Research
//! boundary"); see `docs/findings/2026-10-05-f29-c-2-debris-spawn-consumer.md`.

use bevy::prelude::{Entity, World};
use cs_app::damage::{lower_graph, lower_policy};
use cs_app::debris::{
    DamageDebrisEvent, DamageDebrisRefusal, DamageDebrisReport, PartDebrisBinding, SpawnedDebris,
    apply_debris_state, release_debris,
};
use cs_app::scene::SceneGeneration;
use cs_content::damage::declared_synthetic_airframe_damage;
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageGraph, DamageNode, DamageNodeKey,
    DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventId, PartState,
    SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE, SYNTHETIC_MOUNT_INTEGRITY,
    SYNTHETIC_MOUNT_NODE,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 41;
const PRODUCER: u32 = 1;

// ----------------------------------------------------------------- helpers ---

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn engine() -> DamageNodeKey {
    key(SYNTHETIC_ENGINE_NODE)
}

fn mount() -> DamageNodeKey {
    key(SYNTHETIC_MOUNT_NODE)
}

fn claim() -> ClaimId {
    ClaimId::new("f29c2.test").expect("a valid claim id")
}

fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

fn generation() -> SceneGeneration {
    SceneGeneration::default().next()
}

/// The authored debris object a fixture part spawns: synthetic content, named
/// like the part it belongs to.
fn debris_id(part: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::SceneNode,
        &format!("synthetic.devastator.{part}_wreck"),
    )
    .expect("a valid debris object id")
}

/// The production declared → lowered → registered path for one actor.
fn registered() -> DamageResolver {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the declared graph lowers");
    let policy = lower_policy(&declared).expect("the declared policy lowers");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(actor(1), graph, policy)
        .expect("the lowered actor registers");
    resolver
}

/// Binds `node`'s debris the way a spawn path would: one entity carrying the
/// part's authored binding.
fn bind(
    world: &mut World,
    node: DamageNodeKey,
    debris: Resolved<ContentId>,
    generation: SceneGeneration,
) -> Entity {
    world
        .spawn(PartDebrisBinding::new(actor(1), node, debris, generation))
        .id()
}

/// An internal hit that lands on `node` for `damage`, at the resolver's tick.
fn hit(node: &DamageNodeKey, damage: f64) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(0),
            producer: PRODUCER,
            sequence: 0,
        },
        Some(actor(2)),
        actor(1),
        node.clone(),
        DamageChannel::Internal,
        damage,
    )
    .expect("a finite, non-negative hit")
}

/// Resolves exactly one hit of `damage` on `node`.
fn resolve_hit(resolver: &mut DamageResolver, node: &DamageNodeKey, damage: f64) {
    resolver
        .resolve(Tick(0), &[hit(node, damage)])
        .expect("the resolver accepts the batch");
}

/// Destroys one part outright through the production resolver path.
fn destroy(resolver: &mut DamageResolver, node: &DamageNodeKey, integrity: f64) {
    resolve_hit(resolver, node, integrity);
    assert_eq!(
        resolver.part_state(&actor(1), node),
        Some(PartState::Destroyed),
        "the fixture hit destroyed the part"
    );
}

/// Every debris instance in the world, in entity order.
fn instances(world: &World, actor: ActorId) -> Vec<(Entity, ContentId, SceneGeneration)> {
    world
        .iter_entities()
        .filter_map(|entity_ref| {
            let debris = entity_ref.get::<SpawnedDebris>()?;
            (debris.actor == actor).then_some((
                entity_ref.id(),
                debris.source.clone(),
                debris.generation,
            ))
        })
        .collect()
}

/// The one instance a single destroyed part must own, or a precise complaint.
fn only_instance(world: &World, actor: ActorId) -> (Entity, ContentId, SceneGeneration) {
    let found = instances(world, actor);
    assert_eq!(found.len(), 1, "exactly one debris instance: {found:?}");
    found.into_iter().next().expect("the one instance")
}

// -------------------------------------------------- the minimum scenario ---

/// Destroying a part spawns its authored debris once, and only for the part
/// that was destroyed.
#[test]
fn accept_f29_c_debris_destroying_a_part_spawns_its_authored_debris_once() {
    let mut world = World::new();
    let mut resolver = registered();
    let source = debris_id(SYNTHETIC_ENGINE_NODE);
    let stamp = generation();
    bind(&mut world, engine(), known(source.clone()), stamp);

    // An intact part spawns nothing and changes nothing.
    let baseline = apply_debris_state(&mut world, &resolver, actor(1));
    assert!(
        baseline.report.is_noop() && baseline.log.is_empty(),
        "an intact part needs no debris: {baseline:?}"
    );
    assert!(instances(&world, actor(1)).is_empty());

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);

    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(
        outcome.report,
        DamageDebrisReport {
            spawned: 1,
            ..Default::default()
        },
        "the destroyed part spawns exactly its own debris: {outcome:?}"
    );
    let DamageDebrisEvent::Spawned {
        actor: spawned_by,
        node,
        source: spawned_from,
        generation: stamped,
        entity,
    } = outcome.log.events().first().expect("one spawn event")
    else {
        panic!("the log records a spawn: {:?}", outcome.log.events());
    };
    assert_eq!(spawned_by, &actor(1));
    assert_eq!(node, &engine());
    assert_eq!(
        spawned_from, &source,
        "the authored object is carried verbatim"
    );
    assert_eq!(stamped, &stamp);

    // The record is a real entity carrying the authored object, not a note.
    let instance = world
        .get::<SpawnedDebris>(*entity)
        .expect("the spawned instance exists in the world");
    assert_eq!(instance.actor, actor(1));
    assert_eq!(instance.node, engine());
    assert_eq!(instance.source, source);
    assert_eq!(instance.generation, stamp);
    assert_eq!(
        only_instance(&world, actor(1)),
        (*entity, source, stamp),
        "the world holds exactly the instance the pass spawned"
    );
}

/// A second pass spawns none: the pass converges on one instance, keeps it,
/// and collapses a duplicate that was left behind.
#[test]
fn accept_f29_c_debris_a_second_pass_spawns_none() {
    let mut world = World::new();
    let mut resolver = registered();
    let source = debris_id(SYNTHETIC_ENGINE_NODE);
    let stamp = generation();
    bind(&mut world, engine(), known(source.clone()), stamp);

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let first = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(first.report.spawned, 1);
    let (spawned, _, _) = only_instance(&world, actor(1));

    let second = apply_debris_state(&mut world, &resolver, actor(1));
    assert!(
        second.report.is_noop() && second.log.is_empty(),
        "a converged pass spawns nothing: {second:?}"
    );
    assert_eq!(
        only_instance(&world, actor(1)),
        (spawned, source.clone(), stamp),
        "the same instance survives the second pass"
    );

    // A duplicate left behind (an older generation's leftover) is collapsed
    // back to one instance, still without spawning anything new.
    world.spawn(SpawnedDebris {
        actor: actor(1),
        node: engine(),
        source: source.clone(),
        generation: SceneGeneration::default(),
    });
    assert_eq!(instances(&world, actor(1)).len(), 2);

    let collapsed = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(
        collapsed.report,
        DamageDebrisReport {
            despawned: 1,
            ..Default::default()
        },
        "the duplicate leaves, nothing new spawns: {collapsed:?}"
    );
    assert_eq!(
        only_instance(&world, actor(1)),
        (spawned, source, stamp),
        "one part owns exactly one debris instance"
    );
}

/// A repair removes the debris: as soon as the authority stops calling the
/// part destroyed, the instance is despawned — even if the binding itself is
/// gone with it.
#[test]
fn accept_f29_c_debris_a_repair_removes_the_spawned_debris() {
    let mut world = World::new();
    let mut resolver = registered();
    let source = debris_id(SYNTHETIC_ENGINE_NODE);
    let binding = bind(&mut world, engine(), known(source), generation());

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let spawned = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(spawned.report.spawned, 1);
    assert_eq!(instances(&world, actor(1)).len(), 1);

    // The repair path starts a part from an intact pool again — the same
    // authoritative read every other consumer follows.
    let repaired = registered();
    world.entity_mut(binding).despawn();

    let outcome = apply_debris_state(&mut world, &repaired, actor(1));
    assert_eq!(
        outcome.report,
        DamageDebrisReport {
            despawned: 1,
            ..Default::default()
        },
        "the authority, not the binding, decides what exists: {outcome:?}"
    );
    assert!(
        instances(&world, actor(1)).is_empty(),
        "the debris is gone with the repair"
    );

    let again = apply_debris_state(&mut world, &repaired, actor(1));
    assert!(
        again.report.is_noop() && again.log.is_empty(),
        "a repaired pass is a no-op: {again:?}"
    );
}

/// A reload removes it: `release_debris` is the teardown entry, and it is
/// convergent.
#[test]
fn accept_f29_c_debris_a_reload_releases_every_spawned_debris() {
    let mut world = World::new();
    let mut resolver = registered();
    bind(
        &mut world,
        engine(),
        known(debris_id(SYNTHETIC_ENGINE_NODE)),
        generation(),
    );
    bind(
        &mut world,
        mount(),
        known(debris_id(SYNTHETIC_MOUNT_NODE)),
        generation(),
    );

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    destroy(&mut resolver, &mount(), SYNTHETIC_MOUNT_INTEGRITY);
    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 2, "{outcome:?}");
    assert_eq!(instances(&world, actor(1)).len(), 2);

    // Another actor's debris is not collateral damage.
    let foreign = ActorId {
        session: session(SESSION),
        serial: 7,
    };
    world.spawn(SpawnedDebris {
        actor: foreign,
        node: engine(),
        source: debris_id("other"),
        generation: generation(),
    });

    assert_eq!(
        release_debris(&mut world, actor(1)),
        2,
        "the teardown reports what it released"
    );
    assert!(instances(&world, actor(1)).is_empty());
    assert_eq!(
        instances(&world, foreign).len(),
        1,
        "only this actor's debris goes"
    );

    assert_eq!(
        release_debris(&mut world, actor(1)),
        0,
        "a second teardown finds nothing"
    );
}

/// A part with no authored binding spawns nothing and is not a refusal: the
/// original's per-part debris authoring is unmeasured, so absence is not an
/// error and never a licence to guess an object.
#[test]
fn accept_f29_c_debris_a_part_without_a_binding_spawns_nothing() {
    let mut world = World::new();
    let mut resolver = registered();

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);

    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert!(
        outcome.report.is_noop() && outcome.log.is_empty(),
        "an unbound part is skipped, not refused and not guessed: {outcome:?}"
    );
    assert!(instances(&world, actor(1)).is_empty());
}

// --------------------------------------------------------- the refusals ---

/// An unresolved debris binding is refused by name, with its claim, and
/// nothing is spawned for it.
#[test]
fn accept_f29_c_debris_an_unresolved_binding_is_refused_by_name() {
    let mut world = World::new();
    let mut resolver = registered();
    let reason = "the engine's debris object is unresolved".to_owned();
    bind(
        &mut world,
        engine(),
        Resolved::Unknown {
            claim_id: claim(),
            reason: reason.clone(),
        },
        generation(),
    );

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);

    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 0, "{outcome:?}");
    assert_eq!(
        outcome.log.last(),
        Some(&DamageDebrisEvent::Refused(
            DamageDebrisRefusal::UnresolvedDebris {
                actor: actor(1),
                node: engine(),
                claim_id: claim(),
                reason,
            }
        )),
        "an unmeasured binding carries its claim: {:?}",
        outcome.log
    );
    assert!(
        instances(&world, actor(1)).is_empty(),
        "nothing was guessed into the world"
    );
}

/// A part whose integrity is unresolved asserts neither direction: it is
/// refused by name and an instance that exists is left exactly as it is.
#[test]
fn accept_f29_c_debris_an_unresolved_pool_asserts_neither_direction() {
    let mut world = World::new();
    let reason = "the hull's integrity is unmeasured".to_owned();
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.f29c2-unknown-pool")
            .expect("a valid airframe id"),
        vec![
            DamageNode::new(
                key("hull"),
                DamageNodeKind::InternalStructure,
                Resolved::Unknown {
                    claim_id: claim(),
                    reason: reason.clone(),
                },
            )
            .with_scene_binding(known(debris_id("hull"))),
        ],
    )
    .expect("an unknown pool still validates");
    let mut resolver = DamageResolver::new(session(SESSION), PRODUCER);
    resolver
        .register_actor(
            actor(1),
            graph,
            DamagePolicy {
                attribution: AttributionRule::FirstLethalHit,
            },
        )
        .expect("the actor registers");
    bind(
        &mut world,
        key("hull"),
        known(debris_id("hull")),
        generation(),
    );
    // An instance the pass spawned earlier — an unknown pool neither removes
    // nor endorses it.
    let existing = world
        .spawn(SpawnedDebris {
            actor: actor(1),
            node: key("hull"),
            source: debris_id("hull"),
            generation: generation(),
        })
        .id();

    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(outcome.report.spawned, 0, "{outcome:?}");
    assert_eq!(outcome.report.despawned, 0, "{outcome:?}");
    assert_eq!(
        outcome.log.last(),
        Some(&DamageDebrisEvent::Refused(
            DamageDebrisRefusal::UnresolvedIntegrity {
                actor: actor(1),
                node: key("hull"),
                claim_id: claim(),
                reason,
            }
        )),
        "an unmeasured pool is named, not guessed: {:?}",
        outcome.log
    );
    assert!(
        world.get::<SpawnedDebris>(existing).is_some(),
        "the instance is left exactly as it was"
    );
}

/// A foreign session generation and an unregistered actor are refused by name
/// before a single part is read.
#[test]
fn accept_f29_c_debris_a_foreign_or_unknown_actor_is_refused_by_name() {
    let mut world = World::new();
    let resolver = registered();
    bind(
        &mut world,
        engine(),
        known(debris_id(SYNTHETIC_ENGINE_NODE)),
        generation(),
    );

    let foreign = ActorId {
        session: session(SESSION + 1),
        serial: 1,
    };
    let outcome = apply_debris_state(&mut world, &resolver, foreign);
    assert_eq!(
        outcome.report,
        DamageDebrisReport {
            refused: 1,
            ..Default::default()
        }
    );
    assert_eq!(
        outcome.log.last(),
        Some(&DamageDebrisEvent::Refused(
            DamageDebrisRefusal::ForeignSession {
                expected: SESSION,
                found: SESSION + 1,
            }
        )),
        "a generation-qualified actor is never applied to another generation"
    );
    assert!(instances(&world, foreign).is_empty());

    let unknown = apply_debris_state(&mut world, &resolver, actor(99));
    assert_eq!(unknown.report.refused, 1);
    assert_eq!(
        unknown.log.last(),
        Some(&DamageDebrisEvent::Refused(
            DamageDebrisRefusal::UnknownActor { actor: actor(99) }
        ))
    );
    assert!(instances(&world, actor(1)).is_empty());
    assert!(instances(&world, actor(99)).is_empty());
}

/// A reload that rebinds the part under a live generation supersedes the old
/// instance: the stale one leaves and the live one is spawned in its place.
#[test]
fn accept_f29_c_debris_a_superseded_binding_replaces_the_instance() {
    let mut world = World::new();
    let mut resolver = registered();
    let source = debris_id(SYNTHETIC_ENGINE_NODE);
    let stale = generation();
    let stale_binding = bind(&mut world, engine(), known(source.clone()), stale);

    destroy(&mut resolver, &engine(), SYNTHETIC_ENGINE_INTEGRITY);
    let first = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(first.report.spawned, 1);
    assert_eq!(
        instances(&world, actor(1)).len(),
        1,
        "the stale generation's instance is in the world"
    );

    // The scene reloads: the part's binding is respawned under the next
    // generation, the old entity goes with the release.
    world.entity_mut(stale_binding).despawn();
    let live = stale.next();
    bind(&mut world, engine(), known(source.clone()), live);

    let outcome = apply_debris_state(&mut world, &resolver, actor(1));
    assert_eq!(
        outcome.report,
        DamageDebrisReport {
            spawned: 1,
            despawned: 1,
            ..Default::default()
        },
        "the stale instance leaves, the live one arrives: {outcome:?}"
    );
    let (entity, _, stamped) = only_instance(&world, actor(1));
    assert!(
        world.get::<SpawnedDebris>(entity).is_some(),
        "the surviving instance exists"
    );
    assert_eq!(
        stamped, live,
        "the instance is stamped with the live generation, never the stale one"
    );
    assert!(
        instances(&world, actor(1))
            .iter()
            .all(|(_, _, generation)| *generation == live),
        "no instance of the superseded generation survives"
    );
}

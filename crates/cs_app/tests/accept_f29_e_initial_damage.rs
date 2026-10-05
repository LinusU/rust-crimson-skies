//! Acceptance scenarios F29-E: a mission's authored initial damage reaches
//! the resolver's part state through the production path, and whatever the
//! authoring does not resolve is reported instead of guessed.
//!
//! Task test prefix: `accept_f29_e_`. Drives
//! [`apply_world_initial_damage`] and [`DamageResolver::apply_initial_damage`];
//! removing either fails these tests. Every value is newly authored synthetic
//! fixture data.

use std::collections::BTreeSet;

use cs_app::damage::{WorldDamageMapping, apply_world_initial_damage};
use cs_app::world::arch_world;
use cs_app::world::fixture::{OBJECT_LEG_LEFT, OBJECT_LEG_RIGHT, OBJECT_LINTEL};
use cs_content::world::{WorldInstance, WorldObjectId, WorldPopulation};
use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageError, DamageNodeKey, DamagePolicy,
    DamageResolver, HitEvent, HitEventId, InitialDamage, InitialDamageRefusal, PartState,
    SYNTHETIC_ARMOR_INTEGRITY, SYNTHETIC_ARMOR_NODE, SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE,
    SYNTHETIC_MOUNT_INTEGRITY, SYNTHETIC_MOUNT_NODE, synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::{Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

fn session() -> SessionId {
    SessionId::new(5).expect("a nonzero session")
}

fn actor() -> ActorId {
    ActorId {
        session: session(),
        serial: 1,
    }
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a valid node key")
}

fn provenance() -> Provenance {
    Provenance::designed(ClaimId::new("test.f29e").expect("a valid claim id"))
}

fn statement(node: &str, amount: f64) -> InitialDamage {
    InitialDamage {
        node: key(node),
        amount,
        provenance: provenance(),
    }
}

fn object(name: &str) -> WorldObjectId {
    WorldObjectId::new(name).expect("a valid object key")
}

fn policy() -> DamagePolicy {
    DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    }
}

fn resolver() -> DamageResolver {
    let mut resolver = DamageResolver::new(session(), 1);
    resolver
        .register_actor(actor(), synthetic_airframe_graph(), policy())
        .expect("the synthetic actor registers");
    resolver
}

fn instance(damaged: &[&str]) -> WorldInstance {
    let definition = arch_world().expect("the synthetic arch world is valid");
    WorldInstance::try_new(
        definition.id().clone(),
        Resolved::unknown(
            ClaimId::new("test.f29e.variant").expect("a valid claim id"),
            "variant deliberately unmeasured",
        )
        .expect("a non-empty reason"),
        WorldPopulation::AllAuthored,
        damaged
            .iter()
            .map(|name| object(name))
            .collect::<BTreeSet<_>>(),
        provenance(),
    )
    .expect("all-authored is a valid population")
}

/// The authored statements land in the pools: the part is `Damaged`, the
/// remainder is the pool minus the amount, and later hits start from it.
#[test]
fn accept_f29_e_authored_damage_starts_the_pools_damaged() {
    let mut resolver = resolver();
    let report = resolver
        .apply_initial_damage(
            &actor(),
            &[
                statement(SYNTHETIC_ARMOR_NODE, 5.0),
                statement(SYNTHETIC_MOUNT_NODE, 4.0),
            ],
        )
        .expect("a pristine registered actor accepts authored damage");
    assert_eq!(report.applied.len(), 2);
    assert!(report.unresolved.is_empty());
    assert_eq!(
        resolver.remaining_integrity(&actor(), &key(SYNTHETIC_ARMOR_NODE)),
        Some(SYNTHETIC_ARMOR_INTEGRITY - 5.0)
    );
    assert_eq!(
        resolver.part_state(&actor(), &key(SYNTHETIC_MOUNT_NODE)),
        Some(PartState::Damaged)
    );
    assert_eq!(
        resolver.part_state(&actor(), &key(SYNTHETIC_HULL_NODE)),
        Some(PartState::Intact),
        "an unnamed node stays pristine"
    );

    let hit = HitEvent::try_new(
        HitEventId {
            session: session(),
            tick: Tick(1),
            producer: 1,
            sequence: 0,
        },
        None,
        actor(),
        key(SYNTHETIC_MOUNT_NODE),
        DamageChannel::Internal,
        SYNTHETIC_MOUNT_INTEGRITY - 4.0,
    )
    .expect("a finite hit");
    resolver.resolve(Tick(1), &[hit]).expect("the hit resolves");
    assert_eq!(
        resolver.part_state(&actor(), &key(SYNTHETIC_MOUNT_NODE)),
        Some(PartState::Destroyed),
        "the authored damage counts toward destruction"
    );
}

/// Unknown node, damage beyond the pool, bad amount and a repeated node are
/// reported with their reasons and change nothing.
#[test]
fn accept_f29_e_unresolvable_statements_are_reported_not_applied() {
    let mut resolver = resolver();
    let report = resolver
        .apply_initial_damage(
            &actor(),
            &[
                statement("no_such_part", 1.0),
                statement(SYNTHETIC_HULL_NODE, SYNTHETIC_HULL_INTEGRITY + 1.0),
                statement(SYNTHETIC_ARMOR_NODE, SYNTHETIC_ARMOR_INTEGRITY),
                statement(SYNTHETIC_MOUNT_NODE, f64::NAN),
                statement(SYNTHETIC_MOUNT_NODE, 1.0),
            ],
        )
        .expect("the batch itself is accepted");
    let reasons: Vec<_> = report
        .unresolved
        .iter()
        .map(|u| u.refusal.clone())
        .collect();
    assert_eq!(
        reasons,
        vec![
            InitialDamageRefusal::UnknownNode,
            InitialDamageRefusal::PreDestroyed {
                pool: SYNTHETIC_HULL_INTEGRITY
            },
            InitialDamageRefusal::PreDestroyed {
                pool: SYNTHETIC_ARMOR_INTEGRITY
            },
            InitialDamageRefusal::InvalidAmount,
            InitialDamageRefusal::DuplicateNode,
        ]
    );
    assert!(report.applied.is_empty());
    for node in [
        SYNTHETIC_HULL_NODE,
        SYNTHETIC_ARMOR_NODE,
        SYNTHETIC_MOUNT_NODE,
    ] {
        assert_eq!(
            resolver.part_state(&actor(), &key(node)),
            Some(PartState::Intact),
            "{node} must be untouched"
        );
    }
}

/// An unresolved pool is reported, and the resolver refuses unknown actors,
/// foreign-state actors and a second application after play started.
#[test]
fn accept_f29_e_unresolved_pools_and_late_application_are_refused() {
    use cs_sim::damage::{DamageGraph, DamageNode, DamageNodeKind};
    use cs_types::content::{ContentId, ContentKind};

    let unresolved_pool = Resolved::unknown(
        ClaimId::new("test.f29e.pool").expect("a valid claim id"),
        "pool deliberately unmeasured",
    )
    .expect("a non-empty reason");
    let graph = DamageGraph::try_new(
        ContentId::from_source(ContentKind::Airframe, "synthetic.unresolved").expect("a valid id"),
        vec![DamageNode::new(
            key("mystery"),
            DamageNodeKind::InternalStructure,
            unresolved_pool,
        )],
    )
    .expect("a valid graph");
    let mut resolver = DamageResolver::new(session(), 1);
    resolver
        .register_actor(actor(), graph, policy())
        .expect("registers");
    let report = resolver
        .apply_initial_damage(&actor(), &[statement("mystery", 1.0)])
        .expect("accepted");
    assert_eq!(
        report.unresolved[0].refusal,
        InitialDamageRefusal::UnresolvedPool
    );

    let other = ActorId {
        session: session(),
        serial: 99,
    };
    assert_eq!(
        resolver.apply_initial_damage(&other, &[]),
        Err(DamageError::UnknownActor { actor: other })
    );

    let mut resolver = self::resolver();
    resolver
        .apply_initial_damage(&actor(), &[statement(SYNTHETIC_ARMOR_NODE, 1.0)])
        .expect("first application");
    assert_eq!(
        resolver.apply_initial_damage(&actor(), &[statement(SYNTHETIC_MOUNT_NODE, 1.0)]),
        Err(DamageError::InitialDamageTooLate { actor: actor() }),
        "initial damage is part of registration, not of play"
    );
}

/// The world boundary applies mapped objects and names the unmapped ones;
/// it never invents a node for an object without an authored mapping.
#[test]
fn accept_f29_e_world_boundary_applies_mapped_and_reports_unmapped_objects() {
    let mut resolver = resolver();
    let mut mapping = WorldDamageMapping::new();
    mapping.insert(object(OBJECT_LINTEL), statement(SYNTHETIC_ARMOR_NODE, 7.0));
    // Mapped but not damaged by this load: must be ignored.
    mapping.insert(object(OBJECT_LEG_LEFT), statement(SYNTHETIC_HULL_NODE, 9.0));
    let load = instance(&[OBJECT_LINTEL, OBJECT_LEG_RIGHT]);

    let report = apply_world_initial_damage(&mut resolver, &actor(), &load, &mapping)
        .expect("the boundary applies");
    assert_eq!(report.resolver.applied.len(), 1);
    assert_eq!(report.unmapped_objects, vec![object(OBJECT_LEG_RIGHT)]);
    assert_eq!(
        resolver.remaining_integrity(&actor(), &key(SYNTHETIC_ARMOR_NODE)),
        Some(SYNTHETIC_ARMOR_INTEGRITY - 7.0)
    );
    assert_eq!(
        resolver.part_state(&actor(), &key(SYNTHETIC_HULL_NODE)),
        Some(PartState::Intact),
        "a mapping for an undamaged object does not apply"
    );
}

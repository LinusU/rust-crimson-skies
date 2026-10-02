//! Acceptance scenario F29-A (AC01 minimum scenario and its failure
//! cases): deterministic hit ordering, the declared simultaneous-lethal
//! attribution, single-kill scoring and the distinct lifecycle vocabulary.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Task test prefix: `accept_f29_a_`.
//!
//! These tests drive production code only: [`cs_sim::damage`]'s
//! [`DamageResolver`], [`DamageGraph`], [`HitEvent`] and the
//! `synthetic_airframe_graph` fixture. Removing the ordering rule, the
//! attribution policy, the once-per-actor destruction/scoring emission or
//! the lifecycle ledger makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_sim::damage::{
    ActorId, AttributionRule, DamageChannel, DamageError, DamageEventKind, DamageGraph, DamageNode,
    DamageNodeKey, DamageNodeKind, DamagePolicy, DamageResolver, HitEvent, HitEventError,
    HitEventId, LifecycleKind, PartState, RefusalReason, SYNTHETIC_ARMOR_INTEGRITY,
    SYNTHETIC_ARMOR_NODE, SYNTHETIC_ENGINE_INTEGRITY, SYNTHETIC_ENGINE_NODE,
    SYNTHETIC_HULL_INTEGRITY, SYNTHETIC_HULL_NODE, SYNTHETIC_MOUNT_NODE, SystemKind,
    synthetic_airframe_graph,
};
use cs_types::Tick;
use cs_types::content::{Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 7;
const RESOLVER_PRODUCER: u32 = 1;

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

#[allow(clippy::too_many_arguments)]
fn hit(
    producer: u32,
    sequence: u32,
    attacker: Option<ActorId>,
    target: ActorId,
    node: &str,
    channel: DamageChannel,
    damage: f64,
    tick: u64,
) -> HitEvent {
    HitEvent::try_new(
        HitEventId {
            session: session(SESSION),
            tick: Tick(tick),
            producer,
            sequence,
        },
        attacker,
        target,
        key(node),
        channel,
        damage,
    )
    .expect("test hit is valid")
}

fn resolver(policy: DamagePolicy, target: ActorId) -> DamageResolver {
    let mut resolver = DamageResolver::new(session(SESSION), RESOLVER_PRODUCER);
    resolver
        .register_actor(target, synthetic_airframe_graph(), policy)
        .expect("the synthetic actor registers");
    resolver
}

fn first_lethal() -> DamagePolicy {
    DamagePolicy {
        attribution: AttributionRule::FirstLethalHit,
    }
}

fn event_kinds(resolution: &cs_sim::damage::TickResolution) -> Vec<&DamageEventKind> {
    resolution.events.iter().map(|event| &event.kind).collect()
}

/// AC01 minimum scenario: two same-tick lethal hits award a single kill
/// under the declared `FirstLethalHit` rule — the earlier hit in declared
/// (id) order is credited, so feeding the batch in reverse order cannot
/// change the outcome.
#[test]
fn accept_f29_a_two_same_tick_lethal_hits_award_a_single_kill() {
    let target = actor(1);
    let attacker_early = actor(10);
    let attacker_late = actor(11);
    let mut resolver = resolver(first_lethal(), target);

    // Both hits are individually lethal on the hull. They are fed in
    // reverse id order: the declared ordering rule, not the input order,
    // decides which blow is "first".
    let late = hit(
        9,
        0,
        Some(attacker_late),
        target,
        "hull",
        DamageChannel::Internal,
        50.0,
        5,
    );
    let early = hit(
        3,
        0,
        Some(attacker_early),
        target,
        "hull",
        DamageChannel::Internal,
        50.0,
        5,
    );
    let resolution = resolver
        .resolve(Tick(5), &[late, early.clone()])
        .expect("valid batch");

    let lifecycles: Vec<_> = event_kinds(&resolution)
        .into_iter()
        .filter(|kind| matches!(kind, DamageEventKind::Lifecycle { .. }))
        .collect();
    assert_eq!(
        lifecycles.len(),
        1,
        "two lethal hits must produce exactly one destruction"
    );
    assert_eq!(
        lifecycles[0],
        &DamageEventKind::Lifecycle {
            actor: target,
            kind: LifecycleKind::Destroyed,
        }
    );

    let awards: Vec<_> = event_kinds(&resolution)
        .into_iter()
        .filter(|kind| matches!(kind, DamageEventKind::KillAwarded { .. }))
        .collect();
    assert_eq!(awards.len(), 1, "a single kill is awarded");
    assert_eq!(
        awards[0],
        &DamageEventKind::KillAwarded {
            victim: target,
            credited: Some(attacker_early),
            rule: AttributionRule::FirstLethalHit,
            blow: early.id,
        },
        "the declared rule credits the first lethal hit in id order"
    );
    assert!(resolver.is_destroyed(&target));
}

/// The declared ordering makes resolution a function of the hit set, not
/// of producer append order: the same hits in a different input order
/// produce an identical event sequence.
#[test]
fn accept_f29_a_resolution_is_deterministic_under_input_shuffle() {
    let target = actor(1);
    let hits = [
        hit(
            9,
            0,
            Some(actor(10)),
            target,
            "nose_armor",
            DamageChannel::Armor,
            15.0,
            5,
        ),
        hit(
            3,
            0,
            Some(actor(11)),
            target,
            "engine_1",
            DamageChannel::Internal,
            20.0,
            5,
        ),
        hit(
            5,
            2,
            Some(actor(10)),
            target,
            "hull",
            DamageChannel::Internal,
            10.0,
            5,
        ),
    ];

    let mut forward = resolver(first_lethal(), target);
    let a = forward
        .resolve(
            Tick(5),
            &[hits[0].clone(), hits[1].clone(), hits[2].clone()],
        )
        .expect("valid");

    let mut shuffled = resolver(first_lethal(), target);
    let b = shuffled
        .resolve(
            Tick(5),
            &[hits[2].clone(), hits[0].clone(), hits[1].clone()],
        )
        .expect("valid");

    let kinds_a: Vec<_> = a.events.iter().map(|event| &event.kind).collect();
    let kinds_b: Vec<_> = b.events.iter().map(|event| &event.kind).collect();
    assert_eq!(kinds_a, kinds_b, "input order cannot change the resolution");
}

/// The declared `GreatestDamage` rule credits the attacker whose hits
/// applied the most damage to the victim — even when another attacker
/// struck the killing blow — while `blow` still records the causal hit.
#[test]
fn accept_f29_a_greatest_damage_credits_the_largest_contributor() {
    let target = actor(1);
    let killer = actor(10);
    let biggest = actor(20);
    let mut resolver = resolver(
        DamagePolicy {
            attribution: AttributionRule::GreatestDamage,
        },
        target,
    );

    let blow = hit(
        1,
        0,
        Some(killer),
        target,
        "hull",
        DamageChannel::Internal,
        45.0,
        5,
    );
    let hits = [
        blow.clone(),
        hit(
            1,
            1,
            Some(biggest),
            target,
            "engine_1",
            DamageChannel::Internal,
            20.0,
            5,
        ),
        hit(
            1,
            2,
            Some(biggest),
            target,
            "gun_mount_1",
            DamageChannel::Internal,
            20.0,
            5,
        ),
        hit(
            1,
            3,
            Some(biggest),
            target,
            "nose_armor",
            DamageChannel::Internal,
            30.0,
            5,
        ),
    ];
    // killer applies 40 (hull depleted); biggest applies 15 + 10 + 20 = 45.
    let resolution = resolver.resolve(Tick(5), &hits).expect("valid batch");

    let awards: Vec<_> = event_kinds(&resolution)
        .into_iter()
        .filter(|kind| matches!(kind, DamageEventKind::KillAwarded { .. }))
        .collect();
    assert_eq!(awards.len(), 1);
    assert_eq!(
        awards[0],
        &DamageEventKind::KillAwarded {
            victim: target,
            credited: Some(biggest),
            rule: AttributionRule::GreatestDamage,
            blow: blow.id,
        }
    );
}

/// A kill with no attributable attacker credits no one — the world can
/// destroy but cannot be awarded the kill — and a destroyed actor can
/// never emit destruction or scoring a second time, on the same tick or a
/// later one.
#[test]
fn accept_f29_a_destruction_and_scoring_emit_once_and_none_for_the_world() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);

    // An unattributed lethal hit destroys without crediting anyone.
    let first = resolver
        .resolve(
            Tick(5),
            &[hit(
                2,
                0,
                None,
                target,
                "hull",
                DamageChannel::Internal,
                50.0,
                5,
            )],
        )
        .expect("valid");
    let awards: Vec<_> = event_kinds(&first)
        .into_iter()
        .filter_map(|kind| match kind {
            DamageEventKind::KillAwarded { credited, .. } => Some(*credited),
            _ => None,
        })
        .collect();
    assert_eq!(awards, vec![None]);

    // A later lethal hit from another attacker: part damage still applies
    // elsewhere, but no second destruction or award is emitted.
    let second = resolver
        .resolve(
            Tick(6),
            &[
                hit(
                    2,
                    0,
                    Some(actor(30)),
                    target,
                    "hull",
                    DamageChannel::Internal,
                    50.0,
                    6,
                ),
                hit(
                    2,
                    1,
                    Some(actor(30)),
                    target,
                    "engine_1",
                    DamageChannel::Internal,
                    20.0,
                    6,
                ),
            ],
        )
        .expect("valid");
    for kind in event_kinds(&second) {
        assert!(
            !matches!(
                kind,
                DamageEventKind::Lifecycle { .. } | DamageEventKind::KillAwarded { .. }
            ),
            "a destroyed actor emits no further lifecycle or scoring events"
        );
    }
    assert_eq!(
        resolver.part_state(&target, &key(SYNTHETIC_ENGINE_NODE)),
        Some(PartState::Destroyed),
        "hits on a wreck still apply part damage"
    );
}

/// Armor and internal channels route distinguishably without any
/// multiplier: an armor-channel hit on the guarded hull depletes the armor
/// pool first and overflows the remainder, while an internal-channel hit
/// of the same raw damage reaches the hull directly and leaves the armor
/// untouched.
#[test]
fn accept_f29_a_armor_and_internal_channels_route_distinguishably() {
    let target = actor(1);
    let mut armored = resolver(first_lethal(), target);
    let armor_hit = armored
        .resolve(
            Tick(5),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_HULL_NODE,
                DamageChannel::Armor,
                25.0,
                5,
            )],
        )
        .expect("valid");
    let applied: Vec<_> = event_kinds(&armor_hit)
        .into_iter()
        .filter_map(|kind| match kind {
            DamageEventKind::HitApplied { node, applied, .. } => Some((node.clone(), *applied)),
            _ => None,
        })
        .collect();
    assert_eq!(
        applied,
        vec![
            (key(SYNTHETIC_ARMOR_NODE), SYNTHETIC_ARMOR_INTEGRITY),
            (key(SYNTHETIC_HULL_NODE), 5.0),
        ],
        "the armor pool absorbs first and only the remainder reaches the hull"
    );
    assert_eq!(
        resolver_remaining(&armored, &target, SYNTHETIC_HULL_NODE),
        Some(SYNTHETIC_HULL_INTEGRITY - 5.0)
    );

    let mut internal = resolver(first_lethal(), target);
    internal
        .resolve(
            Tick(5),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_HULL_NODE,
                DamageChannel::Internal,
                25.0,
                5,
            )],
        )
        .expect("valid");
    assert_eq!(
        resolver_remaining(&internal, &target, SYNTHETIC_HULL_NODE),
        Some(SYNTHETIC_HULL_INTEGRITY - 25.0),
        "the internal channel lands its full damage on the named node"
    );
    assert_eq!(
        internal.part_state(&target, &key(SYNTHETIC_ARMOR_NODE)),
        Some(PartState::Intact),
        "the armor pool is untouched by an internal hit"
    );
}

fn resolver_remaining(resolver: &DamageResolver, target: &ActorId, node: &str) -> Option<f64> {
    resolver.remaining_integrity(target, &key(node))
}

/// Destroying a weapon mount emits the part transition and the system
/// disablement — AC03's semantic half: the mount is recorded destroyed
/// and its `Weapon` system down, while propulsion stays enabled.
#[test]
fn accept_f29_a_mount_destruction_emits_part_and_system_transitions() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);
    let resolution = resolver
        .resolve(
            Tick(5),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_MOUNT_NODE,
                DamageChannel::Internal,
                20.0,
                5,
            )],
        )
        .expect("valid");

    let kinds = event_kinds(&resolution);
    assert!(kinds.contains(&&DamageEventKind::PartTransition {
        node: key(SYNTHETIC_MOUNT_NODE),
        from: PartState::Intact,
        to: PartState::Destroyed,
    }));
    assert!(kinds.contains(&&DamageEventKind::SystemDisabled {
        node: key(SYNTHETIC_MOUNT_NODE),
        system: SystemKind::Weapon,
    }));
    assert_eq!(
        resolver.part_state(&target, &key(SYNTHETIC_ENGINE_NODE)),
        Some(PartState::Intact),
        "an unrelated system is unaffected"
    );
    assert!(!resolver.is_destroyed(&target), "a mount is not lethal");
}

/// The five lifecycle transitions are distinct records: bailout is not
/// death, capture is not destruction, and a terminal despawn or mission
/// removal closes the record — each kind fires at most once.
#[test]
fn accept_f29_a_lifecycle_kinds_are_distinct_once_and_terminal() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);

    // Bailout and destruction are separate transitions; recording one
    // does not imply the other.
    let bailout = resolver
        .record_lifecycle(target, LifecycleKind::PilotBailout, Tick(5))
        .expect("bailout records");
    assert!(matches!(
        bailout.kind,
        DamageEventKind::Lifecycle {
            kind: LifecycleKind::PilotBailout,
            ..
        }
    ));
    assert!(!resolver.is_destroyed(&target));

    assert_eq!(
        resolver.record_lifecycle(target, LifecycleKind::PilotBailout, Tick(5)),
        Err(DamageError::DuplicateLifecycle {
            actor: target,
            kind: LifecycleKind::PilotBailout,
        }),
        "each kind fires once per actor per session"
    );

    resolver
        .record_lifecycle(target, LifecycleKind::OwnershipCaptured, Tick(5))
        .expect("capture is a distinct transition");
    resolver
        .record_lifecycle(target, LifecycleKind::Despawned, Tick(6))
        .expect("despawn records");

    assert_eq!(
        resolver.record_lifecycle(target, LifecycleKind::MissionRemoved, Tick(6)),
        Err(DamageError::ActorClosed {
            actor: target,
            terminal: LifecycleKind::Despawned,
        }),
        "a terminal transition closes the record"
    );

    let lifecycle = resolver.lifecycle(&target).expect("registered");
    assert!(lifecycle.contains(&LifecycleKind::PilotBailout));
    assert!(lifecycle.contains(&LifecycleKind::OwnershipCaptured));
    assert!(lifecycle.contains(&LifecycleKind::Despawned));
    assert!(!lifecycle.contains(&LifecycleKind::Destroyed));
    assert!(!lifecycle.contains(&LifecycleKind::MissionRemoved));
}

/// Session and tick confinement: hits stamped for another session or
/// another tick are refused by name before anything is applied, and a
/// foreign-session actor can never register — state cannot leak across
/// generations (STATE-TRANSACTIONS).
#[test]
fn accept_f29_a_foreign_sessions_and_ticks_are_refused() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);

    let mut foreign = hit(1, 0, None, target, "hull", DamageChannel::Internal, 10.0, 5);
    foreign.id.session = session(SESSION + 1);
    assert_eq!(
        resolver.resolve(Tick(5), &[foreign]),
        Err(DamageError::ForeignSession {
            expected: session(SESSION),
            found: session(SESSION + 1),
        })
    );

    assert_eq!(
        resolver.resolve(
            Tick(5),
            &[hit(
                1,
                0,
                None,
                target,
                "hull",
                DamageChannel::Internal,
                10.0,
                6
            )]
        ),
        Err(DamageError::ForeignTick {
            expected: Tick(5),
            found: Tick(6),
        })
    );

    let foreign_actor = ActorId {
        session: session(SESSION + 1),
        serial: 1,
    };
    assert_eq!(
        resolver.register_actor(foreign_actor, synthetic_airframe_graph(), first_lethal()),
        Err(DamageError::ForeignSession {
            expected: session(SESSION),
            found: session(SESSION + 1),
        })
    );
    assert_eq!(
        resolver.register_actor(target, synthetic_airframe_graph(), first_lethal()),
        Err(DamageError::DuplicateActor { actor: target })
    );
}

/// A refused input is an ordered output record, never a silent drop: hits
/// on an unregistered actor or an unknown node are `HitRefused` events
/// naming their reason.
#[test]
fn accept_f29_a_unknown_targets_and_nodes_are_refused_visibly() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);
    let resolution = resolver
        .resolve(
            Tick(5),
            &[
                hit(
                    1,
                    0,
                    None,
                    actor(99),
                    "hull",
                    DamageChannel::Internal,
                    10.0,
                    5,
                ),
                hit(
                    1,
                    1,
                    None,
                    target,
                    "tailplane",
                    DamageChannel::Internal,
                    10.0,
                    5,
                ),
            ],
        )
        .expect("valid");
    let kinds = event_kinds(&resolution);
    assert_eq!(
        kinds,
        vec![
            &DamageEventKind::HitRefused {
                hit: HitEventId {
                    session: session(SESSION),
                    tick: Tick(5),
                    producer: 1,
                    sequence: 0,
                },
                reason: RefusalReason::UnknownTargetActor,
            },
            &DamageEventKind::HitRefused {
                hit: HitEventId {
                    session: session(SESSION),
                    tick: Tick(5),
                    producer: 1,
                    sequence: 1,
                },
                reason: RefusalReason::UnknownNode,
            },
        ]
    );
    assert_eq!(
        resolver.part_state(&target, &key("tailplane")),
        None,
        "a node outside the graph reports no state — distinct from an \
         unresolved pool's `PartState::Unknown`"
    );
    assert_eq!(
        resolver.part_state(&actor(99), &key(SYNTHETIC_HULL_NODE)),
        None,
        "an unregistered actor reports no state"
    );
}

/// Two hits sharing one identity make ordering ambiguous; the batch is
/// refused before anything is applied.
#[test]
fn accept_f29_a_duplicate_hit_ids_are_refused() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);
    let hit = hit(1, 0, None, target, "hull", DamageChannel::Internal, 10.0, 5);
    assert_eq!(
        resolver.resolve(Tick(5), &[hit.clone(), hit.clone()]),
        Err(DamageError::DuplicateHit { id: hit.id })
    );
    assert_eq!(
        resolver.part_state(&target, &key(SYNTHETIC_HULL_NODE)),
        Some(PartState::Intact),
        "a refused batch applies nothing"
    );
}

/// A hit routed at a node whose integrity is `Resolved::Unknown` blocks
/// visibly with its claim — it is never absorbed by an invented capacity
/// and the node's state stays `Unknown`.
#[test]
fn accept_f29_a_unknown_integrity_blocks_the_hit_visibly() {
    let claim = ClaimId::new("f29a.test.unknown-integrity").expect("valid");
    let mut graph_nodes: Vec<DamageNode> = synthetic_airframe_graph().nodes().cloned().collect();
    let engine = graph_nodes
        .iter_mut()
        .find(|node| node.key() == &key(SYNTHETIC_ENGINE_NODE))
        .expect("fixture has the engine");
    *engine = DamageNode::new(
        key(SYNTHETIC_ENGINE_NODE),
        DamageNodeKind::Engine,
        Resolved::Unknown {
            claim_id: claim.clone(),
            reason: "engine integrity unmeasured".to_owned(),
        },
    )
    .with_disables(SystemKind::Propulsion);
    let graph = DamageGraph::try_new(synthetic_airframe_graph().subject().clone(), graph_nodes)
        .expect("graph with an unknown pool still validates");

    let target = actor(1);
    let mut resolver = DamageResolver::new(session(SESSION), RESOLVER_PRODUCER);
    resolver
        .register_actor(target, graph, first_lethal())
        .expect("registers");

    let resolution = resolver
        .resolve(
            Tick(5),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                SYNTHETIC_ENGINE_NODE,
                DamageChannel::Internal,
                10.0,
                5,
            )],
        )
        .expect("valid");
    let kinds = event_kinds(&resolution);
    assert_eq!(
        kinds,
        vec![&DamageEventKind::HitBlocked {
            hit: HitEventId {
                session: session(SESSION),
                tick: Tick(5),
                producer: 1,
                sequence: 0,
            },
            claim_id: claim,
            reason: "engine integrity unmeasured".to_owned(),
        }],
        "the unknown is surfaced, never guessed"
    );
    assert_eq!(
        resolver.part_state(&target, &key(SYNTHETIC_ENGINE_NODE)),
        Some(PartState::Unknown)
    );
}

/// Graph validation rejects every malformed edge and value by name —
/// identity and topology errors can never reach a resolution.
#[test]
fn accept_f29_a_graph_validation_rejects_malformed_graphs() {
    use cs_sim::damage::DamageGraphError;
    let subject = synthetic_airframe_graph().subject().clone();
    let known = |value: f64| {
        Resolved::Known(Known::new(
            value,
            Provenance::designed(ClaimId::new("f29a.test.graph").expect("valid")),
        ))
    };

    // Duplicate keys.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![
                DamageNode::new(key("a"), DamageNodeKind::Engine, known(1.0)),
                DamageNode::new(key("a"), DamageNodeKind::Engine, known(1.0)),
            ],
        ),
        Err(DamageGraphError::DuplicateNode { key: key("a") })
    );

    // An overflow cycle.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![
                DamageNode::new(key("a"), DamageNodeKind::Engine, known(1.0))
                    .with_overflow(key("b")),
                DamageNode::new(key("b"), DamageNodeKind::Engine, known(1.0))
                    .with_overflow(key("a")),
            ],
        ),
        Err(DamageGraphError::OverflowCycle { node: key("a") })
    );

    // A guard edge to a non-armor node.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![
                DamageNode::new(key("a"), DamageNodeKind::InternalStructure, known(1.0))
                    .with_guard(key("b")),
                DamageNode::new(key("b"), DamageNodeKind::Engine, known(1.0)),
            ],
        ),
        Err(DamageGraphError::GuardNotArmor {
            node: key("a"),
            kind: DamageNodeKind::Engine,
        })
    );

    // An armor node that is itself armor-guarded.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![
                DamageNode::new(key("a"), DamageNodeKind::ArmorZone, known(1.0))
                    .with_guard(key("b")),
                DamageNode::new(key("b"), DamageNodeKind::ArmorZone, known(1.0)),
            ],
        ),
        Err(DamageGraphError::ArmorGuarded { node: key("a") })
    );

    // A dangling guard edge.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![
                DamageNode::new(key("a"), DamageNodeKind::Engine, known(1.0))
                    .with_guard(key("ghost"))
            ],
        ),
        Err(DamageGraphError::UnknownGuard {
            node: key("a"),
            guard: key("ghost"),
        })
    );

    // A negative pool.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![DamageNode::new(
                key("a"),
                DamageNodeKind::Engine,
                known(-1.0)
            )],
        ),
        Err(DamageGraphError::NegativeIntegrity {
            node: key("a"),
            value: -1.0,
        })
    );

    // A non-finite pool.
    assert_eq!(
        DamageGraph::try_new(
            subject.clone(),
            vec![DamageNode::new(
                key("a"),
                DamageNodeKind::Engine,
                known(f64::NAN)
            )],
        ),
        Err(DamageGraphError::NonFiniteIntegrity { node: key("a") })
    );
}

/// Typed input boundary: a hit cannot carry non-finite or negative
/// damage — the producer fails at its own boundary.
#[test]
fn accept_f29_a_hit_inputs_reject_non_finite_and_negative_damage() {
    let id = HitEventId {
        session: session(SESSION),
        tick: Tick(5),
        producer: 1,
        sequence: 0,
    };
    assert_eq!(
        HitEvent::try_new(
            id,
            None,
            actor(1),
            key("hull"),
            DamageChannel::Internal,
            f64::NAN,
        ),
        Err(HitEventError::NonFiniteDamage)
    );
    assert_eq!(
        HitEvent::try_new(
            id,
            None,
            actor(1),
            key("hull"),
            DamageChannel::Internal,
            -1.0,
        ),
        Err(HitEventError::NegativeDamage { value: -1.0 })
    );
}

/// The fixture itself: the declared synthetic graph exposes armor,
/// lethal structure, an engine and a mount as distinct nodes.
#[test]
fn accept_f29_a_synthetic_graph_declares_distinct_parts() {
    let graph = synthetic_airframe_graph();
    assert_eq!(graph.len(), 4);
    assert_eq!(
        graph.node(&key(SYNTHETIC_ARMOR_NODE)).map(|n| n.kind()),
        Some(DamageNodeKind::ArmorZone)
    );
    assert!(
        graph
            .node(&key(SYNTHETIC_HULL_NODE))
            .expect("hull exists")
            .is_lethal()
    );
    assert_eq!(
        graph
            .node(&key(SYNTHETIC_HULL_NODE))
            .and_then(|n| n.guarded_by()),
        Some(&key(SYNTHETIC_ARMOR_NODE))
    );
    assert_eq!(
        graph
            .node(&key(SYNTHETIC_ARMOR_NODE))
            .and_then(|n| n.overflow()),
        Some(&key(SYNTHETIC_HULL_NODE))
    );
    assert_eq!(
        graph
            .node(&key(SYNTHETIC_ENGINE_NODE))
            .and_then(|n| n.disables()),
        Some(SystemKind::Propulsion)
    );
    match graph
        .node(&key(SYNTHETIC_ENGINE_NODE))
        .expect("engine exists")
        .integrity()
    {
        Resolved::Known(known) => {
            assert_eq!(known.value, SYNTHETIC_ENGINE_INTEGRITY);
        }
        Resolved::Unknown { .. } => panic!("the fixture's pools are all known"),
    }
}

/// "The same identity discipline but their own rules": two actors in one
/// session resolve side by side under *their own* declared attribution —
/// the aircraft under `FirstLethalHit`, the ship under `GreatestDamage`.
/// A single session-wide rule could not express this.
#[test]
fn accept_f29_a_actors_resolve_under_their_own_declared_rules() {
    let aircraft = actor(1);
    let ship = actor(2);
    let attacker_a = actor(10);
    let attacker_b = actor(11);
    let attacker_c = actor(12);
    let ship_policy = DamagePolicy {
        attribution: AttributionRule::GreatestDamage,
    };

    let mut resolver = DamageResolver::new(session(SESSION), RESOLVER_PRODUCER);
    resolver
        .register_actor(aircraft, synthetic_airframe_graph(), first_lethal())
        .expect("aircraft registers");
    resolver
        .register_actor(ship, synthetic_airframe_graph(), ship_policy)
        .expect("ship registers");
    assert_eq!(resolver.policy(&aircraft), Some(first_lethal()));
    assert_eq!(resolver.policy(&ship), Some(ship_policy));

    let aircraft_blow = hit(
        2,
        0,
        Some(attacker_a),
        aircraft,
        "hull",
        DamageChannel::Internal,
        50.0,
        5,
    );
    let ship_blow = hit(
        7,
        0,
        Some(attacker_b),
        ship,
        "hull",
        DamageChannel::Internal,
        45.0,
        5,
    );
    let hits = [
        aircraft_blow.clone(),
        // attacker_c contributes 15 + 10 + 20 = 45 to the ship — more
        // than attacker_b's 40, but attacker_b struck the killing blow.
        hit(
            5,
            0,
            Some(attacker_c),
            ship,
            "engine_1",
            DamageChannel::Internal,
            20.0,
            5,
        ),
        hit(
            5,
            1,
            Some(attacker_c),
            ship,
            "gun_mount_1",
            DamageChannel::Internal,
            20.0,
            5,
        ),
        hit(
            5,
            2,
            Some(attacker_c),
            ship,
            "nose_armor",
            DamageChannel::Internal,
            30.0,
            5,
        ),
        // A second lethal hit on the aircraft by another attacker.
        hit(
            6,
            0,
            Some(attacker_b),
            aircraft,
            "hull",
            DamageChannel::Internal,
            50.0,
            5,
        ),
        ship_blow.clone(),
    ];
    let resolution = resolver.resolve(Tick(5), &hits).expect("valid batch");

    let destructions: Vec<_> = event_kinds(&resolution)
        .into_iter()
        .filter(|kind| {
            matches!(
                kind,
                DamageEventKind::Lifecycle {
                    kind: LifecycleKind::Destroyed,
                    ..
                }
            )
        })
        .collect();
    assert_eq!(
        destructions,
        vec![
            &DamageEventKind::Lifecycle {
                actor: aircraft,
                kind: LifecycleKind::Destroyed,
            },
            &DamageEventKind::Lifecycle {
                actor: ship,
                kind: LifecycleKind::Destroyed,
            },
        ],
        "each victim emits exactly one destruction, in actor order"
    );

    let awards: Vec<_> = event_kinds(&resolution)
        .into_iter()
        .filter_map(|kind| match kind {
            DamageEventKind::KillAwarded {
                victim,
                credited,
                rule,
                blow,
            } => Some((*victim, *credited, *rule, *blow)),
            _ => None,
        })
        .collect();
    assert_eq!(
        awards,
        vec![
            (
                aircraft,
                Some(attacker_a),
                AttributionRule::FirstLethalHit,
                aircraft_blow.id,
            ),
            (
                ship,
                Some(attacker_c),
                AttributionRule::GreatestDamage,
                ship_blow.id,
            ),
        ],
        "each victim's award is computed under its own declared rule"
    );
}

/// A record closed by a terminal transition records nothing again: a
/// despawned actor still takes part damage from a later hit batch — the
/// hit lands — but no `Destroyed` lifecycle and no kill award can be
/// emitted for it.
#[test]
fn accept_f29_a_terminal_record_emits_no_destruction_or_award() {
    let target = actor(1);
    let mut resolver = resolver(first_lethal(), target);
    resolver
        .record_lifecycle(target, LifecycleKind::Despawned, Tick(5))
        .expect("despawn records");

    let resolution = resolver
        .resolve(
            Tick(6),
            &[hit(
                1,
                0,
                Some(actor(2)),
                target,
                "hull",
                DamageChannel::Internal,
                50.0,
                6,
            )],
        )
        .expect("valid batch");
    let kinds = event_kinds(&resolution);
    assert!(
        kinds.iter().any(|kind| matches!(
            kind,
            DamageEventKind::HitApplied { node, .. } if node == &key(SYNTHETIC_HULL_NODE)
        )),
        "the hit still lands and applies part damage"
    );
    for kind in &kinds {
        assert!(
            !matches!(
                kind,
                DamageEventKind::Lifecycle { .. } | DamageEventKind::KillAwarded { .. }
            ),
            "a closed record emits no lifecycle or scoring event: {kind:?}"
        );
    }
    assert_eq!(
        resolver.part_state(&target, &key(SYNTHETIC_HULL_NODE)),
        Some(PartState::Destroyed)
    );
    assert!(
        !resolver.is_destroyed(&target),
        "no destruction was recorded for the despawned actor"
    );
}

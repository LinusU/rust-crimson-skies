//! Acceptance scenario F29-A: the declared → runtime conversion boundary
//! and the ECS binding record.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Task test prefix: `accept_f29_a_`.
//!
//! These tests drive production code only: [`cs_app::damage`]'s
//! [`lower_graph`], [`lower_policy`] and [`DamageActorBinding`], plus the
//! `cs_sim::damage::DamageResolver` the lowered records feed — the AC01
//! scenario runs end-to-end through the lowered graph, so removing the
//! conversion or carrying an unknown across silently fails them.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_app::damage::{DamageActorBinding, DamageLowerError, lower_graph, lower_policy};
use cs_app::scene::SceneGeneration;
use cs_content::damage::{DamageNodeKind, GraphSubjectKind, declared_synthetic_airframe_damage};
use cs_sim::damage::{
    ActorId, AttributionRule as RuntimeAttribution, DamageChannel, DamageEventKind,
    DamageNodeKind as RuntimeNodeKind, DamageResolver, HitEvent, HitEventId, LifecycleKind,
};
use cs_types::Tick;
use cs_types::content::{ContentKind, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::net::SessionId;

const SESSION: u64 = 3;

fn session(value: u64) -> SessionId {
    SessionId::new(value).expect("a nonzero session generation")
}

fn actor(serial: u64) -> ActorId {
    ActorId {
        session: session(SESSION),
        serial,
    }
}

/// `lower_graph` preserves the whole declared structure: keys, kinds,
/// edges, integrities and scene bindings map field-wise onto the runtime
/// graph — nothing is dropped, renamed or repaired.
#[test]
fn accept_f29_a_lower_graph_preserves_the_declared_structure() {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("the fixture lowers");

    assert_eq!(graph.subject(), declared.subject());
    assert_eq!(graph.len(), declared.nodes().len());

    for declared_node in declared.nodes() {
        let runtime = graph
            .node(
                &cs_sim::damage::DamageNodeKey::new(declared_node.key.as_str())
                    .expect("key lowers"),
            )
            .expect("every declared node lowers");
        let expected_kind = match declared_node.kind {
            DamageNodeKind::ArmorZone => RuntimeNodeKind::ArmorZone,
            DamageNodeKind::InternalStructure => RuntimeNodeKind::InternalStructure,
            DamageNodeKind::Engine => RuntimeNodeKind::Engine,
            DamageNodeKind::WeaponMount => RuntimeNodeKind::WeaponMount,
        };
        assert_eq!(runtime.kind(), expected_kind);
        assert_eq!(runtime.integrity(), &declared_node.integrity);
        assert_eq!(runtime.is_lethal(), declared_node.lethal);
        assert_eq!(
            runtime.overflow().map(|key| key.as_str()),
            declared_node.overflow.as_ref().map(|key| key.as_str())
        );
        assert_eq!(
            runtime.guarded_by().map(|key| key.as_str()),
            declared_node.guarded_by.as_ref().map(|key| key.as_str())
        );

        // Scene bindings shed the SceneNodeId wrapper but keep the id and
        // the resolved state.
        match (&declared_node.scene_binding, runtime.scene_binding()) {
            (Some(Resolved::Known(declared)), Some(Resolved::Known(lowered))) => {
                assert_eq!(lowered.value, *declared.value.as_content_id());
                assert_eq!(lowered.value.kind(), ContentKind::SceneNode);
            }
            (declared, lowered) => panic!("scene binding mismatch: {declared:?} vs {lowered:?}"),
        }
    }
}

/// `lower_policy` lowers a declared attribution rule field-wise, and an
/// `Unknown` rule is refused by claim — a session never resolves kills
/// under a guessed rule.
#[test]
fn accept_f29_a_lower_policy_lowers_the_rule_and_refuses_unknown() {
    let declared = declared_synthetic_airframe_damage();
    let policy = lower_policy(&declared).expect("the fixture's policy lowers");
    assert_eq!(policy.attribution, RuntimeAttribution::FirstLethalHit);

    // The same graph with an unresolved attribution cannot lower a
    // policy: the boundary refuses, naming the claim.
    let claim = ClaimId::new("f29a.test.unknown-attribution").expect("valid");
    let mut nodes = declared.nodes().to_vec();
    nodes.pop();
    let graph = cs_content::damage::DeclaredDamageGraph::try_new(
        declared.subject().clone(),
        declared.origin().clone(),
        GraphSubjectKind::Aircraft,
        cs_content::damage::GraphRules {
            lethal_attribution: Resolved::Unknown {
                claim_id: claim.clone(),
                reason: "original attribution unmeasured".to_owned(),
            },
        },
        nodes,
        declared.provenance().clone(),
    )
    .expect("the graph itself is still valid");

    assert_eq!(
        lower_policy(&graph),
        Err(DamageLowerError::UnknownAttribution {
            claim_id: claim,
            reason: "original attribution unmeasured".to_owned(),
        })
    );
}

/// An unresolved integrity lowers as unresolved: the runtime record
/// carries `Resolved::Unknown` verbatim so the resolver blocks the hits
/// routed at it instead of absorbing them into an invented pool.
#[test]
fn accept_f29_a_unknown_integrity_lowers_through_verbatim() {
    let declared = declared_synthetic_airframe_damage();
    let mut nodes = declared.nodes().to_vec();
    let claim = ClaimId::new("f29a.test.lowered-unknown").expect("valid");
    nodes[2].integrity = Resolved::Unknown {
        claim_id: claim.clone(),
        reason: "engine integrity unmeasured".to_owned(),
    };
    let graph = cs_content::damage::DeclaredDamageGraph::try_new(
        declared.subject().clone(),
        declared.origin().clone(),
        declared.subject_kind(),
        cs_content::damage::GraphRules {
            lethal_attribution: declared.rules().lethal_attribution.clone(),
        },
        nodes,
        declared.provenance().clone(),
    )
    .expect("an unknown pool still validates");

    let lowered = lower_graph(&graph).expect("lowers");
    let engine = lowered
        .node(&cs_sim::damage::DamageNodeKey::new("engine_1").expect("valid"))
        .expect("engine lowers");
    let Resolved::Unknown { claim_id, reason } = engine.integrity() else {
        panic!("the unknown integrity lowered through verbatim");
    };
    assert_eq!(claim_id, &claim);
    assert_eq!(reason, "engine integrity unmeasured");
}

/// The AC01 scenario end-to-end through the production path: declared
/// fixture → `lower_graph` + `lower_policy` → session resolver → two
/// same-tick lethal hits award a single kill under the declared rule.
#[test]
fn accept_f29_a_lowered_graph_resolves_a_single_kill() {
    let declared = declared_synthetic_airframe_damage();
    let graph = lower_graph(&declared).expect("lowers");
    let policy = lower_policy(&declared).expect("policy lowers");

    let target = actor(1);
    let mut resolver = DamageResolver::new(session(SESSION), 1);
    resolver
        .register_actor(target, graph, policy)
        .expect("registers");

    let hit = |producer: u32, attacker: ActorId| {
        HitEvent::try_new(
            HitEventId {
                session: session(SESSION),
                tick: Tick(9),
                producer,
                sequence: 0,
            },
            Some(attacker),
            target,
            cs_sim::damage::DamageNodeKey::new("hull").expect("valid"),
            DamageChannel::Internal,
            50.0,
        )
        .expect("valid")
    };
    // Two same-tick lethal hits in reverse id order.
    let resolution = resolver
        .resolve(Tick(9), &[hit(8, actor(8)), hit(2, actor(2))])
        .expect("valid batch");

    let lifecycles = resolution
        .events
        .iter()
        .filter(|event| {
            matches!(
                event.kind,
                DamageEventKind::Lifecycle {
                    kind: LifecycleKind::Destroyed,
                    ..
                }
            )
        })
        .count();
    let awards: Vec<_> = resolution
        .events
        .iter()
        .filter_map(|event| match &event.kind {
            DamageEventKind::KillAwarded { credited, .. } => Some(*credited),
            _ => None,
        })
        .collect();
    assert_eq!(lifecycles, 1, "two lethal hits emit one destruction");
    assert_eq!(
        awards,
        vec![Some(actor(2))],
        "the declared FirstLethalHit rule credits the earlier hit"
    );
}

/// The binding record ties an entity's actor, graph and generation: a
/// reload under a new generation produces a distinguishable record.
#[test]
fn accept_f29_a_damage_actor_binding_is_generation_qualified() {
    let subject = declared_synthetic_airframe_damage().subject().clone();
    let binding = DamageActorBinding {
        actor: actor(1),
        graph: subject.clone(),
        generation: SceneGeneration(1),
    };
    let reloaded = DamageActorBinding {
        generation: SceneGeneration(1).next(),
        ..binding.clone()
    };
    assert_ne!(binding, reloaded, "a reload stamps a new generation");
    assert_eq!(binding.actor, actor(1));
    assert_eq!(binding.graph, subject);
}

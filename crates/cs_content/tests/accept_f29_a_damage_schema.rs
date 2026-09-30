//! Acceptance scenario F29-A: the declared, provenance-carrying damage
//! schema — its validation, its `Resolved` discipline and the synthetic
//! fixture.
//!
//! Spec: `specs/F29-damage-zones-armor-destruction-and-bailout.md`, stage
//! `### F29-A`. Task test prefix: `accept_f29_a_`.
//!
//! These tests drive production code only: [`cs_content::damage`]'s
//! [`DeclaredDamageGraph`], [`GraphRules`], [`DeclaredDamageNode`] and the
//! `declared_synthetic_airframe_damage` fixture. Removing the validation
//! or silently defaulting an unresolved value makes them fail.
//!
//! Every value here is newly authored synthetic fixture data, never
//! original game data.

use cs_content::damage::{
    AttributionRule, DamageNodeKey, DamageNodeKeyError, DamageNodeKind, DamageSchemaError,
    DeclaredDamageGraph, DeclaredDamageNode, GraphRules, GraphSubjectKind, SystemKind,
    declared_synthetic_airframe_damage,
};
use cs_content::scene::SceneNodeId;
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("test node keys are valid")
}

fn subject() -> ContentId {
    ContentId::from_source(ContentKind::Airframe, "synthetic.devastator").expect("valid")
}

fn provenance() -> Provenance {
    Provenance::designed(ClaimId::new("f29a.test.schema").expect("valid"))
}

fn known_integrity(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(value, provenance()))
}

fn node(name: &str, kind: DamageNodeKind, integrity: f64) -> DeclaredDamageNode {
    DeclaredDamageNode {
        key: key(name),
        kind,
        scene_binding: None,
        integrity: known_integrity(integrity),
        lethal: false,
        disables: None,
        guarded_by: None,
        overflow: None,
    }
}

fn rules() -> GraphRules {
    GraphRules {
        lethal_attribution: Resolved::Known(Known::new(
            AttributionRule::FirstLethalHit,
            provenance(),
        )),
    }
}

fn graph(nodes: Vec<DeclaredDamageNode>) -> Result<DeclaredDamageGraph, DamageSchemaError> {
    DeclaredDamageGraph::try_new(
        subject(),
        Origin::SyntheticFixture,
        GraphSubjectKind::Aircraft,
        rules(),
        nodes,
        provenance(),
    )
}

/// The declared synthetic fixture validates and carries the parts the
/// deliverable names as distinct nodes, each known with provenance.
#[test]
fn accept_f29_a_synthetic_fixture_is_valid_and_distinct() {
    let graph = declared_synthetic_airframe_damage();
    assert_eq!(graph.subject_kind(), GraphSubjectKind::Aircraft);
    assert_eq!(graph.subject(), &subject());
    assert_eq!(graph.origin(), &Origin::SyntheticFixture);
    assert_eq!(graph.nodes().len(), 4);

    for kind in [
        DamageNodeKind::ArmorZone,
        DamageNodeKind::InternalStructure,
        DamageNodeKind::Engine,
        DamageNodeKind::WeaponMount,
    ] {
        assert!(
            graph.nodes().iter().any(|node| node.kind == kind),
            "the fixture keeps {kind} distinct"
        );
    }

    let hull = graph.node(&key("hull")).expect("hull exists");
    assert!(hull.lethal);
    assert_eq!(hull.guarded_by, Some(key("nose_armor")));
    let armor = graph.node(&key("nose_armor")).expect("armor exists");
    assert_eq!(armor.overflow, Some(key("hull")));
}

/// Every load-bearing value is a `Resolved`: integrity pools and scene
/// bindings are known *with provenance* — there is no default path.
#[test]
fn accept_f29_a_fixture_values_carry_provenance() {
    let graph = declared_synthetic_airframe_damage();
    for node in graph.nodes() {
        let Resolved::Known(integrity) = &node.integrity else {
            panic!("fixture integrities are known");
        };
        assert!(integrity.value.is_finite() && integrity.value > 0.0);
        assert_eq!(
            integrity.provenance.class,
            cs_types::evidence::ClaimStatus::Designed,
            "fixture values are declared design, not measured original data"
        );

        let Some(Resolved::Known(binding)) = &node.scene_binding else {
            panic!("fixture scene bindings are known");
        };
        assert_eq!(
            binding.value.as_content_id().kind(),
            ContentKind::SceneNode,
            "a part binding names a scene node"
        );
    }

    let Resolved::Known(attribution) = &graph.rules().lethal_attribution else {
        panic!("the fixture declares its attribution rule");
    };
    assert_eq!(attribution.value, AttributionRule::FirstLethalHit);
}

/// An explicitly unknown value stays unknown: a node whose integrity is
/// `Resolved::Unknown` validates and round-trips as unknown — it can never
/// silently become zero.
#[test]
fn accept_f29_a_unknown_integrity_stays_unknown() {
    let mut unknown_engine = node("engine_1", DamageNodeKind::Engine, 15.0);
    unknown_engine.integrity = Resolved::Unknown {
        claim_id: ClaimId::new("f29a.test.unknown-engine").expect("valid"),
        reason: "engine integrity unmeasured".to_owned(),
    };
    unknown_engine.disables = Some(SystemKind::Propulsion);

    let graph = graph(vec![unknown_engine]).expect("an unknown pool validates");
    let engine = graph.node(&key("engine_1")).expect("exists");
    let Resolved::Unknown { claim_id, reason } = &engine.integrity else {
        panic!("an unknown integrity stays unknown");
    };
    assert_eq!(claim_id.as_str(), "f29a.test.unknown-engine");
    assert_eq!(reason, "engine integrity unmeasured");
}

/// Validation rejects the same malformed graphs the runtime graph
/// rejects — the declared and runtime rules cannot diverge.
#[test]
fn accept_f29_a_schema_rejects_malformed_graphs() {
    // Empty.
    assert_eq!(graph(vec![]), Err(DamageSchemaError::EmptyGraph));

    // Duplicate keys.
    let dup = graph(vec![
        node("a", DamageNodeKind::Engine, 1.0),
        node("a", DamageNodeKind::Engine, 1.0),
    ]);
    assert_eq!(dup, Err(DamageSchemaError::DuplicateNode { key: key("a") }));

    // An overflow cycle.
    let mut a = node("a", DamageNodeKind::Engine, 1.0);
    a.overflow = Some(key("b"));
    let mut b = node("b", DamageNodeKind::Engine, 1.0);
    b.overflow = Some(key("a"));
    assert_eq!(
        graph(vec![a, b]),
        Err(DamageSchemaError::OverflowCycle { node: key("a") })
    );

    // A self overflow.
    let mut selfish = node("a", DamageNodeKind::Engine, 1.0);
    selfish.overflow = Some(key("a"));
    assert_eq!(
        graph(vec![selfish]),
        Err(DamageSchemaError::SelfOverflow { node: key("a") })
    );

    // A dangling overflow.
    let mut dangling = node("a", DamageNodeKind::Engine, 1.0);
    dangling.overflow = Some(key("ghost"));
    assert_eq!(
        graph(vec![dangling]),
        Err(DamageSchemaError::UnknownOverflow {
            node: key("a"),
            overflow: key("ghost"),
        })
    );

    // A guard edge to a non-armor node.
    let mut guarded = node("a", DamageNodeKind::InternalStructure, 1.0);
    guarded.guarded_by = Some(key("b"));
    assert_eq!(
        graph(vec![guarded, node("b", DamageNodeKind::Engine, 1.0)]),
        Err(DamageSchemaError::GuardNotArmor {
            node: key("a"),
            kind: DamageNodeKind::Engine,
        })
    );

    // Armor cannot itself be armor-guarded.
    let mut nested = node("a", DamageNodeKind::ArmorZone, 1.0);
    nested.guarded_by = Some(key("b"));
    assert_eq!(
        graph(vec![nested, node("b", DamageNodeKind::ArmorZone, 1.0)]),
        Err(DamageSchemaError::ArmorGuarded { node: key("a") })
    );

    // A self guard.
    let mut selfish = node("a", DamageNodeKind::ArmorZone, 1.0);
    selfish.guarded_by = Some(key("a"));
    assert_eq!(
        graph(vec![selfish]),
        Err(DamageSchemaError::SelfGuard { node: key("a") })
    );

    // Negative and non-finite known pools are refused, never clamped.
    assert_eq!(
        graph(vec![node("a", DamageNodeKind::Engine, -1.0)]),
        Err(DamageSchemaError::NegativeIntegrity {
            node: key("a"),
            value: -1.0,
        })
    );
    assert_eq!(
        graph(vec![node("a", DamageNodeKind::Engine, f64::INFINITY)]),
        Err(DamageSchemaError::NonFiniteIntegrity { node: key("a") })
    );
}

/// Node keys share the content-id grammar: lowercased, bounded, no path
/// separators — a key can never smuggle a lookup elsewhere.
#[test]
fn accept_f29_a_node_key_grammar_matches_content_id_discipline() {
    assert_eq!(DamageNodeKey::new(""), Err(DamageNodeKeyError::Empty));
    assert_eq!(
        DamageNodeKey::new("UPPER_CASE"),
        Ok(key("upper_case")),
        "uppercase folds like ContentId"
    );
    assert!(matches!(
        DamageNodeKey::new("a/b"),
        Err(DamageNodeKeyError::BadCharacter { ch: '/' })
    ));
    assert!(matches!(
        DamageNodeKey::new("..."),
        Err(DamageNodeKeyError::NoAlphanumeric)
    ));
    assert!(matches!(
        DamageNodeKey::new(&"x".repeat(129)),
        Err(DamageNodeKeyError::TooLong { len: 129 })
    ));
}

/// A scene binding is a typed [`SceneNodeId`], never a raw string: it
/// cannot name a non-`scene_node` id.
#[test]
fn accept_f29_a_scene_bindings_are_typed_scene_nodes() {
    let binding_id =
        ContentId::from_source(ContentKind::SceneNode, "synthetic.devastator.hull").expect("valid");
    let binding = SceneNodeId::from_content_id(binding_id).expect("names a scene node");
    let mut bound = node("hull", DamageNodeKind::InternalStructure, 40.0);
    bound.scene_binding = Some(Resolved::Known(Known::new(binding, provenance())));
    let graph = graph(vec![bound]).expect("a bound node validates");
    let Some(Resolved::Known(found)) = &graph.node(&key("hull")).expect("hull").scene_binding
    else {
        panic!("the binding survives");
    };
    assert_eq!(found.value.as_content_id().kind(), ContentKind::SceneNode);

    // A non-scene-node id cannot be wrapped at all.
    let wrong_kind =
        ContentId::from_source(ContentKind::Mesh, "synthetic.devastator.hull").expect("valid");
    assert!(SceneNodeId::from_content_id(wrong_kind).is_err());
}

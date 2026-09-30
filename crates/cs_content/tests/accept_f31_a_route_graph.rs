//! Acceptance scenarios F31-A for the declared route graph: the
//! provenance-carrying content record.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-A`. Task test prefix: `accept_f31_a_`.
//!
//! These tests drive the production public API of `cs_content::routes` from
//! outside the crate: [`RouteDefinition::try_new`] and the declared synthetic
//! fixture. Refusing a duplicate id, a sequence shortcut or a non-finite
//! position, and carrying an unknown as an explicit unknown, are the
//! behaviors that fail if the validating constructor or the fixture is
//! removed.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_content::routes::{
    ReferenceFrame, RouteDefinition, RouteDraft, RouteEdge, RouteError, RouteNode, RouteNodeId,
    RouteTermination, TriggerShape, TriggerVolume, TriggerVolumeId, declared_synthetic_arch_route,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f31a.integration"))
}

fn route_id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Route, key).expect("valid route id")
}

fn edge(from: &str, to: &str) -> RouteEdge {
    RouteEdge {
        from: RouteNodeId::try_new(from).expect("valid node id"),
        to: RouteNodeId::try_new(to).expect("valid node id"),
    }
}

fn draft(nodes: Vec<RouteNode>, edges: Vec<RouteEdge>) -> RouteDraft {
    RouteDraft {
        id: route_id("synthetic.integration"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::End,
        clearance_m: Resolved::Known(Known::new(2.0, designed())),
        nodes,
        edges,
        provenance: designed(),
    }
}

/// The declared fixture is a synthetic, world-anchored route with a mandatory
/// arch marker that carries a stable trigger volume, and its edges connect
/// only adjacent sequences.
#[test]
fn accept_f31_a_declared_arch_route_is_synthetic_with_a_stable_arch_trigger() {
    let route = declared_synthetic_arch_route();
    assert_eq!(route.id().as_str(), "route/synthetic.arch");
    assert_eq!(route.origin(), &Origin::SyntheticFixture);
    assert!(
        !route.origin().is_original(),
        "a synthetic fixture route can never be original data"
    );
    assert_eq!(route.frame(), &ReferenceFrame::World);
    assert_eq!(route.termination(), RouteTermination::End);
    assert_eq!(route.nodes().len(), 5);
    assert_eq!(route.edges().len(), 4);
    assert_eq!(route.mandatory_node_count(), 4);
    assert_eq!(route.clearance_m().clone().known(), Some(2.0));

    // The sequences are strictly increasing in authored order, so progress
    // derived from the ordered list cannot skip a marker.
    let sequences: Vec<u32> = route.nodes().iter().map(|node| node.sequence).collect();
    assert_eq!(sequences, vec![0, 1, 2, 3, 4]);

    let arch = route
        .nodes()
        .iter()
        .find(|node| node.id.as_str() == "arch")
        .expect("the arch node exists");
    let volume = arch
        .trigger
        .clone()
        .known()
        .expect("the arch trigger is resolved")
        .expect("the arch node has an authored trigger volume");
    assert_eq!(volume.id.as_str(), "synthetic.arch.opening");
    assert_eq!(volume.shape, TriggerShape::Sphere { radius_m: 5.0 });

    // A plain waypoint carries a known absence, not an unknown.
    let start = route
        .nodes()
        .iter()
        .find(|node| node.id.as_str() == "start")
        .expect("the start node exists");
    assert_eq!(start.trigger.clone().known(), Some(None));
}

/// A duplicate node id, a non-increasing sequence and an edge between
/// non-adjacent sequences are each refused by name by the public constructor.
#[test]
fn accept_f31_a_declared_route_refuses_duplicates_and_sequence_shortcuts() {
    let base = declared_synthetic_arch_route();

    let mut duplicate = base.nodes().to_vec();
    duplicate[1].id = duplicate[0].id.clone();
    assert_eq!(
        RouteDefinition::try_new(draft(duplicate, Vec::new())),
        Err(RouteError::DuplicateNodeId {
            id: "start".to_owned()
        })
    );

    let mut unordered = base.nodes().to_vec();
    unordered[1].sequence = 0;
    assert_eq!(
        RouteDefinition::try_new(draft(unordered, Vec::new())),
        Err(RouteError::SequenceNotIncreasing {
            index: 1,
            previous: 0,
            current: 0,
        })
    );

    let shortcut = vec![edge("start", "arch")];
    assert_eq!(
        RouteDefinition::try_new(draft(base.nodes().to_vec(), shortcut)),
        Err(RouteError::EdgeNotAdjacent {
            from: "start".to_owned(),
            to: "arch".to_owned(),
        })
    );
}

/// A non-finite known position is an authoring error, while an unknown
/// position is content that stays explicitly unknown — never a silent origin.
#[test]
fn accept_f31_a_declared_route_refuses_nonfinite_and_carries_unknowns() {
    let base = declared_synthetic_arch_route();

    let mut corrupt = base.nodes().to_vec();
    corrupt[2].position_m = Resolved::Known(Known::new([f64::NAN, 5.0, 0.0], designed()));
    assert_eq!(
        RouteDefinition::try_new(draft(corrupt, Vec::new())),
        Err(RouteError::NonFinitePosition {
            node: "arch".to_owned(),
            component: 0,
        })
    );

    let mut unknown = base.nodes().to_vec();
    unknown[2].position_m =
        Resolved::unknown(claim("f31a.unknown-position"), "route layout not measured")
            .expect("a reason is present");
    let route = RouteDefinition::try_new(draft(unknown, Vec::new()))
        .expect("an unknown position is content, not an authoring error");
    assert_eq!(route.nodes()[2].position_m.clone().known(), None);
    assert!(!route.nodes()[2].position_m.is_known());
}

/// A trigger volume id is stable and validated with the same grammar as a
/// node id, and a non-positive known shape is refused.
#[test]
fn accept_f31_a_trigger_volume_ids_are_stable_and_shapes_are_positive() {
    assert_eq!(
        TriggerVolumeId::try_new(""),
        Err(RouteError::EmptyTriggerId)
    );
    assert_eq!(
        TriggerVolumeId::try_new("a\nb"),
        Err(RouteError::TriggerIdControlCharacter('\n'))
    );
    let id = TriggerVolumeId::try_new("synthetic.arch.opening").expect("valid volume id");
    assert_eq!(id.as_str(), "synthetic.arch.opening");

    let mut base = declared_synthetic_arch_route().nodes().to_vec();
    base[2].trigger = Resolved::Known(Known::new(
        Some(TriggerVolume {
            id: id.clone(),
            shape: TriggerShape::Sphere { radius_m: 0.0 },
        }),
        designed(),
    ));
    assert_eq!(
        RouteDefinition::try_new(draft(base, Vec::new())),
        Err(RouteError::NonPositiveTrigger {
            node: "arch".to_owned()
        })
    );

    let mut duplicate = declared_synthetic_arch_route().nodes().to_vec();
    let volume = |id: &str| TriggerVolume {
        id: TriggerVolumeId::try_new(id).expect("valid volume id"),
        shape: TriggerShape::Sphere { radius_m: 1.0 },
    };
    duplicate[1].trigger =
        Resolved::Known(Known::new(Some(volume("synthetic.shared")), designed()));
    duplicate[2].trigger =
        Resolved::Known(Known::new(Some(volume("synthetic.shared")), designed()));
    assert_eq!(
        RouteDefinition::try_new(draft(duplicate, Vec::new())),
        Err(RouteError::DuplicateTriggerVolume {
            id: "synthetic.shared".to_owned()
        })
    );
}

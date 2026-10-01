//! Acceptance scenarios F31-C for the producer half: the declared route is
//! resolved to known navigation values, and an explicit unknown is refused by
//! name instead of being defaulted.
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Task test prefix: `accept_f31_c_`.
//!
//! These tests call the production [`RouteDefinition::resolve`] boundary from
//! outside the crate. Removing the arrival-radius field or making `resolve`
//! default an unknown makes one of them fail.
//!
//! Every value here is newly authored synthetic fixture data, never original
//! game data.

use cs_content::routes::{
    ReferenceFrame, RouteDefinition, RouteDraft, RouteError, RouteResolutionError,
    RouteTermination, declared_synthetic_arch_route,
};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::ClaimId;

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("valid claim id")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f31c.integration"))
}

fn draft(nodes: Vec<cs_content::routes::RouteNode>, clearance_m: Resolved<f64>) -> RouteDraft {
    RouteDraft {
        id: ContentId::from_source(ContentKind::Route, "synthetic.c").expect("valid route id"),
        origin: Origin::SyntheticFixture,
        frame: ReferenceFrame::World,
        termination: RouteTermination::End,
        clearance_m,
        nodes,
        edges: Vec::new(),
        provenance: designed(),
    }
}

/// The declared fixture resolves every navigation field to a known value, and
/// its arrival radii and sequences are the ones the runtime fixture uses.
#[test]
fn accept_f31_c_resolve_returns_known_navigation_fields() {
    let resolved = declared_synthetic_arch_route()
        .resolve()
        .expect("the declared fixture resolves");

    assert_eq!(resolved.id().as_str(), "route/synthetic.arch");
    assert_eq!(resolved.clearance_m().value, 2.0);
    let sequences: Vec<u32> = resolved.nodes().iter().map(|node| node.sequence).collect();
    assert_eq!(sequences, vec![0, 1, 2, 3, 4]);
    let radii: Vec<f64> = resolved
        .nodes()
        .iter()
        .map(|node| node.arrival_radius_m.value)
        .collect();
    assert_eq!(
        radii,
        vec![3.0, 5.0, 5.0, 5.0, 8.0],
        "arrival radii are the navigation volumes the runtime fixture uses"
    );
    let mandatory: Vec<bool> = resolved.nodes().iter().map(|node| node.mandatory).collect();
    assert_eq!(mandatory, vec![false, true, true, true, true]);
}

/// An unknown clearance, position or arrival radius is an explicit refusal
/// naming the field, never a silent zero.
#[test]
fn accept_f31_c_resolve_refuses_unknown_navigation_fields_by_name() {
    let base = declared_synthetic_arch_route();

    let unknown_clearance = RouteDefinition::try_new(draft(
        base.nodes().to_vec(),
        Resolved::unknown(claim("f31c.unknown-clearance"), "clearance not measured")
            .expect("a reason is present"),
    ))
    .expect("an unknown clearance is content, not an authoring error");
    assert_eq!(
        unknown_clearance.resolve(),
        Err(RouteResolutionError::UnknownClearance {
            claim_id: claim("f31c.unknown-clearance"),
            reason: "clearance not measured".to_owned(),
        })
    );

    let mut unknown_position = base.nodes().to_vec();
    unknown_position[1].position_m =
        Resolved::unknown(claim("f31c.unknown-position"), "route layout not measured")
            .expect("a reason is present");
    let route = RouteDefinition::try_new(draft(
        unknown_position,
        Resolved::Known(Known::new(2.0, designed())),
    ))
    .expect("an unknown position is content");
    assert_eq!(
        route.resolve(),
        Err(RouteResolutionError::UnknownPosition {
            node: "funnel".to_owned(),
            claim_id: claim("f31c.unknown-position"),
            reason: "route layout not measured".to_owned(),
        })
    );

    let mut unknown_radius = base.nodes().to_vec();
    unknown_radius[2].arrival_radius_m =
        Resolved::unknown(claim("f31c.unknown-radius"), "arrival radius not measured")
            .expect("a reason is present");
    let route = RouteDefinition::try_new(draft(
        unknown_radius,
        Resolved::Known(Known::new(2.0, designed())),
    ))
    .expect("an unknown arrival radius is content");
    assert_eq!(
        route.resolve(),
        Err(RouteResolutionError::UnknownArrivalRadius {
            node: "arch".to_owned(),
            claim_id: claim("f31c.unknown-radius"),
            reason: "arrival radius not measured".to_owned(),
        })
    );
}

/// A known arrival radius that is non-finite or non-positive is an authoring
/// error refused at the content boundary.
#[test]
fn accept_f31_c_arrival_radius_must_be_finite_and_positive() {
    let base = declared_synthetic_arch_route();

    let mut non_finite = base.nodes().to_vec();
    non_finite[0].arrival_radius_m = Resolved::Known(Known::new(f64::NAN, designed()));
    assert_eq!(
        RouteDefinition::try_new(draft(
            non_finite,
            Resolved::Known(Known::new(2.0, designed())),
        )),
        Err(RouteError::NonFiniteArrivalRadius {
            node: "start".to_owned(),
        })
    );

    let mut zero = base.nodes().to_vec();
    zero[0].arrival_radius_m = Resolved::Known(Known::new(0.0, designed()));
    assert_eq!(
        RouteDefinition::try_new(draft(zero, Resolved::Known(Known::new(2.0, designed())))),
        Err(RouteError::NonPositiveArrivalRadius {
            node: "start".to_owned(),
            value: 0.0,
        })
    );
}

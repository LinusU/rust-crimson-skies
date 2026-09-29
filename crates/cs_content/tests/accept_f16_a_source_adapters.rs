//! Acceptance scenario F16-A (AC01): round-trip a position, a normal and a
//! quaternion through **every** declared source adapter within the declared
//! tolerances, plus the rest of the deliverable's quantity list (distances,
//! angles, winding), the absolute forward mappings, and the failure cases.
//!
//! These tests exercise production code only: `cs_content::coordinates`
//! over the canonical types in `cs_types::space`. Removing or neutering an
//! adapter's axis map, orientation sign, rotation sense, scale, angle unit
//! or winding rule makes them fail. The forward-mapping test is deliberately
//! hand-computed, because a wrong-but-consistent pair of conversions would
//! still round-trip.

use cs_content::coordinates::{
    ANGLE_ROUND_TRIP_TOLERANCE_RAD, AngleUnit, Axis, CoordinateSource,
    DIRECTION_ROUND_TRIP_TOLERANCE, DISTANCE_ROUND_TRIP_TOLERANCE_M,
    POSITION_ROUND_TRIP_TOLERANCE_M, ROTATION_ROUND_TRIP_TOLERANCE, RotationSense, SourceAdapter,
    SourceAxis, SourceConvention, SourceError,
};
use cs_types::content::{Origin, Provenance};
use cs_types::evidence::{ClaimId, ClaimStatus};
use cs_types::space::{Meters, Quaternion, Radians, SpaceError, UnitVec3, Winding, WorldPosition};

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

/// AC01's minimum scenario: a position, a normal and a quaternion round-trip
/// through every declared adapter inside its declared tolerance, and every
/// declared adapter is actually exercised.
#[test]
fn accept_f16_a_round_trip_position_normal_quaternion_through_every_adapter() {
    let adapters = SourceAdapter::declared();
    assert!(
        adapters.len() >= 3,
        "the declared registry must hold more than an identity adapter"
    );

    let canonical_position =
        WorldPosition::try_new([1234.5, -67.25, 8901.125]).expect("finite fixture position");
    let length = (1.0_f64 + 4.0 + 9.0).sqrt();
    let normal_parts = [1.0 / length, 2.0 / length, 3.0 / length];
    let normal = UnitVec3::try_new(normal_parts).expect("fixture normal is unit length");
    let rotation = Quaternion::from_axis_angle(normal, Radians(1.234_5))
        .expect("fixture rotation is unit length");

    let mut exercised = 0;
    for adapter in &adapters {
        let label = adapter.source().label();

        let source_position = adapter.position_from_canonical(canonical_position);
        let back = adapter
            .position_to_canonical(source_position)
            .expect("finite round trip");
        for (axis, (actual, wanted)) in back
            .to_array()
            .into_iter()
            .zip(canonical_position.to_array())
            .enumerate()
        {
            assert!(
                (actual - wanted).abs() <= POSITION_ROUND_TRIP_TOLERANCE_M,
                "{label}: position round trip exceeds the declared tolerance \
                 {POSITION_ROUND_TRIP_TOLERANCE_M} m on axis {axis}: {actual} != {wanted}"
            );
        }

        let source_normal = adapter.normal_from_canonical(normal);
        let back = adapter
            .normal_to_canonical(source_normal)
            .expect("unit normal round trip");
        for (axis, (actual, wanted)) in back
            .to_array()
            .into_iter()
            .zip(normal.to_array())
            .enumerate()
        {
            assert!(
                (actual - wanted).abs() <= DIRECTION_ROUND_TRIP_TOLERANCE,
                "{label}: normal round trip exceeds the declared tolerance \
                 {DIRECTION_ROUND_TRIP_TOLERANCE} on axis {axis}: {actual} != {wanted}"
            );
        }

        let source_rotation = adapter.rotation_from_canonical(rotation);
        let back = adapter
            .rotation_to_canonical(source_rotation)
            .expect("unit rotation round trip");
        for (index, (actual, wanted)) in back.into_iter().zip(rotation.components()).enumerate() {
            assert!(
                (actual - wanted).abs() <= ROTATION_ROUND_TRIP_TOLERANCE,
                "{label}: quaternion round trip exceeds the declared tolerance \
                 {ROTATION_ROUND_TRIP_TOLERANCE} at component {index}: {actual} != {wanted}"
            );
        }

        exercised += 1;
    }
    assert_eq!(
        exercised,
        adapters.len(),
        "every declared adapter must round-trip"
    );
}

/// The rest of the deliverable's quantity list: distances, angles and
/// winding round-trip through every adapter inside their declared
/// tolerances, and the declared tolerances are usable bounds.
#[test]
fn accept_f16_a_round_trip_distance_angle_and_winding_through_every_adapter() {
    for tolerance in [
        POSITION_ROUND_TRIP_TOLERANCE_M,
        DISTANCE_ROUND_TRIP_TOLERANCE_M,
        DIRECTION_ROUND_TRIP_TOLERANCE,
        ROTATION_ROUND_TRIP_TOLERANCE,
        ANGLE_ROUND_TRIP_TOLERANCE_RAD,
    ] {
        assert!(
            tolerance.is_finite() && tolerance > 0.0 && tolerance < 1.0,
            "a declared tolerance must be a positive, finite, tight bound, got {tolerance}"
        );
    }

    for adapter in SourceAdapter::declared() {
        let label = adapter.source().label().to_owned();

        let distance = adapter
            .distance_to_canonical(adapter.distance_from_canonical(Meters(437.5)))
            .expect("finite distance");
        assert!(
            (distance.0 - 437.5).abs() <= DISTANCE_ROUND_TRIP_TOLERANCE_M,
            "{label}: distance round trip left the declared tolerance: {} != 437.5",
            distance.0
        );

        let quadrant = std::f64::consts::FRAC_PI_4;
        let angle = adapter
            .angle_to_canonical(adapter.angle_from_canonical(Radians(quadrant)))
            .expect("finite angle");
        assert!(
            (angle.0 - quadrant).abs() <= ANGLE_ROUND_TRIP_TOLERANCE_RAD,
            "{label}: angle round trip left the declared tolerance: {} != {quadrant}",
            angle.0
        );

        for winding in [Winding::CounterClockwise, Winding::Clockwise] {
            let back = adapter.winding_from_canonical(adapter.winding_to_canonical(winding));
            assert_eq!(
                back, winding,
                "{label}: winding labels must round-trip through {winding:?}"
            );
        }
    }
}

/// Absolute, hand-computed mappings for each declared source, so a pair of
/// conversions that is consistently wrong cannot pass by round-tripping.
///
/// Expected values are derived from each declaration by hand (see the table
/// in `docs/findings/2026-09-29-f16-a-units-typed-time-and-coordinate-adapters.md`):
/// canonical X/Y/Z are read from the declared source axes, and a source
/// rotation is checked against its physical meaning.
#[test]
fn accept_f16_a_forward_mapping_matches_the_declared_convention() {
    let adapters = SourceAdapter::declared();
    let by_label = |label: &str| {
        adapters
            .iter()
            .find(|adapter| adapter.source().label() == label)
            .unwrap_or_else(|| panic!("declared source {label} must exist"))
            .clone()
    };

    let close = |actual: [f64; 3], wanted: [f64; 3], label: &str, what: &str| {
        for (index, (a, w)) in actual.into_iter().zip(wanted).enumerate() {
            assert!(
                (a - w).abs() <= POSITION_ROUND_TRIP_TOLERANCE_M,
                "{label}: {what} component {index} is {a}, expected {w}"
            );
        }
    };
    let close4 = |actual: [f64; 4], wanted: [f64; 4], label: &str, what: &str| {
        for (index, (a, w)) in actual.into_iter().zip(wanted).enumerate() {
            assert!(
                (a - w).abs() <= ROTATION_ROUND_TRIP_TOLERANCE,
                "{label}: {what} component {index} is {a}, expected {w}"
            );
        }
    };

    // The canonical convention maps to itself.
    let canonical = by_label("canonical");
    close(
        canonical
            .position_to_canonical([1.0, 2.0, 3.0])
            .expect("finite")
            .to_array(),
        [1.0, 2.0, 3.0],
        "canonical",
        "position",
    );
    assert!(
        (canonical.angle_to_canonical(1.5).expect("finite").0 - 1.5).abs()
            <= ANGLE_ROUND_TRIP_TOLERANCE_RAD,
        "canonical: radians are already radians"
    );
    assert!(
        (canonical.distance_to_canonical(7.5).expect("finite").0 - 7.5).abs()
            <= DISTANCE_ROUND_TRIP_TOLERANCE_M,
        "canonical: meters are already meters"
    );
    assert!(
        !canonical.reverses_vertex_order(),
        "canonical: identity map, no reversal"
    );
    assert!(
        canonical.source().convention().is_orientation_preserving(),
        "canonical: identity preserves orientation"
    );

    // Right-handed Z-up, Y forward, degrees: canonical x = source x,
    // canonical y = source z, canonical z = -source y.
    let z_up = by_label("fixture.z-up-right-handed-degrees");
    close(
        z_up.position_to_canonical([1.0, 2.0, 3.0])
            .expect("finite")
            .to_array(),
        [1.0, 3.0, -2.0],
        "fixture.z-up-right-handed-degrees",
        "position",
    );
    let inverse =
        z_up.position_from_canonical(WorldPosition::try_new([1.0, 3.0, -2.0]).expect("finite"));
    close(
        [inverse[0], inverse[1], inverse[2]],
        [1.0, 2.0, 3.0],
        "fixture.z-up-right-handed-degrees",
        "inverse position",
    );
    let half = std::f64::consts::FRAC_1_SQRT_2;
    assert!(
        (z_up.angle_to_canonical(180.0).expect("finite").0 - std::f64::consts::PI).abs()
            <= ANGLE_ROUND_TRIP_TOLERANCE_RAD,
        "fixture.z-up-right-handed-degrees: 180° must become π rad"
    );
    // A right-hand-rule +90° about source Z (the up axis) is a
    // right-hand-rule +90° about canonical Y.
    close4(
        z_up.rotation_to_canonical([0.0, 0.0, half, half])
            .expect("unit rotation"),
        [0.0, half, 0.0, half],
        "fixture.z-up-right-handed-degrees",
        "+90° about source Z rotation",
    );
    assert!(
        z_up.source().convention().is_orientation_preserving(),
        "fixture.z-up-right-handed-degrees: negating a single axis while swapping two stays proper"
    );
    assert!(
        z_up.reverses_vertex_order(),
        "fixture.z-up-right-handed-degrees: Y feeding -Z flips the apparent winding, so vertex order must be reversed"
    );
    assert_eq!(
        z_up.winding_to_canonical(Winding::CounterClockwise),
        Winding::CounterClockwise,
        "fixture.z-up-right-handed-degrees: CCW front faces stay CCW in the label"
    );

    // Left-handed Z-up (X forward, Y right, Z up), centimeters, degrees,
    // clockwise front faces, left-hand-rule rotations:
    // canonical x = source y, canonical y = source z, canonical z = -source x.
    let left = by_label("fixture.left-handed-z-up-centimeters-degrees");
    close(
        left.position_to_canonical([100.0, 200.0, 50.0])
            .expect("finite")
            .to_array(),
        [2.0, 0.5, -1.0],
        "fixture.left-handed-z-up-centimeters-degrees",
        "position",
    );
    assert!(
        (left.distance_to_canonical(100.0).expect("finite").0 - 1.0).abs()
            <= DISTANCE_ROUND_TRIP_TOLERANCE_M,
        "fixture.left-handed-z-up-centimeters-degrees: 100 cm is 1 m"
    );
    assert!(
        (left.angle_to_canonical(90.0).expect("finite").0 - std::f64::consts::FRAC_PI_2).abs()
            <= ANGLE_ROUND_TRIP_TOLERANCE_RAD,
        "fixture.left-handed-z-up-centimeters-degrees: 90° is π/2 rad"
    );
    // A left-hand-rule +90° about source Z (up) is a physical +90° about
    // canonical +Y, i.e. right-hand-rule after conversion.
    close4(
        left.rotation_to_canonical([0.0, 0.0, half, half])
            .expect("unit rotation"),
        [0.0, half, 0.0, half],
        "fixture.left-handed-z-up-centimeters-degrees",
        "left-hand-rule +90° about source Z rotation",
    );
    assert!(
        !left.source().convention().is_orientation_preserving(),
        "fixture.left-handed-z-up-centimeters-degrees: negating exactly one canonical axis mirrors"
    );
    assert!(
        left.reverses_vertex_order(),
        "fixture.left-handed-z-up-centimeters-degrees: an improper map plus a negative depth sign reverses vertices"
    );
    assert_eq!(
        left.winding_to_canonical(Winding::CounterClockwise),
        Winding::Clockwise,
        "fixture.left-handed-z-up-centimeters-degrees: CW front faces must be labelled CCW in canonical space"
    );
    assert_eq!(
        left.winding_to_canonical(Winding::Clockwise),
        Winding::CounterClockwise,
        "the source's front faces (CW) land on the canonical front (CCW)"
    );
}

/// Geometric check of the winding rule: map a real triangle through each
/// adapter, apply the declared vertex-order reversal, and confirm the
/// resulting canonical winding equals the declared label — for both source
/// windings. This uses cross products rather than the implementation's own
/// algebra, so the two answers must agree.
#[test]
fn accept_f16_a_winding_labels_and_vertex_order_agree_with_the_geometry() {
    for adapter in SourceAdapter::declared() {
        let label = adapter.source().label().to_owned();
        let convention = adapter.source().convention();
        let view = convention.winding_reference().index();
        let others: Vec<usize> = (0..3).filter(|axis| *axis != view).collect();

        for swap in [false, true] {
            // A triangle in the plane perpendicular to the view axis, so its
            // winding is well defined along that axis.
            let mut vertex_a = [0.25, 0.25, 0.25];
            let mut vertex_b = [0.25, 0.25, 0.25];
            vertex_a[others[0]] += 1.0;
            vertex_b[others[1]] += 1.0;
            let vertex_c = [0.25, 0.25, 0.25];
            // [start, step-one, step-two], optionally swapped to flip the
            // source winding.
            let mut source_vertices = [vertex_a, vertex_b, vertex_c];
            if swap {
                source_vertices.swap(1, 2);
            }

            let source_winding = winding_sign(source_vertices, view);
            let mut canonical: Vec<[f64; 3]> = source_vertices
                .iter()
                .map(|vertex| {
                    adapter
                        .position_to_canonical(*vertex)
                        .expect("finite vertex")
                        .to_array()
                })
                .collect();
            if adapter.reverses_vertex_order() {
                canonical.reverse();
            }
            let canonical_winding = winding_sign(canonical.try_into().expect("three vertices"), 2);
            assert_eq!(
                canonical_winding,
                adapter.winding_to_canonical(source_winding),
                "{label}: the mapped triangle's geometry must match the declared label \
                 (source {source_winding:?}, reversal {})",
                adapter.reverses_vertex_order()
            );
        }
    }

    // Front faces of every declared source land on canonical front faces.
    for adapter in SourceAdapter::declared() {
        let front = adapter.source().convention().front_face();
        assert_eq!(
            adapter.winding_to_canonical(front),
            Winding::CANONICAL_FRONT,
            "{}: front faces must survive conversion as canonical front faces",
            adapter.source().label()
        );
    }
}

/// Winding sign of a triangle along the chosen axis: CCW when the cross
/// product points along `+axis`.
fn winding_sign(vertices: [[f64; 3]; 3], axis: usize) -> Winding {
    let ab = [
        vertices[1][0] - vertices[0][0],
        vertices[1][1] - vertices[0][1],
        vertices[1][2] - vertices[0][2],
    ];
    let ac = [
        vertices[2][0] - vertices[0][0],
        vertices[2][1] - vertices[0][1],
        vertices[2][2] - vertices[0][2],
    ];
    let cross = [
        ab[1] * ac[2] - ab[2] * ac[1],
        ab[2] * ac[0] - ab[0] * ac[2],
        ab[0] * ac[1] - ab[1] * ac[0],
    ];
    let component = cross[axis];
    assert!(
        component.abs() > 1e-12,
        "test triangle must not be degenerate"
    );
    if component > 0.0 {
        Winding::CounterClockwise
    } else {
        Winding::Clockwise
    }
}

/// Boundary failure cases: every adapter refuses non-finite input by field
/// name, and refuses non-unit normals and rotations with their measured
/// length instead of silently renormalizing them.
#[test]
fn accept_f16_a_nonfinite_and_nonunit_inputs_are_refused_at_the_adapter_boundary() {
    for adapter in SourceAdapter::declared() {
        let label = adapter.source().label();

        assert_eq!(
            adapter.position_to_canonical([f64::NAN, 0.0, 0.0]),
            Err(SpaceError::NonFinite {
                field: "position[0]"
            }),
            "{label}: non-finite positions must be refused by name"
        );
        assert_eq!(
            adapter.direction_to_canonical([0.0, 0.0, f64::INFINITY]),
            Err(SpaceError::NonFinite {
                field: "direction[2]"
            }),
            "{label}: non-finite directions must be refused by name"
        );
        assert_eq!(
            adapter.normal_to_canonical([1.0, 1.0, 0.0]),
            Err(SpaceError::NotUnit {
                length: std::f64::consts::SQRT_2
            }),
            "{label}: a non-unit normal must be reported with its length"
        );
        assert_eq!(
            adapter.rotation_to_canonical([1.0, 1.0, 1.0, 1.0]),
            Err(SpaceError::NotUnit { length: 2.0 }),
            "{label}: a denormalized rotation must be reported with its length"
        );
        assert!(
            matches!(
                adapter.distance_to_canonical(f64::NAN),
                Err(SpaceError::NonFinite { field: "distance" })
            ),
            "{label}: non-finite distances must be refused"
        );
        assert!(
            matches!(
                adapter.angle_to_canonical(f64::INFINITY),
                Err(SpaceError::NonFinite { field: "angle" })
            ),
            "{label}: non-finite angles must be refused"
        );
    }
}

/// Registry and provenance: the declared sources are complete, uniquely
/// labelled, carry F14-A provenance, and — critically — none of them claims
/// to be an original measurement, because F16-D has not measured one yet.
#[test]
fn accept_f16_a_declared_sources_are_registered_provenanced_and_never_claim_original() {
    let adapters = SourceAdapter::declared();
    assert!(adapters.len() >= 3, "at least three sources are declared");

    let mut labels: Vec<&str> = adapters
        .iter()
        .map(|adapter| adapter.source().label())
        .collect();
    labels.sort_unstable();
    let before = labels.len();
    labels.dedup();
    assert_eq!(labels.len(), before, "source labels must be unique");

    let mut origins = adapters
        .iter()
        .map(|adapter| adapter.source().origin().clone())
        .collect::<Vec<Origin>>();
    origins.sort_by_key(|origin| origin.label());
    origins.dedup();
    assert!(
        origins.contains(&Origin::Designed) && origins.contains(&Origin::SyntheticFixture),
        "the registry mixes the identity design with synthetic fixtures"
    );

    for adapter in &adapters {
        let source = adapter.source();
        assert!(
            !source.origin().is_original(),
            "{}: no source may claim original provenance before F16-D measures it",
            source.label()
        );
        assert_eq!(
            source.provenance().class,
            ClaimStatus::Designed,
            "{}: every declared convention is authored design",
            source.label()
        );
        assert!(
            !source.provenance().claim_id.as_str().is_empty(),
            "{}: every declaration backs a claim id",
            source.label()
        );
    }

    // The declarations themselves validate: rebuilding them from scratch is
    // what the module's own constructors promise.
    let rebuilt = SourceConvention::new(
        [
            SourceAxis::positive(Axis::X),
            SourceAxis::positive(Axis::Y),
            SourceAxis::negative(Axis::Z),
        ],
        Axis::Z,
        1.0,
        AngleUnit::Radians,
        RotationSense::RightHandRule,
        Winding::CounterClockwise,
    );
    assert!(rebuilt.is_ok(), "a valid declaration must rebuild");
    let mismatched = SourceConvention::new(
        [
            SourceAxis::positive(Axis::X),
            SourceAxis::positive(Axis::Y),
            SourceAxis::negative(Axis::Z),
        ],
        Axis::Y,
        1.0,
        AngleUnit::Radians,
        RotationSense::RightHandRule,
        Winding::CounterClockwise,
    );
    assert!(
        matches!(
            mismatched,
            Err(SourceError::WindingReferenceAxisMismatch { .. })
        ),
        "a winding reference that does not feed canonical Z must be refused"
    );

    let source = CoordinateSource::new(
        "test.source",
        rebuilt.expect("valid"),
        Origin::SyntheticFixture,
        Provenance::designed(claim("f16a.test.source")),
    )
    .expect("valid source");
    let adapter = SourceAdapter::new(source);
    assert_eq!(adapter.source().label(), "test.source");
}

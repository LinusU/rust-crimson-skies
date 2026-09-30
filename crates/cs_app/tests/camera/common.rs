//! Shared helpers for the F21-A camera acceptance tests.
//!
//! Everything here is authored test scaffolding, never production logic:
//! the `known`/`claim` helpers build designed provenance exactly the way
//! the fixtures do, `origin_pose` is the identity camera the framing
//! scenario uses, and `assert_close` is the tolerance assertion shared by
//! the projection tests.

use cs_app::camera::CameraPose;
use cs_types::content::{Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;
use cs_types::space::{Quaternion, WorldPosition};

/// The claim id every designed test value is recorded under.
pub fn claim() -> ClaimId {
    ClaimId::new("f21a.camera-test").expect("valid claim id")
}

/// Wraps a test value with designed provenance.
pub fn known<T>(value: T) -> Resolved<T> {
    Resolved::Known(Known::new(value, Provenance::designed(claim())))
}

/// A camera at the world origin, looking along canonical forward (`-Z`)
/// with `+Y` up and `+X` right — the canonical pose of F16.
pub fn origin_pose() -> CameraPose {
    CameraPose::new(
        WorldPosition::try_new([0.0, 0.0, 0.0]).expect("the origin is finite"),
        Quaternion::IDENTITY,
    )
}

/// Asserts two `f64` values agree within `tolerance`.
pub fn assert_close(actual: f64, expected: f64, tolerance: f64, what: &str) {
    assert!(
        (actual - expected).abs() <= tolerance,
        "{what}: expected {expected}, got {actual} (tolerance {tolerance})"
    );
}

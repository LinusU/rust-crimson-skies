//! Canonical orientation math for the camera rigs (F21-B).
//!
//! Spec: `specs/F21-cameras-cockpit-views-and-spyglass.md`, stage
//! `### F21-B`. Shared contract: `docs/contracts/UI-NETWORK.md`.
//!
//! [`cs_types::space::Quaternion`] validates a rotation and can rotate a
//! **unit** vector, which is all a camera basis needs. Three things are
//! missing for a rig and are here, each a named function so a rig never
//! inlines its own matrix maths:
//!
//! * [`compose`] — apply one rotation after another. A rig needs it twice per
//!   frame: the aircraft's rotation with the viewpoint's head rotation, and
//!   the frame's orientation with the look offset.
//! * [`rotate_vector`] — rotate a **non-unit** vector, which is what a
//!   [`BodyOffset`] is. `Quaternion::rotate` cannot take one, and quietly
//!   normalizing a metre-valued offset would move the camera.
//! * [`yaw_pitch`] and [`look_rotation`] — the two orientations a rig builds:
//!   a head/look turn, and "look at this world point".
//!
//! Everything here is canonical-space arithmetic in F16's frame (`+X` right,
//! `+Y` up, `-Z` forward, right-handed). Every function that can produce a
//! degenerate result reports [`SpaceError`] instead of returning a rotation
//! that is not one: no camera may be handed a NaN axis or a zero-length
//! "up".
//!
//! Nothing here is original game behavior. The original camera's head-turn
//! axis, its look-at convention and whether it lags at all are unmeasured
//! (F21-D); these are the project design the rigs are built on, recorded in
//! `docs/findings/2026-10-03-f21-b-camera-rigs.md`.

use cs_types::space::{Quaternion, Radians, SpaceError, UnitVec3, WorldPosition};

/// Applies `inner` first and `outer` second: `compose(outer, inner)`.
///
/// The naming is deliberate and load-bearing. `compose(q, IDENTITY) == q`,
/// `compose(IDENTITY, q) == q`, and a rig that wants "the aircraft's attitude
/// with the pilot's head turned 10° up" writes
/// `compose(head, aircraft_rotation)`.
///
/// # Errors
///
/// [`SpaceError`] when the product is not a usable unit rotation, which a
/// validated pair of factors cannot produce.
pub fn compose(outer: Quaternion, inner: Quaternion) -> Result<Quaternion, SpaceError> {
    let [ax, ay, az, aw] = outer.components();
    let [bx, by, bz, bw] = inner.components();
    Quaternion::try_new([
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ])
}

/// Rotates a general vector — one that need not be a unit length, such as a
/// metre-valued body offset.
///
/// # Errors
///
/// [`SpaceError::NonFinite`] when a rotated component is NaN or infinite,
/// which is how an offset that overflowed f64 is reported instead of being
/// handed on as a position.
pub fn rotate_vector(rotation: Quaternion, value: [f64; 3]) -> Result<[f64; 3], SpaceError> {
    let [qx, qy, qz, qw] = rotation.components();
    let [vx, vy, vz] = value;
    let cross = [qy * vz - qz * vy, qz * vx - qx * vz, qx * vy - qy * vx];
    let twice_w_cross = [
        2.0 * qw * cross[0],
        2.0 * qw * cross[1],
        2.0 * qw * cross[2],
    ];
    let q_cross = [
        qy * cross[2] - qz * cross[1],
        qz * cross[0] - qx * cross[2],
        qx * cross[1] - qy * cross[0],
    ];
    let rotated = [
        vx + twice_w_cross[0] + 2.0 * q_cross[0],
        vy + twice_w_cross[1] + 2.0 * q_cross[1],
        vz + twice_w_cross[2] + 2.0 * q_cross[2],
    ];
    if rotated.iter().any(|component| !component.is_finite()) {
        return Err(SpaceError::NonFinite { field: "rotated" });
    }
    Ok(rotated)
}

/// A yaw about `+Y` composed with a pitch about `+X`: the rotation a head
/// turn or a free-look offset is.
///
/// Positive yaw turns the forward axis toward `-X` (the canonical left) and
/// positive pitch toward `+Y` (up), both by the right-hand rule. The pitch is
/// applied first, about the body's own right axis, and the yaw second, about
/// the canonical up: `compose(yaw, pitch)` composes them that way. That is the
/// order that never rolls — yaw applied first would tilt the right axis the
/// pitch then turns about.
///
/// # Errors
///
/// [`SpaceError`] when a composed factor is not a usable rotation, which two
/// validated angles cannot produce.
pub fn yaw_pitch(yaw: Radians, pitch: Radians) -> Result<Quaternion, SpaceError> {
    let about_up = Quaternion::from_axis_angle(UnitVec3::UP, yaw)?;
    let about_right = Quaternion::from_axis_angle(canonical_right(), pitch)?;
    compose(about_up, about_right)
}

/// The canonical right axis, `+X`.
///
/// `cs_types::space` names the canonical forward and up axes as constants and
/// leaves the right axis as the one component a caller states itself; this is
/// that statement, in one place.
fn canonical_right() -> UnitVec3 {
    UnitVec3::try_new([1.0, 0.0, 0.0]).expect("+X is a unit vector")
}

/// The rotation whose forward axis (`-Z`) points from `eye` to `target`,
/// keeping `+Y` as close to up as the direction allows.
///
/// `up_hint` is what the rig wants up: a camera that looks straight up or down
/// has no unique right axis, and the rig chooses by hinting. When the hint is
/// parallel to the direction there is no right axis at all and this reports
/// [`SpaceError::NotUnit`] instead of inventing a roll.
///
/// # Errors
///
/// [`SpaceError`] when the eye and the target coincide (a zero-length
/// direction), when `up_hint` is parallel to the direction, or when the
/// derived basis is not a rotation.
pub fn look_rotation(direction: UnitVec3, up_hint: UnitVec3) -> Result<Quaternion, SpaceError> {
    // Canonical camera axes: forward is `-Z`, so the basis' third column is
    // the *back* axis, `-direction`.
    let back = negate(direction);
    let right = cross(up_hint.to_array(), back.to_array())?;
    let up = cross(back.to_array(), right.to_array())?;
    basis_rotation(right, up, back)
}

/// The direction from `eye` to `target`, when it has one.
///
/// `None` means the two positions coincide: there is no direction between a
/// point and itself, so a rig cannot aim there and must not invent an angle.
///
/// # Errors
///
/// [`SpaceError`] when either endpoint is not a usable position.
pub fn direction_to(
    eye: WorldPosition,
    target: WorldPosition,
) -> Result<Option<UnitVec3>, SpaceError> {
    let [ex, ey, ez] = eye.to_array();
    let [tx, ty, tz] = target.to_array();
    let delta = [tx - ex, ty - ey, tz - ez];
    let length = (delta[0] * delta[0] + delta[1] * delta[1] + delta[2] * delta[2]).sqrt();
    if length == 0.0 {
        return Ok(None);
    }
    Ok(Some(UnitVec3::try_new([
        delta[0] / length,
        delta[1] / length,
        delta[2] / length,
    ])?))
}

fn negate(value: UnitVec3) -> UnitVec3 {
    let [x, y, z] = value.to_array();
    // `negate` is only called with a validated unit vector, so the negation
    // is a unit vector too.
    UnitVec3::try_new([-x, -y, -z]).expect("the negation of a unit vector is a unit vector")
}

fn cross(first: [f64; 3], second: [f64; 3]) -> Result<UnitVec3, SpaceError> {
    UnitVec3::try_new([
        first[1] * second[2] - first[2] * second[1],
        first[2] * second[0] - first[0] * second[2],
        first[0] * second[1] - first[1] * second[0],
    ])
}

/// The rotation whose columns are `(right, up, back)`.
///
/// The four-branch form is Shepperd's method: it picks whichever of the four
/// expressions has the largest divisor, so the result stays numerically usable
/// at 180° rotations where the naive `w`-based formula divides by a value
/// approaching zero.
///
/// # Errors
///
/// [`SpaceError`] when the three columns do not form a rotation.
fn basis_rotation(right: UnitVec3, up: UnitVec3, back: UnitVec3) -> Result<Quaternion, SpaceError> {
    // Rows of the basis matrix; column c is right/up/back.
    let [r0, r1, r2] = right.to_array();
    let [u0, u1, u2] = up.to_array();
    let [b0, b1, b2] = back.to_array();

    let trace = r0 + u1 + b2;
    let components = if trace > 0.0 {
        let s = (1.0 + trace).sqrt() * 2.0;
        [(u2 - b1) / s, (b0 - r2) / s, (r1 - u0) / s, 0.25 * s]
    } else if r0 > u1 && r0 > b2 {
        let s = (1.0 + r0 - u1 - b2).sqrt() * 2.0;
        [0.25 * s, (u0 + r1) / s, (b0 + r2) / s, (u2 - b1) / s]
    } else if u1 > b2 {
        let s = (1.0 + u1 - r0 - b2).sqrt() * 2.0;
        [(u0 + r1) / s, 0.25 * s, (b1 + u2) / s, (b0 - r2) / s]
    } else {
        let s = (1.0 + b2 - r0 - u1).sqrt() * 2.0;
        [(b0 + r2) / s, (b1 + u2) / s, 0.25 * s, (r1 - u0) / s]
    };
    Quaternion::try_new(components)
}

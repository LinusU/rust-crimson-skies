//! Presentation-side pose interpolation (F20-A).
//!
//! The evaluator ([`cs_sim::animated_object::AnimatedObject`]) commits node
//! poses at integer ticks. Between two committed ticks the render path may
//! draw a blended pose — F20 non-negotiable behavior 1 confines that blend
//! to presentation: [`interpolated_pose`] takes the pose at the last
//! committed tick and at the next one plus a fractional alpha in `[0, 1]`,
//! and returns only a pose. It cannot emit an event, fire a marker or
//! advance clip time — no marker path exists in this signature.

use cs_sim::animated_object::{AnimationError, PoseSample};

/// Blends two adjacent committed tick poses for one render frame.
///
/// `previous` is the node's evaluated pose at the last committed tick,
/// `next` at the next one, and `alpha` the frame's fractional position in
/// that tick interval. At `alpha == 0.0` or `1.0` the result is exactly the
/// committed endpoint, so presentation never disagrees with the simulation
/// at tick boundaries.
///
/// # Errors
///
/// [`AnimationError::AlphaOutOfRange`] when `alpha` is non-finite or outside
/// `[0, 1]` — an out-of-range blend is an extrapolation the caller must not
/// silently perform.
pub fn interpolated_pose(
    previous: &PoseSample,
    next: &PoseSample,
    alpha: f64,
) -> Result<PoseSample, AnimationError> {
    PoseSample::interpolate(previous, next, alpha)
}

//! Anchor sockets: the one pose renderer and pickup logic both read.

use cs_script::ir::ActorId;
use cs_types::Tick;

use super::math::{Quat, add, cross, sub};
use super::trajectory::{Pose, Trajectory};

/// A named attachment point fixed in an actor's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorSocket {
    pub actor: ActorId,
    pub socket: u16,
    /// Offset from the actor origin, in the actor's frame.
    pub offset_m: [f64; 3],
}

/// The anchor's world pose and velocity at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AnchorSample {
    pub tick: Tick,
    pub position_m: [f64; 3],
    pub orientation: Quat,
    /// Includes the rotational contribution `omega x r`.
    pub velocity_m_s: [f64; 3],
}

/// The only place an anchor's world pose is computed. `cs_app` presentation
/// and pickup eligibility both call this with the actor's sampled [`Pose`];
/// neither keeps its own copy of the transform.
#[must_use]
pub fn anchor_sample(tick: Tick, pose: &Pose, socket: &AnchorSocket) -> AnchorSample {
    let r = pose.orientation.rotate(socket.offset_m);
    AnchorSample {
        tick,
        position_m: add(pose.position_m, r),
        orientation: pose.orientation,
        velocity_m_s: add(pose.velocity_m_s, cross(pose.angular_velocity_rad_s, r)),
    }
}

/// Velocity of `a` relative to `b`.
#[must_use]
pub fn relative_velocity_m_s(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    sub(a, b)
}

/// The designed envelope a pickup must satisfy.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PickupEnvelope {
    pub max_distance_m: f64,
    pub max_relative_speed_m_s: f64,
}

/// Why a pickup is not eligible.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PickupRefusal {
    TooFar {
        distance_m: f64,
    },
    TooFast {
        relative_speed_m_s: f64,
    },
    /// The envelope or a sampled value was non-finite.
    NonFinite,
}

/// Evaluates the pickup against the anchor at `tick`, sampled from the
/// anchor actor's trajectory. Uses relative velocity, never ground speed.
///
/// # Errors
///
/// [`PickupRefusal`] when out of the envelope.
pub fn pickup_eligible(
    tick: Tick,
    anchor_trajectory: &Trajectory,
    socket: &AnchorSocket,
    taker_position_m: [f64; 3],
    taker_velocity_m_s: [f64; 3],
    envelope: PickupEnvelope,
) -> Result<AnchorSample, PickupRefusal> {
    let a = anchor_sample(tick, &anchor_trajectory.sample(tick), socket);
    let rel = relative_velocity_m_s(taker_velocity_m_s, a.velocity_m_s);
    let distance_m = super::math::norm(sub(taker_position_m, a.position_m));
    let relative_speed_m_s = super::math::norm(rel);
    if !(distance_m.is_finite()
        && relative_speed_m_s.is_finite()
        && envelope.max_distance_m.is_finite()
        && envelope.max_relative_speed_m_s.is_finite())
    {
        return Err(PickupRefusal::NonFinite);
    }
    if distance_m > envelope.max_distance_m {
        return Err(PickupRefusal::TooFar { distance_m });
    }
    if relative_speed_m_s > envelope.max_relative_speed_m_s {
        return Err(PickupRefusal::TooFast { relative_speed_m_s });
    }
    Ok(a)
}

//! Swept interaction eligibility (F36-A).
//!
//! Spec: `specs/F36-docking-passenger-pickups-boarding-and-plane-swaps.md`,
//! stage `### F36-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Non-negotiable behavior 1: "Eligibility uses swept position, orientation,
//! relative velocity and mission authorization. A single radius test is
//! insufficient for moving docking hooks." This module's single production
//! path, [`evaluate_eligibility`], samples the target anchor through the F34
//! [`anchor_sample`] (so renderer and eligibility read the same pose),
//! measures the closest approach of the initiator's motion *relative to the
//! anchor over a sweep interval* instead of the instantaneous distance, checks
//! relative speed, checks the approach direction against the anchor's declared
//! docking axis, and refuses when the target cannot be reached at all.
//!
//! Every envelope value is designed project content; the original docking
//! envelope is unrecovered.

use std::fmt;

use cs_types::Tick;

use crate::world_actors::anchor::{AnchorSample, AnchorSocket, anchor_sample};
use crate::world_actors::trajectory::Trajectory;

/// Why an [`EligibilityEnvelope`] was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EnvelopeError {
    /// A field was NaN or infinite.
    NonFinite {
        /// Which field: the name used in the record.
        field: &'static str,
    },
    /// The capture radius was not strictly positive.
    NonPositiveRadius {
        /// The rejected value.
        value: f64,
    },
    /// A speed bound was negative.
    NegativeSpeed {
        /// Which field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
    /// The approach angle was outside `[0, 180]`.
    AngleOutOfRange {
        /// The rejected value, in degrees.
        value: f64,
    },
    /// The approach axis was zero length, so no direction is declared.
    ZeroAxis,
}

impl fmt::Display for EnvelopeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositiveRadius { value } => {
                write!(f, "capture radius {value} must be greater than zero")
            }
            Self::NegativeSpeed { field, value } => {
                write!(f, "{field} {value} must not be negative")
            }
            Self::AngleOutOfRange { value } => {
                write!(f, "approach angle {value} must be within [0, 180] degrees")
            }
            Self::ZeroAxis => write!(f, "the approach axis must have a non-zero direction"),
        }
    }
}

impl std::error::Error for EnvelopeError {}

/// The designed eligibility envelope of one interaction.
///
/// `approach_axis_local` is the direction the initiator must be travelling
/// when it enters, expressed in the target anchor actor's local frame; it is
/// normalized at construction so a [0, 0, 0] axis cannot silently mean "no
/// direction check".
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EligibilityEnvelope {
    /// The largest closest-approach distance, in metres, that still counts.
    pub capture_radius_m: f64,
    /// The largest initiator-relative-to-target speed, in m/s, that still
    /// counts.
    pub max_relative_speed_m_s: f64,
    /// The smallest closing speed along the line of centres, in m/s.
    pub min_closing_speed_m_s: f64,
    /// The largest angle between the initiator's velocity and the approach
    /// axis, in degrees.
    pub max_approach_angle_deg: f64,
    /// The unit approach direction in the target anchor's local frame.
    pub approach_axis_local: [f64; 3],
}

impl EligibilityEnvelope {
    /// Validates and builds an envelope, normalizing the approach axis.
    ///
    /// # Errors
    ///
    /// [`EnvelopeError`] for a non-finite field, a non-positive radius, a
    /// negative speed bound, an out-of-range angle or a zero axis.
    pub fn try_new(
        capture_radius_m: f64,
        max_relative_speed_m_s: f64,
        min_closing_speed_m_s: f64,
        max_approach_angle_deg: f64,
        approach_axis_local: [f64; 3],
    ) -> Result<Self, EnvelopeError> {
        if !capture_radius_m.is_finite() {
            return Err(EnvelopeError::NonFinite {
                field: "capture_radius_m",
            });
        }
        if capture_radius_m <= 0.0 {
            return Err(EnvelopeError::NonPositiveRadius {
                value: capture_radius_m,
            });
        }
        if !max_relative_speed_m_s.is_finite() {
            return Err(EnvelopeError::NonFinite {
                field: "max_relative_speed_m_s",
            });
        }
        if max_relative_speed_m_s < 0.0 {
            return Err(EnvelopeError::NegativeSpeed {
                field: "max_relative_speed_m_s",
                value: max_relative_speed_m_s,
            });
        }
        if !min_closing_speed_m_s.is_finite() {
            return Err(EnvelopeError::NonFinite {
                field: "min_closing_speed_m_s",
            });
        }
        if min_closing_speed_m_s < 0.0 {
            return Err(EnvelopeError::NegativeSpeed {
                field: "min_closing_speed_m_s",
                value: min_closing_speed_m_s,
            });
        }
        if !max_approach_angle_deg.is_finite() {
            return Err(EnvelopeError::NonFinite {
                field: "max_approach_angle_deg",
            });
        }
        if !(0.0..=180.0).contains(&max_approach_angle_deg) {
            return Err(EnvelopeError::AngleOutOfRange {
                value: max_approach_angle_deg,
            });
        }
        if !approach_axis_local.iter().all(|v| v.is_finite()) {
            return Err(EnvelopeError::NonFinite {
                field: "approach_axis_local",
            });
        }
        let len = norm(approach_axis_local);
        if len <= 0.0 {
            return Err(EnvelopeError::ZeroAxis);
        }
        Ok(Self {
            capture_radius_m,
            max_relative_speed_m_s,
            min_closing_speed_m_s,
            max_approach_angle_deg,
            approach_axis_local: scale(approach_axis_local, 1.0 / len),
        })
    }
}

/// Why an initiator is not eligible to latch.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EligibilityRefusal {
    /// The closest approach over the sweep is outside the capture radius.
    TooFar {
        /// The closest-approach distance, in metres.
        closest_approach_m: f64,
    },
    /// The initiator passes the target too fast to latch.
    TooFast {
        /// The initiator-relative-to-target speed, in m/s.
        relative_speed_m_s: f64,
    },
    /// The initiator is travelling in the wrong direction to enter.
    WrongDirection {
        /// The angle, in degrees, between the initiator's velocity and the
        /// approach axis.
        angle_deg: f64,
    },
    /// The initiator is not closing on the target at all.
    NotClosing {
        /// The closing speed along the line of centres, in m/s.
        closing_speed_m_s: f64,
    },
    /// An input or envelope value was NaN or infinite.
    NonFinite,
}

impl fmt::Display for EligibilityRefusal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TooFar { closest_approach_m } => write!(
                f,
                "closest approach {closest_approach_m} m is outside the capture radius"
            ),
            Self::TooFast { relative_speed_m_s } => write!(
                f,
                "relative speed {relative_speed_m_s} m/s is too fast to latch"
            ),
            Self::WrongDirection { angle_deg } => write!(
                f,
                "approach direction is {angle_deg} degrees off the docking axis"
            ),
            Self::NotClosing { closing_speed_m_s } => write!(
                f,
                "closing speed {closing_speed_m_s} m/s does not reach the target"
            ),
            Self::NonFinite => write!(f, "an eligibility input was non-finite"),
        }
    }
}

impl std::error::Error for EligibilityRefusal {}

/// Evaluates one initiator against a target's anchor using a swept test.
///
/// The target anchor is sampled at `tick` through [`anchor_sample`], so the
/// pose is the one the renderer and collision both read. The initiator's
/// constant-velocity motion relative to the anchor is swept over
/// `sweep_seconds`, and the closest approach over that interval — not the
/// instantaneous distance — decides [`EligibilityRefusal::TooFar`], which is
/// what lets a fast pass that crosses a moving hook still count while a
/// radius-only test would miss it.
///
/// Checks run in this order: finiteness, closest approach, relative speed,
/// approach direction, closing speed.
///
/// # Errors
///
/// [`EligibilityRefusal`] naming the first condition the initiator fails.
pub fn evaluate_eligibility(
    tick: Tick,
    target_trajectory: &Trajectory,
    anchor: &AnchorSocket,
    initiator_position_m: [f64; 3],
    initiator_velocity_m_s: [f64; 3],
    sweep_seconds: f64,
    envelope: &EligibilityEnvelope,
) -> Result<AnchorSample, EligibilityRefusal> {
    let sample = anchor_sample(tick, &target_trajectory.sample(tick), anchor);
    if !initiator_position_m.iter().all(|v| v.is_finite())
        || !initiator_velocity_m_s.iter().all(|v| v.is_finite())
        || !sample.position_m.iter().all(|v| v.is_finite())
        || !sample.velocity_m_s.iter().all(|v| v.is_finite())
        || !sample.orientation.is_unit()
        || !sweep_seconds.is_finite()
        || sweep_seconds < 0.0
    {
        return Err(EligibilityRefusal::NonFinite);
    }

    let relative_velocity = sub(initiator_velocity_m_s, sample.velocity_m_s);
    let relative_speed_m_s = norm(relative_velocity);

    // Closest approach of the relative path `d0 + relative_velocity * t`.
    let d0 = sub(initiator_position_m, sample.position_m);
    let t = if relative_speed_m_s <= 1e-12 {
        0.0
    } else {
        let along = -dot(d0, relative_velocity) / (relative_speed_m_s * relative_speed_m_s);
        along.clamp(0.0, sweep_seconds)
    };
    let closest_approach_m = norm(add(d0, scale(relative_velocity, t)));
    if closest_approach_m > envelope.capture_radius_m {
        return Err(EligibilityRefusal::TooFar { closest_approach_m });
    }

    if relative_speed_m_s > envelope.max_relative_speed_m_s {
        return Err(EligibilityRefusal::TooFast { relative_speed_m_s });
    }

    // Direction: the initiator's travel against the anchor's declared axis.
    let initiator_speed = norm(initiator_velocity_m_s);
    if initiator_speed <= 1e-12 {
        return Err(EligibilityRefusal::NotClosing {
            closing_speed_m_s: 0.0,
        });
    }
    let axis_world = sample.orientation.rotate(envelope.approach_axis_local);
    let cosine = (dot(initiator_velocity_m_s, axis_world) / initiator_speed).clamp(-1.0, 1.0);
    let angle_deg = cosine.acos().to_degrees();
    if angle_deg > envelope.max_approach_angle_deg {
        return Err(EligibilityRefusal::WrongDirection { angle_deg });
    }

    // Closing: is the distance actually decreasing?
    let distance = norm(d0);
    let closing_speed_m_s = if distance <= 1e-12 {
        0.0
    } else {
        -dot(relative_velocity, scale(d0, 1.0 / distance))
    };
    if closing_speed_m_s < envelope.min_closing_speed_m_s {
        return Err(EligibilityRefusal::NotClosing { closing_speed_m_s });
    }

    Ok(sample)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}

fn scale(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

//! Instrument values and display-unit conversion (F46-A) and the HUD frame
//! projection over the session authorities (F46-B).
//!
//! Spec: `specs/F46-hud-instruments-mission-map-and-pause.md`, stages
//! `### F46-A` and `### F46-B`. Shared contract:
//! `docs/contracts/UI-NETWORK.md` and, for rebinding on an aircraft swap,
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! The HUD is a read-only projection of one aircraft's authoritative state.
//! [`Hud::project`] takes an [`AircraftSample`] (SI units, canonical axes)
//! and returns [`Instruments`]:
//!
//! * **Horizon and heading** from the attitude quaternion ([`attitude`]).
//!   Designed convention: pitch is the elevation of the nose, roll is positive
//!   with the right wing down, heading is measured from canonical forward
//!   (-Z) and increases toward +X. Heading is `None` with the nose vertical
//!   and roll is `None` when the wings are vertical to the horizon axis; no
//!   angle is invented there.
//! * **Speeds, altitude and the low-altitude warning** converted per
//!   `cs_content::hud::HudPolicy`; the simulation stays SI. The warning has
//!   hysteresis and resets when the HUD binds to another aircraft.
//! * **Binding**: a sample stamped with another session or actor than the
//!   bound one is refused ([`HudError::Stale`]), so a previous aircraft's
//!   values can never be shown after a swap.
//!
//! [`Hud::frame`] (F46-B) is the whole HUD: the instruments above plus the
//! gauge clusters and the target display, derived from the authorities the
//! session owns — the F27-C weapon session, the F28-C ordnance session, the
//! F29 damage resolver and the F30-C published consumer views. Every one of
//! them is read for the bound actor only, a foreign-generation authority is
//! refused ([`HudError::ForeignAuthority`]) rather than read as an empty
//! gauge, and an absent authority produces no rows rather than invented
//! ones. See `docs/findings/2026-10-07-f46-b-hud-gauges-and-target-display.md`.
//!
//! Everything is **designed** and synthetic. The original units, datum,
//! thresholds, dial layouts, gauge semantics and target appearance are
//! F46-D's to compare; see
//! `docs/findings/2026-10-01-f46-a-instrument-values.md`.

mod frame;

pub use frame::{DamageZone, HudFrame, HudSources, MountedGun, MountedOrdnance, WeaponGauge};

use std::fmt;

use cs_content::hud::{AltitudeDatum, HudPolicy, HudPolicyError, SpeedReference};
use cs_types::net::{ActorId, SessionId};
use cs_types::space::{Quaternion, Radians, SpaceError, UnitVec3};

/// Horizon and heading derived from an attitude.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Attitude {
    /// Nose elevation above the horizon, in `[-pi/2, pi/2]`.
    pub pitch: Radians,
    /// Bank, positive with the right wing down, in `(-pi, pi]`; `None` when the
    /// nose is vertical and bank has no meaning.
    pub roll: Option<Radians>,
    /// Compass heading in `[0, 2*pi)`; `None` when the nose is vertical.
    pub heading: Option<Radians>,
}

/// Below this horizontal length a direction has no usable bearing.
const DEGENERATE: f64 = 1e-9;

/// Derives pitch, roll and heading from `q`.
///
/// # Errors
///
/// [`SpaceError`] if rotating the body axes does not give a unit vector, which
/// a validated [`Quaternion`] cannot cause.
pub fn attitude(q: Quaternion) -> Result<Attitude, SpaceError> {
    let forward = q.rotate(UnitVec3::FORWARD)?;
    let up = q.rotate(UnitVec3::UP)?;
    let right = q.rotate(UnitVec3::try_new([1.0, 0.0, 0.0])?)?;
    let pitch = Radians(forward.y().clamp(-1.0, 1.0).asin());
    let heading = (forward.x().hypot(forward.z()) > DEGENERATE).then(|| {
        Radians(
            forward
                .x()
                .atan2(-forward.z())
                .rem_euclid(std::f64::consts::TAU),
        )
    });
    let roll = (right.y().abs() > DEGENERATE || up.y().abs() > DEGENERATE)
        .then(|| Radians((-right.y()).atan2(up.y())));
    Ok(Attitude {
        pitch,
        roll,
        heading,
    })
}

/// One aircraft's authoritative kinematic state, in SI and canonical axes.
///
/// The sample carries identity and kinematics only: the gauges an aircraft's
/// state implies are derived from the session authorities inside
/// [`Hud::frame`], never asserted by the caller — a sample that could name
/// its own ammunition would be a second truth beside `WeaponState`.
#[derive(Clone, Debug, PartialEq)]
pub struct AircraftSample {
    /// The session generation the sample belongs to.
    pub session: SessionId,
    /// The aircraft the sample describes.
    pub actor: ActorId,
    /// Body attitude.
    pub attitude: Quaternion,
    /// World velocity, metres per second.
    pub velocity_mps: [f64; 3],
    /// Wind velocity, metres per second.
    pub wind_mps: [f64; 3],
    /// Height above the world origin plane, metres.
    pub height_m: f64,
    /// Terrain height directly below, metres, when known.
    pub ground_height_m: Option<f64>,
}

/// The values every flight instrument shows for one sample.
#[derive(Clone, Debug, PartialEq)]
pub struct Instruments {
    /// The session generation the values belong to.
    pub session: SessionId,
    /// The aircraft the values describe.
    pub actor: ActorId,
    /// Horizon and heading.
    pub attitude: Attitude,
    /// Speed through the air, in the policy's speed unit.
    pub air_speed: f64,
    /// Speed over the ground, in the policy's speed unit.
    pub ground_speed: f64,
    /// The speed the main gauge reads: one of the two above, per policy.
    pub gauge_speed: f64,
    /// Altitude from the policy's datum, in the policy's altitude unit.
    pub altitude: f64,
    /// Whether the low-altitude warning is showing.
    pub low_altitude_warning: bool,
}

/// Why a sample could not be projected.
#[derive(Clone, Debug, PartialEq)]
pub enum HudError {
    /// The HUD is not bound to an aircraft.
    Unbound,
    /// The sample is for another session or aircraft than the bound one.
    Stale {
        /// What the HUD is bound to.
        bound: (SessionId, ActorId),
        /// What the sample carried.
        got: (SessionId, ActorId),
    },
    /// A sample field is NaN or infinite.
    NonFinite(&'static str),
    /// The policy measures from the terrain but no ground height was given.
    MissingGroundHeight,
    /// The bound actor belongs to another session than the one it binds in.
    ActorSessionMismatch,
    /// An authority offered to the frame belongs to another session
    /// generation — a stale weapon, ordnance or damage table is refused,
    /// never read as an empty gauge.
    ForeignAuthority {
        /// Which source carried the wrong generation.
        source: &'static str,
        /// The session generation the HUD is bound to.
        expected: SessionId,
        /// The generation the source holds.
        found: u64,
    },
    /// The attitude could not be rotated into body axes.
    Space(SpaceError),
    /// The display policy is invalid.
    Policy(HudPolicyError),
}

impl fmt::Display for HudError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unbound => f.write_str("the HUD is not bound to an aircraft"),
            Self::Stale { bound, got } => write!(
                f,
                "stale sample for {} / {}; the HUD is bound to {} / {}",
                got.0, got.1, bound.0, bound.1
            ),
            Self::NonFinite(field) => write!(f, "sample field {field} is not finite"),
            Self::MissingGroundHeight => {
                f.write_str("altitude is measured from the terrain but no ground height was given")
            }
            Self::ActorSessionMismatch => {
                f.write_str("the actor does not belong to the session it is bound in")
            }
            Self::ForeignAuthority {
                source,
                expected,
                found,
            } => write!(
                f,
                "the {source} authority belongs to session {found}, but the HUD is bound to {expected}"
            ),
            Self::Space(error) => write!(f, "attitude: {error}"),
            Self::Policy(error) => write!(f, "display policy: {error}"),
        }
    }
}

impl std::error::Error for HudError {}

/// The HUD's instrument projection and its binding.
#[derive(Clone, Debug)]
pub struct Hud {
    policy: HudPolicy,
    bound: Option<(SessionId, ActorId)>,
    low_altitude: bool,
}

impl Hud {
    /// An unbound HUD under `policy`.
    ///
    /// # Errors
    ///
    /// [`HudError::Policy`] when the policy does not validate.
    pub fn new(policy: HudPolicy) -> Result<Self, HudError> {
        policy.validate().map_err(HudError::Policy)?;
        Ok(Self {
            policy,
            bound: None,
            low_altitude: false,
        })
    }

    /// Binds to `actor` in `session`, dropping everything remembered about the
    /// previous aircraft (the low-altitude warning).
    ///
    /// # Errors
    ///
    /// [`HudError::ActorSessionMismatch`] when the actor is of another session.
    pub fn bind(&mut self, session: SessionId, actor: ActorId) -> Result<(), HudError> {
        if actor.session != session {
            return Err(HudError::ActorSessionMismatch);
        }
        self.bound = Some((session, actor));
        self.low_altitude = false;
        Ok(())
    }

    /// The aircraft the HUD reads, if any.
    #[must_use]
    pub fn bound(&self) -> Option<(SessionId, ActorId)> {
        self.bound
    }

    /// Projects `sample` into instrument values and advances the warning.
    ///
    /// # Errors
    ///
    /// [`HudError::Unbound`], [`HudError::Stale`], [`HudError::NonFinite`] or
    /// [`HudError::MissingGroundHeight`]; on an error the HUD state is
    /// unchanged.
    pub fn project(&mut self, sample: &AircraftSample) -> Result<Instruments, HudError> {
        let bound = self.bound.ok_or(HudError::Unbound)?;
        if (sample.session, sample.actor) != bound {
            return Err(HudError::Stale {
                bound,
                got: (sample.session, sample.actor),
            });
        }
        check_finite("velocity_mps", &sample.velocity_mps)?;
        check_finite("wind_mps", &sample.wind_mps)?;
        check_finite("height_m", &[sample.height_m])?;
        if let Some(ground) = sample.ground_height_m {
            check_finite("ground_height_m", &[ground])?;
        }

        let altitude_m = match self.policy.altitude_datum.value {
            AltitudeDatum::WorldOrigin => sample.height_m,
            AltitudeDatum::TerrainBelow => {
                sample.height_m
                    - sample
                        .ground_height_m
                        .ok_or(HudError::MissingGroundHeight)?
            }
        };
        let attitude = attitude(sample.attitude).map_err(HudError::Space)?;

        let air = std::array::from_fn::<f64, 3, _>(|i| sample.velocity_mps[i] - sample.wind_mps[i]);
        let air_mps = (air[0] * air[0] + air[1] * air[1] + air[2] * air[2]).sqrt();
        let ground_mps = sample.velocity_mps[0].hypot(sample.velocity_mps[2]);
        let unit = self.policy.speed_unit.value.per_meter_per_second();
        let (air_speed, ground_speed) = (air_mps * unit, ground_mps * unit);
        let gauge_speed = match self.policy.airspeed_reference.value {
            SpeedReference::AirRelative => air_speed,
            SpeedReference::Ground => ground_speed,
        };

        if altitude_m < self.policy.low_altitude_warn_m.value {
            self.low_altitude = true;
        } else if altitude_m > self.policy.low_altitude_clear_m.value {
            self.low_altitude = false;
        }

        Ok(Instruments {
            session: sample.session,
            actor: sample.actor,
            attitude,
            air_speed,
            ground_speed,
            gauge_speed,
            altitude: altitude_m * self.policy.altitude_unit.value.per_meter(),
            low_altitude_warning: self.low_altitude,
        })
    }
}

fn check_finite(field: &'static str, values: &[f64]) -> Result<(), HudError> {
    if values.iter().all(|v| v.is_finite()) {
        Ok(())
    } else {
        Err(HudError::NonFinite(field))
    }
}

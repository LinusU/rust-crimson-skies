//! The authoritative wind field's effect on flight and projectiles (F19-B).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-B`, non-negotiable behavior 2 and acceptance case AC02. Shared
//! contract: `docs/contracts/IDENTITY-CONTENT.md`; the air-relative sign
//! convention is `docs/contracts/FLIGHT-PHYSICS.md`, "Coordinate convention":
//! `v_air = v_world - wind_world`.
//!
//! F19 non-negotiable behavior 2 says *the wind used by flight and
//! projectiles is the same authoritative field*. This module is where that
//! sentence becomes one function instead of two:
//!
//! * [`AuthoritativeWind`] lifts the single
//!   [`cs_content::environment::WindField`] out of a current
//!   [`cs_content::environment::EnvironmentState`] — the record a timeline
//!   event replaces — and refuses to exist at all while that field is an
//!   explicit unknown. A caller therefore cannot quietly fly still air
//!   through a storm whose wind nobody measured (F19 behavior 1: unknown
//!   weather tuning stays unknown).
//! * [`AuthoritativeWind::air_relative`] is the **one** conversion every
//!   consumer uses. [`FlightModel`](cs_sim::flight::FlightModel) already
//!   applies `v_air = v_world - wind_world` internally, so
//!   [`AuthoritativeWind::flight_environment`] hands it that very velocity
//!   rather than a second approximation of it; a projectile uses
//!   [`AuthoritativeWind::world_velocity`] and
//!   [`AuthoritativeWind::air_relative`] for the same field.
//! * [`AuthoritativeWind::relative_air_velocity`] is what a weapon system
//!   needs (closing speed between a projectile and a target). It is
//!   deliberately **wind-invariant**: the air moves both bodies together, so
//!   two world velocities that are N apart stay N apart in the air no matter
//!   what the wind does. That is the "consistently" half of AC02 — a gust
//!   changes each body's airspeed by the same amount and must not invent a
//!   closing speed.
//!
//! # What this module does not claim
//!
//! No original wind value, unit or tuning is reproduced here. The sign
//! convention and the subtraction are the designed `FLIGHT-PHYSICS` contract;
//! a projectile's drag, ballistics and wind-shear profile are unmeasured and
//! are *not* modelled — [`ProjectileMotion`] carries a constant air-relative
//! velocity and nothing else, so nothing here can be mistaken for a measured
//! ballistic model. The unknowns are recorded in
//! `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`.
//!
//! # Where this conversion lives, and why that is not settled
//!
//! The code that *applies* a wind is below this module: `cs_sim` owns
//! [`FlightEnvironment::wind_velocity_mps`] and the subtraction inside
//! [`FlightModel`](cs_sim::flight::FlightModel), and F27-B will need the same
//! subtraction again for swept ballistics. The crate dependency runs
//! `cs_app -> cs_sim` and never the reverse, so **`cs_sim` cannot call
//! [`AuthoritativeWind::air_relative`]**. The authoritative *record* is shared —
//! one [`EnvironmentState::wind`] every consumer reads — but the *conversion*
//! is currently reachable only from above the simulation, so a second copy in
//! `cs_sim` is a live risk this module cannot prevent from where it sits.
//!
//! Treat this as settled by task #434 `F19-WIND-CONVERSION-OWNER`, not by this
//! module. Until then, the "one conversion" claim in this file's docs is a
//! statement about the consumers that exist today, not about the whole engine.

use std::fmt;

use cs_content::environment::EnvironmentState;
use cs_sim::flight::FlightEnvironment;
use cs_types::content::Resolved;
use cs_types::evidence::ClaimId;

/// The largest velocity component, in m/s, [`AuthoritativeWind::try_new`] and
/// [`ProjectileMotion::try_new`] accept.
///
/// It is a representability bound, not a tuning limit: a wind or a muzzle
/// velocity beyond it is refused by name instead of silently producing a
/// force the physics would then amplify. `FLIGHT-PHYSICS` fixes no such
/// limit for the original; this is a designed guard against a mistyped
/// import, and it is far above any speed the game plausibly uses.
pub const MAX_AIR_VELOCITY_MPS: f64 = 1.0e4;

/// Why no [`AuthoritativeWind`] could be built.
#[derive(Clone, Debug, PartialEq)]
pub enum WindUnavailable {
    /// The state's wind is an explicit unknown: its claim id and reason.
    ///
    /// There is no still-air default. F19 behavior 1 makes "unknown wind"
    /// and "zero wind" different records, and only the first one is legal
    /// here.
    Unknown {
        /// The claim id of the unknown field.
        claim_id: ClaimId,
        /// Why the evidence left it unknown.
        reason: String,
    },
    /// A component was NaN, infinite or beyond [`MAX_AIR_VELOCITY_MPS`].
    OutOfRange {
        /// The offending component index.
        component: usize,
        /// The rejected value.
        value: f64,
    },
}

impl fmt::Display for WindUnavailable {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unknown { claim_id, reason } => write!(
                f,
                "the authoritative wind is unknown ({}): {reason}",
                claim_id.as_str()
            ),
            Self::OutOfRange { component, value } => write!(
                f,
                "wind_velocity_m_s[{component}] = {value} is not a usable air velocity"
            ),
        }
    }
}

impl std::error::Error for WindUnavailable {}

/// Why no [`ProjectileMotion`] could be built.
#[derive(Clone, Debug, PartialEq)]
pub enum ProjectileError {
    /// A component of the authored air-relative velocity was NaN, infinite
    /// or beyond [`MAX_AIR_VELOCITY_MPS`].
    OutOfRange {
        /// The offending component index.
        component: usize,
        /// The rejected value.
        value: f64,
    },
    /// A supplied distance or time was negative or not finite.
    NonFiniteExtent {
        /// The offending field, as the caller spelled it.
        field: &'static str,
    },
}

impl fmt::Display for ProjectileError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OutOfRange { component, value } => write!(
                f,
                "air_velocity_m_s[{component}] = {value} is not a usable projectile velocity"
            ),
            Self::NonFiniteExtent { field } => write!(f, "{field} must be finite"),
        }
    }
}

impl std::error::Error for ProjectileError {}

/// The one authoritative wind field, as flight and projectiles consume it.
///
/// It holds a velocity and nothing else: no renderer state, no screen fog, no
/// decoration. The single field is
/// [`EnvironmentState::wind`](cs_content::environment::EnvironmentState::wind),
/// so an aircraft and a projectile in the same tick cannot read two different
/// airs — which is exactly what F19 non-negotiable behavior 2 asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AuthoritativeWind {
    velocity_m_s: [f64; 3],
}

impl AuthoritativeWind {
    /// Lifts the current authoritative wind out of an environment state.
    ///
    /// This is the only path a running session uses: it reads the state the
    /// weather timeline replaced, so a gust is picked up on the tick it fired
    /// and never from a copy that could go stale.
    ///
    /// # Errors
    ///
    /// [`WindUnavailable::Unknown`] when the state's wind is an explicit
    /// unknown. There is no still-air substitute: a session whose wind nobody
    /// measured must decide what to do (F19-C owns that policy) rather than
    /// have this constructor invent calm air.
    pub fn from_state(state: &EnvironmentState) -> Result<Self, WindUnavailable> {
        match state.wind() {
            Resolved::Known(known) => Self::try_new(known.value.velocity_m_s()),
            Resolved::Unknown { claim_id, reason } => Err(WindUnavailable::Unknown {
                claim_id: claim_id.clone(),
                reason: reason.clone(),
            }),
        }
    }

    /// Builds the field from a bare world velocity, with the same validation a
    /// `cs_content::environment::WindField` record already applies.
    ///
    /// # Errors
    ///
    /// [`WindUnavailable::OutOfRange`] for a NaN, infinite or
    /// beyond-[`MAX_AIR_VELOCITY_MPS`] component.
    pub fn try_new(velocity_m_s: [f64; 3]) -> Result<Self, WindUnavailable> {
        for (component, value) in velocity_m_s.iter().enumerate() {
            if !value.is_finite() || value.abs() > MAX_AIR_VELOCITY_MPS {
                return Err(WindUnavailable::OutOfRange {
                    component,
                    value: *value,
                });
            }
        }
        Ok(Self { velocity_m_s })
    }

    /// The wind velocity in canonical world meters per second. The same
    /// accessor the record carries, so a consumer reading the effect and a
    /// consumer reading the record see one number.
    #[must_use]
    pub const fn velocity_m_s(&self) -> [f64; 3] {
        self.velocity_m_s
    }

    /// The air-relative velocity of something moving at `world_velocity_m_s`
    /// in world space: `v_air = v_world - wind_world`.
    ///
    /// This is the `FLIGHT-PHYSICS` sign convention and it is the one
    /// conversion this module offers. An aircraft instrument, a projectile
    /// and a closing-speed test all call it, so none of them can drift onto a
    /// different sign or a different field.
    #[must_use]
    pub fn air_relative(&self, world_velocity_m_s: [f64; 3]) -> [f64; 3] {
        [
            world_velocity_m_s[0] - self.velocity_m_s[0],
            world_velocity_m_s[1] - self.velocity_m_s[1],
            world_velocity_m_s[2] - self.velocity_m_s[2],
        ]
    }

    /// The world velocity of something whose velocity is *authored* in
    /// air-relative terms — a projectile launched with a muzzle speed
    /// relative to the air it is about to fly through.
    ///
    /// It is the exact inverse of [`AuthoritativeWind::air_relative`], so
    /// `world_velocity(air_relative(v)) == v` for every representable `v`.
    #[must_use]
    pub fn world_velocity(&self, air_velocity_m_s: [f64; 3]) -> [f64; 3] {
        [
            air_velocity_m_s[0] + self.velocity_m_s[0],
            air_velocity_m_s[1] + self.velocity_m_s[1],
            air_velocity_m_s[2] + self.velocity_m_s[2],
        ]
    }

    /// The air-relative speed of a world velocity: the aircraft's true
    /// airspeed, and the same number a projectile reads off its own velocity.
    ///
    /// This is `|v_air|`. It is the magnitude the flight model computes as
    /// its `InstrumentState::airspeed_mps`, so the effect and the model can
    /// be compared in one assertion.
    #[must_use]
    pub fn airspeed_m_s(&self, world_velocity_m_s: [f64; 3]) -> f64 {
        norm(self.air_relative(world_velocity_m_s))
    }

    /// The velocity of `target` as seen from `origin`, both given as **world**
    /// velocities.
    ///
    /// The wind cancels: subtracting it from two bodies removes it from their
    /// difference, so this is equally the difference of their air-relative
    /// velocities and it does not depend on which [`AuthoritativeWind`]
    /// instance asks. F19-B's AC02 requires the wind change to be consistent
    /// across aircraft and projectiles, and a relative velocity that moved
    /// with the gust would be the classic inconsistency.
    #[must_use]
    pub fn relative_air_velocity(
        &self,
        origin_world_m_s: [f64; 3],
        target_world_m_s: [f64; 3],
    ) -> [f64; 3] {
        sub(target_world_m_s, origin_world_m_s)
    }

    /// A [`FlightEnvironment`] with this wind, otherwise identical to `base`.
    ///
    /// The flight model consumes the air velocity itself, so this is how the
    /// environment's field reaches the aircraft: gravity and air density stay
    /// whatever `base` declared, and the wind is replaced by *this* field —
    /// not added to `base`'s, because there is exactly one wind.
    #[must_use]
    pub fn flight_environment(&self, base: &FlightEnvironment) -> FlightEnvironment {
        FlightEnvironment {
            gravity_mps2: base.gravity_mps2,
            air_density_kg_m3: base.air_density_kg_m3,
            wind_velocity_mps: self.velocity_m_s,
        }
    }
}

/// A projectile's motion through the authoritative wind field.
///
/// It carries a **constant air-relative velocity** and nothing else: no
/// drag, no gravity, no wind shear, no ballistics. Those are unmeasured
/// (F19-B's findings), and a projectile that silently dragged would be a
/// tuning table nobody measured. What this record *does* own is the wind
/// coupling AC02 asks about: the world velocity a projectile actually has,
/// and the fact that the same air-relative speed is reported before and after
/// a gust.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ProjectileMotion {
    air_velocity_m_s: [f64; 3],
}

impl ProjectileMotion {
    /// Builds a projectile from an air-relative velocity.
    ///
    /// # Errors
    ///
    /// [`ProjectileError::OutOfRange`] for a NaN, infinite or
    /// beyond-[`MAX_AIR_VELOCITY_MPS`] component.
    pub fn try_new(air_velocity_m_s: [f64; 3]) -> Result<Self, ProjectileError> {
        for (component, value) in air_velocity_m_s.iter().enumerate() {
            if !value.is_finite() || value.abs() > MAX_AIR_VELOCITY_MPS {
                return Err(ProjectileError::OutOfRange {
                    component,
                    value: *value,
                });
            }
        }
        Ok(Self { air_velocity_m_s })
    }

    /// The authored air-relative velocity, in m/s.
    #[must_use]
    pub const fn air_velocity_m_s(&self) -> [f64; 3] {
        self.air_velocity_m_s
    }

    /// The airspeed this projectile flies at through the air: the magnitude
    /// of its own velocity, which no gust changes.
    #[must_use]
    pub fn airspeed_m_s(&self) -> f64 {
        norm(self.air_velocity_m_s)
    }

    /// The world velocity this projectile has in `wind`.
    ///
    /// It moves with the air mass, which is the whole point of AC02: the
    /// aircraft's true airspeed changes when the wind does, and the
    /// projectile's world velocity changes by exactly the same amount.
    #[must_use]
    pub fn world_velocity_m_s(&self, wind: &AuthoritativeWind) -> [f64; 3] {
        wind.world_velocity(self.air_velocity_m_s)
    }

    /// The distance travelled in world meters over `ticks` fixed ticks of
    /// `dt_s` seconds each, without collision.
    ///
    /// # Errors
    ///
    /// [`ProjectileError::NonFiniteExtent`] when `dt_s` is negative or not
    /// finite.
    pub fn travel_m(
        &self,
        wind: &AuthoritativeWind,
        ticks: u64,
        dt_s: f64,
    ) -> Result<[f64; 3], ProjectileError> {
        if !dt_s.is_finite() {
            return Err(ProjectileError::NonFiniteExtent { field: "dt_s" });
        }
        let elapsed_s = (ticks as f64) * dt_s;
        if elapsed_s < 0.0 {
            return Err(ProjectileError::NonFiniteExtent {
                field: "ticks * dt_s",
            });
        }
        Ok(scale(self.world_velocity_m_s(wind), elapsed_s))
    }

    /// The air-relative speed at which this projectile closes on a target
    /// moving **through the air** at `target_air_velocity_m_s`.
    ///
    /// It is the signed component of the projectile's air-relative velocity,
    /// measured against the target's air-relative velocity, along the
    /// projectile's own flight path: positive while the range closes and
    /// negative when the target pulls away. A caller that knows only a
    /// target's *world* velocity converts it with
    /// [`AuthoritativeWind::air_relative`] first, so the wind is subtracted
    /// once and in one place.
    ///
    /// No wind appears in this number, and that is the point: a relative
    /// quantity inside one air mass cannot move when the whole air mass
    /// moves. A closing speed that changed with the gust would be the
    /// inconsistency AC02 forbids.
    #[must_use]
    pub fn closing_speed_m_s(&self, target_air_velocity_m_s: [f64; 3]) -> f64 {
        let relative = sub(self.air_velocity_m_s, target_air_velocity_m_s);
        dot(relative, normalize(self.air_velocity_m_s))
    }

    /// The rate at which this projectile closes on a target whose **world**
    /// velocity is `target_world_m_s`, measured along the projectile's own
    /// flight path.
    ///
    /// This is the world-space closure and it *does* depend on the wind,
    /// which is correct rather than convenient: a target whose world velocity
    /// is held fixed while the air speeds up is genuinely being blown at a
    /// different rate relative to the projectile. Use
    /// [`ProjectileMotion::closing_speed_m_s`] when the intent is the
    /// closure inside the air.
    #[must_use]
    pub fn world_closing_speed_m_s(
        &self,
        wind: &AuthoritativeWind,
        target_world_m_s: [f64; 3],
    ) -> f64 {
        dot(
            sub(self.world_velocity_m_s(wind), target_world_m_s),
            normalize(self.air_velocity_m_s),
        )
    }
}

/// Euclidean length of a three-component vector.
fn norm(value: [f64; 3]) -> f64 {
    dot(value, value).sqrt()
}

fn dot(lhs: [f64; 3], rhs: [f64; 3]) -> f64 {
    lhs[0] * rhs[0] + lhs[1] * rhs[1] + lhs[2] * rhs[2]
}

fn sub(lhs: [f64; 3], rhs: [f64; 3]) -> [f64; 3] {
    [lhs[0] - rhs[0], lhs[1] - rhs[1], lhs[2] - rhs[2]]
}

fn scale(value: [f64; 3], factor: f64) -> [f64; 3] {
    [value[0] * factor, value[1] * factor, value[2] * factor]
}

/// A unit direction, or the canonical forward axis at exactly zero.
///
/// The epsilon is named for the reason `FLIGHT-PHYSICS` allows one: a
/// projectile at rest has no direction to close along, and zero speed means
/// closing speed is zero whatever direction is chosen — so this only ever
/// picks a harmless direction, it never imposes a minimum flying speed.
fn normalize(value: [f64; 3]) -> [f64; 3] {
    const EPSILON_MPS: f64 = 1e-12;
    let length = norm(value);
    if length <= EPSILON_MPS {
        [0.0, 0.0, -1.0]
    } else {
        scale(value, 1.0 / length)
    }
}

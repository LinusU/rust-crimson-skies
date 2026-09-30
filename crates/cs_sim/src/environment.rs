//! The one air-relative velocity conversion, beside the code that applies it.
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, non-negotiable
//! behavior 2 (*wind used by flight and projectiles is the same authoritative
//! field*); shared contract `docs/contracts/FLIGHT-PHYSICS.md`,
//! "Coordinate convention", which fixes `v_air = v_world - wind_world`. This
//! module was created by task #434 `F19-WIND-CONVERSION-OWNER`.
//!
//! # Why the conversion lives here
//!
//! F19-B recorded the seam it could not close from where it sat
//! (`docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`,
//! "An architectural seam this stage ran into"): the crate graph runs
//! `cs_app -> cs_sim` and never the reverse, so the code that *applies* a wind
//! — [`flight::FlightEnvironment::wind_velocity_mps`] and the subtraction in
//! [`flight::FlightModel::compute`] — could not call the conversion that lived
//! above it in `cs_app::environment::air`. Every new consumer of a wind inside
//! the simulation would then have had to carry its own copy of the subtraction,
//! and F27-B needs exactly that for swept ballistics in `cs_sim::weapons`.
//!
//! So the conversion is owned here, next to its consumers, and
//! `cs_app::environment::air` re-exports it: one implementation, reachable
//! from both sides of the dependency, with no crate depending on another in a
//! new direction.
//!
//! # What is *not* here, deliberately
//!
//! * **No wind record.** Reading the authoritative field out of a
//!   `cs_content::environment::EnvironmentState` needs `cs_content`, which
//!   `cs_sim` may not depend on (`docs/01-ARCHITECTURE.md` allows `cs_types`
//!   and `cs_script` only). That binding — and the refusal of an unknown wind,
//!   with no still-air default — stays in `cs_app::environment::air` as
//!   `AuthoritativeWind::from_state`. The *record* is shared because
//!   `EnvironmentState::wind` is one field every consumer reads; the
//!   *conversion* is shared because it is the three functions below.
//! * **No wind shear, drag, ballistics or tuning.** A wind here is one
//!   velocity for one tick, not a profile through the atmosphere. The original
//!   projectile dynamics are unmeasured and stay recorded as unknown in
//!   `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`.
//! * **No still air.** Nothing in this module supplies a default wind, so a
//!   caller that has no measured field still has to decide what to do.

/// The air-relative velocity of a body moving at `world_velocity_m_s` through
/// air moving at `wind_velocity_m_s`: `v_air = v_world - wind_world`.
///
/// This is the `FLIGHT-PHYSICS` convention applied component-wise, and it is
/// the **only** implementation of that subtraction in the workspace:
/// [`flight::FlightEnvironment::air_relative_velocity_m_s`] (which the flight
/// models call), `cs_app::environment::air::AuthoritativeWind::air_relative`
/// and every future weapon consumer all route through it.
///
/// # Examples
///
/// A body drifting *with* the air mass has no airspeed, which is the property
/// that distinguishes this from adding the wind to a velocity:
///
/// ```
/// # use cs_sim::environment::air_relative_velocity_m_s;
/// assert_eq!(air_relative_velocity_m_s([3.0, 0.0, -1.5], [3.0, 0.0, -1.5]), [0.0; 3]);
/// ```
#[must_use]
pub const fn air_relative_velocity_m_s(
    world_velocity_m_s: [f64; 3],
    wind_velocity_m_s: [f64; 3],
) -> [f64; 3] {
    [
        world_velocity_m_s[0] - wind_velocity_m_s[0],
        world_velocity_m_s[1] - wind_velocity_m_s[1],
        world_velocity_m_s[2] - wind_velocity_m_s[2],
    ]
}

/// The world velocity of a body whose velocity is *authored* in air-relative
/// terms — a projectile launched with a muzzle speed relative to the air it is
/// about to fly through: `v_world = v_air + wind_world`.
///
/// It is the exact inverse of [`air_relative_velocity_m_s`], so
/// `world_velocity_from_air_m_s(air_relative_velocity_m_s(v, w), w) == v` for
/// every `v`, with no rounding in either direction.
#[must_use]
pub const fn world_velocity_from_air_m_s(
    air_velocity_m_s: [f64; 3],
    wind_velocity_m_s: [f64; 3],
) -> [f64; 3] {
    [
        air_velocity_m_s[0] + wind_velocity_m_s[0],
        air_velocity_m_s[1] + wind_velocity_m_s[1],
        air_velocity_m_s[2] + wind_velocity_m_s[2],
    ]
}

/// The airspeed of a body moving at `world_velocity_m_s` through air moving at
/// `wind_velocity_m_s`: `|v_air|`.
///
/// This is the number
/// [`flight::FlightOutput`](crate::flight::FlightOutput)'s instrument reports,
/// so a caller that wants to compare a measurement with the field it was
/// measured in asks for this function rather than computing a norm of its own.
#[must_use]
pub fn airspeed_m_s(world_velocity_m_s: [f64; 3], wind_velocity_m_s: [f64; 3]) -> f64 {
    let air = air_relative_velocity_m_s(world_velocity_m_s, wind_velocity_m_s);
    (air[0] * air[0] + air[1] * air[1] + air[2] * air[2]).sqrt()
}

#[cfg(test)]
mod tests {
    use super::{air_relative_velocity_m_s, airspeed_m_s, world_velocity_from_air_m_s};

    /// The conversion itself: component-wise subtraction, its exact inverse,
    /// and the airspeed both halves agree on.
    #[test]
    fn accept_f19_b_the_air_relative_conversion_is_component_wise_and_exact() {
        let wind = [3.0, 0.0, -1.5];
        let world = [12.0, -4.0, -55.0];

        assert_eq!(
            air_relative_velocity_m_s(world, wind),
            [12.0 - 3.0, -4.0 - 0.0, -55.0 - -1.5],
            "the convention subtracts the wind component-wise, in world space"
        );

        // The inverse is exact: a velocity handed through the field twice is
        // unchanged, so no consumer can round-trip and lose a component.
        let air = air_relative_velocity_m_s(world, wind);
        assert_eq!(world_velocity_from_air_m_s(air, wind), world);
        assert_eq!(
            air_relative_velocity_m_s(world_velocity_from_air_m_s(air, wind), wind),
            air
        );

        // A body drifting *with* the air has no airspeed, and one flying
        // against it has twice the wind added: the sign is what the convention
        // fixes, and both directions follow from it.
        assert_eq!(airspeed_m_s(wind, wind), 0.0);
        let headwind = [-wind[0], -wind[1], -wind[2]];
        assert_eq!(
            air_relative_velocity_m_s(wind, headwind),
            [2.0 * wind[0], 2.0 * wind[1], 2.0 * wind[2]],
            "a headwind adds its velocity to the body's own"
        );
        assert_eq!(airspeed_m_s([3.0, 0.0, -4.0], [0.0; 3]), 5.0);
        assert_eq!(
            airspeed_m_s([12.0, -4.0, -55.0], wind),
            length(air_relative_velocity_m_s([12.0, -4.0, -55.0], wind)),
            "the airspeed is the magnitude of the converted velocity, not of the world velocity"
        );
    }

    /// The conversion is linear in the wind, so a change of wind moves a body
    /// by exactly the wind's own difference, with the sign the conversion
    /// fixes: a body *held* at one world velocity is pushed against the new
    /// air, and a body *authored* in air terms is carried by it. AC02 asks for
    /// both to move by one difference and never by two estimates of it.
    #[test]
    fn accept_f19_b_a_wind_change_moves_air_and_world_velocities_by_the_same_difference() {
        let world = [12.0, -4.0, -55.0];
        let calm = [0.0; 3];
        let gust = [16.0, 0.0, 2.0];

        let calm_air = air_relative_velocity_m_s(world, calm);
        let gust_air = air_relative_velocity_m_s(world, gust);
        assert_ne!(calm_air, gust_air, "the two fields really are two fields");
        assert_eq!(
            [
                gust_air[0] - calm_air[0],
                gust_air[1] - calm_air[1],
                gust_air[2] - calm_air[2],
            ],
            [-gust[0], -gust[1], -gust[2]],
            "a body held at one world velocity is pushed against the new air, by \
             the wind's own difference"
        );

        // A shot authored in air terms keeps its airspeed, and its world
        // velocity moves by the same difference the air moved.
        let shot = [0.0, 0.0, -120.0];
        let carried_calm = world_velocity_from_air_m_s(shot, calm);
        let carried_gust = world_velocity_from_air_m_s(shot, gust);
        assert_eq!(
            airspeed_m_s(carried_gust, gust),
            airspeed_m_s(carried_calm, calm),
            "a projectile's airspeed is its own velocity, not the wind's"
        );
        assert_eq!(
            [
                carried_gust[0] - carried_calm[0],
                carried_gust[1] - carried_calm[1],
                carried_gust[2] - carried_calm[2],
            ],
            [gust[0] - calm[0], gust[1] - calm[1], gust[2] - calm[2]],
            "the projectile's world velocity shifts by the wind's difference too"
        );
    }

    /// A zero wind is a real field, not an absence: the conversion is defined
    /// for it and leaves the world velocity alone. What it is *not* is a
    /// substitute for a wind nobody measured — that refusal lives where the
    /// content record is read, in `cs_app::environment::air`.
    #[test]
    fn accept_f19_b_a_zero_wind_converts_but_is_never_substituted_for_an_unknown_one() {
        let world = [12.0, -4.0, -55.0];
        assert_eq!(
            air_relative_velocity_m_s(world, [0.0; 3]),
            world,
            "a measured still-air field leaves the world velocity alone"
        );
        assert_eq!(airspeed_m_s(world, [0.0; 3]), length(world));
    }

    fn length(value: [f64; 3]) -> f64 {
        (value[0] * value[0] + value[1] * value[1] + value[2] * value[2]).sqrt()
    }
}

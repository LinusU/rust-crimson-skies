//! Environment data, sky frame, weather clock and environment effects at the
//! Bevy boundary (F19-A, F19-B).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stages
//! `### F19-A` and `### F19-B`. Shared contract:
//! `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! F19-A supplied the *records* and their time domain; F19-B implements
//! their *effects*. The split stays visible because the two have different
//! owners:
//!
//! * [`frame`] owns [`SkyFrame`], the record F19 non-negotiable behavior 3
//!   asks for: a dome centred on the camera's **world** position, with the
//!   authored sky orientation and sun direction carried through unchanged.
//!   It is built from a [`SpatialAnchor`] or from a local coordinate plus
//!   its [`WorldOrigin`], and it reports a stale frame
//!   ([`SkyFrame::is_centered_on`]) instead of letting a renderer pop the
//!   sky.
//! * [`clock`] owns [`EnvironmentClock`], the thin, total wiring from a
//!   definition's authored [`cs_content::environment::EnvironmentTimeline`]
//!   onto [`cs_sim::visibility::VisibilityTimeline`]: one authored event
//!   becomes one event at the same tick, and the current state is the
//!   record's own state, never a cached copy.
//! * [`air`] owns [`AuthoritativeWind`], the **one** wind field flight and
//!   projectiles read — `v_air = v_world - wind_world` — plus
//!   [`ProjectileMotion`], which carries a projectile's constant air-relative
//!   velocity through that same field. An unknown wind is refused, never
//!   replaced by still air (AC02). The conversion itself is owned by
//!   `cs_sim::environment`, next to the flight models that apply it, and
//!   re-exported here as [`air_relative_velocity_m_s`],
//!   [`world_velocity_from_air_m_s`] and [`airspeed_m_s`] (task #434
//!   `F19-WIND-CONVERSION-OWNER`), so there is one implementation on both
//!   sides of the dependency.
//! * [`effects`] owns what a frame may be *drawn* from: [`SkyEffect`]
//!   (authored texture, missing-texture diagnostic, or a generated sky only
//!   under an explicitly labeled synthetic/developer run), [`FogEffect`]
//!   (screen-space transmittance and fade, and never a sight range),
//!   [`LightEffect`] (a sun/ambient rig, or `None` when either half is
//!   unknown) and [`EnvironmentEffects`], which gathers one frame of all of
//!   them from a definition and a clock.
//! * [`cosmetic`] owns the decorative side: the [`CosmeticField`] particles
//!   rain and snow are drawn from, drawn only from the run's
//!   `COSMETIC_WEATHER_DOMAIN` stream and advected only by the
//!   authoritative wind.
//! * [`fixture`] authors the two synthetic environments
//!   ([`clear_sky_environment`] and [`storm_environment`]) through the same
//!   constructors a real importer will call. They are production bootstrap
//!   code in the same sense as [`crate::synthetic`] and
//!   [`crate::world::fixture`].
//!
//! Every module here is deliberately free of Bevy, ECS and asset types so the
//! environment can be exercised headless. F19-C wires these records into
//! their real producer and consumer; F19-D audits retail environments and
//! needs `gpu` + `retail`, which this stage does not claim.
//!
//! What is **not** claimed here: no original environment data was read, no
//! original sky/weather format is reproduced, and no renderer draws a sky from
//! these records yet. The unknowns these stages met are recorded in
//! `docs/findings/2026-09-30-f19-a-environment-data-and-time-domains.md` and
//! `docs/findings/2026-09-30-f19-b-sky-fog-light-and-weather-effects.md`.
//!
//! [`SpatialAnchor`]: crate::origin::SpatialAnchor
//! [`WorldOrigin`]: crate::origin::WorldOrigin

pub mod air;
pub mod clock;
pub mod cosmetic;
pub mod effects;
pub mod fixture;
pub mod frame;

pub use air::{
    AuthoritativeWind, MAX_AIR_VELOCITY_MPS, ProjectileError, ProjectileMotion, WindUnavailable,
    air_relative_velocity_m_s, airspeed_m_s, world_velocity_from_air_m_s,
};
pub use clock::EnvironmentClock;
pub use cosmetic::{
    COSMETIC_FIELD_HALF_EXTENT_M, COSMETIC_PARTICLE_COUNT, CosmeticField, CosmeticFieldError,
    CosmeticParticle, PrecipitationEffect,
};
pub use effects::{
    CloudLayerEffect, EnvironmentEffects, FogEffect, FogEffectError, LightEffect, SkyEffect,
    SunLightRig,
};
pub use fixture::{
    CLEAR_ENV_KEY, CLEAR_SKY_TEXTURE_KEY, STORM_ENV_KEY, STORM_RATE_HZ, VISIBILITY_TICK,
    WIND_SHIFT_TICK, clear_sky_environment, storm_environment,
};
pub use frame::{SKY_CENTERING_TOLERANCE_M, SkyFrame, SkyFrameError};

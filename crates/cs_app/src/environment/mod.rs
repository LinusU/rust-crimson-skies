//! Environment data, sky frame and weather clock at the Bevy boundary
//! (F19-A).
//!
//! Spec: `specs/F19-sky-atmosphere-weather-and-visibility.md`, stage
//! `### F19-A`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
//!
//! Stage F19-A is deliberately *not* a runtime: the records live engine-free
//! in [`cs_content::environment`] and the clock rules live in
//! [`cs_sim::visibility`]; this module is the one place the two meet, and it
//! stays free of rendering, ECS and assets so it can be exercised headless:
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
//! * [`fixture`] authors the two synthetic environments
//!   ([`clear_sky_environment`] and [`storm_environment`]) through the same
//!   constructors a real importer will call. They are production bootstrap
//!   code in the same sense as [`crate::synthetic`] and
//!   [`crate::world::fixture`].
//!
//! Stage F19-B implements the sky/fog/light and the discovered weather
//! effects on top of these records; F19-C wires the authoritative wind and
//! visibility policies into their actual producer and consumer; F19-D audits
//! retail environments and needs `gpu` + `retail`, which this stage does not
//! claim.
//!
//! What is **not** claimed here: no original environment data was read, no
//! original sky/weather format is reproduced, and no renderer consumes this
//! yet. The unknowns this stage met are recorded in
//! `docs/findings/2026-09-30-f19-a-environment-data-and-time-domains.md`.

pub mod clock;
pub mod fixture;
pub mod frame;

pub use clock::EnvironmentClock;
pub use fixture::{
    CLEAR_ENV_KEY, CLEAR_SKY_TEXTURE_KEY, STORM_ENV_KEY, STORM_RATE_HZ, VISIBILITY_TICK,
    WIND_SHIFT_TICK, clear_sky_environment, storm_environment,
};
pub use frame::{SKY_CENTERING_TOLERANCE_M, SkyFrame, SkyFrameError};

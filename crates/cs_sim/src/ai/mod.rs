//! AI navigation, routes and obstacle avoidance (F31).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`. Shared
//! contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! * [`navigation`] is stage **F31-A**: the Bevy-free route graph and maneuver
//!   envelope the follower consumes, the monotonic route progress, the swept
//!   arrival and blocker tests, and [`navigation::Navigator::decide`] — a pure
//!   function of one typed tick that emits the same flight command a player's
//!   controls produce.
//!
//! The provenance-carrying producer record a route importer will emit is
//! `cs_content::routes`; this crate cannot depend on `cs_content`
//! (`docs/01-ARCHITECTURE.md`), so the consumer declares its own normalized
//! route and F31-C owns the conversion. Pursuit, bounded avoidance across
//! ticks and the integrated flight loop are F31-B; original route wiring is
//! F31-C and original-data coverage is F31-D.

pub mod navigation;

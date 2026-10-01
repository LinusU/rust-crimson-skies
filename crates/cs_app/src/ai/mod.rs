//! AI navigation in the mission ECS (F31-C follow-up, task #446).
//!
//! Spec: `specs/F31-ai-navigation-routes-and-obstacle-avoidance.md`, stage
//! `### F31-C`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! F31-C projected a declared route into the runtime `cs_sim::ai::navigation`
//! contract and drove it at the `tools/cs_inspect` conversion boundary, but no
//! running mission owned the [`NavigationSet`](cs_sim::ai::navigation::NavigationSet),
//! fed it the integrated flight state or bound an authored moving anchor to a
//! runtime actor. [`navigation`] is that mission-side wiring:
//!
//! * it owns the session's `NavigationSet` as a resource,
//! * it binds a route's authored moving anchor (`ReferenceFrame::Moving` plus
//!   an `AnchorKind`) to the live world transform of the ECS entity that
//!   carries the runtime anchor id (a carrier, train or escort), sampled every
//!   fixed tick, and
//! * it runs in the fixed schedule *before* the F24-B fixed-wing driver, reads
//!   the aircraft's authoritative Avian state, and writes the follower's
//!   `FlightInput` back into the same command boundary a player's controls use.
//!
//! Everything here is newly authored project design; the original route
//! encoding, cadence and anchor binding are unmeasured (F31-D).

pub mod navigation;

pub use navigation::{
    AiNavigation, AiNavigationPlugin, AnchorBinding, BoundRoute, MovingAnchor, NavigationRefusal,
    NavigationRefusalReason, NavigationTickReport, RouteBindingError, RoutePursuit,
    SYNTHETIC_MOVING_ANCHOR_ID, SYNTHETIC_MOVING_ANCHOR_RUNTIME_ID, SYNTHETIC_MOVING_ROUTE_ID,
    SYNTHETIC_NAVIGATION_SEED, SYNTHETIC_NAVIGATION_SESSION, bind_route,
    declared_synthetic_moving_route, synthetic_moving_anchor_id,
};

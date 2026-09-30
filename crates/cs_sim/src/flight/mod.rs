//! Fixed-wing flight equations, tuning schema and synthetic probes (F24).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`.
//! Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! Stage **F24-A** defines the typed contract and the equations, not a
//! runtime. The module is split so each part is one owner:
//!
//! * [`tuning`] is the normalized numeric tuning the model consumes
//!   (`AirframeTuning`, mass, engine, drag, lift, stall, angular response,
//!   assists, boost) plus the loadout-mass and damage records. It validates
//!   at the boundary and refuses a corrupt value by name.
//! * [`model`] is the one place the equations live: air-relative velocity,
//!   angle of attack, dynamic pressure, lift/drag, thrust, world-space
//!   gravity and a bounded rate-command torque. It is a pure function of one
//!   tick, so equal inputs produce equal forces at any render rate.
//! * [`synthetic`] is the declared synthetic fixture and open-loop probe the
//!   acceptance tests drive.
//!
//! The provenance-carrying, normalization-side schema that records where each
//! tuning value came from lives in `cs_content::flight_tuning`; F24-C wires
//! that producer into this consumer. What the original 2000 PC game's exact
//! equations and units were is not recovered here (`F24` "Research boundary");
//! no field in this module is an extracted original coefficient, and the
//! calibration against fingerprinted reference traces is F24-D.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so the model input is declared on the
//! consuming side.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod model;
pub mod synthetic;
pub mod tuning;

pub use model::{
    BODY_FORWARD, BODY_RIGHT, BODY_UP, EngineState, FlightDiagnostics, FlightEnvironment,
    FlightError, FlightInput, FlightInputError, FlightModel, FlightOutput, FlightState,
    InstrumentState,
};
pub use synthetic::{
    SyntheticCase, SyntheticEnvelope, SyntheticEnvelopeError, SyntheticManeuver, SyntheticProbe,
    synthetic_cases, synthetic_fixed_wing, synthetic_trace_envelope, synthetic_trace_envelopes,
};
pub use tuning::{
    AIRSPEED_EPSILON_MPS, AirframeTuning, AirframeTuningError, AngularResponse, AssistProfile,
    BoostParameters, DamageState, DamageStateError, DragParameters, EngineCurve, HandlingProfile,
    LiftCurve, LoadoutMass, LoadoutMassError, MassProperties, ModelKind, StallBehavior,
};

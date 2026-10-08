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
//! * [`autogyro`] is F25-A's and F25-B's exceptional side: the shared
//!   [`autogyro::FlightTelemetry`] interface HUD, AI and probes read, the
//!   [`autogyro::RotorDrive`] a fixed tick advances together with the explicit
//!   [`autogyro::RotorSpeedMapping`] its mesh is drawn at, the
//!   [`autogyro::ReferenceManeuverEnvelope`] an exceptional airframe must be
//!   recorded on, and the [`autogyro::ExceptionalControlLaw`] that evaluates one
//!   tick of an exceptional airframe. The law is the shared boundary plus a
//!   rotor — lift along the shaft axis, drag against the airflow, a
//!   torque-reaction yaw and an exact gyroscopic precession torque — with the
//!   control authority coming from the rotor as well as from airspeed. Its
//!   rotor drive takes an air-relative speed and nothing else, so no throttle
//!   setting reaches it and a profile claiming hover is refused by name rather
//!   than flown.
//!
//! The provenance-carrying, normalization-side schema that records where each
//! tuning value came from lives in `cs_content::flight_tuning`; F24-C wires
//! that producer into this consumer. What the original 2000 PC game's exact
//! equations and units were is not recovered here (`F24` "Research boundary");
//! no field in this module is an extracted original coefficient, the
//! calibration against fingerprinted reference traces is F24-D, and the
//! exceptional law's profile is declared design with
//! [`autogyro::ExceptionalProfile::is_measured`] `false`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so the model input is declared on the
//! consuming side.
//!
//! * [`original`] is task #796's stage: the original 2000 PC game's recovered
//!   fixed-wing law (provenance `OWNER-STATIC-2026-10-08`, static analysis of
//!   the owner's decrypted image), plus the flat parameter vocabulary
//!   ([`original::AIRFRAME_FIELDS`], [`original::GLOBAL_FIELDS`]) the
//!   content-side importer fills from `vehicle.zrd`, `engines.zrd` and
//!   `player.zrd`. It is a `ModelKind` of its own
//!   ([`ModelKind::OriginalFixedWing`]) rather than a profile of the designed
//!   law: its lift is velocity steering, its atmosphere is a hard two-layer
//!   ceiling, and it integrates angular momentum kinematically. Its module
//!   docs carry the mapping onto `docs/contracts/FLIGHT-PHYSICS.md` that
//!   keeps gravity and drag from being applied twice.
//!
//! [`cs_types`]: cs_types
//! [`cs_script`]: cs_script

pub mod autogyro;
pub mod model;
pub mod original;
pub mod synthetic;
pub mod tuning;

pub use autogyro::{
    EnvelopeError, EnvelopeStatus, ExceptionalControlLaw, ExceptionalDiagnostics,
    ExceptionalLawError, ExceptionalProfile, ExceptionalTick, FlightTelemetry, HoverCapability,
    ManeuverKind, ManeuverSpec, MappedVisualRate, ProfileError, ReferenceManeuverEnvelope,
    RotorDrive, RotorSpeedMapping, RotorTelemetry, RotorVisualSample, SYNTHETIC_TICK_DT_S,
    SharedTelemetry, TelemetryError, TelemetryFrame, synthetic_exceptional_envelope,
    synthetic_exceptional_profile, synthetic_exceptional_tuning, synthetic_rotor_drive,
    synthetic_rotor_mapping,
};
pub use model::{
    BODY_FORWARD, BODY_RIGHT, BODY_UP, EngineState, FlightDiagnostics, FlightEnvironment,
    FlightError, FlightInput, FlightInputError, FlightModel, FlightOutput, FlightState,
    InstrumentState,
};
pub use original::{
    AIRFRAME_FIELDS, Atmosphere, DynamicsKind, GLOBAL_FIELDS, OriginalAirframe,
    OriginalFlightModel, OriginalGlobals, OriginalInput, OriginalParamsError, OriginalState,
    OriginalStep, OriginalStepError, atmosphere, authority, drag_coefficient, lift_target,
    max_lift_coefficient, nose_direction, thrust_coefficient,
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

//! Fixed-wing force production on the Avian schedule (F24-B).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-B`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`.
//!
//! F24-A defined the equations as a pure per-tick function; this module is
//! the bounded production path that actually flies a body:
//!
//! * [`FlightAircraft`] is the component one flying body carries. It owns the
//!   [`FlightModel`], the `FlightEnvironment` it flies in, the [`LoadoutMass`]
//!   and [`DamageState`] the equations see, the [`EngineState`] whose spool
//!   follows the throttle command at the tuning's response rate, the remaining
//!   boost capacity, the currently held [`FlightInput`] and the last computed
//!   [`FlightOutput`] (the instruments a panel reads).
//! * [`spawn_flight_body`] is the one place a flight body enters the world. It
//!   wraps [`spawn_body`], so layer bindings and event flags are never
//!   restated, then binds the *declared* mass properties: the body's mass is
//!   `loadout + airframe` — never a second, possibly different number
//!   (non-negotiable behavior 4) — and `AngularInertia` gets the tuning's
//!   principal values with `NoAutoMass`/`NoAutoAngularInertia`, so the
//!   collider's derived properties cannot silently change what the declared
//!   tuning says the integrator must use.
//! * [`drive_flight_aircraft`] runs in `FixedUpdate`, once per fixed tick and
//!   before the `FixedPostUpdate` drain: it reads the body's authoritative
//!   `Rotation`/`LinearVelocity`/world-space `AngularVelocity` (rotated into
//!   body space), advances the spool, computes the tick's forces and submits
//!   them as a one-tick [`ForceRequest`]. The F23-A adapter applies it to the
//!   tick that produced it, so a render frame that runs zero, one or five
//!   ticks produces exactly zero, one or five force computations (AC04).
//! * [`FlightTickReport`] counts every tick, driven aircraft, parked skip and
//!   refusal, and keeps the last refusal with its reason, so a rejected tick
//!   is visible instead of silent.
//!
//! # Discipline inherited from the contract
//!
//! * **One gravity.** The model applies world-space gravity itself, so a
//!   flight world keeps Avian's [`Gravity`] at zero. A non-zero global gravity
//!   would double-count it; the driver refuses the tick and records
//!   [`FlightRefusalReason::GravityConflict`] rather than secretly producing
//!   half-weight or double-weight flight.
//! * **No second integrator.** This module never writes `Position`,
//!   `Rotation` or the velocities during a tick; it reads them and submits
//!   forces, and Avian remains the only pose/velocity integrator.
//! * **Commands persist like axes.** The held [`FlightInput`] is the same
//!   contract `cs_sim::control::AxisState` gives continuous axes — a held
//!   stick keeps its deflection on every tick until a producer replaces it.
//!   The input-session producer that *writes* the command lives in
//!   `cs_app::input`; F24-C's declared producers are the equipment record, the
//!   handling-profile selection and the instrument consumer below.
//! * **Bounded authority.** The arcade controller is the model's rate-command
//!   plus bounded feedback torque: a full deflection commands at most
//!   `angular.max_rate_radps`, and the torque never exceeds
//!   `angular.max_torque_nm`, so the aircraft's response stays inside the
//!   declared envelope at any frame rate.
//!
//! # F24-C producers and consumers
//!
//! * [`FlightEquipment`] is the mission/loadout producer: the declared loadout
//!   mass, damage state and boost reserve a body carries. When present,
//!   [`drive_flight_aircraft`] binds it into the [`FlightAircraft`] record
//!   *before* computing the tick; a rejected equipment record refuses the tick
//!   by name ([`FlightRefusalReason::Equipment`]) instead of flying half-applied
//!   state, and the next tick retries with whatever the producer declares then.
//!   Removing the component tears the producer down: the aircraft keeps its last
//!   bound equipment.
//! * [`airframe_tuning_from_declared`] and [`declared_flight_model`] are the
//!   profile-selection path: one `cs_content` record plus the requested
//!   [`HandlingProfile`] become the production [`AirframeTuning`], refusing a
//!   record that names a different profile rather than silently choosing one.
//! * [`FlightInstruments`] is the panel consumer: after the drive,
//!   [`publish_flight_instruments`] copies the tick's measured
//!   [`InstrumentState`] together with its [`FlightEvidenceClass`]. The class is
//!   derived from the tuning's [`Origin`], so a synthetic-model trace can never
//!   be published as an original-reference result.
//!
//! **Designed wiring, not original data.** Nothing here is an extracted
//! original behavior; the tick ordering is project design and the calibration
//! against original reference traces is F24-D.

use std::fmt;

use avian3d::prelude::{
    AngularInertia, AngularVelocity, Gravity, LinearVelocity, Mass, NoAutoAngularInertia,
    NoAutoMass, RigidBody, Rotation,
};
use bevy::{
    ecs::schedule::IntoScheduleConfigs,
    math::Quat,
    prelude::{
        App, Component, Entity, FixedUpdate, Plugin, Query, Res, ResMut, Resource, Transform, Vec3,
        With, Without, World,
    },
    time::{Fixed, Time},
};
use cs_sim::collision::{CollisionLayer, ShapeClass};
use cs_sim::flight::{
    AirframeTuning, AirframeTuningError, AngularResponse, AssistProfile, BoostParameters,
    DamageState, DamageStateError, DragParameters, EngineCurve, EngineState, FlightEnvironment,
    FlightError, FlightInput, FlightInputError, FlightModel, FlightOutput, FlightState,
    HandlingProfile, InstrumentState, LiftCurve, LoadoutMass, LoadoutMassError, MassProperties,
    ModelKind, StallBehavior,
};
use cs_types::content::Origin;
use cs_types::space::{Quaternion, SpaceError};

use cs_content::flight_tuning::{DeclaredAirframeTuning, TuningSchema};

use super::adapter::{ForceRequest, ForceRequestError, ForceRequests};
use super::body::{BodyError, BodyMode, BodySpec, spawn_body};

/// Why a [`FlightAircraft`] record was refused at its boundary.
#[derive(Clone, Debug, PartialEq)]
pub enum FlightAircraftError {
    /// The tuning failed validation.
    Tuning(AirframeTuningError),
    /// The tuning is not for a fixed-wing airframe; exceptional airframes have
    /// their own control law (F25) and this driver never guesses one.
    UnsupportedModelKind {
        /// The kind that was refused.
        found: ModelKind,
    },
    /// The environment was rejected; the payload is the offending field.
    Environment(&'static str),
    /// The loadout mass was rejected.
    Loadout(LoadoutMassError),
    /// The damage state was rejected.
    Damage(DamageStateError),
    /// The held command was rejected.
    Input(FlightInputError),
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A named field fell outside its declared range.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
        /// The inclusive lower bound.
        min: f64,
        /// The inclusive upper bound.
        max: f64,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: &'static str,
    },
}

impl fmt::Display for FlightAircraftError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Tuning(error) => write!(f, "{error}"),
            Self::UnsupportedModelKind { found } => write!(
                f,
                "the fixed-wing flight driver cannot fly a {} airframe",
                found.label()
            ),
            Self::Environment(field) => write!(f, "{field} is not a usable environment"),
            Self::Loadout(error) => write!(f, "{error}"),
            Self::Damage(error) => write!(f, "{error}"),
            Self::Input(error) => write!(f, "{error}"),
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field} value {value} is outside [{min}, {max}]"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
        }
    }
}

impl std::error::Error for FlightAircraftError {}

impl From<AirframeTuningError> for FlightAircraftError {
    fn from(error: AirframeTuningError) -> Self {
        Self::Tuning(error)
    }
}

impl From<LoadoutMassError> for FlightAircraftError {
    fn from(error: LoadoutMassError) -> Self {
        Self::Loadout(error)
    }
}

impl From<DamageStateError> for FlightAircraftError {
    fn from(error: DamageStateError) -> Self {
        Self::Damage(error)
    }
}

impl From<FlightInputError> for FlightAircraftError {
    fn from(error: FlightInputError) -> Self {
        Self::Input(error)
    }
}

/// Why a declared airframe record could not become a production
/// [`AirframeTuning`].
///
/// The mapping is a boundary, not a repair: a record whose profile does not
/// match the requested one, whose model kind or profile this build does not
/// know, whose required field is missing or explicitly unknown, or whose
/// assembled tuning fails the model's own validation is refused **by name**
/// (`FLIGHT-PHYSICS`: "do not silently clamp corrupted tuning into plausible
/// values").
#[derive(Clone, Debug, PartialEq)]
pub enum FlightTuningError {
    /// The declared record failed its own schema check.
    Record(cs_content::flight_tuning::DeclaredTuningError),
    /// The record names one handling profile but the caller requested another,
    /// so selecting a profile could never silently fly the record's profile.
    ProfileMismatch {
        /// The profile the record names.
        record: String,
        /// The profile the caller requested.
        requested: &'static str,
    },
    /// The record names a handling profile this build does not know.
    UnknownProfile {
        /// The unknown label.
        label: String,
    },
    /// The record enables the assist set while naming the fidelity profile.
    ///
    /// F24 non-negotiable behavior 5 keeps original and modern handling
    /// separate: the profile a calibrated reference trace is compared against
    /// must fly with its assist contributions recorded as exactly zero, so an
    /// assist set on the fidelity profile is refused rather than silently
    /// folded into the calibrated model.
    FidelityAssistsEnabled {
        /// The profile the record names.
        profile: String,
    },
    /// The record names a model kind this build does not know.
    UnknownModelKind {
        /// The unknown label.
        label: String,
    },
    /// A field the model needs is missing or explicitly unknown, so the record
    /// is incomplete and is not read as a zero.
    MissingField {
        /// The absent field's canonical name.
        name: &'static str,
    },
    /// The assembled tuning failed the model's own validation.
    Tuning(AirframeTuningError),
}

impl fmt::Display for FlightTuningError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Record(error) => write!(f, "{error}"),
            Self::ProfileMismatch { record, requested } => write!(
                f,
                "the declared airframe names the {record} profile but {requested} was requested"
            ),
            Self::UnknownProfile { label } => write!(f, "{label} is not a known handling profile"),
            Self::FidelityAssistsEnabled { profile } => write!(
                f,
                "the {profile} profile must not enable assists; only the improved profile may"
            ),
            Self::UnknownModelKind { label } => write!(f, "{label} is not a known model kind"),
            Self::MissingField { name } => {
                write!(f, "the declared airframe does not state the field {name}")
            }
            Self::Tuning(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FlightTuningError {}

impl From<AirframeTuningError> for FlightTuningError {
    fn from(error: AirframeTuningError) -> Self {
        Self::Tuning(error)
    }
}

impl From<cs_content::flight_tuning::DeclaredTuningError> for FlightTuningError {
    fn from(error: cs_content::flight_tuning::DeclaredTuningError) -> Self {
        Self::Record(error)
    }
}

/// Reads one required numeric field of a declared record.
///
/// A field that is absent or explicitly unknown is refused by name rather than
/// read as zero (F14 non-negotiable behavior 3).
fn declared_field(
    record: &DeclaredAirframeTuning,
    name: &'static str,
) -> Result<f64, FlightTuningError> {
    record
        .known_value(name)
        .ok_or(FlightTuningError::MissingField { name })
}

/// Maps one provenance-carrying declared airframe record into the numeric
/// tuning the production flight model consumes, for the requested profile.
///
/// This is the F24-C producer boundary: the content crate owns where each
/// value came from, and this function is the one place those values become the
/// model input. It refuses a record that names a different profile than the
/// caller requested (so an improved profile is never silently flown as the
/// fidelity one), a fidelity record that enables the assist set (behavior 5:
/// the calibrated profile flies with assist contributions of exactly zero), a
/// record whose schema check fails, an unknown model kind, and a required
/// missing/unknown field. The record's [`Origin`] travels into the tuning
/// unchanged, so a synthetic record can never be reported as an original
/// airframe.
///
/// Boost equipment is optional: a record that leaves `boost.consumption_per_s`
/// unknown maps to [`BoostParameters::NONE`] — no boost equipment — instead of
/// a fabricated zero-thrust boost. A record that states a consumption but omits
/// `boost.thrust_n` is refused by name rather than quietly dropping the
/// declared consumption.
///
/// # Errors
///
/// [`FlightTuningError`] naming the first problem.
pub fn airframe_tuning_from_declared(
    record: &DeclaredAirframeTuning,
    profile: HandlingProfile,
) -> Result<AirframeTuning, FlightTuningError> {
    let declared_profile = HandlingProfile::from_label(&record.profile).ok_or_else(|| {
        FlightTuningError::UnknownProfile {
            label: record.profile.clone(),
        }
    })?;
    if declared_profile != profile {
        return Err(FlightTuningError::ProfileMismatch {
            record: record.profile.clone(),
            requested: profile.label(),
        });
    }
    // Profile separation (F24 non-negotiable behavior 5): the fidelity profile is
    // the one a calibrated reference trace is compared against, so it must fly
    // with assist contributions of exactly zero. Only the explicitly named
    // improved profile may enable the declared assist set; a fidelity record that
    // enables them is refused rather than silently folding assists into the
    // calibrated model.
    if profile == HandlingProfile::Fidelity && record.assists_enabled {
        return Err(FlightTuningError::FidelityAssistsEnabled {
            profile: record.profile.clone(),
        });
    }
    record.validate(&TuningSchema::fixed_wing())?;
    let model_kind = ModelKind::from_label(&record.model_kind).ok_or_else(|| {
        FlightTuningError::UnknownModelKind {
            label: record.model_kind.clone(),
        }
    })?;

    let boost = if let Some(consumption_per_s) = record.known_value("boost.consumption_per_s") {
        BoostParameters {
            thrust_n: declared_field(record, "boost.thrust_n")?,
            consumption_per_s,
        }
    } else {
        BoostParameters::NONE
    };

    let tuning = AirframeTuning {
        model_kind,
        profile,
        origin: record.origin.clone(),
        mass: MassProperties {
            mass_kg: declared_field(record, "mass.mass_kg")?,
            inertia_kg_m2: [
                declared_field(record, "mass.inertia_kg_m2[0]")?,
                declared_field(record, "mass.inertia_kg_m2[1]")?,
                declared_field(record, "mass.inertia_kg_m2[2]")?,
            ],
        },
        engine: EngineCurve {
            idle_thrust_n: declared_field(record, "engine.idle_thrust_n")?,
            max_thrust_n: declared_field(record, "engine.max_thrust_n")?,
            throttle_response_per_s: declared_field(record, "engine.throttle_response_per_s")?,
        },
        boost,
        drag: DragParameters {
            zero_lift_coefficient: declared_field(record, "drag.zero_lift_coefficient")?,
            induced_coefficient: declared_field(record, "drag.induced_coefficient")?,
        },
        lift: LiftCurve {
            lift_at_zero_alpha: declared_field(record, "lift.lift_at_zero_alpha")?,
            lift_slope_per_rad: declared_field(record, "lift.lift_slope_per_rad")?,
            max_lift_coefficient: declared_field(record, "lift.max_lift_coefficient")?,
        },
        stall: StallBehavior {
            stall_angle_rad: declared_field(record, "stall.stall_angle_rad")?,
            stall_width_rad: declared_field(record, "stall.stall_width_rad")?,
            residual_fraction: declared_field(record, "stall.residual_fraction")?,
        },
        angular: AngularResponse {
            rate_gain_per_s: declared_field(record, "angular.rate_gain_per_s")?,
            rate_damping_per_s: declared_field(record, "angular.rate_damping_per_s")?,
            max_rate_radps: [
                declared_field(record, "angular.max_rate_radps[0]")?,
                declared_field(record, "angular.max_rate_radps[1]")?,
                declared_field(record, "angular.max_rate_radps[2]")?,
            ],
            max_torque_nm: [
                declared_field(record, "angular.max_torque_nm[0]")?,
                declared_field(record, "angular.max_torque_nm[1]")?,
                declared_field(record, "angular.max_torque_nm[2]")?,
            ],
            control_airspeed_full_mps: declared_field(record, "angular.control_airspeed_full_mps")?,
        },
        assists: AssistProfile {
            enabled: record.assists_enabled,
            bank_level_gain_nm_per_rad: declared_field(
                record,
                "assists.bank_level_gain_nm_per_rad",
            )?,
            bank_level_max_torque_nm: declared_field(record, "assists.bank_level_max_torque_nm")?,
        },
        reference_area_m2: declared_field(record, "reference_area_m2")?,
    };
    AirframeTuning::try_new(tuning).map_err(FlightTuningError::Tuning)
}

/// Builds the production flight model of one declared record for `profile`.
///
/// # Errors
///
/// [`FlightTuningError`] from [`airframe_tuning_from_declared`].
pub fn declared_flight_model(
    record: &DeclaredAirframeTuning,
    profile: HandlingProfile,
) -> Result<FlightModel, FlightTuningError> {
    Ok(FlightModel::new(airframe_tuning_from_declared(
        record, profile,
    )?))
}

/// The equipment a mission or loadout producer declares for one aircraft.
///
/// This is the producer half F24-B left as a bare setter boundary
/// (`set_loadout`/`set_damage`/`set_boost_capacity`): the same three values,
/// named as one record so a mission can insert or rewrite them without
/// reaching into the flight model. When a body carries it,
/// [`drive_flight_aircraft`] binds it into the [`FlightAircraft`] record every
/// tick, so a mid-flight fuel burn, ordnance release or damage event reaches the
/// equations (non-negotiable behavior 4: the same mass a UI performance bar
/// reads is the mass the integrator flies).
///
/// Every field is validated as a whole by [`FlightEquipment::validate`], so a
/// corrupt record is refused *before* any field is applied — no half-bound
/// loadout with pristine damage.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlightEquipment {
    /// The loadout mass the equations see.
    pub loadout: LoadoutMass,
    /// The damage state the equations see.
    pub damage: DamageState,
    /// The remaining boost reserve, in the units `boost.consumption_per_s`
    /// drains. Zero means unavailable.
    pub boost_capacity_units: f64,
}

impl FlightEquipment {
    /// A pristine, empty, boostless equipment record.
    pub const EMPTY: Self = Self {
        loadout: LoadoutMass::EMPTY,
        damage: DamageState::PRISTINE,
        boost_capacity_units: 0.0,
    };

    /// Builds a record only when every field validates.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Loadout`], [`FlightAircraftError::Damage`],
    /// [`FlightAircraftError::NonFinite`] or [`FlightAircraftError::Negative`]
    /// naming the first offending field.
    pub fn try_new(
        loadout: LoadoutMass,
        damage: DamageState,
        boost_capacity_units: f64,
    ) -> Result<Self, FlightAircraftError> {
        let equipment = Self {
            loadout,
            damage,
            boost_capacity_units,
        };
        equipment.validate()?;
        Ok(equipment)
    }

    /// Validates the record as a whole, applying nothing.
    ///
    /// # Errors
    ///
    /// The same named errors [`FlightEquipment::try_new`] returns.
    pub fn validate(&self) -> Result<(), FlightAircraftError> {
        self.loadout.validate()?;
        self.damage.validate()?;
        if !self.boost_capacity_units.is_finite() {
            return Err(FlightAircraftError::NonFinite {
                field: "boost_capacity_units",
            });
        }
        if self.boost_capacity_units < 0.0 {
            return Err(FlightAircraftError::Negative {
                field: "boost_capacity_units",
            });
        }
        Ok(())
    }
}

/// A dynamic rigid body flown by the fixed-wing model.
///
/// The component is the per-aircraft record the fixed tick consumes: the
/// model, the air it flies in, the loadout and damage the forces see, the
/// engine spool, the boost reserve and the held command. Every setter
/// re-validates its value, so a corrupt record can never sit inside a live
/// aircraft and produce half-valid forces (`FLIGHT-PHYSICS`: "Reject nonfinite
/// inputs at boundaries").
#[derive(Component, Clone, Debug, PartialEq)]
pub struct FlightAircraft {
    model: FlightModel,
    environment: FlightEnvironment,
    loadout: LoadoutMass,
    damage: DamageState,
    engine: EngineState,
    boost_capacity_units: f64,
    command: FlightInput,
    last_output: Option<FlightOutput>,
    last_output_tick: Option<u64>,
    last_equipment: Option<FlightEquipment>,
}

impl FlightAircraft {
    /// An aircraft that flies `model` in `environment`, parked: empty loadout,
    /// pristine damage, engine stopped, no boost reserve, neutral command.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Tuning`] for a tuning that fails validation,
    /// [`FlightAircraftError::UnsupportedModelKind`] for a non-fixed-wing
    /// model kind, and [`FlightAircraftError::Environment`] for a rejected
    /// environment.
    pub fn new(
        model: FlightModel,
        environment: FlightEnvironment,
    ) -> Result<Self, FlightAircraftError> {
        if model.tuning().model_kind != ModelKind::FixedWing {
            return Err(FlightAircraftError::UnsupportedModelKind {
                found: model.tuning().model_kind,
            });
        }
        model.tuning().validate()?;
        environment
            .validate()
            .map_err(FlightAircraftError::Environment)?;
        Ok(Self {
            model,
            environment,
            loadout: LoadoutMass::EMPTY,
            damage: DamageState::PRISTINE,
            engine: EngineState::STOPPED,
            boost_capacity_units: 0.0,
            command: FlightInput::NEUTRAL,
            last_output: None,
            last_output_tick: None,
            last_equipment: None,
        })
    }

    /// The model this aircraft flies.
    pub const fn model(&self) -> &FlightModel {
        &self.model
    }

    /// The environment the equations see.
    pub const fn environment(&self) -> FlightEnvironment {
        self.environment
    }

    /// Replaces the environment, validating it.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Environment`] naming the rejected field.
    pub fn set_environment(
        &mut self,
        environment: FlightEnvironment,
    ) -> Result<(), FlightAircraftError> {
        environment
            .validate()
            .map_err(FlightAircraftError::Environment)?;
        self.environment = environment;
        Ok(())
    }

    /// The loadout mass the equations see.
    pub const fn loadout(&self) -> LoadoutMass {
        self.loadout
    }

    /// The total mass the body integrates: airframe plus the declared loadout.
    ///
    /// The driver rewrites the body's `Mass` to this value every tick, so a
    /// loadout change — fuel burn, ordnance release — moves the integrator's
    /// mass with the same number the gravity force is computed from
    /// (non-negotiable behavior 4: there is exactly one mass).
    pub fn total_mass_kg(&self) -> Result<f64, LoadoutMassError> {
        self.loadout.total_mass_kg(self.model.tuning().mass.mass_kg)
    }

    /// Replaces the loadout mass, validating it.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Loadout`] naming the rejected field.
    pub fn set_loadout(&mut self, loadout: LoadoutMass) -> Result<(), FlightAircraftError> {
        loadout.validate()?;
        self.loadout = loadout;
        Ok(())
    }

    /// The damage state the equations see.
    pub const fn damage(&self) -> DamageState {
        self.damage
    }

    /// Replaces the damage state, validating it.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Damage`] naming the rejected field.
    pub fn set_damage(&mut self, damage: DamageState) -> Result<(), FlightAircraftError> {
        damage.validate()?;
        self.damage = damage;
        Ok(())
    }

    /// The engine state the next tick advances.
    pub const fn engine(&self) -> EngineState {
        self.engine
    }

    /// Replaces the engine state (start/stop, seeded spool).
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::NonFinite`] for a non-finite spool and
    /// [`FlightAircraftError::OutOfRange`] for a spool outside `[0, 1]` —
    /// the state is stored, never repaired.
    pub fn set_engine(&mut self, engine: EngineState) -> Result<(), FlightAircraftError> {
        if !engine.spool.is_finite() {
            return Err(FlightAircraftError::NonFinite {
                field: "engine.spool",
            });
        }
        if !(0.0..=1.0).contains(&engine.spool) {
            return Err(FlightAircraftError::OutOfRange {
                field: "engine.spool",
                value: engine.spool,
                min: 0.0,
                max: 1.0,
            });
        }
        self.engine = engine;
        Ok(())
    }

    /// The remaining boost reserve, in the units `boost.consumption_per_s`
    /// drains. Zero means unavailable.
    pub const fn boost_capacity_units(&self) -> f64 {
        self.boost_capacity_units
    }

    /// Sets the boost reserve.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::NonFinite`] and
    /// [`FlightAircraftError::Negative`].
    pub fn set_boost_capacity(&mut self, units: f64) -> Result<(), FlightAircraftError> {
        if !units.is_finite() {
            return Err(FlightAircraftError::NonFinite {
                field: "boost_capacity_units",
            });
        }
        if units < 0.0 {
            return Err(FlightAircraftError::Negative {
                field: "boost_capacity_units",
            });
        }
        self.boost_capacity_units = units;
        Ok(())
    }

    /// Binds a producer's [`FlightEquipment`] atomically: the whole record is
    /// validated first, and only a fully valid record replaces the aircraft's
    /// loadout, damage and reserve.
    ///
    /// Binding is idempotent: a record identical to the last one bound is a
    /// no-op. That is what keeps the boost reserve a *draining* quantity — a
    /// mission that keeps declaring the same equipment does not silently refill
    /// a reserve the ticks consumed. A producer that grants or changes a
    /// reserve writes a different record.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError`] naming the first rejected equipment field;
    /// nothing on the aircraft changes when it is returned.
    pub fn bind_equipment(
        &mut self,
        equipment: &FlightEquipment,
    ) -> Result<(), FlightAircraftError> {
        if self.last_equipment.as_ref() == Some(equipment) {
            return Ok(());
        }
        equipment.validate()?;
        self.set_loadout(equipment.loadout)?;
        self.set_damage(equipment.damage)?;
        self.set_boost_capacity(equipment.boost_capacity_units)?;
        self.last_equipment = Some(*equipment);
        Ok(())
    }

    /// The held command the next tick executes.
    ///
    /// A held deflection persists across ticks like a held axis (`cs_sim`'s
    /// `AxisState` semantics): the producer writes it when the input changes,
    /// the tick reads it every fixed step.
    pub const fn command(&self) -> FlightInput {
        self.command
    }

    /// Replaces the held command, validating its normalized ranges.
    ///
    /// # Errors
    ///
    /// [`FlightAircraftError::Input`] for a non-finite or out-of-range field;
    /// callers with raw human input should go through
    /// [`FlightInput::clamped`] first.
    pub fn set_command(&mut self, command: FlightInput) -> Result<(), FlightAircraftError> {
        command.validate()?;
        self.command = command;
        Ok(())
    }

    /// The output of the most recent computed tick — the instruments and the
    /// per-source [`cs_sim::flight::FlightDiagnostics`] a probe reads. `None`
    /// before the first driven tick and after a refused tick would have left
    /// a stale reading; the refusal itself is in [`FlightTickReport`].
    ///
    /// A refused tick deliberately keeps the previous output: it is the last
    /// *measured* state, and `last_output` never pretends a refused tick
    /// produced forces.
    pub const fn last_output(&self) -> Option<FlightOutput> {
        self.last_output
    }

    /// The fixed tick at which [`last_output`](Self::last_output) was computed,
    /// or `None` before the first driven tick.
    ///
    /// The instrument consumer uses this to publish a reading only on the tick
    /// that actually produced it: a refused tick keeps the previous output (the
    /// last *measured* state) and must not stamp it as a fresh measurement.
    pub const fn last_output_tick(&self) -> Option<u64> {
        self.last_output_tick
    }

    /// Reads the body's authoritative state, advances the engine spool by the
    /// held throttle command and computes one tick of forces.
    ///
    /// The engine spool moves *before* the equations run so the tick's thrust
    /// is the response to the command the tick holds, and boost capacity is
    /// consumed by exactly what the output reports as accepted — a press while
    /// empty consumes nothing (`FLIGHT-PHYSICS`, "Boost and special models").
    fn compute_tick(
        &mut self,
        tick: u64,
        rotation: &Rotation,
        linear: &LinearVelocity,
        angular: &AngularVelocity,
        dt_s: f64,
    ) -> Result<FlightOutput, FlightRefusalReason> {
        let orientation = canonical_rotation(rotation)?;
        let angular_body = world_to_body(orientation, angular.0.to_array().map(f64::from));
        self.engine.advance(
            self.command.throttle,
            self.model.tuning().engine.throttle_response_per_s,
            dt_s,
        );
        let state = FlightState {
            orientation,
            linear_velocity_mps: linear.0.to_array().map(f64::from),
            angular_velocity_radps: angular_body,
            engine: self.engine,
            boost_available: self.boost_capacity_units > 0.0,
        };
        let output = self
            .model
            .compute(
                &self.environment,
                &self.loadout,
                &self.damage,
                &state,
                &self.command,
                dt_s,
            )
            .map_err(FlightRefusalReason::Model)?;
        self.boost_capacity_units =
            (self.boost_capacity_units - output.accepted_boost_consumption).max(0.0);
        self.last_output = Some(output);
        self.last_output_tick = Some(tick);
        Ok(output)
    }
}

/// Why one aircraft's tick produced no force request.
///
/// Every refusal is recorded in [`FlightTickReport`] with the entity, the tick
/// and this reason: a rejected tick is loud, never a silently parked aircraft
/// (`FLIGHT-PHYSICS`: boundaries refuse, they do not repair).
#[derive(Clone, Debug, PartialEq)]
pub enum FlightRefusalReason {
    /// The world's [`Gravity`] is not zero while the flight model applies its
    /// own world-space gravity force. Flying anyway would double gravity; the
    /// tick is refused and counted instead.
    GravityConflict {
        /// The conflicting global gravity, in m/s².
        gravity_mps2: [f32; 3],
    },
    /// The entity carries [`FlightAircraft`] but no [`RigidBody`], so it can
    /// never integrate a force.
    MissingBody,
    /// The entity has a [`RigidBody`] but the named physics readback is
    /// absent — it was not spawned through the production path.
    MissingState {
        /// The absent component.
        field: &'static str,
    },
    /// The declared airframe plus loadout mass could not be bound to the
    /// body's `Mass` — the integrator would fly a different mass than the
    /// model's forces were computed with.
    Mass,
    /// The producer's [`FlightEquipment`] record was rejected, so the tick
    /// flew nothing rather than half-applying a corrupt loadout, damage state
    /// or reserve. The record stays on the entity and is retried next tick.
    Equipment(FlightAircraftError),
    /// The body's rotation readback was not a usable unit quaternion.
    Rotation(SpaceError),
    /// The flight model refused the tick.
    Model(FlightError),
    /// The computed force could not enter the adapter's one-tick queue.
    Request(ForceRequestError),
}

impl fmt::Display for FlightRefusalReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::GravityConflict { gravity_mps2 } => write!(
                f,
                "global gravity {gravity_mps2:?} conflicts with the flight model's own gravity"
            ),
            Self::MissingBody => {
                f.write_str("FlightAircraft without a RigidBody can never be flown")
            }
            Self::MissingState { field } => {
                write!(f, "the flight body carries no {field} to read")
            }
            Self::Mass => f.write_str("the declared total mass is not a usable body mass"),
            Self::Equipment(error) => {
                write!(f, "the declared equipment record is not usable: {error}")
            }
            Self::Rotation(error) => write!(f, "{error}"),
            Self::Model(error) => write!(f, "{error}"),
            Self::Request(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FlightRefusalReason {}

/// One refused aircraft tick: which entity, which fixed tick, and why.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightRefusal {
    /// The aircraft entity that was refused.
    pub entity: Entity,
    /// The fixed tick the refusal happened on.
    pub tick: u64,
    /// Why it was refused.
    pub reason: FlightRefusalReason,
}

/// The flight driver's per-session accounting.
///
/// `ticks` counts the fixed ticks the driver ran — it must equal the physics
/// ledger's tick count. `driven` counts the aircraft-ticks that produced an
/// applied force request, so with one aircraft `driven == ticks` is the
/// "one force request per fixed tick" invariant (AC04's runtime half).
/// `parked` counts aircraft-ticks skipped because the body is not dynamic (a
/// kinematic scripted actor is a legitimate non-flying state), and `refused`
/// counts every loud rejection, with `last_refusal` keeping the newest one.
#[derive(Resource, Clone, Debug, Default, PartialEq)]
pub struct FlightTickReport {
    /// Fixed ticks the driver ran.
    pub ticks: u64,
    /// Aircraft-ticks that produced an applied force request.
    pub driven: u64,
    /// Aircraft-ticks skipped because the body is not dynamic.
    pub parked: u64,
    /// Aircraft-ticks refused for a recorded reason.
    pub refused: u64,
    /// Aircraft-ticks whose measured output reached an instrument panel this
    /// tick.
    ///
    /// Only a tick that actually computed forces publishes: a refused or parked
    /// aircraft leaves a panel with the last measured state rather than a fresh
    /// reading stamped with a tick that measured nothing.
    pub instrumented: u64,
    /// The most recent refusal.
    pub last_refusal: Option<FlightRefusal>,
}

/// What one flight body needs to enter the world.
///
/// The declared airframe tuning supplies the mass properties; this record
/// supplies the rest. The body's actual mass is always
/// `loadout.total_mass_kg(tuning.mass.mass_kg)` — computed inside
/// [`spawn_flight_body`], never restated by the caller.
#[derive(Clone, Debug, PartialEq)]
pub struct FlightSpawnSpec {
    /// Which declared collision layer the aircraft belongs to.
    pub layer: CollisionLayer,
    /// Solid obstacle or sensor volume.
    pub shape: ShapeClass,
    /// Half of each collider dimension, in meters. Strictly positive.
    pub half_extents_m: [f32; 3],
    /// Initial world position, in meters.
    pub position_m: [f32; 3],
    /// Initial body-to-world orientation.
    pub orientation: Quaternion,
    /// Initial world-space linear velocity, in m/s.
    pub linear_velocity_mps: [f32; 3],
    /// The engine state at spawn (stopped, or a seeded spool).
    pub engine: EngineState,
    /// The loadout mass at spawn.
    pub loadout: LoadoutMass,
    /// The damage state at spawn.
    pub damage: DamageState,
    /// The boost reserve at spawn.
    pub boost_capacity_units: f64,
    /// The held command at spawn.
    pub command: FlightInput,
    /// The environment the aircraft flies in.
    pub environment: FlightEnvironment,
}

impl FlightSpawnSpec {
    /// A level, pristine aircraft on the `Aircraft` layer at `position_m` with
    /// `linear_velocity_mps`: engine stopped, empty loadout, no boost reserve,
    /// neutral command, sea-level air.
    ///
    /// `half_extents_m` is a collider size, not an aerodynamic input — the
    /// forces come from the tuning's `reference_area_m2`, so the declared
    /// half-extents only decide what the aircraft collides with.
    pub fn level_at(position_m: [f32; 3], linear_velocity_mps: [f32; 3]) -> Self {
        Self {
            layer: CollisionLayer::Aircraft,
            shape: ShapeClass::Solid,
            half_extents_m: [0.5, 0.5, 0.5],
            position_m,
            orientation: Quaternion::IDENTITY,
            linear_velocity_mps,
            engine: EngineState::STOPPED,
            loadout: LoadoutMass::EMPTY,
            damage: DamageState::PRISTINE,
            boost_capacity_units: 0.0,
            command: FlightInput::NEUTRAL,
            environment: FlightEnvironment::SEA_LEVEL,
        }
    }
}

/// Why a flight body could not enter the world.
#[derive(Debug)]
pub enum FlightSpawnError {
    /// The aircraft record was rejected; nothing spawned.
    Aircraft(FlightAircraftError),
    /// The declared inertia could not be bound to the body.
    Inertia,
    /// The underlying body spec was rejected; nothing spawned.
    Body(BodyError),
}

impl fmt::Display for FlightSpawnError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Aircraft(error) => write!(f, "{error}"),
            Self::Inertia => write!(f, "the declared principal inertia is not usable"),
            Self::Body(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for FlightSpawnError {}

impl From<FlightAircraftError> for FlightSpawnError {
    fn from(error: FlightAircraftError) -> Self {
        Self::Aircraft(error)
    }
}

impl From<BodyError> for FlightSpawnError {
    fn from(error: BodyError) -> Self {
        Self::Body(error)
    }
}

/// Spawns one flight body through the production creation path.
///
/// Validation runs before anything spawns — a rejected spec leaves no half a
/// body behind. The spawned body is dynamic, carries the layer bindings
/// [`spawn_body`] declares, the tuning's declared total mass and principal
/// inertia (with `NoAutoMass`/`NoAutoAngularInertia` so the collider's derived
/// values cannot add to them), the spawn orientation on both `Rotation` and
/// `Transform`, and the [`FlightAircraft`] record the fixed tick drives.
///
/// The aircraft's mass is computed here from the tuning plus the declared
/// loadout: there is exactly one mass, and it is the same number the flight
/// model uses for its gravity force and a UI performance bar reads
/// (non-negotiable behavior 4).
pub fn spawn_flight_body(
    world: &mut World,
    model: FlightModel,
    spec: &FlightSpawnSpec,
) -> Result<Entity, FlightSpawnError> {
    let tuning = model.tuning().clone();
    let mut aircraft = FlightAircraft::new(model, spec.environment)?;
    aircraft.set_loadout(spec.loadout)?;
    aircraft.set_damage(spec.damage)?;
    aircraft.set_engine(spec.engine)?;
    aircraft.set_boost_capacity(spec.boost_capacity_units)?;
    aircraft.set_command(spec.command)?;

    let total_mass_kg = spec
        .loadout
        .total_mass_kg(tuning.mass.mass_kg)
        .map_err(FlightAircraftError::from)?;
    let mass_kg = total_mass_kg as f32;
    if !mass_kg.is_finite() || mass_kg <= 0.0 {
        return Err(FlightAircraftError::NonFinite {
            field: "total_mass_kg",
        }
        .into());
    }

    let entity = spawn_body(
        world,
        &BodySpec {
            layer: spec.layer,
            shape: spec.shape,
            mode: BodyMode::Dynamic,
            mass_kg,
            half_extents_m: spec.half_extents_m,
            position_m: spec.position_m,
            linear_velocity_m_s: spec.linear_velocity_mps,
        },
    )?;

    let [x, y, z, w] = spec.orientation.components();
    let rotation = Quat::from_xyzw(x as f32, y as f32, z as f32, w as f32).normalize();
    let inertia = tuning.mass.inertia_kg_m2;
    // The tuning lists the principal inertia in (roll, pitch, yaw) order, and
    // roll/pitch/yaw turn about the body +Z (forward is -Z), +X and +Y axes
    // (FLIGHT-PHYSICS, "Coordinate convention"), so the Avian principal vector
    // (about local X, Y, Z) is (pitch, yaw, roll).
    let principal = Vec3::new(inertia[1] as f32, inertia[2] as f32, inertia[0] as f32);
    let angular_inertia =
        AngularInertia::try_new(principal).map_err(|_| FlightSpawnError::Inertia)?;

    let mut target = world.entity_mut(entity);
    target.insert(Rotation(rotation));
    if let Some(mut transform) = target.get_mut::<Transform>() {
        transform.rotation = rotation;
    }
    target.insert((angular_inertia, NoAutoAngularInertia, NoAutoMass, aircraft));
    Ok(entity)
}

/// Registers the flight driver and its tick report.
///
/// The driver runs in `FixedUpdate`, inside the pinned fixed schedule and
/// before the `FixedPostUpdate` drain the F23-A adapter owns, so every fixed
/// tick submits exactly one [`ForceRequest`] per aircraft and that request is
/// applied to the same tick — never buffered across a render frame.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FlightForcesPlugin;

impl Plugin for FlightForcesPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<FlightTickReport>();
        // The instrument consumer reads the tick the driver just computed, so it
        // is chained after it inside the same fixed tick.
        app.add_systems(
            FixedUpdate,
            (drive_flight_aircraft, publish_flight_instruments).chain(),
        );
    }
}

/// Drives every flight body through one fixed tick.
///
/// Runs once per fixed tick in `FixedUpdate`. For each dynamic aircraft it
/// reads the authoritative pose and velocities, computes the tick's forces
/// and queues a one-tick request the `FixedPostUpdate` drain applies to this
/// tick. A non-dynamic aircraft body is parked (counted, not an error — a
/// scripted actor is not flown), a missing `RigidBody` or readback is refused
/// and counted, and a non-zero global gravity refuses the tick rather than
/// doubling the model's own gravity.
/// The bodies the driver flies: the record plus the rigid body and its
/// authoritative readbacks. The readbacks are optional so a body spawned
/// outside the production path is refused by name rather than silently
/// ignored by a narrower query.
type FlightBodies<'world, 'state> = Query<
    'world,
    'state,
    (
        Entity,
        &'static mut FlightAircraft,
        &'static RigidBody,
        Option<&'static Rotation>,
        Option<&'static LinearVelocity>,
        Option<&'static AngularVelocity>,
        Option<&'static mut Mass>,
        Option<&'static FlightEquipment>,
    ),
>;

fn drive_flight_aircraft(
    time: Res<Time<Fixed>>,
    gravity: Res<Gravity>,
    mut requests: ResMut<ForceRequests>,
    mut bodies: FlightBodies,
    missing_bodies: Query<Entity, (With<FlightAircraft>, Without<RigidBody>)>,
    mut report: ResMut<FlightTickReport>,
) {
    report.ticks += 1;
    let dt_s = time.timestep().as_secs_f64();
    let gravity_conflict = gravity.0 != Vec3::ZERO;

    for (entity, mut aircraft, body, rotation, linear, angular, mass, equipment) in &mut bodies {
        if !body.is_dynamic() {
            report.parked += 1;
            continue;
        }
        // Producer state first: a declared equipment record is bound into the
        // record before the equations run, and a rejected one refuses the whole
        // tick rather than flying a half-bound loadout/damage/reserve. The
        // record stays on the entity, so the next tick retries it.
        if let Some(equipment) = equipment
            && let Err(error) = aircraft.bind_equipment(equipment)
        {
            refuse(&mut report, entity, FlightRefusalReason::Equipment(error));
            continue;
        }
        if gravity_conflict {
            refuse(
                &mut report,
                entity,
                FlightRefusalReason::GravityConflict {
                    gravity_mps2: gravity.0.to_array(),
                },
            );
            continue;
        }
        let (rotation, linear, angular, mut mass) = match (rotation, linear, angular, mass) {
            (Some(rotation), Some(linear), Some(angular), Some(mass)) => {
                (rotation, linear, angular, mass)
            }
            (rotation, linear, angular, mass) => {
                let field = if rotation.is_none() {
                    "rotation"
                } else if linear.is_none() {
                    "linear_velocity"
                } else if angular.is_none() {
                    "angular_velocity"
                } else {
                    let _ = mass;
                    "mass"
                };
                refuse(
                    &mut report,
                    entity,
                    FlightRefusalReason::MissingState { field },
                );
                continue;
            }
        };

        // One mass, always: the integrator's `Mass` follows the declared
        // airframe + loadout total, so a mid-flight loadout change moves the
        // same mass the gravity force is computed from. `NoAutoMass` keeps
        // the collider's derived contribution out of the recomputation.
        let total_mass = match aircraft.total_mass_kg() {
            Ok(total) => total as f32,
            Err(_) => {
                refuse(&mut report, entity, FlightRefusalReason::Mass);
                continue;
            }
        };
        if !total_mass.is_finite() || total_mass <= 0.0 {
            refuse(&mut report, entity, FlightRefusalReason::Mass);
            continue;
        }
        if *mass != Mass(total_mass) {
            *mass = Mass(total_mass);
        }

        match aircraft.compute_tick(report.ticks, rotation, linear, angular, dt_s) {
            Ok(output) => {
                match ForceRequest::new(
                    entity,
                    output.world_force_n.map(|value| value as f32),
                    output.world_torque_nm.map(|value| value as f32),
                ) {
                    Ok(request) => {
                        requests.submit(request);
                        report.driven += 1;
                    }
                    Err(error) => refuse(&mut report, entity, FlightRefusalReason::Request(error)),
                }
            }
            Err(reason) => refuse(&mut report, entity, reason),
        }
    }

    for entity in &missing_bodies {
        refuse(&mut report, entity, FlightRefusalReason::MissingBody);
    }
}

/// Records a refused aircraft tick.
fn refuse(report: &mut FlightTickReport, entity: Entity, reason: FlightRefusalReason) {
    report.refused += 1;
    report.last_refusal = Some(FlightRefusal {
        entity,
        tick: report.ticks,
        reason,
    });
}

/// How strong a claim a published instrument reading may support.
///
/// The class travels with the reading so a consumer never has to guess. It is
/// derived from the tuning's [`Origin`]: this build produces synthetic results
/// from synthetic/designed tuning, and even a tuning located in the owner's
/// installation is **not** an original-reference trace — that is the
/// capability-gated F24-D calibration against `REF-OWNER-FIRST-CAPTURE`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FlightEvidenceClass {
    /// The tuning is newly authored synthetic fixture or designed content; a
    /// trace measured from it is a synthetic result, never original-reference.
    Synthetic,
    /// The tuning's values were located in the owner's installation. The
    /// reading still is not an original-reference trace until F24-D fits it to
    /// fingerprinted captures.
    InstallationBacked,
}

impl FlightEvidenceClass {
    /// The class of a tuning with `origin`.
    #[must_use]
    pub fn from_origin(origin: &Origin) -> Self {
        if origin.is_original() {
            Self::InstallationBacked
        } else {
            Self::Synthetic
        }
    }

    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Synthetic => "synthetic",
            Self::InstallationBacked => "installation_backed",
        }
    }

    /// Whether the reading bounds the designed model only.
    #[must_use]
    pub const fn is_synthetic(self) -> bool {
        matches!(self, Self::Synthetic)
    }
}

/// The last measured instrument reading of one aircraft, for a panel consumer.
///
/// [`publish_flight_instruments`] writes it after the drive, so a panel reads a
/// snapshot instead of reaching into the flight record. `tick` is the fixed tick
/// that produced the reading — never the tick it was published on — so a
/// reading that survived a refused tick is still labelled with the tick that
/// actually measured it.
#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct FlightInstruments {
    /// The measured state of the publishing tick.
    pub state: InstrumentState,
    /// The strongest claim the reading supports.
    pub evidence: FlightEvidenceClass,
    /// The fixed tick whose computation produced `state`.
    pub tick: u64,
}

/// Publishes each driven aircraft's measured output to its instrument panel.
///
/// Runs chained after [`drive_flight_aircraft`] in the same fixed tick. Only an
/// aircraft whose `last_output` was produced *this* tick is published, so a
/// refused or parked aircraft leaves the panel showing its last measured state
/// instead of a fresh-looking reading the tick did not measure.
fn publish_flight_instruments(
    mut report: ResMut<FlightTickReport>,
    mut panels: Query<(&FlightAircraft, &mut FlightInstruments)>,
) {
    let tick = report.ticks;
    for (aircraft, mut panel) in &mut panels {
        if aircraft.last_output_tick() != Some(tick) {
            continue;
        }
        if let Some(output) = aircraft.last_output() {
            panel.state = output.instrument_state;
            panel.evidence = FlightEvidenceClass::from_origin(&aircraft.model().tuning().origin);
            panel.tick = tick;
            report.instrumented += 1;
        }
    }
}

/// Converts the body's authoritative `Rotation` readback into the canonical
/// unit quaternion.
///
/// The integrated f32 rotation is renormalized on the way to f64 — that is a
/// readback of the *physical* pose, not input repair — and then still
/// validated, so a degenerate readback is refused by name instead of feeding
/// the equations a non-unit rotation.
fn canonical_rotation(rotation: &Rotation) -> Result<Quaternion, FlightRefusalReason> {
    let components = rotation.0.to_array().map(f64::from);
    let length = (components[0] * components[0]
        + components[1] * components[1]
        + components[2] * components[2]
        + components[3] * components[3])
        .sqrt();
    if !length.is_finite() || length <= 0.0 {
        return Err(FlightRefusalReason::Rotation(SpaceError::NonFinite {
            field: "rotation",
        }));
    }
    Quaternion::try_new([
        components[0] / length,
        components[1] / length,
        components[2] / length,
        components[3] / length,
    ])
    .map_err(FlightRefusalReason::Rotation)
}

/// Rotates `vector` from world space into body space.
///
/// Avian's `AngularVelocity` is world-space; the model's
/// `FlightState::angular_velocity_radps` is body-space. Rotating by the
/// conjugate of the body-to-world orientation gives the components in the
/// body basis (`FLIGHT-PHYSICS`, "Coordinate convention").
fn world_to_body(orientation: Quaternion, vector: [f64; 3]) -> [f64; 3] {
    let [x, y, z, w] = orientation.components();
    rotate_vector(vector, [-x, -y, -z, w])
}

/// Rotates `vector` by the unit quaternion `rotation` (`(x, y, z, w)` order):
/// `v' = v + 2w(q × v) + 2q × (q × v)`, the same expansion `cs_sim`'s flight
/// model uses for the body-to-world direction.
fn rotate_vector(vector: [f64; 3], rotation: [f64; 4]) -> [f64; 3] {
    let [x, y, z, w] = rotation;
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    let twice = scale(axis_cross, 2.0 * w);
    let second = cross(axis, axis_cross);
    add(add(vector, twice), scale(second, 2.0))
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The quaternion helpers must be self-consistent: a world vector rotated
    /// into body space by the conjugate is the inverse of the model's
    /// body-to-world rotation, and basis vectors land on their own axes.
    #[test]
    fn accept_f24_b_world_to_body_is_the_inverse_rotation() {
        let orientation = Quaternion::from_axis_angle(
            cs_types::space::UnitVec3::try_new([0.0, 1.0, 0.0]).expect("unit axis"),
            cs_types::space::Radians(1.0),
        )
        .expect("unit quaternion");

        // A world vector along +X rotated into the body frame of an aircraft
        // yawed +1 rad about +Y: body -Z is forward, so world +X sits mostly
        // behind/right of the nose.
        let body = world_to_body(orientation, [1.0, 0.0, 0.0]);
        assert!(body.iter().all(|value| value.is_finite()));
        let length_sq = body.iter().map(|value| value * value).sum::<f64>();
        assert!(
            (length_sq - 1.0).abs() < 1e-12,
            "a rotation must preserve length: {body:?}"
        );

        // Round-trip: body -> world -> body returns the input.
        let [x, y, z, w] = orientation.components();
        let world = rotate_vector(body, [x, y, z, w]);
        for (got, expected) in world.into_iter().zip([1.0, 0.0, 0.0]) {
            assert!(
                (got - expected).abs() < 1e-12,
                "round-trip drift: {world:?}"
            );
        }
    }

    /// The component boundary refuses a non-fixed-wing tuning, a corrupt
    /// engine spool, a negative boost reserve and an out-of-range command by
    /// name.
    #[test]
    fn accept_f24_b_component_boundary_refuses_by_name() {
        let model = FlightModel::new(cs_sim::flight::synthetic_fixed_wing());
        let mut aircraft =
            FlightAircraft::new(model, FlightEnvironment::SEA_LEVEL).expect("valid aircraft");

        assert_eq!(
            aircraft.set_engine(EngineState {
                running: true,
                spool: f64::NAN,
            }),
            Err(FlightAircraftError::NonFinite {
                field: "engine.spool"
            })
        );
        assert_eq!(
            aircraft.set_boost_capacity(-1.0),
            Err(FlightAircraftError::Negative {
                field: "boost_capacity_units"
            })
        );
        assert_eq!(
            aircraft.set_command(FlightInput {
                pitch: 2.0,
                ..FlightInput::NEUTRAL
            }),
            Err(FlightAircraftError::Input(FlightInputError::OutOfRange {
                field: "input.pitch",
                value: 2.0,
                min: -1.0,
                max: 1.0,
            }))
        );

        let mut exceptional_tuning = cs_sim::flight::synthetic_fixed_wing();
        exceptional_tuning.model_kind = ModelKind::Exceptional;
        match FlightAircraft::new(
            FlightModel::new(exceptional_tuning),
            FlightEnvironment::SEA_LEVEL,
        ) {
            Err(FlightAircraftError::UnsupportedModelKind {
                found: ModelKind::Exceptional,
            }) => {}
            other => panic!("exceptional model kinds must be refused: {other:?}"),
        }
    }
}

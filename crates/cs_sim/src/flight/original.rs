//! The original 2000 PC game's fixed-wing flight law (task #796).
//!
//! Provenance label **`OWNER-STATIC-2026-10-08`**: every equation, constant
//! and default in this module was recovered by owner-requested static
//! analysis of `crimson.decrypted.exe` (sha256
//! `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`, image
//! base `0x400000`). The virtual addresses are cited inline and written up in
//! `docs/findings/2026-10-08-flight-original-fixed-wing-law.md`.
//!
//! This is **static code evidence**, which the owner accepts in place of an
//! original run for this purpose. It is never `verified_original` and no
//! original executable ran; nothing here is a measured behaviour. The
//! per-airframe *numbers* are not hard-coded here at all: they live in
//! `vehicle.zrd`, `engines.zrd` and `player.zrd` and are imported by
//! `cs_content::original_airframe`. Only the constants the original keeps in
//! its own image (the atmosphere table, `fall_off`, `bank_off`, the fades'
//! units) appear below, each with the address it was read from.
//!
//! # The law in one step
//!
//! One step per frame, `dt <= `[`MAX_STEP_S`] seconds, no substeps:
//!
//! 1. the actual throttle slews toward the command at
//!    [`THROTTLE_SLEW_PER_S`] per second and a player burns fuel; **every**
//!    later use of "throttle" is this actual value, never the command, so a
//!    fuel-exhausted aircraft whose throttle froze cannot thrust again;
//! 2. [`atmosphere`] gives `k`, `rho` and the speed of sound — a **hard
//!    two-layer ceiling** above [`LOW_CEILING_FT`], with no interpolation;
//! 3. [`thrust_coefficient`] times the actual throttle, engine factor,
//!    reference area and the nose-orientation factor, along the nose;
//! 4. [`drag_coefficient`] times `q * S * drag_factor`, against the velocity;
//! 5. lift is **velocity steering**, not a lift curve: [`lift_target`] blends
//!    the target velocity from the current one to the nose direction across
//!    [`LIFT_AOA_DEG`], the demanded acceleration becomes a load factor and
//!    that becomes `CL`;
//! 6. gravity is the acceleration `-g` on `y`;
//! 7. the rotation terms accumulate into a world-space angular-momentum
//!    increment, damping runs as `exp(-damp * dt)` and the attitude rotates by
//!    **`2 * |omega| * dt`** per step — the original's quaternion exponential
//!    does not halve the angle, which is why the steady rates are about twice
//!    the naive value;
//! 8. `v += a * dt`, then `pos += v * dt` (semi-implicit), with a floor on the
//!    nose-ward speed for AI only;
//! 9. the fake-dynamics branch ([`DynamicsKind::Fake`]: the autogyro and far
//!    AI) replaces 2–8 with `a = fd_speed * throttle * nose - v`.
//!
//! # Mapping onto the shared contract
//!
//! `docs/contracts/FLIGHT-PHYSICS.md` sends forces and torques out and lets
//! Avian integrate. This law is the contract's documented exception
//! ("Exceptional airframes implement the same input/output boundary but can
//! use a different control law"), added as
//! [`ModelKind::OriginalFixedWing`](super::tuning::ModelKind::OriginalFixedWing)
//! rather than by bending the designed `CL(alpha)` law:
//!
//! * **Linear** — the law integrates velocity itself, so a consumer that hands
//!   motion to a rigid body must take [`OriginalStep::world_force`] =
//!   `(W / 9.82) * a`, i.e. `m = W / 9.82`, and **disable the body's own
//!   gravity and drag**: gravity is already inside `a` (step 6) and drag is
//!   already inside it (step 4). Applying both would double-count both, which
//!   the contract forbids.
//! * **Angular** — the original integrates angular momentum kinematically and
//!   rotates the attitude by `2 * |omega| * dt`. A torque-driven rigid body
//!   cannot reproduce that un-halved rotation, so while this model kind is
//!   active the law owns attitude, and [`OriginalStep::world_torque`] is the
//!   exact equivalent `delta L / dt` of the step, reported for instruments and
//!   for a consumer that must not integrate it a second time.

use cs_types::space::Quaternion;

use super::model::{BODY_FORWARD, BODY_RIGHT, BODY_UP};
use super::tuning::ModelKind;

// ------------------------------------------------------------- constants ---

/// The provenance label of every equation and default in this module.
///
/// Static analysis of the owner's decrypted image; never `verified_original`.
pub const PROVENANCE_LABEL: &str = "OWNER-STATIC-2026-10-08";

/// Metres per foot, as the original writes it (`3.2808399`).
pub const METERS_TO_FEET: f64 = 3.2808399;

/// The original turns a force into an acceleration with `a = F * 9.82 / W`
/// (`0x48ff88`).
pub const FORCE_TO_ACCEL: f64 = 9.82;

/// The largest step the law accepts, in seconds (`0x491c60` clamps at 1/8 s).
pub const MAX_STEP_S: f64 = 0.125;

/// Throttle slew rate per second: the actual throttle moves at most `0.5` per
/// second toward the command.
pub const THROTTLE_SLEW_PER_S: f64 = 0.5;

/// Player fuel burned per second at full throttle (`dt * throttle * 5`).
pub const FUEL_PER_S: f64 = 5.0;

/// The two-layer atmosphere switch, in feet (`0x463640` sets `2000 m` here).
pub const LOW_CEILING_FT: f64 = 6561.68;

/// Lower-layer `k` (`0x41aca0`).
pub const LOW_K: f64 = 0.9884208;
/// Lower-layer density ratio: `rho = LOW_DENSITY_RATIO * SEA_LEVEL_DENSITY`.
pub const LOW_DENSITY_RATIO: f64 = 0.9544815;
/// Upper-layer `k` (`0x41aca0`).
pub const HIGH_K: f64 = 0.7348;
/// Upper-layer density ratio: `rho = HIGH_DENSITY_RATIO * SEA_LEVEL_DENSITY`.
pub const HIGH_DENSITY_RATIO: f64 = 0.0570481;
/// Sea-level density in slugs per cubic foot (`0x41aca0`).
pub const SEA_LEVEL_DENSITY: f64 = 0.002377;
/// `a_ft = (k + 1) * 558` in the atmosphere routine (`0x41aca0`).
pub const SOUND_SPEED_SCALE: f64 = 558.0;

/// Below this speed the drag term is not applied at all (`0x41ada0`), m/s.
pub const DRAG_MIN_SPEED_MPS: f64 = 0.1;

/// Below this speed the demanded load factor is zero, m/s (`8 ft/s` in the
/// original, which is what the constant spells).
pub const LIFT_MIN_SPEED_MPS: f64 = 2.4384;

/// The bank coupling constant `fall_off` (`0x6289f8`).
pub const FALL_OFF: f64 = 0.205;
/// The bank coupling constant `bank_off` (`0x6289fc`).
pub const BANK_OFF: f64 = 0.165;

/// The `level_off_rate` default the image's default table (`0x478a00`)
/// gives: no `vehicle.zrd` record states the key, so the importer supplies
/// this value.
pub const DEFAULT_LEVEL_OFF_RATE: f64 = 4.0;

/// Nitro replaces the throttle with this thrust factor.
pub const NITRO_THROTTLE: f64 = 1.8;
/// Nitro multiplies drag by this factor.
pub const NITRO_DRAG_FACTOR: f64 = 0.8;

/// AI aircraft never fly slower than this nose-ward, m/s (`4 mph`).
pub const AI_MIN_FORWARD_SPEED_MPS: f64 = 4.47;

/// The `player.zrd` speed keys are stored in miles per hour (`0x4735b0`).
pub const MPH_TO_MPS: f64 = 0.44704;

/// The control fades' speeds in degrees, as `liftAOAs` states them.
pub const LIFT_AOA_DEG: [f64; 2] = [5.0, 9.0];

/// `Gcmd` is clamped into this window before it becomes a coefficient.
pub const LOAD_COMMAND_WINDOW: [f64; 2] = [-5.0, 9.0];
/// The lift coefficient is clamped into this window.
pub const LIFT_COEFFICIENT_WINDOW: [f64; 2] = [-1.8, 1.8];

/// The dev tool at `0x491c60` measures a top speed with the attitude held;
/// [`OriginalInput::hold_attitude`] reproduces that.
pub const HOLD_ATTITUDE_TOOL_VA: &str = "0x491c60";

// --------------------------------------------------------------- errors ---

/// Why a parameter record was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum OriginalParamsError {
    /// The flat record named a field this law does not consume.
    UnknownField {
        /// The undeclared name.
        name: String,
    },
    /// The flat record stated the same field twice.
    DuplicateField {
        /// The repeated name.
        name: String,
    },
    /// A required field was not stated at all.
    MissingField {
        /// The absent field.
        name: &'static str,
    },
    /// A stated value was NaN or infinite.
    NonFinite {
        /// The offending field.
        name: &'static str,
    },
    /// A value the law divides by was not positive.
    NonPositive {
        /// The offending field.
        name: &'static str,
        /// The refused value.
        value: f64,
    },
}

impl std::fmt::Display for OriginalParamsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownField { name } => {
                write!(f, "the original flight law does not consume {name}")
            }
            Self::DuplicateField { name } => write!(f, "the field {name} was stated twice"),
            Self::MissingField { name } => write!(f, "the required field {name} is missing"),
            Self::NonFinite { name } => write!(f, "the field {name} must be finite"),
            Self::NonPositive { name, value } => {
                write!(f, "the field {name} must be positive, got {value}")
            }
        }
    }
}

impl std::error::Error for OriginalParamsError {}

/// Why one step was refused.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum OriginalStepError {
    /// The timestep was non-finite, non-positive or above [`MAX_STEP_S`].
    Timestep {
        /// The refused step, in seconds.
        dt_s: f64,
    },
    /// A state or input field was not finite.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// The attitude could not be rebuilt from its own integration.
    Attitude,
}

impl std::fmt::Display for OriginalStepError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Timestep { dt_s } => {
                write!(f, "a step must be in (0, {MAX_STEP_S}], got {dt_s}")
            }
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::Attitude => write!(f, "the integrated attitude is not a unit quaternion"),
        }
    }
}

impl std::error::Error for OriginalStepError {}

// ----------------------------------------------------------- parameters ---

/// The flat field names the per-airframe record is built from.
pub const AIRFRAME_FIELDS: [&str; 15] = [
    "roll_torque",
    "pitch_torque",
    "rudder_torque",
    "level_off_rate",
    "return_rate",
    "ang_momentum_damp",
    "rec_moments_inertia_x",
    "rec_moments_inertia_y",
    "rec_moments_inertia_z",
    "fd_speed",
    "engine_factor",
    "drag_factor",
    "veh_weight",
    "ref_area",
    "gravity",
];

/// The flat field names the global (`player.zrd`) record is built from.
///
/// Speed keys keep the document's miles per hour in their name
/// ([`MPH_TO_MPS`] converts them) and angle keys keep degrees, so no consumer
/// can mistake a stored value's unit.
pub const GLOBAL_FIELDS: [&str; 17] = [
    "nom_gravity",
    "lift_aoa_0_deg",
    "lift_aoa_1_deg",
    "max_aoa_deg",
    "high_g_0",
    "high_g_1",
    "low_g_0",
    "low_g_1",
    "lift_accel_rate",
    "stall_mag",
    "turn_fade_in_mph",
    "turn_fade_out_mph",
    "yaw_low_speed",
    "yaw_high_speed",
    "yaw_fade_in_mph",
    "yaw_max_mph",
    "yaw_fade_out_mph",
];

/// Reads `fields` against `declared`: every name must be declared, finite and
/// unique, and every declared name must be present, in declaration order.
fn read_fields(
    declared: &[&'static str],
    fields: &[(&'static str, f64)],
) -> Result<Vec<(&'static str, f64)>, OriginalParamsError> {
    let mut seen: Vec<&'static str> = Vec::with_capacity(fields.len());
    for &(name, value) in fields {
        if !declared.contains(&name) {
            return Err(OriginalParamsError::UnknownField {
                name: name.to_owned(),
            });
        }
        if seen.contains(&name) {
            return Err(OriginalParamsError::DuplicateField {
                name: name.to_owned(),
            });
        }
        seen.push(name);
        if !value.is_finite() {
            let field = declared
                .iter()
                .copied()
                .find(|field| *field == name)
                .expect("a declared field name");
            return Err(OriginalParamsError::NonFinite { name: field });
        }
    }
    let mut values = Vec::with_capacity(declared.len());
    for &name in declared {
        let entry = fields
            .iter()
            .find(|entry| entry.0 == name)
            .ok_or(OriginalParamsError::MissingField { name })?;
        values.push((name, entry.1));
    }
    Ok(values)
}

/// One per-airframe parameter set: the `vehicle.zrd` record's `dynamics`
/// values, its engine factor and its gravity, already resolved through the
/// `kind_of` inheritance chain by `cs_content::original_airframe`.
///
/// Values arrive flat so the provenance-carrying importer and this consumer
/// share a vocabulary without either crate depending on the other
/// (`docs/01-ARCHITECTURE.md`: `cs_sim` may not depend on `cs_content`).
/// [`OriginalAirframe::from_values`] refuses an unknown, missing, duplicated,
/// non-finite or — for the three values a non-positive one would break the
/// law (`veh_weight` and `ref_area` are divisors, a non-positive damping
/// rate would turn decay into growth) — non-positive field by name: nothing
/// is clamped and no default is invented here.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalAirframe {
    /// Roll `delta L` gain.
    pub roll_torque: f64,
    /// Pitch `delta L` gain.
    pub pitch_torque: f64,
    /// Rudder `delta L` gain.
    pub rudder_torque: f64,
    /// Level-Off assist gain. No record states it, so the image default
    /// [`DEFAULT_LEVEL_OFF_RATE`] is what the importer supplies.
    pub level_off_rate: f64,
    /// Weathervane gain.
    pub return_rate: f64,
    /// Angular damping, in `1/s`.
    pub ang_momentum_damp: f64,
    /// `1 / I` per body axis: `x` pitch, `y` yaw, `z` roll.
    pub rec_moments_inertia: [f64; 3],
    /// Fake-dynamics speed, m/s.
    pub fd_speed: f64,
    /// The `engines.zrd` factor for this record's engine id.
    pub engine_factor: f64,
    /// The drag factor.
    pub drag_factor: f64,
    /// Weight `W` in the units `q * S` uses (lb).
    pub veh_weight: f64,
    /// Reference area `S`, in square feet.
    pub ref_area: f64,
    /// Gravity `g`, m/s^2. No record states it; the importer supplies the
    /// `player.zrd` `nom_gravity`.
    pub gravity: f64,
}

impl OriginalAirframe {
    /// Builds the record from a flat field list.
    ///
    /// # Errors
    ///
    /// [`OriginalParamsError`] naming the first problem: an undeclared or
    /// repeated field, a missing field, a non-finite value, or a non-positive
    /// weight, reference area or damping rate.
    pub fn from_values(fields: &[(&'static str, f64)]) -> Result<Self, OriginalParamsError> {
        let values = read_fields(&AIRFRAME_FIELDS, fields)?;
        let get = |name: &str| -> f64 {
            values
                .iter()
                .find(|entry| entry.0 == name)
                .map(|entry| entry.1)
                .expect("read_fields returned every declared field")
        };
        for name in ["veh_weight", "ref_area", "ang_momentum_damp"] {
            let value = get(name);
            if value <= 0.0 {
                return Err(OriginalParamsError::NonPositive { name, value });
            }
        }
        Ok(Self {
            roll_torque: get("roll_torque"),
            pitch_torque: get("pitch_torque"),
            rudder_torque: get("rudder_torque"),
            level_off_rate: get("level_off_rate"),
            return_rate: get("return_rate"),
            ang_momentum_damp: get("ang_momentum_damp"),
            rec_moments_inertia: [
                get("rec_moments_inertia_x"),
                get("rec_moments_inertia_y"),
                get("rec_moments_inertia_z"),
            ],
            fd_speed: get("fd_speed"),
            engine_factor: get("engine_factor"),
            drag_factor: get("drag_factor"),
            veh_weight: get("veh_weight"),
            ref_area: get("ref_area"),
            gravity: get("gravity"),
        })
    }

    /// The `model_kind` this law is declared as.
    pub const fn model_kind() -> ModelKind {
        ModelKind::OriginalFixedWing
    }
}

/// The global flight constants: the first `player.zrd` entry's `0x4735b0`
/// block, in the document's own units (see [`GLOBAL_FIELDS`]).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalGlobals {
    /// Default gravity, m/s^2 (`nom_gravity`).
    pub nom_gravity: f64,
    /// The two lift-steering blend angles, degrees (`liftAOAs`).
    pub lift_aoa_deg: [f64; 2],
    /// The authority-limit angle of attack, degrees (`maxAOA`).
    pub max_aoa_deg: f64,
    /// The high-G authority window (`highGs`).
    pub high_g: [f64; 2],
    /// The low-G authority window (`lowGs`).
    pub low_g: [f64; 2],
    /// Velocity-steering gain, `1/s` (`lift_accel_rate`).
    pub lift_accel_rate: f64,
    /// Stall nose-drop magnitude (`stall_mag`).
    pub stall_mag: f64,
    /// The roll/pitch authority fade speeds, mph (`turn_fade_in`/`_out`).
    pub turn_fade_mph: [f64; 2],
    /// Rudder authority below the fade-in speed (`yaw_low_speed`).
    pub yaw_low_speed: f64,
    /// Rudder authority above the fade-out speed (`yaw_high_speed`).
    pub yaw_high_speed: f64,
    /// The rudder authority speeds, mph (`yaw_fade_in`/`yaw_max`/`yaw_fade_out`).
    pub yaw_fade_mph: [f64; 3],
}

impl OriginalGlobals {
    /// Builds the globals from a flat field list.
    ///
    /// # Errors
    ///
    /// [`OriginalParamsError`] naming the first problem, exactly as
    /// [`OriginalAirframe::from_values`] does.
    pub fn from_values(fields: &[(&'static str, f64)]) -> Result<Self, OriginalParamsError> {
        let values = read_fields(&GLOBAL_FIELDS, fields)?;
        let get = |name: &str| -> f64 {
            values
                .iter()
                .find(|entry| entry.0 == name)
                .map(|entry| entry.1)
                .expect("read_fields returned every declared field")
        };
        Ok(Self {
            nom_gravity: get("nom_gravity"),
            lift_aoa_deg: [get("lift_aoa_0_deg"), get("lift_aoa_1_deg")],
            max_aoa_deg: get("max_aoa_deg"),
            high_g: [get("high_g_0"), get("high_g_1")],
            low_g: [get("low_g_0"), get("low_g_1")],
            lift_accel_rate: get("lift_accel_rate"),
            stall_mag: get("stall_mag"),
            turn_fade_mph: [get("turn_fade_in_mph"), get("turn_fade_out_mph")],
            yaw_low_speed: get("yaw_low_speed"),
            yaw_high_speed: get("yaw_high_speed"),
            yaw_fade_mph: [
                get("yaw_fade_in_mph"),
                get("yaw_max_mph"),
                get("yaw_fade_out_mph"),
            ],
        })
    }
}

/// Which of the original's two integrators an airframe uses.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DynamicsKind {
    /// Steps 2–8: the full aero and rotation law.
    Full,
    /// Step 9: `a = fd_speed * throttle * nose - v`. `is_autogyro` selects it
    /// in `vehicle.zrd`; the "far AI" distance that also selects it was not
    /// recovered, so a runtime caller declares it.
    Fake,
}

// ----------------------------------------------------------------- state ---

/// One airframe's kinematic state under this law.
///
/// The law owns pose and velocity while `ModelKind::OriginalFixedWing` is
/// active (see the module docs on the contract mapping).
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalState {
    /// World position, metres. `y` is altitude.
    pub position_m: [f64; 3],
    /// World velocity, m/s.
    pub velocity_mps: [f64; 3],
    /// Body-to-world attitude.
    pub orientation: Quaternion,
    /// World-space angular momentum `L`, in the law's own units
    /// (`omega = R^T * diag(recI) * R * L`).
    pub angular_momentum_world: [f64; 3],
    /// The actual (slewed) throttle, `[0, 1]`.
    pub throttle: f64,
    /// Remaining player fuel. AI aircraft are not burned (the source files
    /// give them no fuel load), so `0.0` is fine for them; `0.0` freezes a
    /// player's throttle.
    pub fuel: f64,
    /// The Level-Off assist toggle (Shift+L, command 47).
    pub level_off: bool,
}

impl OriginalState {
    /// A state at `altitude_m` with attitude `orientation`, at rest, with the
    /// throttle already at `throttle` and `fuel` remaining.
    ///
    /// The original's spawn speed was not recovered (the findings list it as
    /// unknown), so it is the caller's declared initial condition rather than
    /// something this law invents.
    #[must_use]
    pub fn at(altitude_m: f64, orientation: Quaternion, throttle: f64, fuel: f64) -> Self {
        Self {
            position_m: [0.0, altitude_m, 0.0],
            velocity_mps: [0.0, 0.0, 0.0],
            orientation,
            angular_momentum_world: [0.0, 0.0, 0.0],
            throttle,
            fuel,
            level_off: false,
        }
    }

    /// Validates every field the law reads.
    ///
    /// # Errors
    ///
    /// [`OriginalStepError::NonFinite`] naming the first offending field.
    pub fn validate(&self) -> Result<(), OriginalStepError> {
        for (name, value) in [
            ("state.position_m.x", self.position_m[0]),
            ("state.position_m.y", self.position_m[1]),
            ("state.position_m.z", self.position_m[2]),
            ("state.velocity_mps.x", self.velocity_mps[0]),
            ("state.velocity_mps.y", self.velocity_mps[1]),
            ("state.velocity_mps.z", self.velocity_mps[2]),
            (
                "state.angular_momentum_world.x",
                self.angular_momentum_world[0],
            ),
            (
                "state.angular_momentum_world.y",
                self.angular_momentum_world[1],
            ),
            (
                "state.angular_momentum_world.z",
                self.angular_momentum_world[2],
            ),
            ("state.throttle", self.throttle),
            ("state.fuel", self.fuel),
        ] {
            if !value.is_finite() {
                return Err(OriginalStepError::NonFinite { field: name });
            }
        }
        Ok(())
    }
}

/// One frame's commands, clamped to their declared ranges before use
/// (`FLIGHT-PHYSICS`: "Clamp human controls to their declared ranges").
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OriginalInput {
    /// Roll command in `[-1, 1]`.
    pub roll: f64,
    /// Pitch command in `[-1, 1]`.
    pub pitch: f64,
    /// Rudder command in `[-1, 1]`.
    pub yaw: f64,
    /// Throttle command in `[0, 1]`.
    pub throttle: f64,
    /// Nitro: the throttle is replaced by [`NITRO_THROTTLE`] and drag is
    /// scaled by [`NITRO_DRAG_FACTOR`]. Its consumption was not recovered.
    pub nitro: bool,
    /// Engine out: thrust is zero.
    pub engine_out: bool,
    /// The player law (the blend and the weathervane) rather than the AI one.
    pub is_player: bool,
    /// Hold the attitude fixed while the forces still integrate: the dev tool
    /// [`HOLD_ATTITUDE_TOOL_VA`] measures a top speed this way. Production
    /// flight leaves it `false`.
    pub hold_attitude: bool,
}

impl Default for OriginalInput {
    fn default() -> Self {
        Self {
            roll: 0.0,
            pitch: 0.0,
            yaw: 0.0,
            throttle: 0.0,
            nitro: false,
            engine_out: false,
            is_player: true,
            hold_attitude: false,
        }
    }
}

impl OriginalInput {
    /// A full-throttle, hands-off input for the given pilot type.
    #[must_use]
    pub fn full_throttle(is_player: bool) -> Self {
        Self {
            throttle: 1.0,
            is_player,
            ..Self::default()
        }
    }

    fn clamped(self) -> Self {
        Self {
            roll: clamp(self.roll, -1.0, 1.0),
            pitch: clamp(self.pitch, -1.0, 1.0),
            yaw: clamp(self.yaw, -1.0, 1.0),
            throttle: clamp(self.throttle, 0.0, 1.0),
            ..self
        }
    }
}

// ------------------------------------------------------------ atmosphere ---

/// The two-layer atmosphere the original uses.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Atmosphere {
    /// The `k` of the layer.
    pub k: f64,
    /// Density, slugs per cubic foot.
    pub rho: f64,
    /// `a_ft = (k + 1) * 558`, the layer's speed-of-sound term in ft/s.
    pub sound_speed_ftps: f64,
}

/// The atmosphere at `altitude_m`: a **hard ceiling** at [`LOW_CEILING_FT`]
/// with no interpolation (`0x41aca0`).
///
/// Below it `k = `[`LOW_K`] and `rho = `[`LOW_DENSITY_RATIO`] times
/// [`SEA_LEVEL_DENSITY`]; above it [`HIGH_K`] and [`HIGH_DENSITY_RATIO`].
#[must_use]
pub fn atmosphere(altitude_m: f64) -> Atmosphere {
    let altitude_ft = altitude_m * METERS_TO_FEET;
    let (k, ratio) = if altitude_ft <= LOW_CEILING_FT {
        (LOW_K, LOW_DENSITY_RATIO)
    } else {
        (HIGH_K, HIGH_DENSITY_RATIO)
    };
    Atmosphere {
        k,
        rho: ratio * SEA_LEVEL_DENSITY,
        sound_speed_ftps: (k + 1.0) * SOUND_SPEED_SCALE,
    }
}

/// The thrust coefficient `Tc` (`0x41acf0`), with `mach_prime` already floored
/// at `0.1` by the caller.
///
/// It carries the units of pressure (lb/ft^2): the step multiplies it by
/// throttle, engine factor and `S`.
#[must_use]
pub fn thrust_coefficient(mach_prime: f64, atmosphere: &Atmosphere) -> f64 {
    let scaled_speed = (0.84 * mach_prime + 0.112) * atmosphere.sound_speed_ftps;
    let numerator =
        0.73 * (0.12 - mach_prime / 60.0) * (0.5 * atmosphere.rho * scaled_speed * scaled_speed);
    let denominator = mach_prime * (1.33 * atmosphere.k).powf(1.41 * mach_prime);
    numerator / denominator
}

/// The wave drag coefficient `CD = 0.73 * (0.12 + 0.8 M + 0.5 M^2)`
/// (`0x41ada0`).
#[must_use]
pub fn drag_coefficient(mach: f64) -> f64 {
    0.73 * (0.12 + 0.8 * mach + 0.5 * mach * mach)
}

/// The lift-coefficient ceiling `CLmax = 0.75 - 0.15 M`.
#[must_use]
pub fn max_lift_coefficient(mach: f64) -> f64 {
    0.75 - 0.15 * mach
}

/// Mach from a speed in m/s under a layer: `M = v / (a_ft * 0.3048)`, which is
/// the same number as `v_ft / a_ft` because `3.2808399` and `0.3048` are
/// reciprocals.
#[must_use]
pub fn mach_from_speed(speed_mps: f64, atmosphere: &Atmosphere) -> f64 {
    (speed_mps * METERS_TO_FEET) / atmosphere.sound_speed_ftps
}

/// The body's nose direction in world space (`-Z`).
#[must_use]
pub fn nose_direction(orientation: Quaternion) -> [f64; 3] {
    rotate_vector(BODY_FORWARD, orientation)
}

// ---------------------------------------------------------- the lift law ---

/// Step 5's target velocity: the player blends between the two `liftAOAs`,
/// AI always aims along the nose.
///
/// * at or below `liftAOAs[0]` the target is the current velocity (nothing is
///   demanded);
/// * at or above `liftAOAs[1]` the target is `|v|` along the nose;
/// * in between it is the linear blend in `cos(alpha)` with
///   `t = (cos a0 - cos alpha) / (cos a0 - cos a1)`.
#[must_use]
pub fn lift_target(
    nose: [f64; 3],
    velocity: [f64; 3],
    lift_aoa_deg: [f64; 2],
    is_player: bool,
) -> [f64; 3] {
    let speed = norm(velocity);
    if !is_player {
        return scale(nose, speed);
    }
    let cosine = if speed > 0.0 {
        clamp(dot(nose, unit(velocity)), -1.0, 1.0)
    } else {
        1.0
    };
    let low = lift_aoa_deg[0].to_radians().cos();
    let high = lift_aoa_deg[1].to_radians().cos();
    if cosine >= low {
        return velocity;
    }
    if cosine <= high {
        return scale(nose, speed);
    }
    let blend = (low - cosine) / (low - high);
    add(scale(nose, speed * blend), scale(velocity, 1.0 - blend))
}

// ------------------------------------------------------------ vector math ---

fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
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

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn scale(a: [f64; 3], factor: f64) -> [f64; 3] {
    [a[0] * factor, a[1] * factor, a[2] * factor]
}

fn norm(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

fn unit(a: [f64; 3]) -> [f64; 3] {
    let length = norm(a);
    if length <= f64::EPSILON {
        [0.0, 0.0, 0.0]
    } else {
        scale(a, 1.0 / length)
    }
}

fn clamp(value: f64, min: f64, max: f64) -> f64 {
    if value < min {
        min
    } else if value > max {
        max
    } else {
        value
    }
}

/// Body-to-world rotation of a vector by a unit quaternion `(x, y, z, w)`.
fn rotate_vector(vector: [f64; 3], orientation: Quaternion) -> [f64; 3] {
    let [x, y, z, w] = orientation.components();
    let axis = [x, y, z];
    let axis_cross = cross(axis, vector);
    let twice = scale(axis_cross, 2.0 * w);
    let second = cross(axis, axis_cross);
    add(add(vector, twice), scale(second, 2.0))
}

/// The unit quaternion for a right-hand rotation of `angle` about `axis`.
fn axis_angle(axis: [f64; 3], angle: f64) -> Option<[f64; 4]> {
    let axis = unit(axis);
    if norm(axis) <= f64::EPSILON || !angle.is_finite() {
        return None;
    }
    let half = angle * 0.5;
    let (sin, cos) = half.sin_cos();
    let raw = [axis[0] * sin, axis[1] * sin, axis[2] * sin, cos];
    let length = (raw[0] * raw[0] + raw[1] * raw[1] + raw[2] * raw[2] + raw[3] * raw[3]).sqrt();
    if !length.is_finite() || length <= f64::EPSILON {
        return None;
    }
    Some([
        raw[0] / length,
        raw[1] / length,
        raw[2] / length,
        raw[3] / length,
    ])
}

/// Hamilton product `a * b` of two `(x, y, z, w)` quaternions.
fn quaternion_product(a: [f64; 4], b: [f64; 4]) -> [f64; 4] {
    let [ax, ay, az, aw] = a;
    let [bx, by, bz, bw] = b;
    [
        aw * bx + ax * bw + ay * bz - az * by,
        aw * by - ax * bz + ay * bw + az * bx,
        aw * bz + ax * by - ay * bx + az * bw,
        aw * bw - ax * bx - ay * by - az * bz,
    ]
}

/// Rotates `vector` by the rotation vector `rotation` (axis times angle).
fn rotate_by(vector: [f64; 3], rotation: [f64; 3]) -> [f64; 3] {
    let angle = norm(rotation);
    if angle <= f64::EPSILON {
        return vector;
    }
    let axis = scale(rotation, 1.0 / angle);
    let cos = angle.cos();
    let sin = angle.sin();
    add(
        add(scale(vector, cos), scale(cross(axis, vector), sin)),
        scale(axis, dot(axis, vector) * (1.0 - cos)),
    )
}

/// The half-angle rotation vector that carries `from` onto `to`:
/// `axis * (angle / 2)`, the vector part of the corresponding quaternion.
///
/// Parallel vectors give the zero vector; antiparallel ones use `fallback`.
fn half_angle_rotation(from: [f64; 3], to: [f64; 3], fallback: [f64; 3]) -> [f64; 3] {
    let from = unit(from);
    let to = unit(to);
    let sine = cross(from, to);
    let cosine = clamp(dot(from, to), -1.0, 1.0);
    let sine_length = norm(sine);
    if sine_length <= f64::EPSILON {
        if cosine > 0.0 {
            return [0.0, 0.0, 0.0];
        }
        return scale(unit(fallback), std::f64::consts::FRAC_PI_4);
    }
    let axis = scale(sine, 1.0 / sine_length);
    scale(axis, sine_length.atan2(cosine) * 0.5)
}

// ------------------------------------------------------------- the model ---

/// What one step did, for probes, instruments and the contract's
/// force/torque boundary.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalStep {
    /// The total linear acceleration the law produced, m/s^2, gravity
    /// included.
    pub linear_acceleration_mps2: [f64; 3],
    /// The equivalent world force `(W / 9.82) * a`. A consumer that hands this
    /// to a rigid body must disable its own gravity **and** its own drag
    /// (module docs, "Mapping onto the shared contract").
    pub world_force: [f64; 3],
    /// The exact equivalent world torque `delta L / dt` of this step. It is
    /// reported, not integrated twice: this law owns attitude.
    pub world_torque: [f64; 3],
    /// Thrust magnitude in the law's force units (0 under fake dynamics).
    pub thrust: f64,
    /// Drag magnitude (0 under fake dynamics or below the drag floor).
    pub drag: f64,
    /// Lift magnitude (0 under fake dynamics).
    pub lift: f64,
    /// Load factor `n = q * S * CL * n_y / W`.
    pub load_factor: f64,
    /// Angle of attack, radians, between the nose and the velocity.
    pub attack_rad: f64,
    /// The lift coefficient actually used.
    pub lift_coefficient: f64,
    /// Dynamic pressure `q`, lb/ft^2.
    pub dynamic_pressure: f64,
    /// Mach at the start of the step.
    pub mach: f64,
    /// The atmosphere the step used.
    pub atmosphere: Atmosphere,
    /// The angular velocity `omega` the step integrated, rad/s, world frame.
    pub angular_velocity_radps: [f64; 3],
    /// The actual throttle after this step's slew.
    pub throttle: f64,
    /// Fuel after this step's burn (players only).
    pub fuel: f64,
    /// Whether the AI minimum forward speed clamped the velocity.
    pub speed_floor_applied: bool,
}

/// The forces of one step, before gravity and before the `F * 9.82 / W` scale.
#[derive(Clone, Copy, Debug, PartialEq)]
struct AeroStep {
    thrust: f64,
    drag: f64,
    lift: [f64; 3],
    load_factor: f64,
    lift_coefficient: f64,
    attack_rad: f64,
    mach: f64,
    dynamic_pressure: f64,
}

/// The model: one airframe's parameters, the globals and the integrator kind.
#[derive(Clone, Debug, PartialEq)]
pub struct OriginalFlightModel {
    /// The per-airframe parameters.
    pub airframe: OriginalAirframe,
    /// The `player.zrd` globals.
    pub globals: OriginalGlobals,
    /// Which integrator this airframe uses.
    pub dynamics: DynamicsKind,
}

impl OriginalFlightModel {
    /// The full-law model for `airframe` under `globals`.
    #[must_use]
    pub fn full(airframe: OriginalAirframe, globals: OriginalGlobals) -> Self {
        Self {
            airframe,
            globals,
            dynamics: DynamicsKind::Full,
        }
    }

    /// Advances the state by `dt_s`.
    ///
    /// # Errors
    ///
    /// [`OriginalStepError::Timestep`] outside `(0, MAX_STEP_S]`,
    /// [`OriginalStepError::NonFinite`] for a non-finite state field, and
    /// [`OriginalStepError::Attitude`] if the integrated attitude is not a
    /// unit quaternion.
    pub fn step(
        &self,
        state: &mut OriginalState,
        input: OriginalInput,
        dt_s: f64,
    ) -> Result<OriginalStep, OriginalStepError> {
        if !dt_s.is_finite() || dt_s <= 0.0 || dt_s > MAX_STEP_S {
            return Err(OriginalStepError::Timestep { dt_s });
        }
        state.validate()?;
        let input = input.clamped();
        let airframe = &self.airframe;

        // 1. Throttle slew and player fuel.
        let frozen = input.is_player && state.fuel <= 0.0;
        if !frozen {
            state.throttle = slew(state.throttle, input.throttle, THROTTLE_SLEW_PER_S * dt_s);
        }
        if input.is_player && !frozen {
            state.fuel -= dt_s * state.throttle * FUEL_PER_S;
        }
        let throttle = state.throttle;

        // Body axes in world space.
        let orientation = state.orientation;
        let right = rotate_vector(BODY_RIGHT, orientation);
        let up = rotate_vector(BODY_UP, orientation);
        let back = rotate_vector([0.0, 0.0, 1.0], orientation);
        let nose = scale(back, -1.0);

        let velocity = state.velocity_mps;
        let speed = norm(velocity);
        let velocity_hat = if speed > 0.0 {
            scale(velocity, 1.0 / speed)
        } else {
            [0.0, 0.0, 0.0]
        };

        let atmosphere = atmosphere(state.position_m[1]);
        let mach = mach_from_speed(speed, &atmosphere);
        let dynamic_pressure = 0.5 * atmosphere.rho * (speed * METERS_TO_FEET).powi(2);

        // Steps 2-6, or step 9's fake dynamics.
        let (linear_acceleration_mps2, aero) = if self.dynamics == DynamicsKind::Fake {
            let target = scale(nose, airframe.fd_speed * throttle);
            (
                sub(target, velocity),
                AeroStep {
                    thrust: 0.0,
                    drag: 0.0,
                    lift: [0.0, 0.0, 0.0],
                    load_factor: 0.0,
                    lift_coefficient: 0.0,
                    attack_rad: angle_between(nose, velocity),
                    mach,
                    dynamic_pressure,
                },
            )
        } else {
            let aero = self.aero(
                input,
                throttle,
                &velocity,
                [right, up, back],
                nose,
                mach,
                dynamic_pressure,
                atmosphere,
            );
            let total_force = add(
                add(scale(nose, aero.thrust), scale(velocity_hat, -aero.drag)),
                aero.lift,
            );
            let acceleration = add(
                scale(total_force, FORCE_TO_ACCEL / airframe.veh_weight),
                [0.0, -airframe.gravity, 0.0],
            );
            (acceleration, aero)
        };

        // 7. Rotation: accumulate `delta L`, damp, integrate `2 |omega| dt`.
        let momentum_before = state.angular_momentum_world;
        let (angular_velocity_radps, world_torque) = if input.hold_attitude {
            ([0.0, 0.0, 0.0], [0.0, 0.0, 0.0])
        } else {
            let delta = self.rotation_increment(
                input,
                state.level_off,
                &aero,
                speed,
                [right, up, back],
                nose,
                velocity_hat,
                dt_s,
            );
            let damped = scale(
                add(momentum_before, delta),
                (-airframe.ang_momentum_damp * dt_s).exp(),
            );
            let omega = angular_velocity_world(damped, orientation, airframe);
            state.orientation = integrate_attitude(orientation, omega, dt_s)?;
            state.angular_momentum_world = damped;
            let torque = scale(sub(damped, momentum_before), 1.0 / dt_s);
            (omega, torque)
        };

        // 8. Semi-implicit integration.
        state.velocity_mps = add(velocity, scale(linear_acceleration_mps2, dt_s));
        let mut speed_floor_applied = false;
        if !input.is_player && self.dynamics == DynamicsKind::Full {
            let forward_speed = dot(state.velocity_mps, nose);
            if forward_speed < AI_MIN_FORWARD_SPEED_MPS {
                state.velocity_mps = add(
                    state.velocity_mps,
                    scale(nose, AI_MIN_FORWARD_SPEED_MPS - forward_speed),
                );
                speed_floor_applied = true;
            }
        }
        state.position_m = add(state.position_m, scale(state.velocity_mps, dt_s));

        Ok(OriginalStep {
            linear_acceleration_mps2,
            world_force: scale(
                linear_acceleration_mps2,
                airframe.veh_weight / FORCE_TO_ACCEL,
            ),
            world_torque,
            thrust: aero.thrust,
            drag: aero.drag,
            lift: norm(aero.lift),
            load_factor: aero.load_factor,
            attack_rad: aero.attack_rad,
            lift_coefficient: aero.lift_coefficient,
            dynamic_pressure,
            mach,
            atmosphere,
            angular_velocity_radps,
            throttle,
            fuel: state.fuel,
            speed_floor_applied,
        })
    }

    /// Steps 3–6: thrust along the nose, drag against the velocity, the
    /// velocity-steering lift, and the load factor each of them produces.
    ///
    /// `throttle` is the **actual** (slewed, possibly frozen) throttle from
    /// step 1, never the [`OriginalInput::throttle`] command: the command is
    /// only the value the actual one chases at [`THROTTLE_SLEW_PER_S`] per
    /// second, and a frozen throttle (no fuel) is what stops the engine.
    #[allow(clippy::too_many_arguments)]
    fn aero(
        &self,
        input: OriginalInput,
        throttle: f64,
        velocity: &[f64; 3],
        axes: [[f64; 3]; 3],
        nose: [f64; 3],
        mach: f64,
        dynamic_pressure: f64,
        atmosphere: Atmosphere,
    ) -> AeroStep {
        let airframe = &self.airframe;
        let weight = airframe.veh_weight;
        let area = airframe.ref_area;
        let [right, up, back] = axes;

        // 3. Thrust.
        let mach_prime = mach.max(0.1);
        let coefficient = thrust_coefficient(mach_prime, &atmosphere);
        let back_y = back[1];
        let orientation_factor = (if back_y <= 0.0 {
            1.0 + 0.13 * back_y
        } else {
            1.0
        }) * (1.0 + 0.24 * back_y);
        let thrust_throttle = if input.nitro {
            NITRO_THROTTLE
        } else {
            throttle
        };
        let mut thrust =
            coefficient * thrust_throttle * airframe.engine_factor * area * orientation_factor;
        if input.engine_out {
            thrust = 0.0;
        }

        // 4. Drag.
        let mut drag = dynamic_pressure * area * airframe.drag_factor * drag_coefficient(mach);
        if input.nitro {
            drag *= NITRO_DRAG_FACTOR;
        }
        let speed = norm(*velocity);
        if speed <= DRAG_MIN_SPEED_MPS {
            drag = 0.0;
        }

        // 5. Lift is velocity steering.
        let attack = angle_between(nose, *velocity);
        let target = lift_target(nose, *velocity, self.globals.lift_aoa_deg, input.is_player);
        let steer = add(
            scale(sub(target, *velocity), self.globals.lift_accel_rate),
            [0.0, airframe.gravity, 0.0],
        );
        let (ax, ay) = (dot(steer, right), dot(steer, up));
        let magnitude = (ax * ax + ay * ay).sqrt();
        let g_command = if speed <= LIFT_MIN_SPEED_MPS {
            0.0
        } else {
            magnitude / FORCE_TO_ACCEL
        };
        let normal = if magnitude > f64::EPSILON {
            [ax / magnitude, ay / magnitude]
        } else {
            [0.7, 0.7]
        };
        let cl_max = max_lift_coefficient(mach);
        let coefficient = if dynamic_pressure * area > 0.0 {
            let demanded = g_command.clamp(LOAD_COMMAND_WINDOW[0], LOAD_COMMAND_WINDOW[1]) * weight
                / (dynamic_pressure * area);
            demanded
                .clamp(LIFT_COEFFICIENT_WINDOW[0], LIFT_COEFFICIENT_WINDOW[1])
                .min(cl_max)
        } else {
            0.0
        };
        let direction = add(scale(right, normal[0]), scale(up, normal[1]));
        let lift_magnitude = dynamic_pressure * area * coefficient;

        AeroStep {
            thrust,
            drag,
            lift: scale(direction, lift_magnitude),
            load_factor: lift_magnitude * normal[1] / weight,
            lift_coefficient: coefficient,
            attack_rad: attack,
            mach,
            dynamic_pressure,
        }
    }

    /// Steps 7's control terms: every term is already multiplied by `dt` and
    /// is expressed in world space.
    #[allow(clippy::too_many_arguments)]
    fn rotation_increment(
        &self,
        input: OriginalInput,
        level_off: bool,
        aero: &AeroStep,
        speed: f64,
        axes: [[f64; 3]; 3],
        nose: [f64; 3],
        velocity_hat: [f64; 3],
        dt_s: f64,
    ) -> [f64; 3] {
        let airframe = &self.airframe;
        let globals = &self.globals;
        let [right, up, back] = axes;

        let fade_in = globals.turn_fade_mph[0] * MPH_TO_MPS;
        let fade_out = globals.turn_fade_mph[1] * MPH_TO_MPS;
        let fade = ramp(speed, fade_in, fade_out);
        let yaw_authority = yaw_authority(speed, globals);
        let lambda = authority(aero.attack_rad, aero.load_factor, globals);

        let mut increment = [0.0, 0.0, 0.0];

        // Roll: about the body Z (roll) axis, no authority limit.
        increment = add(
            increment,
            scale(back, airframe.roll_torque * input.roll * fade * dt_s),
        );

        // Pitch: about the body X axis, limited when it pushes alpha up.
        let mut pitch = scale(right, airframe.pitch_torque * input.pitch * fade * dt_s);
        if rotation_increases_attack(pitch, nose, velocity_hat) {
            pitch = scale(pitch, lambda);
        }
        increment = add(increment, pitch);

        // Yaw: about the body Y axis, same limit.
        let mut yaw = scale(
            up,
            airframe.rudder_torque * input.yaw * yaw_authority * dt_s,
        );
        if rotation_increases_attack(yaw, nose, velocity_hat) {
            yaw = scale(yaw, lambda);
        }
        increment = add(increment, yaw);

        // Bank coupling (`fall_off` 0x6289f8, `bank_off` 0x6289fc).
        increment = add(increment, scale(up, FALL_OFF * right[1] * dt_s));
        let inverted_pitch = if up[1] < 0.0 { FALL_OFF * up[1] } else { 0.0 };
        increment = add(
            increment,
            scale(right, (BANK_OFF * right[1].abs() - inverted_pitch) * dt_s),
        );

        // Player weathervane: the half-angle from the nose to the velocity.
        if input.is_player && norm(velocity_hat) > 0.0 {
            increment = add(
                increment,
                scale(
                    half_angle_rotation(nose, velocity_hat, right),
                    airframe.return_rate * dt_s,
                ),
            );
        }

        // Level-Off assist (Shift+L, command 47), hands off.
        if level_off && input.roll == 0.0 && input.pitch == 0.0 {
            let mut level = scale(
                half_angle_rotation(up, BODY_UP, right),
                airframe.level_off_rate * dt_s,
            );
            if rotation_increases_attack(level, nose, velocity_hat) {
                level = scale(level, lambda);
            }
            increment = add(increment, level);
        }

        // Stall: strip any nose-up component, then drop the nose.
        let cl_max = max_lift_coefficient(aero.mach);
        let stall_margin =
            1.0 - aero.dynamic_pressure * airframe.ref_area * cl_max.min(1.8) / airframe.veh_weight;
        if stall_margin > 0.0 {
            let nose_up = dot(increment, right);
            if nose_up > 0.0 {
                increment = add(increment, scale(right, -nose_up));
            }
            increment = add(
                increment,
                scale(right, -globals.stall_mag * stall_margin * dt_s),
            );
        }

        increment
    }
}

// ------------------------------------------------------------- utilities ---

fn slew(current: f64, target: f64, step: f64) -> f64 {
    if target < current {
        (current - step).max(target)
    } else {
        (current + step).min(target)
    }
}

fn ramp(value: f64, low: f64, high: f64) -> f64 {
    if high <= low {
        return if value >= high { 1.0 } else { 0.0 };
    }
    clamp((value - low) / (high - low), 0.0, 1.0)
}

fn lerp(low: f64, high: f64, t: f64) -> f64 {
    low + (high - low) * t
}

/// The rudder authority factor `fY`: `yaw_low` below the fade-in speed, up to
/// `1.0` at `yaw_max`, then down to `yaw_high` at `yaw_fade_out`.
fn yaw_authority(speed_mps: f64, globals: &OriginalGlobals) -> f64 {
    let fade_in = globals.yaw_fade_mph[0] * MPH_TO_MPS;
    let maximum = globals.yaw_fade_mph[1] * MPH_TO_MPS;
    let fade_out = globals.yaw_fade_mph[2] * MPH_TO_MPS;
    if speed_mps < fade_in {
        globals.yaw_low_speed
    } else if speed_mps < maximum {
        lerp(
            globals.yaw_low_speed,
            1.0,
            (speed_mps - fade_in) / (maximum - fade_in),
        )
    } else if speed_mps < fade_out {
        lerp(
            1.0,
            globals.yaw_high_speed,
            (speed_mps - maximum) / (fade_out - maximum),
        )
    } else {
        globals.yaw_high_speed
    }
}

/// The angle between two directions, in `[0, pi]`; `0` for a zero vector.
fn angle_between(a: [f64; 3], b: [f64; 3]) -> f64 {
    let la = norm(a);
    let lb = norm(b);
    if la <= f64::EPSILON || lb <= f64::EPSILON {
        return 0.0;
    }
    clamp(dot(a, b) / (la * lb), -1.0, 1.0).acos()
}

/// The authority limit `lambda`: how much of a nose-increasing rotation the
/// law still allows, from the angle of attack and the load factor.
///
/// `lambda = (cos a - cos maxAOA) / (1 - cos maxAOA)` floored at zero, then
/// narrowed by the `highGs` window above `high_g[0]` and the `lowGs` window
/// below `low_g[0]`. The result is floored at zero as well: the sheet states
/// the floor only for the angle-of-attack term, and a negative authority would
/// reverse the pilot's input instead of limiting it.
#[must_use]
pub fn authority(attack_rad: f64, load_factor: f64, globals: &OriginalGlobals) -> f64 {
    let limit = globals.max_aoa_deg.to_radians().cos();
    let mut lambda = (attack_rad.cos() - limit) / (1.0 - limit);
    if lambda < 0.0 {
        lambda = 0.0;
    }
    if load_factor > globals.high_g[0] {
        let span = globals.high_g[1] - globals.high_g[0];
        if span != 0.0 {
            lambda = lambda.min((globals.high_g[1] - load_factor) / span);
        }
    }
    if load_factor < globals.low_g[0] {
        // The low window runs downwards, so its span is negative; the ratio
        // is still positive because both terms are.
        let span = globals.low_g[1] - globals.low_g[0];
        if span != 0.0 {
            lambda = lambda.min((globals.low_g[1] - load_factor) / span);
        }
    }
    if lambda < 0.0 { 0.0 } else { lambda }
}

/// Whether applying `rotation` (a world rotation vector) would push the nose
/// further from the velocity — the condition for scaling a term by
/// [`authority`].
fn rotation_increases_attack(rotation: [f64; 3], nose: [f64; 3], velocity_hat: [f64; 3]) -> bool {
    if norm(rotation) <= f64::EPSILON || norm(velocity_hat) <= f64::EPSILON {
        return false;
    }
    angle_between(rotate_by(nose, rotation), velocity_hat) > angle_between(nose, velocity_hat)
}

/// `omega = R * diag(recI) * R^T * L`: the world angular velocity that goes
/// with the world angular momentum `L`.
fn angular_velocity_world(
    momentum: [f64; 3],
    orientation: Quaternion,
    airframe: &OriginalAirframe,
) -> [f64; 3] {
    let [x, y, z] = rotate_vector(momentum, conjugate(orientation));
    let body = [
        x * airframe.rec_moments_inertia[0],
        y * airframe.rec_moments_inertia[1],
        z * airframe.rec_moments_inertia[2],
    ];
    rotate_vector(body, orientation)
}

fn conjugate(orientation: Quaternion) -> Quaternion {
    let [x, y, z, w] = orientation.components();
    Quaternion::try_new([-x, -y, -z, w]).expect("a conjugate of a unit quaternion is unit")
}

/// The attitude integration: a world-space `omega` rotates the attitude by
/// **`2 * |omega| * dt`** — the original's quaternion exponential does not
/// halve the angle, so steady rates come out about twice the naive value.
fn integrate_attitude(
    orientation: Quaternion,
    omega: [f64; 3],
    dt_s: f64,
) -> Result<Quaternion, OriginalStepError> {
    let rate = norm(omega);
    if rate <= f64::EPSILON {
        return Ok(orientation);
    }
    let delta = axis_angle(scale(omega, 1.0 / rate), 2.0 * rate * dt_s)
        .ok_or(OriginalStepError::Attitude)?;
    let combined = quaternion_product(delta, orientation.components());
    Quaternion::try_new(combined).map_err(|_| OriginalStepError::Attitude)
}

// ----------------------------------------------------------------- tests ---

#[cfg(test)]
mod tests {
    use super::*;

    /// The atmosphere step at `2000 m` and above the ceiling, against the
    /// formulas as the law states them (hand values, not a second call into
    /// the code under test).
    #[test]
    fn accept_flight_original_atmosphere_step_at_2000_m_matches_the_formula() {
        let low = atmosphere(2000.0);
        assert_eq!(low.k, 0.9884208);
        assert_eq!(low.rho, 0.9544815 * 0.002377);
        assert_eq!(low.sound_speed_ftps, (0.9884208 + 1.0) * 558.0);

        // Just above the hard ceiling: no interpolation, a different layer.
        let high = atmosphere(2500.0);
        assert_eq!(high.k, 0.7348);
        assert_eq!(high.rho, 0.0570481 * 0.002377);
        assert_eq!(high.sound_speed_ftps, (0.7348 + 1.0) * 558.0);

        // The switch sits at 2000 m: 6561.6798 ft is at or below it and a
        // hundred metres more is above it, with no interpolation between.
        assert_eq!(atmosphere(1999.9).k, LOW_K);
        assert_eq!(atmosphere(2000.0).k, LOW_K);
        assert_eq!(atmosphere(2000.1).k, HIGH_K);
        assert!((LOW_CEILING_FT / METERS_TO_FEET - 2000.0).abs() < 1.0e-3);
    }

    /// The thrust coefficient, against the formula written out by hand, and
    /// the balance it strikes against drag at the Bloodhawk's top speed.
    #[test]
    fn accept_flight_original_thrust_coefficient_matches_the_formula() {
        let atmosphere = atmosphere(2000.0);
        let mach_prime = 0.1_f64;
        let expected = 0.73
            * (0.12 - mach_prime / 60.0)
            * (0.5
                * atmosphere.rho
                * ((0.84 * mach_prime + 0.112) * atmosphere.sound_speed_ftps).powi(2))
            / (mach_prime * (1.33 * atmosphere.k).powf(1.41 * mach_prime));
        assert_eq!(thrust_coefficient(mach_prime, &atmosphere), expected);

        // Mach at 134.3 m/s under the 2000 m layer, and the balance: the
        // Bloodhawk's thrust (engine 0.62, S 330) equals its drag there.
        let speed = 134.3;
        let mach = mach_from_speed(speed, &atmosphere);
        assert!((mach - 0.39713).abs() < 0.0001, "mach = {mach}");
        let thrust = thrust_coefficient(mach.max(0.1), &atmosphere) * 1.0 * 0.62 * 330.0;
        let drag = 0.5
            * atmosphere.rho
            * (speed * 3.2808399).powi(2)
            * 330.0
            * 0.37
            * drag_coefficient(mach);
        assert!(
            (thrust - drag).abs() / drag < 0.01,
            "T = {thrust} lb, D = {drag} lb at {speed} m/s"
        );
    }

    /// `CD` and `CLmax` are the formulas the law states.
    #[test]
    fn accept_flight_original_drag_and_lift_coefficients_match_the_formulas() {
        for mach in [0.0, 0.1, 0.39713, 1.0, 2.5] {
            assert_eq!(
                drag_coefficient(mach),
                0.73 * (0.12 + 0.8 * mach + 0.5 * mach * mach)
            );
            assert_eq!(max_lift_coefficient(mach), 0.75 - 0.15 * mach);
        }
    }

    /// The lift-steering blend at 3, 7 and 12 degrees: full velocity below the
    /// first `liftAOA`, a linear blend in `cos` between them, the nose
    /// direction above the second.
    #[test]
    fn accept_flight_original_lift_steering_blend_at_3_7_and_12_degrees() {
        let nose = BODY_FORWARD;
        let velocity_at = |degrees: f64| -> [f64; 3] {
            let radians = degrees.to_radians();
            // The nose is -Z; drop the velocity `degrees` below it in Y.
            unit([0.0, -radians.sin(), -radians.cos()])
        };

        // 3 degrees: below `liftAOAs[0]`, so the target is the velocity.
        let velocity = scale(velocity_at(3.0), 50.0);
        assert_eq!(
            lift_target(nose, velocity, LIFT_AOA_DEG, true),
            velocity,
            "at 3 degrees the target is the current velocity"
        );

        // 12 degrees: above `liftAOAs[1]`, so the target is |v| on the nose.
        let velocity = scale(velocity_at(12.0), 50.0);
        let target = lift_target(nose, velocity, LIFT_AOA_DEG, true);
        assert!(
            (norm(target) - 50.0).abs() < 1e-9 && dot(unit(target), nose) > 1.0 - 1e-9,
            "at 12 degrees the target is the nose direction"
        );

        // 7 degrees: the hand-computed blend
        // t = (cos5 - cos7) / (cos5 - cos9), target = t * |v| * nose + (1-t) v.
        let velocity = scale(velocity_at(7.0), 50.0);
        let alpha = 7.0_f64.to_radians();
        let t = (5.0_f64.to_radians().cos() - alpha.cos())
            / (5.0_f64.to_radians().cos() - 9.0_f64.to_radians().cos());
        let expected = add(scale(nose, 50.0 * t), scale(velocity, 1.0 - t));
        let target = lift_target(nose, velocity, LIFT_AOA_DEG, true);
        for axis in 0..3 {
            assert!(
                (target[axis] - expected[axis]).abs() < 1e-12,
                "7 degree blend component {axis}: {} vs {}",
                target[axis],
                expected[axis]
            );
        }
        assert!((t - 0.42891995215091244).abs() < 1e-12, "t = {t}");

        // AI always aims at the nose, whatever the angle.
        let velocity = scale(velocity_at(3.0), 50.0);
        let target = lift_target(nose, velocity, LIFT_AOA_DEG, false);
        assert!((norm(target) - 50.0).abs() < 1e-9);
        assert!(dot(unit(target), nose) > 1.0 - 1e-9);
    }

    /// The authority limit: it falls from 1 at zero angle of attack to 0 at
    /// `maxAOA`, and the high/low G windows narrow it further.
    #[test]
    fn accept_flight_original_authority_limit_narrows_with_attack_and_load() {
        let globals = globals();
        let zero = authority(0.0, 0.0, &globals);
        assert!((zero - 1.0).abs() < 1e-12, "lambda = {zero}");

        let limit = globals.max_aoa_deg.to_radians();
        assert_eq!(authority(limit, 0.0, &globals), 0.0);
        assert_eq!(authority(limit + 0.1, 0.0, &globals), 0.0);

        let middle = authority(limit * 0.5, 0.0, &globals);
        assert!(middle > 0.0 && middle < 1.0, "lambda = {middle}");

        // High G: (15 - n) / 6 above n = 9, reaching zero at 15.
        let mild = authority(0.0, 12.0, &globals);
        assert!(
            (mild - (15.0 - 12.0) / 6.0).abs() < 1e-12,
            "lambda = {mild}"
        );
        assert_eq!(authority(0.0, 15.0, &globals), 0.0);
        assert_eq!(authority(0.0, 16.0, &globals), 0.0);

        // Low G: (-9 - n) / (-3) below n = -6, reaching zero at -9.
        let low = authority(0.0, -7.5, &globals);
        assert!((low - (-9.0 - -7.5) / -3.0).abs() < 1e-12, "lambda = {low}");
        assert_eq!(authority(0.0, -9.0, &globals), 0.0);

        // Under a high load the narrower of the two limits wins.
        let combined = authority(limit * 0.25, 14.0, &globals);
        assert!((combined - (15.0 - 14.0) / 6.0).abs() < 1e-12, "{combined}");
    }

    /// The attitude rotates by `2 * |omega| * dt` per step: the original's
    /// quaternion exponential does not halve the angle.
    #[test]
    fn accept_flight_original_rotation_is_twice_omega_dt() {
        let omega = [0.0, 1.0, 0.0];
        let dt = 0.01;
        let rotated = integrate_attitude(Quaternion::IDENTITY, omega, dt)
            .expect("the integration produces a unit quaternion");
        let [x, y, z, w] = rotated.components();
        let theta = 2.0 * norm(omega) * dt;
        assert!((w - (theta * 0.5).cos()).abs() < 1e-12, "w = {w}");
        assert!((y - (theta * 0.5).sin()).abs() < 1e-12, "y = {y}");
        assert_eq!((x, z), (0.0, 0.0));

        let moved = angle_between(rotate_vector(BODY_FORWARD, rotated), BODY_FORWARD);
        assert!(
            (moved - theta).abs() < 1e-9,
            "the nose moved {moved} rad, expected {theta}"
        );
        assert!(
            (moved - norm(omega) * dt).abs() > 1e-9,
            "and it is not the halved angle"
        );
    }

    /// A held control input settles at about `2 * recI * torque / damp`, the
    /// rate the task states for the Bloodhawk: 89 deg/s pitch, 189 deg/s
    /// roll. The velocity is kept on the nose so the angle of attack stays
    /// zero and the authority limit stays at 1, and the rate is measured about
    /// the commanded body axis so the bank coupling's cross terms are not
    /// counted as roll or pitch.
    #[test]
    fn accept_flight_original_steady_rates_match_the_torque_damping_rule() {
        let model = OriginalFlightModel::full(airframe(), globals());

        // The body axis a held command turns about: X for pitch, Z for roll.
        let axis_of = |pitch: bool| {
            [
                if pitch { 1.0 } else { 0.0 },
                0.0,
                if pitch { 0.0 } else { 1.0 },
            ]
        };

        let steady = |pitch: bool, steps: usize| -> f64 {
            let mut input = OriginalInput::full_throttle(true);
            input.pitch = if pitch { 1.0 } else { 0.0 };
            input.roll = if pitch { 0.0 } else { 1.0 };
            let mut state = OriginalState::at(1000.0, Quaternion::IDENTITY, 1.0, 1e9);
            // Already at cruise: the control fades turn on at 50 mph, so a
            // start from rest would measure the fade, not the damping.
            state.velocity_mps = [0.0, 0.0, -134.0];
            let mut rate = 0.0;
            for _ in 0..steps {
                let step = model
                    .step(&mut state, input, 0.01)
                    .expect("the declared step is valid");
                // Keep the velocity on the nose so the attack stays zero.
                let speed = norm(state.velocity_mps);
                state.velocity_mps = scale(nose_direction(state.orientation), speed);
                let axis = rotate_vector(axis_of(pitch), state.orientation);
                rate = 2.0 * dot(step.angular_velocity_radps, axis);
            }
            rate
        };

        let pitch = steady(true, 100);
        let expected_pitch = 2.0 * 1.18 * 3.3 / 5.0;
        assert!(
            (pitch - expected_pitch).abs() / expected_pitch < 0.05,
            "pitch rate {pitch} rad/s vs {expected_pitch}"
        );
        assert!(
            (pitch.to_degrees() - 89.0).abs() < 5.0,
            "{} deg/s",
            pitch.to_degrees()
        );

        let roll = steady(false, 100);
        let expected_roll = 2.0 * 1.1 * 7.5 / 5.0;
        assert!(
            (roll - expected_roll).abs() / expected_roll < 0.05,
            "roll rate {roll} rad/s vs {expected_roll}"
        );
        assert!(
            (roll.to_degrees() - 189.0).abs() < 10.0,
            "{} deg/s",
            roll.to_degrees()
        );
    }

    /// Thrust follows the **actual** slewed throttle of step 1, never the
    /// command: the command is only the value the actual one chases, and the
    /// freeze at `fuel <= 0` is what stops the engine.
    #[test]
    fn accept_flight_original_thrust_follows_the_actual_throttle() {
        let model = OriginalFlightModel::full(airframe(), globals());
        let input = OriginalInput::full_throttle(true); // command 1.0

        // The thrust the formulas give at `throttle` for a level, still
        // aircraft at ground level (orientation factor 1, `M' = 0.1`).
        let expected = |throttle: f64| -> f64 {
            thrust_coefficient(0.1, &atmosphere(0.0)) * throttle * 0.62 * 330.0
        };

        // The actual throttle starts at 0 and moves at 0.5/s, so after one
        // 0.01 s step it is 0.005 and the thrust must be 0.005's, not the
        // command's.
        let mut state = OriginalState::at(0.0, Quaternion::IDENTITY, 0.0, 1e9);
        let step = model
            .step(&mut state, input, 0.01)
            .expect("the declared step is valid");
        assert!(
            (step.throttle - 0.005).abs() < 1.0e-12,
            "the actual throttle is {}",
            step.throttle
        );
        let command = expected(1.0);
        let slewed = expected(0.005);
        assert!(
            (step.thrust - slewed).abs() <= 1.0e-9 * slewed.max(1.0),
            "thrust is {} lb, expected {slewed} lb (the command would give {command} lb)",
            step.thrust
        );

        // With no fuel the throttle is frozen at 0: full throttle on the
        // stick cannot turn the engine back on.
        let mut dry = OriginalState::at(0.0, Quaternion::IDENTITY, 0.0, 0.0);
        let dry_step = model
            .step(&mut dry, input, 0.01)
            .expect("the declared step is valid");
        assert_eq!(dry.throttle, 0.0, "a frozen throttle does not move");
        assert_eq!(dry.fuel, 0.0, "a dry tank burns nothing");
        assert_eq!(
            dry_step.thrust, 0.0,
            "a frozen throttle thrusts at its own (zero) value, not the command"
        );

        // And an aircraft that has already run the slew up to 1.0 keeps
        // thrusting at that value while the command is cut: the command is
        // never what the law reads.
        let mut full = OriginalState::at(0.0, Quaternion::IDENTITY, 1.0, 1e9);
        let cut = OriginalInput {
            throttle: 0.0,
            is_player: true,
            ..OriginalInput::default()
        };
        let first = model
            .step(&mut full, cut, 0.01)
            .expect("the declared step is valid");
        assert!(
            (first.thrust - expected(0.995)).abs() <= 1.0e-9 * expected(0.995),
            "thrust is {} lb after one step of the slew",
            first.thrust
        );
        assert!(
            (full.throttle - 0.995).abs() < 1.0e-12,
            "the actual throttle is {}",
            full.throttle
        );
    }

    /// A step outside `(0, 0.125]` is refused by name, and the fake-dynamics
    /// branch relaxes the velocity towards `fd_speed * throttle`.
    #[test]
    fn accept_flight_original_step_bounds_and_fake_dynamics() {
        let model = OriginalFlightModel::full(airframe(), globals());
        let mut state = OriginalState::at(0.0, Quaternion::IDENTITY, 1.0, 1e9);
        let input = OriginalInput::full_throttle(true);
        assert_eq!(
            model.step(&mut state, input, 0.126),
            Err(OriginalStepError::Timestep { dt_s: 0.126 })
        );
        assert_eq!(
            model.step(&mut state, input, 0.0),
            Err(OriginalStepError::Timestep { dt_s: 0.0 })
        );
        assert!(model.step(&mut state, input, 0.125).is_ok());

        let fake = OriginalFlightModel {
            airframe: airframe(),
            globals: globals(),
            dynamics: DynamicsKind::Fake,
        };
        let mut state = OriginalState::at(0.0, Quaternion::IDENTITY, 1.0, 1e9);
        for _ in 0..600 {
            fake.step(&mut state, input, 0.01)
                .expect("the fake branch steps");
        }
        let speed = norm(state.velocity_mps);
        assert!(
            (speed - fake.airframe.fd_speed).abs() < 0.5,
            "fake dynamics converged to {speed}, fd_speed {}",
            fake.airframe.fd_speed
        );
    }

    /// A parameter record with an unknown, missing or non-finite field is
    /// refused by name: nothing is defaulted and nothing is clamped.
    #[test]
    fn accept_flight_original_parameter_records_are_refused_by_name() {
        let complete = airframe_fields();
        assert!(OriginalAirframe::from_values(&complete).is_ok());

        let mut unknown = complete.clone();
        unknown.push(("wing.wingardium", 1.0));
        assert!(matches!(
            OriginalAirframe::from_values(&unknown),
            Err(OriginalParamsError::UnknownField { .. })
        ));

        let mut missing = complete.clone();
        missing.retain(|(name, _)| *name != "veh_weight");
        assert_eq!(
            OriginalAirframe::from_values(&missing),
            Err(OriginalParamsError::MissingField { name: "veh_weight" })
        );

        let mut non_finite = complete.clone();
        for entry in &mut non_finite {
            if entry.0 == "fd_speed" {
                entry.1 = f64::NAN;
            }
        }
        assert_eq!(
            OriginalAirframe::from_values(&non_finite),
            Err(OriginalParamsError::NonFinite { name: "fd_speed" })
        );

        let mut duplicate = complete.clone();
        duplicate.push(("drag_factor", 1.0));
        assert!(matches!(
            OriginalAirframe::from_values(&duplicate),
            Err(OriginalParamsError::DuplicateField { .. })
        ));

        let mut zero = complete;
        for entry in &mut zero {
            if entry.0 == "ref_area" {
                entry.1 = 0.0;
            }
        }
        assert!(matches!(
            OriginalAirframe::from_values(&zero),
            Err(OriginalParamsError::NonPositive {
                name: "ref_area",
                value
            }) if value == 0.0
        ));
    }

    // ------------------------------------------------------------- helpers --

    fn airframe_fields() -> Vec<(&'static str, f64)> {
        vec![
            ("roll_torque", 7.5),
            ("pitch_torque", 3.3),
            ("rudder_torque", 2.0),
            ("level_off_rate", DEFAULT_LEVEL_OFF_RATE),
            ("return_rate", 3.0),
            ("ang_momentum_damp", 5.0),
            ("rec_moments_inertia_x", 1.18),
            ("rec_moments_inertia_y", 1.0),
            ("rec_moments_inertia_z", 1.1),
            ("fd_speed", 135.0),
            ("engine_factor", 0.62),
            ("drag_factor", 0.37),
            ("veh_weight", 1900.0),
            ("ref_area", 330.0),
            ("gravity", 20.0),
        ]
    }

    /// The retail Bloodhawk parameters as the importer supplies them (the
    /// retail assertions live in `cs_content`'s acceptance tests).
    fn airframe() -> OriginalAirframe {
        OriginalAirframe::from_values(&airframe_fields()).expect("the fixture is complete")
    }

    fn globals() -> OriginalGlobals {
        OriginalGlobals::from_values(&[
            ("nom_gravity", 20.0),
            ("lift_aoa_0_deg", 5.0),
            ("lift_aoa_1_deg", 9.0),
            ("max_aoa_deg", 46.0),
            ("high_g_0", 9.0),
            ("high_g_1", 15.0),
            ("low_g_0", -6.0),
            ("low_g_1", -9.0),
            ("lift_accel_rate", 0.75),
            ("stall_mag", 1.25),
            ("turn_fade_in_mph", 10.0),
            ("turn_fade_out_mph", 50.0),
            ("yaw_low_speed", 0.0625),
            ("yaw_high_speed", 0.17),
            ("yaw_fade_in_mph", 10.0),
            ("yaw_max_mph", 50.0),
            ("yaw_fade_out_mph", 400.0),
        ])
        .expect("the declared globals are complete")
    }
}

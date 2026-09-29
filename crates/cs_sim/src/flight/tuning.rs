//! Normalized airframe tuning and flight-equation inputs (F24-A).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stage `### F24-A`. Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! sections "Inputs and outputs" and "Boost and special models".
//!
//! This module is the **typed numeric tuning** the flight model consumes:
//! `model_kind`, mass and inertia, engine curve, drag parameters, lift curve,
//! stall behavior, angular response, assist profile and the shared reference
//! area. [`AirframeTuning::try_new`] is the boundary: a non-finite value, a
//! non-positive mass/area/gain or an out-of-range fraction is refused by name
//! instead of being silently clamped into something that looks plausible
//! (`FLIGHT-PHYSICS`: "Reject nonfinite inputs at boundaries ... do not
//! silently clamp corrupted tuning into plausible values").
//!
//! **Designed starting model, not original data.** Every field, default and
//! bound here is newly authored project design. The original 2000 PC game's
//! exact force equations and tuning units were not recovered by public
//! research (`F24` "Research boundary"), so nothing in this module claims to
//! be an extracted original coefficient; the provenance-carrying,
//! normalization-side schema lives in `cs_content::flight_tuning` and the
//! calibration against original reference traces is F24-D.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`), so the numeric model input is declared here,
//! on the consuming side, while the content crate owns where each value came
//! from. F24-C maps the `cs_content` record into this type; until then the
//! synthetic fixture in [`super::synthetic`] is the only producer.

use cs_types::content::Origin;

/// The named numerical epsilon below which an air-relative velocity counts as
/// zero, used only to avoid singular directions — never to impose a minimum
/// flying speed (`FLIGHT-PHYSICS`: "Define a small, named numerical epsilon
/// only to avoid singularities").
pub const AIRSPEED_EPSILON_MPS: f64 = 1.0e-6;

/// Which flight model an airframe uses.
///
/// Fixed-wing airframes share the one control law in this module. An
/// `Exceptional` airframe (an autogyro or another special model) implements
/// the same input/output boundary but may use a different control law; its
/// roles and telemetry are F25-A's deliverable, and nothing here invents a
/// helicopter hover for it (`FLIGHT-PHYSICS`: "Do not use the word autogyro
/// as permission to invent helicopter hover").
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ModelKind {
    /// A conventional fixed-wing airframe.
    FixedWing,
    /// A special model with its own control law (F25-A).
    Exceptional,
}

impl ModelKind {
    /// Every declared kind, in a stable order.
    pub const ALL: [Self; 2] = [Self::FixedWing, Self::Exceptional];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::FixedWing => "fixed_wing",
            Self::Exceptional => "exceptional",
        }
    }

    /// Looks a kind up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|kind| kind.label() == label)
    }
}

/// Which named handling profile a tuning belongs to.
///
/// F24 non-negotiable behavior 5 and the sheet's deliverable: an improved
/// handling profile is optional and named, and it is **never** silently the
/// fidelity profile a reference trace is compared against.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HandlingProfile {
    /// The profile compared against original reference traces.
    #[default]
    Fidelity,
    /// An optional, explicitly selected improved-handling profile.
    Improved,
}

impl HandlingProfile {
    /// Every profile, in a stable order.
    pub const ALL: [Self; 2] = [Self::Fidelity, Self::Improved];

    /// The stable label used in reports and persisted records.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Fidelity => "fidelity",
            Self::Improved => "improved",
        }
    }

    /// Looks a profile up by its label; `None` for an unknown label.
    #[must_use]
    pub fn from_label(label: &str) -> Option<Self> {
        Self::ALL
            .iter()
            .copied()
            .find(|profile| profile.label() == label)
    }
}

/// Why an airframe tuning was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum AirframeTuningError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field, as the caller spells it.
        field: &'static str,
    },
    /// A field that must be strictly positive was zero or negative.
    NonPositive {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: &'static str,
    },
    /// A field fell outside its declared inclusive range.
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
}

impl std::fmt::Display for AirframeTuningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::NonPositive { field } => write!(f, "{field} must be greater than zero"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
            Self::OutOfRange {
                field,
                value,
                min,
                max,
            } => write!(f, "{field} value {value} is outside [{min}, {max}]"),
        }
    }
}

impl std::error::Error for AirframeTuningError {}

/// Mass and inertia of one airframe, in SI units.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MassProperties {
    /// Empty airframe mass, in kilograms. Strictly positive.
    pub mass_kg: f64,
    /// Principal moments of inertia (roll, pitch, yaw), in kg·m². Each
    /// strictly positive.
    pub inertia_kg_m2: [f64; 3],
}

impl MassProperties {
    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_positive("mass.mass_kg", self.mass_kg)?;
        for (axis, value) in ["roll", "pitch", "yaw"].into_iter().zip(self.inertia_kg_m2) {
            check_positive(INERTIA_FIELDS[axis_index(axis)], value)?;
        }
        Ok(())
    }
}

/// The engine thrust curve and its response rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EngineCurve {
    /// Thrust at zero throttle, in newtons. Must not be negative.
    pub idle_thrust_n: f64,
    /// Thrust at full throttle, in newtons. Must not be less than
    /// [`idle_thrust_n`](Self::idle_thrust_n).
    pub max_thrust_n: f64,
    /// How fast the engine spool follows a commanded throttle, in fractions
    /// of full range per second. Strictly positive.
    pub throttle_response_per_s: f64,
}

impl EngineCurve {
    /// Validates the fields and their ordering.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_non_negative("engine.idle_thrust_n", self.idle_thrust_n)?;
        check_non_negative("engine.max_thrust_n", self.max_thrust_n)?;
        check_positive(
            "engine.throttle_response_per_s",
            self.throttle_response_per_s,
        )?;
        if self.max_thrust_n < self.idle_thrust_n {
            return Err(AirframeTuningError::OutOfRange {
                field: "engine.max_thrust_n",
                value: self.max_thrust_n,
                min: self.idle_thrust_n,
                max: f64::INFINITY,
            });
        }
        Ok(())
    }

    /// Thrust at a normalized `[0, 1]` spool setting, in newtons.
    #[must_use]
    pub fn thrust_at(&self, spool: f64) -> f64 {
        let spool = spool.clamp(0.0, 1.0);
        self.idle_thrust_n + (self.max_thrust_n - self.idle_thrust_n) * spool
    }
}

/// Boost equipment data: thrust modification and consumption rate.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoostParameters {
    /// Extra thrust while boost is accepted, in newtons. Must not be
    /// negative.
    pub thrust_n: f64,
    /// Capacity consumed per second while boost is accepted. Strictly
    /// positive.
    pub consumption_per_s: f64,
}

impl BoostParameters {
    /// A disabled boost: no thrust, no consumption.
    pub const NONE: Self = Self {
        thrust_n: 0.0,
        consumption_per_s: 0.0,
    };

    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_non_negative("boost.thrust_n", self.thrust_n)?;
        check_non_negative("boost.consumption_per_s", self.consumption_per_s)?;
        Ok(())
    }
}

/// Drag coefficients (parasitic and induced).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DragParameters {
    /// Parasitic (zero-lift) drag coefficient. Must not be negative.
    pub zero_lift_coefficient: f64,
    /// Induced-drag factor applied to `CL²`. Must not be negative.
    pub induced_coefficient: f64,
}

impl DragParameters {
    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_non_negative("drag.zero_lift_coefficient", self.zero_lift_coefficient)?;
        check_non_negative("drag.induced_coefficient", self.induced_coefficient)?;
        Ok(())
    }

    /// `CD(CL) = CD0 + k·CL²`.
    #[must_use]
    pub fn coefficient_at(&self, lift_coefficient: f64) -> f64 {
        self.zero_lift_coefficient + self.induced_coefficient * lift_coefficient * lift_coefficient
    }
}

/// The linear lift curve and its ceiling.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LiftCurve {
    /// Lift coefficient at zero angle of attack. Any finite value.
    pub lift_at_zero_alpha: f64,
    /// Lift-curve slope, in coefficient per radian. Strictly positive.
    pub lift_slope_per_rad: f64,
    /// The largest magnitude the lift coefficient may take. Strictly
    /// positive.
    pub max_lift_coefficient: f64,
}

impl LiftCurve {
    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_finite("lift.lift_at_zero_alpha", self.lift_at_zero_alpha)?;
        check_positive("lift.lift_slope_per_rad", self.lift_slope_per_rad)?;
        check_positive("lift.max_lift_coefficient", self.max_lift_coefficient)?;
        Ok(())
    }
}

/// Gradual-stall behavior: the finite, smooth loss of lift past an angle.
///
/// The spec requires a *finite-stall curve*, not a discontinuous "below the
/// stall speed it falls" branch, so stall is expressed as an angle together
/// with a width over which lift and control authority decline smoothly.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StallBehavior {
    /// The angle of attack, in radians, at which the stall factor starts to
    /// fall below one. Strictly positive.
    pub stall_angle_rad: f64,
    /// How far past the stall angle the factor takes to reach its residual,
    /// in radians. Strictly positive.
    pub stall_width_rad: f64,
    /// The fraction of lift and control authority that remains fully
    /// stalled, in `[0, 1]`.
    pub residual_fraction: f64,
}

impl StallBehavior {
    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_positive("stall.stall_angle_rad", self.stall_angle_rad)?;
        check_positive("stall.stall_width_rad", self.stall_width_rad)?;
        check_fraction("stall.residual_fraction", self.residual_fraction)?;
        Ok(())
    }

    /// The smooth stall factor in `[residual_fraction, 1]` at `angle_rad`.
    ///
    /// It is exactly `1` up to the stall angle, declines smoothly across the
    /// stall width and holds the residual past it, so no branch introduces a
    /// discontinuity and no angle produces a non-finite value.
    #[must_use]
    pub fn factor_at(&self, angle_rad: f64) -> f64 {
        let magnitude = angle_rad.abs();
        if magnitude <= self.stall_angle_rad {
            return 1.0;
        }
        let over = magnitude - self.stall_angle_rad;
        if over >= self.stall_width_rad {
            return self.residual_fraction;
        }
        let t = over / self.stall_width_rad;
        let smooth = t * t * (3.0 - 2.0 * t);
        1.0 + (self.residual_fraction - 1.0) * smooth
    }
}

/// Rate-command attitude control: gains, limits and the speed authority ramp.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AngularResponse {
    /// Angular-rate error gain, in `s⁻¹` of desired angular acceleration per
    /// rad/s of error. Strictly positive.
    pub rate_gain_per_s: f64,
    /// Extra explicit rate damping, in `s⁻¹`. Must not be negative.
    pub rate_damping_per_s: f64,
    /// Maximum commanded body rate per axis (roll, pitch, yaw), in rad/s.
    /// Each strictly positive.
    pub max_rate_radps: [f64; 3],
    /// Maximum corrective torque per axis, in N·m. Each strictly positive.
    pub max_torque_nm: [f64; 3],
    /// The airspeed at which control surfaces have full authority, in m/s.
    /// Below it authority ramps smoothly to zero at zero airspeed. Strictly
    /// positive.
    pub control_airspeed_full_mps: f64,
}

impl AngularResponse {
    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_positive("angular.rate_gain_per_s", self.rate_gain_per_s)?;
        check_non_negative("angular.rate_damping_per_s", self.rate_damping_per_s)?;
        for (index, value) in self.max_rate_radps.into_iter().enumerate() {
            check_positive(RATE_FIELDS[index], value)?;
        }
        for (index, value) in self.max_torque_nm.into_iter().enumerate() {
            check_positive(TORQUE_FIELDS[index], value)?;
        }
        check_positive(
            "angular.control_airspeed_full_mps",
            self.control_airspeed_full_mps,
        )?;
        Ok(())
    }
}

/// The declared assist profile.
///
/// Assists are opt-in engine design: they are recorded separately in the
/// flight output, and a calibrated probe disables them
/// (F24 non-negotiable behavior 5). The only assist modeled in F24-A is the
/// optional bank/level torque, which can only act about the roll axis and
/// therefore cannot cancel gravity or recover a stall by itself.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct AssistProfile {
    /// Whether any assist may contribute at all.
    pub enabled: bool,
    /// Roll gain of the bank/level assist, in N·m per radian of bank error.
    /// Must not be negative.
    pub bank_level_gain_nm_per_rad: f64,
    /// The most torque the bank/level assist may apply, in N·m. Strictly
    /// positive when the assist can contribute.
    pub bank_level_max_torque_nm: f64,
}

impl AssistProfile {
    /// Assists off: the calibrated-probe profile.
    pub const CALIBRATED: Self = Self {
        enabled: false,
        bank_level_gain_nm_per_rad: 0.0,
        bank_level_max_torque_nm: 1.0,
    };

    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        check_non_negative(
            "assists.bank_level_gain_nm_per_rad",
            self.bank_level_gain_nm_per_rad,
        )?;
        if !self.bank_level_max_torque_nm.is_finite() {
            return Err(AirframeTuningError::NonFinite {
                field: "assists.bank_level_max_torque_nm",
            });
        }
        if self.enabled && self.bank_level_max_torque_nm <= 0.0 {
            return Err(AirframeTuningError::NonPositive {
                field: "assists.bank_level_max_torque_nm",
            });
        }
        Ok(())
    }
}

/// The complete normalized tuning the fixed-wing model consumes.
///
/// Build one through [`AirframeTuning::try_new`]; a value that passes is
/// finite and inside every declared bound, so the force equations never have
/// to repair corrupted tuning. The `origin` says whether the values are newly
/// authored engine design or synthetic fixture data — never an original
/// coefficient, which no measurement has produced yet.
#[derive(Clone, Debug, PartialEq)]
pub struct AirframeTuning {
    /// Which flight model this airframe uses.
    pub model_kind: ModelKind,
    /// Which named handling profile these values belong to.
    pub profile: HandlingProfile,
    /// Where the values came from.
    pub origin: Origin,
    /// Mass and inertia.
    pub mass: MassProperties,
    /// Engine thrust curve.
    pub engine: EngineCurve,
    /// Boost equipment.
    pub boost: BoostParameters,
    /// Drag coefficients.
    pub drag: DragParameters,
    /// Lift curve.
    pub lift: LiftCurve,
    /// Gradual-stall behavior.
    pub stall: StallBehavior,
    /// Attitude-control response.
    pub angular: AngularResponse,
    /// Optional assists.
    pub assists: AssistProfile,
    /// Reference wing area shared by lift and drag, in m². Strictly positive.
    pub reference_area_m2: f64,
}

impl AirframeTuning {
    /// Validates every field at the boundary.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field. The value is
    /// never repaired in place.
    pub fn validate(&self) -> Result<(), AirframeTuningError> {
        self.mass.validate()?;
        self.engine.validate()?;
        self.boost.validate()?;
        self.drag.validate()?;
        self.lift.validate()?;
        self.stall.validate()?;
        self.angular.validate()?;
        self.assists.validate()?;
        check_positive("reference_area_m2", self.reference_area_m2)?;
        Ok(())
    }

    /// Builds a tuning only when every field validates.
    ///
    /// # Errors
    ///
    /// [`AirframeTuningError`] naming the first offending field.
    pub fn try_new(tuning: Self) -> Result<Self, AirframeTuningError> {
        tuning.validate()?;
        Ok(tuning)
    }
}

/// Why a loadout mass was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum LoadoutMassError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A field that must not be negative was negative.
    Negative {
        /// The offending field.
        field: &'static str,
    },
    /// The total mass was not strictly positive.
    NonPositiveTotal,
}

impl std::fmt::Display for LoadoutMassError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::Negative { field } => write!(f, "{field} must not be negative"),
            Self::NonPositiveTotal => {
                write!(f, "the total aircraft mass must be greater than zero")
            }
        }
    }
}

impl std::error::Error for LoadoutMassError {}

/// The mass a loadout adds to the empty airframe.
///
/// Non-negotiable behavior 4: the same mass the flight model integrates is
/// what a UI performance bar reads, so loadout/armor changes reach the model
/// through this one record rather than a cosmetic value. Fuel, ordnance and
/// armor are the declared contributors; a difference from the empty airframe
/// mass in [`AirframeTuning::mass`] is therefore visible in the equations.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LoadoutMass {
    /// Fuel mass, in kilograms.
    pub fuel_kg: f64,
    /// Ordnance mass, in kilograms.
    pub ordnance_kg: f64,
    /// Armor mass, in kilograms.
    pub armor_kg: f64,
}

impl LoadoutMass {
    /// The empty loadout: no added mass.
    pub const EMPTY: Self = Self {
        fuel_kg: 0.0,
        ordnance_kg: 0.0,
        armor_kg: 0.0,
    };

    /// Validates the contributors.
    ///
    /// # Errors
    ///
    /// [`LoadoutMassError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), LoadoutMassError> {
        for (field, value) in [
            ("loadout.fuel_kg", self.fuel_kg),
            ("loadout.ordnance_kg", self.ordnance_kg),
            ("loadout.armor_kg", self.armor_kg),
        ] {
            if !value.is_finite() {
                return Err(LoadoutMassError::NonFinite { field });
            }
            if value < 0.0 {
                return Err(LoadoutMassError::Negative { field });
            }
        }
        Ok(())
    }

    /// Added loadout mass, in kilograms.
    #[must_use]
    pub fn added_mass_kg(&self) -> f64 {
        self.fuel_kg + self.ordnance_kg + self.armor_kg
    }

    /// Total aircraft mass, in kilograms, with `empty_mass_kg`.
    ///
    /// # Errors
    ///
    /// [`LoadoutMassError::NonPositiveTotal`] when neither the empty airframe
    /// nor the loadout contributes any mass.
    pub fn total_mass_kg(&self, empty_mass_kg: f64) -> Result<f64, LoadoutMassError> {
        let total = empty_mass_kg + self.added_mass_kg();
        if total <= 0.0 || !total.is_finite() {
            return Err(LoadoutMassError::NonPositiveTotal);
        }
        Ok(total)
    }
}

/// Why a damage state was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum DamageStateError {
    /// A named field contained NaN or infinity.
    NonFinite {
        /// The offending field.
        field: &'static str,
    },
    /// A field fell outside `[0, 1]`.
    OutOfRange {
        /// The offending field.
        field: &'static str,
        /// The rejected value.
        value: f64,
    },
}

impl std::fmt::Display for DamageStateError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite { field } => write!(f, "{field} must be finite"),
            Self::OutOfRange { field, value } => {
                write!(f, "{field} value {value} is outside [0, 1]")
            }
        }
    }
}

impl std::error::Error for DamageStateError {}

/// How damage scales the model's authority, as fractions of the pristine
/// value.
///
/// A damaged airframe keeps the same equations: damage only scales thrust,
/// control authority and lift, so it cannot secretly switch to a second model
/// (F24-C wires real damage producers).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DamageState {
    /// Control authority, in `[0, 1]`.
    pub control_authority: f64,
    /// Thrust authority, in `[0, 1]`.
    pub thrust_authority: f64,
    /// Lift scale, in `[0, 1]`.
    pub lift_scale: f64,
}

impl DamageState {
    /// The undamaged state: every authority is `1`.
    pub const PRISTINE: Self = Self {
        control_authority: 1.0,
        thrust_authority: 1.0,
        lift_scale: 1.0,
    };

    /// Validates the fields.
    ///
    /// # Errors
    ///
    /// [`DamageStateError`] naming the first offending field.
    pub fn validate(&self) -> Result<(), DamageStateError> {
        for (field, value) in [
            ("damage.control_authority", self.control_authority),
            ("damage.thrust_authority", self.thrust_authority),
            ("damage.lift_scale", self.lift_scale),
        ] {
            if !value.is_finite() {
                return Err(DamageStateError::NonFinite { field });
            }
            if !(0.0..=1.0).contains(&value) {
                return Err(DamageStateError::OutOfRange { field, value });
            }
        }
        Ok(())
    }
}

const INERTIA_FIELDS: [&str; 3] = [
    "mass.inertia_kg_m2[0]",
    "mass.inertia_kg_m2[1]",
    "mass.inertia_kg_m2[2]",
];
const RATE_FIELDS: [&str; 3] = [
    "angular.max_rate_radps[0]",
    "angular.max_rate_radps[1]",
    "angular.max_rate_radps[2]",
];
const TORQUE_FIELDS: [&str; 3] = [
    "angular.max_torque_nm[0]",
    "angular.max_torque_nm[1]",
    "angular.max_torque_nm[2]",
];

fn axis_index(axis: &str) -> usize {
    match axis {
        "roll" => 0,
        "pitch" => 1,
        _ => 2,
    }
}

fn check_finite(field: &'static str, value: f64) -> Result<(), AirframeTuningError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(AirframeTuningError::NonFinite { field })
    }
}

fn check_positive(field: &'static str, value: f64) -> Result<(), AirframeTuningError> {
    check_finite(field, value)?;
    if value > 0.0 {
        Ok(())
    } else {
        Err(AirframeTuningError::NonPositive { field })
    }
}

fn check_non_negative(field: &'static str, value: f64) -> Result<(), AirframeTuningError> {
    check_finite(field, value)?;
    if value >= 0.0 {
        Ok(())
    } else {
        Err(AirframeTuningError::Negative { field })
    }
}

fn check_fraction(field: &'static str, value: f64) -> Result<(), AirframeTuningError> {
    check_finite(field, value)?;
    if (0.0..=1.0).contains(&value) {
        Ok(())
    } else {
        Err(AirframeTuningError::OutOfRange {
            field,
            value,
            min: 0.0,
            max: 1.0,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mass() -> MassProperties {
        MassProperties {
            mass_kg: 1200.0,
            inertia_kg_m2: [1400.0, 2100.0, 2600.0],
        }
    }

    fn engine() -> EngineCurve {
        EngineCurve {
            idle_thrust_n: 400.0,
            max_thrust_n: 9000.0,
            throttle_response_per_s: 1.5,
        }
    }

    fn angular() -> AngularResponse {
        AngularResponse {
            rate_gain_per_s: 4.0,
            rate_damping_per_s: 0.5,
            max_rate_radps: [2.0, 1.5, 1.0],
            max_torque_nm: [20_000.0, 30_000.0, 8_000.0],
            control_airspeed_full_mps: 40.0,
        }
    }

    fn tuning() -> AirframeTuning {
        AirframeTuning {
            model_kind: ModelKind::FixedWing,
            profile: HandlingProfile::Fidelity,
            origin: Origin::Designed,
            mass: mass(),
            engine: engine(),
            boost: BoostParameters::NONE,
            drag: DragParameters {
                zero_lift_coefficient: 0.03,
                induced_coefficient: 0.06,
            },
            lift: LiftCurve {
                lift_at_zero_alpha: 0.15,
                lift_slope_per_rad: 4.5,
                max_lift_coefficient: 1.6,
            },
            stall: StallBehavior {
                stall_angle_rad: 0.28,
                stall_width_rad: 0.18,
                residual_fraction: 0.25,
            },
            angular: angular(),
            assists: AssistProfile::CALIBRATED,
            reference_area_m2: 21.0,
        }
    }

    /// The failure half of the tuning boundary: a corrupt value is refused by
    /// its own name, never repaired into something plausible.
    #[test]
    fn accept_f24_a_tuning_boundary_rejects_corrupt_values_by_name() {
        let mut negative_mass = tuning();
        negative_mass.mass.mass_kg = -1.0;
        assert_eq!(
            negative_mass.validate(),
            Err(AirframeTuningError::NonPositive {
                field: "mass.mass_kg"
            })
        );

        let mut nan_area = tuning();
        nan_area.reference_area_m2 = f64::NAN;
        assert_eq!(
            nan_area.validate(),
            Err(AirframeTuningError::NonFinite {
                field: "reference_area_m2"
            })
        );

        let mut reversed_idle = tuning();
        reversed_idle.engine.idle_thrust_n = 1200.0;
        reversed_idle.engine.max_thrust_n = 1000.0;
        assert_eq!(
            reversed_idle.validate(),
            Err(AirframeTuningError::OutOfRange {
                field: "engine.max_thrust_n",
                value: 1000.0,
                min: 1200.0,
                max: f64::INFINITY,
            })
        );

        let mut bad_stall = tuning();
        bad_stall.stall.residual_fraction = 1.5;
        assert_eq!(
            bad_stall.validate(),
            Err(AirframeTuningError::OutOfRange {
                field: "stall.residual_fraction",
                value: 1.5,
                min: 0.0,
                max: 1.0,
            })
        );

        let mut bad_axis = tuning();
        bad_axis.angular.max_torque_nm[1] = 0.0;
        assert_eq!(
            bad_axis.validate(),
            Err(AirframeTuningError::NonPositive {
                field: "angular.max_torque_nm[1]"
            })
        );

        assert!(AirframeTuning::try_new(tuning()).is_ok());
    }

    /// The stall curve is continuous, bounded and finite: it is one at and
    /// below the stall angle, reaches the residual at the end of the width and
    /// stays finite far past stall on both signs.
    #[test]
    fn accept_f24_a_stall_factor_is_smooth_bounded_and_symmetric() {
        let stall = StallBehavior {
            stall_angle_rad: 0.3,
            stall_width_rad: 0.2,
            residual_fraction: 0.2,
        };
        assert_eq!(stall.factor_at(0.0), 1.0);
        assert_eq!(stall.factor_at(0.3), 1.0);
        assert_eq!(stall.factor_at(-0.3), 1.0);
        assert!(
            (stall.factor_at(0.5) - 0.2).abs() < 1e-12,
            "the residual is reached at the end of the stall width"
        );

        let mut previous = 1.0;
        for step in 0..=100 {
            let angle = 0.3 + 0.2 * (step as f64 / 100.0);
            let factor = stall.factor_at(angle);
            assert!(factor.is_finite());
            assert!((0.2..=1.0).contains(&factor));
            assert!(factor <= previous + 1e-12, "the factor must not rise");
            previous = factor;
        }
        assert!(stall.factor_at(f64::MAX).is_finite());
        assert!((stall.factor_at(f64::MAX) - 0.2).abs() < 1e-12);
    }

    /// Mass, profile and origin are structural: an improved profile is named
    /// and never the default fidelity one, and loadout mass adds up.
    #[test]
    fn accept_f24_a_profiles_and_loadout_mass_are_explicit() {
        assert_eq!(HandlingProfile::default(), HandlingProfile::Fidelity);
        assert_eq!(HandlingProfile::Fidelity.label(), "fidelity");
        assert_eq!(
            HandlingProfile::from_label("improved"),
            Some(HandlingProfile::Improved)
        );
        assert_eq!(
            ModelKind::from_label("fixed_wing"),
            Some(ModelKind::FixedWing)
        );
        assert_eq!(ModelKind::from_label("helicopter"), None);

        let loaded = LoadoutMass {
            fuel_kg: 300.0,
            ordnance_kg: 200.0,
            armor_kg: 50.0,
        };
        assert_eq!(loaded.added_mass_kg(), 550.0);
        assert_eq!(loaded.total_mass_kg(1200.0), Ok(1750.0));
        assert_eq!(
            LoadoutMass::EMPTY.total_mass_kg(0.0),
            Err(LoadoutMassError::NonPositiveTotal)
        );
        assert_eq!(
            LoadoutMass {
                fuel_kg: -1.0,
                ..LoadoutMass::EMPTY
            }
            .validate(),
            Err(LoadoutMassError::Negative {
                field: "loadout.fuel_kg"
            })
        );

        // A disabled assist needs no positive torque limit; an enabled one
        // does, so the limit is checked only when the assist can contribute.
        let disabled = AssistProfile {
            enabled: false,
            bank_level_gain_nm_per_rad: 0.0,
            bank_level_max_torque_nm: 0.0,
        };
        assert_eq!(disabled.validate(), Ok(()));
        assert_eq!(
            AssistProfile {
                enabled: true,
                ..disabled
            }
            .validate(),
            Err(AirframeTuningError::NonPositive {
                field: "assists.bank_level_max_torque_nm"
            })
        );

        assert_eq!(DamageState::PRISTINE.validate(), Ok(()));
        assert_eq!(
            DamageState {
                control_authority: 1.5,
                ..DamageState::PRISTINE
            }
            .validate(),
            Err(DamageStateError::OutOfRange {
                field: "damage.control_authority",
                value: 1.5,
            })
        );
    }
}

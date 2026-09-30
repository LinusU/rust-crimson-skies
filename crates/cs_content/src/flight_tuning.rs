//! The provenance-carrying flight-tuning schema (F24-A).
//!
//! Spec: `specs/F24-fixed-wing-flight-engine-stall-and-arcade-assists.md`,
//! stages `### F24-A` (this stage) and `### F24-C` (the mapping into the
//! consuming model). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`,
//! section "Inputs and outputs".
//!
//! `cs_sim::flight::tuning` is the *numeric* tuning the equations consume; it
//! may depend only on `cs_types`, so it cannot carry the content-side records
//! that say where a value came from. This module is that missing half: the
//! declared field list ([`TuningSchema::fixed_wing`]) with each field's unit
//! and approved range, and a [`DeclaredAirframeTuning`] whose every value is a
//! [`Resolved`] — either known with [`Provenance`], or an explicit unknown
//! with a reason. There is no third case where a missing value becomes a zero
//! (spec F14 non-negotiable behavior 3; `IDENTITY-CONTENT`).
//!
//! [`DeclaredAirframeTuning::validate`] joins the two: a field the schema does
//! not declare is refused, a required field that is missing or explicitly
//! unknown is refused, and a known value that is non-finite or outside its
//! approved range is refused by name. Nothing is clamped: a field the schema
//! does not cover never reaches the equations through repair
//! (`FLIGHT-PHYSICS`: "do not silently clamp corrupted tuning into plausible
//! values").
//!
//! # What is measured and what is designed
//!
//! Every unit, bound, name and value here is **newly authored project
//! design**, not a measurement. Public research did not recover the original
//! 2000 PC game's flight equations, tuning units or airframe coefficients
//! (`F24` "Research boundary"), so the declared table's `Origin::Designed`
//! says exactly that: the approved ranges are this project's own chosen
//! envelope for a calibration that F24-D performs against fingerprinted
//! reference traces. The one declared airframe carries
//! [`Origin::SyntheticFixture`] and is a bootstrap projection of
//! `cs_sim::flight::synthetic::synthetic_fixed_wing`; F24-C is the stage that
//! asserts the two agree and wires the record into the model. F24-C also adds
//! the optional, explicitly named improved record and
//! [`declared_synthetic_airframe_for`], the profile producer a caller selects
//! from: an unknown profile label is refused with `None` rather than falling
//! back to the fidelity record. No value here is an extracted original
//! coefficient.

use cs_types::content::{
    Known, Origin, PermittedRange, Provenance, RangeError, Resolved, ResolvedError,
};
use cs_types::evidence::ClaimId;

/// The label of the fidelity handling profile: the declared profile a
/// calibrated probe compares against.
pub const FIDELITY_PROFILE: &str = "fidelity";

/// The label of the optional improved-handling profile.
///
/// F24 non-negotiable behavior 5: an improved profile is explicitly named and
/// is never silently the fidelity profile a reference trace is compared
/// against.
pub const IMPROVED_PROFILE: &str = "improved";

/// One declared tuning field's contract: its stable name, its unit, the
/// inclusive range a consumer may accept and whether a complete record must
/// state it.
///
/// The `unit` is a diagnostic label, exactly as in
/// [`crate::config::FieldSpec`]: it names what the number means and is never
/// interpreted by this module. A derived unit (`N*m`, `1/rad`) is spelled in
/// its own terms rather than forced into the canonical
/// [`Unit`](cs_types::content::Unit) vocabulary, which is reserved for the
/// base quantities normalization creates.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TuningFieldSpec {
    /// The stable field name, e.g. `mass.mass_kg`.
    pub name: &'static str,
    /// A human-readable unit label, e.g. `kg` or `rad/s`. Never interpreted.
    pub unit: &'static str,
    /// The approved inclusive range a known value must fall inside.
    pub range: PermittedRange,
    /// Whether a complete record must state the field as known.
    pub required: bool,
}

impl TuningFieldSpec {
    /// Declares a field, validating the name and the range.
    ///
    /// # Errors
    ///
    /// [`TuningSchemaError::EmptyName`] for a blank name, and
    /// [`TuningSchemaError::Range`] when the bounds are non-finite or
    /// reversed.
    pub fn new(
        name: &'static str,
        unit: &'static str,
        min: f64,
        max: f64,
        required: bool,
    ) -> Result<Self, TuningSchemaError> {
        if name.trim().is_empty() {
            return Err(TuningSchemaError::EmptyName);
        }
        let range = PermittedRange::new(min, max).map_err(|detail| TuningSchemaError::Range {
            field: name,
            detail,
        })?;
        Ok(Self {
            name,
            unit,
            range,
            required,
        })
    }

    /// Whether `value` is finite and inside the approved range.
    #[must_use]
    pub fn accepts(&self, value: f64) -> bool {
        self.range.contains(value)
    }
}

/// Why a declared tuning schema was rejected.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum TuningSchemaError {
    /// A field's name was empty or only whitespace.
    EmptyName,
    /// Two fields declared the same name.
    DuplicateName {
        /// The name that appeared more than once.
        name: &'static str,
    },
    /// A field's approved range was not usable.
    Range {
        /// The offending field.
        field: &'static str,
        /// Why the range was rejected.
        detail: RangeError,
    },
}

impl std::fmt::Display for TuningSchemaError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyName => write!(f, "a declared tuning field name must not be empty"),
            Self::DuplicateName { name } => {
                write!(f, "the tuning field {name} was declared more than once")
            }
            Self::Range { field, detail } => write!(f, "{field} has an unusable range: {detail}"),
        }
    }
}

impl std::error::Error for TuningSchemaError {}

/// The declared field list one tuning record is checked against.
///
/// A schema is a closed vocabulary: a record that names a field the schema
/// does not declare is refused rather than passed through, so a renamed or
/// misspelled field can never reach the model as a silent default.
#[derive(Clone, Debug, PartialEq)]
pub struct TuningSchema {
    fields: Vec<TuningFieldSpec>,
}

impl TuningSchema {
    /// Builds a schema, refusing an empty name and a duplicate name.
    ///
    /// # Errors
    ///
    /// [`TuningSchemaError::EmptyName`] and
    /// [`TuningSchemaError::DuplicateName`].
    pub fn new(fields: Vec<TuningFieldSpec>) -> Result<Self, TuningSchemaError> {
        for (index, field) in fields.iter().enumerate() {
            if field.name.trim().is_empty() {
                return Err(TuningSchemaError::EmptyName);
            }
            if fields[..index]
                .iter()
                .any(|earlier| earlier.name == field.name)
            {
                return Err(TuningSchemaError::DuplicateName { name: field.name });
            }
        }
        Ok(Self { fields })
    }

    /// Every declared field, in declaration order.
    #[must_use]
    pub fn fields(&self) -> &[TuningFieldSpec] {
        &self.fields
    }

    /// The number of declared fields.
    #[must_use]
    pub fn len(&self) -> usize {
        self.fields.len()
    }

    /// Whether the schema declares no fields.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.fields.is_empty()
    }

    /// The spec of `name`, or `None` for a field the schema does not declare.
    #[must_use]
    pub fn field(&self, name: &str) -> Option<&TuningFieldSpec> {
        self.fields.iter().find(|field| field.name == name)
    }

    /// The fields a complete record must state as known.
    pub fn required(&self) -> impl Iterator<Item = &TuningFieldSpec> {
        self.fields.iter().filter(|field| field.required)
    }

    /// The declared fixed-wing schema: every numeric field the
    /// `cs_sim::flight` model consumes, in a stable order.
    ///
    /// The names mirror the consuming model's field names so F24-C's mapping
    /// is a direct correspondence, not a translation table. `model_kind`,
    /// handling `profile` and the assist enable flag are record identity, not
    /// numeric fields, and live on [`DeclaredAirframeTuning`].
    ///
    /// Boost equipment is optional: an airframe without it may leave those
    /// fields explicitly unknown rather than claim a zero thrust it never
    /// measured.
    #[must_use]
    pub fn fixed_wing() -> Self {
        Self::new(vec![
            spec("mass.mass_kg", "kg", 1.0, 2.0e5, true),
            spec("mass.inertia_kg_m2[0]", "kg*m^2", 1.0e-3, 1.0e9, true),
            spec("mass.inertia_kg_m2[1]", "kg*m^2", 1.0e-3, 1.0e9, true),
            spec("mass.inertia_kg_m2[2]", "kg*m^2", 1.0e-3, 1.0e9, true),
            spec("engine.idle_thrust_n", "N", 0.0, 1.0e8, true),
            spec("engine.max_thrust_n", "N", 0.0, 1.0e8, true),
            spec("engine.throttle_response_per_s", "1/s", 1.0e-6, 1.0e4, true),
            spec("boost.thrust_n", "N", 0.0, 1.0e8, false),
            spec("boost.consumption_per_s", "1/s", 0.0, 1.0e4, false),
            spec("drag.zero_lift_coefficient", "1", 0.0, 1.0e2, true),
            spec("drag.induced_coefficient", "1", 0.0, 1.0e2, true),
            spec("lift.lift_at_zero_alpha", "1", -1.0e2, 1.0e2, true),
            spec("lift.lift_slope_per_rad", "1/rad", 1.0e-6, 1.0e3, true),
            spec("lift.max_lift_coefficient", "1", 1.0e-6, 1.0e2, true),
            spec(
                "stall.stall_angle_rad",
                "rad",
                1.0e-6,
                std::f64::consts::PI,
                true,
            ),
            spec(
                "stall.stall_width_rad",
                "rad",
                1.0e-6,
                std::f64::consts::PI,
                true,
            ),
            spec("stall.residual_fraction", "1", 0.0, 1.0, true),
            spec("angular.rate_gain_per_s", "1/s", 1.0e-6, 1.0e4, true),
            spec("angular.rate_damping_per_s", "1/s", 0.0, 1.0e4, true),
            spec("angular.max_rate_radps[0]", "rad/s", 1.0e-6, 1.0e3, true),
            spec("angular.max_rate_radps[1]", "rad/s", 1.0e-6, 1.0e3, true),
            spec("angular.max_rate_radps[2]", "rad/s", 1.0e-6, 1.0e3, true),
            spec("angular.max_torque_nm[0]", "N*m", 1.0e-6, 1.0e9, true),
            spec("angular.max_torque_nm[1]", "N*m", 1.0e-6, 1.0e9, true),
            spec("angular.max_torque_nm[2]", "N*m", 1.0e-6, 1.0e9, true),
            spec(
                "angular.control_airspeed_full_mps",
                "m/s",
                1.0e-6,
                1.0e3,
                true,
            ),
            spec(
                "assists.bank_level_gain_nm_per_rad",
                "N*m/rad",
                0.0,
                1.0e9,
                true,
            ),
            spec(
                "assists.bank_level_max_torque_nm",
                "N*m",
                1.0e-6,
                1.0e9,
                true,
            ),
            spec("reference_area_m2", "m^2", 1.0e-6, 1.0e6, true),
        ])
        .expect("the declared fixed-wing schema is valid")
    }
}

/// Builds a declared field spec, panicking only for the hard-coded table
/// whose bounds are visible above (the `coordinates.rs` `declared` pattern).
fn spec(
    name: &'static str,
    unit: &'static str,
    min: f64,
    max: f64,
    required: bool,
) -> TuningFieldSpec {
    TuningFieldSpec::new(name, unit, min, max, required)
        .expect("a declared flight-tuning field spec is valid")
}

/// One field of a declared record: its name and either a known value with
/// provenance or an explicit unknown with a reason.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredTuningValue {
    /// The field name, matched against a [`TuningSchema`].
    pub field: String,
    /// The value or the explicit unknown.
    pub value: Resolved<f64>,
}

impl DeclaredTuningValue {
    /// A known value with its provenance.
    #[must_use]
    pub fn known(field: impl Into<String>, value: f64, provenance: Provenance) -> Self {
        Self {
            field: field.into(),
            value: Resolved::Known(cs_types::content::Known::new(value, provenance)),
        }
    }

    /// An explicit unknown, refusing an empty reason.
    ///
    /// # Errors
    ///
    /// [`ResolvedError::EmptyReason`] when `reason` is blank.
    pub fn unknown(
        field: impl Into<String>,
        claim_id: ClaimId,
        reason: &str,
    ) -> Result<Self, ResolvedError> {
        Ok(Self {
            field: field.into(),
            value: Resolved::unknown(claim_id, reason)?,
        })
    }

    /// The known value, or `None` when the field is explicitly unknown.
    #[must_use]
    pub fn known_value(&self) -> Option<f64> {
        match &self.value {
            Resolved::Known(known) => Some(known.value),
            Resolved::Unknown { .. } => None,
        }
    }
}

/// Why a declared airframe tuning was rejected against its schema.
#[derive(Clone, Debug, PartialEq)]
pub enum DeclaredTuningError {
    /// The record's id was empty.
    EmptyId,
    /// The record's `model_kind` was empty.
    EmptyModelKind,
    /// The record's `profile` was empty.
    EmptyProfile,
    /// A value named a field the schema does not declare.
    UnknownField {
        /// The undeclared name, as the record spelled it.
        name: String,
    },
    /// The record stated the same field twice.
    DuplicateField {
        /// The name that appeared more than once.
        name: String,
    },
    /// A required field did not appear in the record at all.
    MissingField {
        /// The required field.
        name: &'static str,
    },
    /// A required field was explicitly unknown, so the record is incomplete.
    RequiredFieldUnknown {
        /// The required field.
        name: &'static str,
    },
    /// A known value was NaN or infinite.
    NonFinite {
        /// The offending field.
        name: String,
    },
    /// A known value fell outside its approved range.
    OutOfRange {
        /// The offending field.
        name: String,
        /// The rejected value.
        value: f64,
        /// The inclusive lower bound.
        min: f64,
        /// The inclusive upper bound.
        max: f64,
    },
}

impl std::fmt::Display for DeclaredTuningError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::EmptyId => write!(f, "a declared airframe tuning id must not be empty"),
            Self::EmptyModelKind => write!(f, "model_kind must not be empty"),
            Self::EmptyProfile => write!(f, "profile must not be empty"),
            Self::UnknownField { name } => {
                write!(f, "the schema does not declare a tuning field {name}")
            }
            Self::DuplicateField { name } => write!(f, "the tuning field {name} was stated twice"),
            Self::MissingField { name } => write!(f, "the required tuning field {name} is missing"),
            Self::RequiredFieldUnknown { name } => {
                write!(f, "the required tuning field {name} is explicitly unknown")
            }
            Self::NonFinite { name } => write!(f, "{name} must be finite"),
            Self::OutOfRange {
                name,
                value,
                min,
                max,
            } => write!(f, "{name} value {value} is outside [{min}, {max}]"),
        }
    }
}

impl std::error::Error for DeclaredTuningError {}

/// A declared airframe's tuning, checked against a [`TuningSchema`].
///
/// The record's identity (`id`, `model_kind`, `profile`) and its [`Origin`]
/// travel with it, so a synthetic fixture can never be mistaken for a retail
/// airframe (`Origin::is_original`), and its [`Provenance`] names the claim
/// the whole record backs.
#[derive(Clone, Debug, PartialEq)]
pub struct DeclaredAirframeTuning {
    /// Stable id, e.g. `fixture.synthetic-fixed-wing`.
    pub id: String,
    /// Which flight model the airframe uses, in the model's own vocabulary
    /// (`fixed_wing`, `exceptional`).
    pub model_kind: String,
    /// The named handling profile (`fidelity`, `improved`).
    pub profile: String,
    /// Whether the declared assist set may contribute this tick.
    ///
    /// This is record identity, not one of the numeric tuning fields: F24
    /// non-negotiable behavior 5 requires the fidelity profile to fly with its
    /// assist contributions recorded as exactly zero, so only an explicitly
    /// named improved record may set this. The field names the assist set's
    /// numeric gains and limits; the boolean only decides whether they act.
    pub assists_enabled: bool,
    /// Where the record came from.
    pub origin: Origin,
    /// The claim the record backs.
    pub provenance: Provenance,
    /// The declared values.
    pub values: Vec<DeclaredTuningValue>,
}

impl DeclaredAirframeTuning {
    /// The known value of `name`, or `None` when the field is absent or
    /// explicitly unknown.
    #[must_use]
    pub fn known_value(&self, name: &str) -> Option<f64> {
        self.values
            .iter()
            .find(|value| value.field == name)
            .and_then(DeclaredTuningValue::known_value)
    }

    /// Every field the schema requires that the record does not state as
    /// known, in schema order.
    #[must_use]
    pub fn missing_required(&self, schema: &TuningSchema) -> Vec<&'static str> {
        schema
            .required()
            .filter(|field| {
                !self
                    .values
                    .iter()
                    .any(|value| value.field == field.name && value.known_value().is_some())
            })
            .map(|field| field.name)
            .collect()
    }

    /// Validates the record against `schema`.
    ///
    /// # Errors
    ///
    /// [`DeclaredTuningError`] naming the first problem: an empty identity,
    /// an undeclared or repeated field, a required field that is missing or
    /// explicitly unknown, or a known value that is non-finite or outside its
    /// approved range.
    pub fn validate(&self, schema: &TuningSchema) -> Result<(), DeclaredTuningError> {
        if self.id.trim().is_empty() {
            return Err(DeclaredTuningError::EmptyId);
        }
        if self.model_kind.trim().is_empty() {
            return Err(DeclaredTuningError::EmptyModelKind);
        }
        if self.profile.trim().is_empty() {
            return Err(DeclaredTuningError::EmptyProfile);
        }

        for (index, declared) in self.values.iter().enumerate() {
            let Some(field) = schema.field(&declared.field) else {
                return Err(DeclaredTuningError::UnknownField {
                    name: declared.field.clone(),
                });
            };
            if self.values[..index]
                .iter()
                .any(|earlier| earlier.field == declared.field)
            {
                return Err(DeclaredTuningError::DuplicateField {
                    name: declared.field.clone(),
                });
            }
            match declared.value.provenance() {
                None => {
                    if field.required {
                        return Err(DeclaredTuningError::RequiredFieldUnknown { name: field.name });
                    }
                }
                Some(_) => {
                    let value = declared
                        .known_value()
                        .expect("a provenance-backed value is known");
                    if !value.is_finite() {
                        return Err(DeclaredTuningError::NonFinite {
                            name: declared.field.clone(),
                        });
                    }
                    if !field.accepts(value) {
                        return Err(DeclaredTuningError::OutOfRange {
                            name: declared.field.clone(),
                            value,
                            min: field.range.min(),
                            max: field.range.max(),
                        });
                    }
                }
            }
        }

        for field in schema.required() {
            let stated = self.values.iter().any(|value| value.field == field.name);
            if !stated {
                return Err(DeclaredTuningError::MissingField { name: field.name });
            }
        }
        Ok(())
    }
}

/// The declared synthetic fixed-wing record: a bootstrap projection of
/// `cs_sim::flight::synthetic::synthetic_fixed_wing`.
///
/// Every value is newly authored development data (`Origin::SyntheticFixture`)
/// and validates against [`TuningSchema::fixed_wing`]. The values deliberately
/// match the numeric synthetic fixture so F24-C can assert the projection and
/// the mapping agree; they are not an original airframe and claim no original
/// coefficient.
#[must_use]
pub fn declared_synthetic_airframe() -> DeclaredAirframeTuning {
    let mut values = Vec::new();
    let mut push = |field: &str, value: f64| {
        // A claim id's grammar excludes `[`/`]`, so the array-index fields are
        // spelled with `-` in the id while the field name stays canonical.
        let suffix = field.replace(['[', ']'], "-");
        let provenance = Provenance::designed(
            ClaimId::new(&format!("f24a.tuning.synthetic-fixed-wing.{suffix}"))
                .expect("the declared claim id is valid"),
        );
        values.push(DeclaredTuningValue::known(field, value, provenance));
    };
    push("mass.mass_kg", 1200.0);
    push("mass.inertia_kg_m2[0]", 1400.0);
    push("mass.inertia_kg_m2[1]", 2100.0);
    push("mass.inertia_kg_m2[2]", 2600.0);
    push("engine.idle_thrust_n", 400.0);
    push("engine.max_thrust_n", 9000.0);
    push("engine.throttle_response_per_s", 1.5);
    push("boost.thrust_n", 3000.0);
    push("boost.consumption_per_s", 0.25);
    push("drag.zero_lift_coefficient", 0.03);
    push("drag.induced_coefficient", 0.06);
    push("lift.lift_at_zero_alpha", 0.15);
    push("lift.lift_slope_per_rad", 4.5);
    push("lift.max_lift_coefficient", 1.6);
    push("stall.stall_angle_rad", 0.28);
    push("stall.stall_width_rad", 0.18);
    push("stall.residual_fraction", 0.25);
    push("angular.rate_gain_per_s", 4.0);
    push("angular.rate_damping_per_s", 0.5);
    push("angular.max_rate_radps[0]", 2.0);
    push("angular.max_rate_radps[1]", 1.5);
    push("angular.max_rate_radps[2]", 1.0);
    push("angular.max_torque_nm[0]", 20_000.0);
    push("angular.max_torque_nm[1]", 30_000.0);
    push("angular.max_torque_nm[2]", 8_000.0);
    push("angular.control_airspeed_full_mps", 40.0);
    push("assists.bank_level_gain_nm_per_rad", 0.0);
    push("assists.bank_level_max_torque_nm", 1.0);
    push("reference_area_m2", 21.0);

    DeclaredAirframeTuning {
        id: "fixture.synthetic-fixed-wing".to_owned(),
        model_kind: "fixed_wing".to_owned(),
        profile: FIDELITY_PROFILE.to_owned(),
        assists_enabled: false,
        origin: Origin::SyntheticFixture,
        provenance: Provenance::designed(
            ClaimId::new("f24a.tuning.synthetic-fixed-wing")
                .expect("the declared claim id is valid"),
        ),
        values,
    }
}

/// The declared synthetic improved-handling airframe: the same synthetic
/// fixture data with the optional improved profile named explicitly.
///
/// It exists so F24-C can prove profile selection reaches the production
/// flight model instead of being a label nothing reads. The values differ only
/// in controller response and the bank/level assist, which is what "improved
/// handling" means for this designed model; the mass, engine, drag and lift
/// curves stay the synthetic airframe's, and nothing here is an original
/// coefficient (`F24` "Research boundary").
#[must_use]
pub fn declared_synthetic_improved_airframe() -> DeclaredAirframeTuning {
    let mut record = declared_synthetic_airframe();
    record.id = "fixture.synthetic-fixed-wing.improved".to_owned();
    record.profile = IMPROVED_PROFILE.to_owned();
    // The improved profile may use the declared bank/level assist; the fidelity
    // profile must not.
    record.assists_enabled = true;
    record.provenance = Provenance::designed(
        ClaimId::new("f24a.tuning.synthetic-fixed-wing.improved")
            .expect("the declared claim id is valid"),
    );
    // A more responsive, more damped controller plus the declared assist. These
    // are newly authored design values, not measurements.
    set_known(&mut record, "angular.rate_gain_per_s", 6.0);
    set_known(&mut record, "angular.rate_damping_per_s", 2.0);
    set_known(&mut record, "assists.bank_level_gain_nm_per_rad", 8_000.0);
    record
}

/// The declared synthetic airframe that `profile` names, or `None` for a
/// profile label the synthetic producer does not declare.
///
/// The explicit `None` is the point: an unknown or misspelled profile is
/// refused by the caller rather than falling back to the fidelity record
/// (F24 non-negotiable behavior 5).
#[must_use]
pub fn declared_synthetic_airframe_for(profile: &str) -> Option<DeclaredAirframeTuning> {
    match profile {
        FIDELITY_PROFILE => Some(declared_synthetic_airframe()),
        IMPROVED_PROFILE => Some(declared_synthetic_improved_airframe()),
        _ => None,
    }
}

/// Replaces (or adds) one known value of a declared record, keeping the
/// field's existing provenance when it already has one.
fn set_known(record: &mut DeclaredAirframeTuning, field: &str, value: f64) {
    let provenance = record
        .values
        .iter()
        .find(|declared| declared.field == field)
        .and_then(|declared| declared.value.provenance().cloned())
        .unwrap_or_else(|| {
            Provenance::designed(
                ClaimId::new("f24a.tuning.synthetic-fixed-wing.improved")
                    .expect("the declared claim id is valid"),
            )
        });
    if let Some(existing) = record
        .values
        .iter_mut()
        .find(|declared| declared.field == field)
    {
        existing.value = Resolved::Known(Known::new(value, provenance));
    } else {
        record
            .values
            .push(DeclaredTuningValue::known(field, value, provenance));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("a valid claim id")
    }

    /// The declared schema is a closed, duplicate-free vocabulary and its
    /// required/optional split is explicit (boost is the only optional group).
    #[test]
    fn accept_f24_a_fixed_wing_schema_is_unique_and_covers_the_model_fields() {
        let schema = TuningSchema::fixed_wing();
        assert!(schema.len() >= 25, "the schema covers every tuning group");
        assert!(!schema.is_empty());
        for (index, field) in schema.fields().iter().enumerate() {
            assert!(!field.name.is_empty());
            assert!(
                schema.fields()[..index]
                    .iter()
                    .all(|earlier| earlier.name != field.name),
                "{} is declared twice",
                field.name
            );
            assert!(field.range.contains(field.range.min()));
            assert!(field.range.contains(field.range.max()));
        }
        // A field the schema does not declare is not in the vocabulary.
        assert!(schema.field("mass.mass_kg").is_some());
        assert!(schema.field("warp.drive_power").is_none());

        // Boost equipment is optional; everything else is required.
        let boost: Vec<_> = schema
            .fields()
            .iter()
            .filter(|field| !field.required)
            .map(|field| field.name)
            .collect();
        assert_eq!(boost, vec!["boost.thrust_n", "boost.consumption_per_s"]);
    }

    /// The schema boundary refuses a duplicate and an empty name by name.
    #[test]
    fn accept_f24_a_schema_rejects_duplicate_and_empty_names() {
        let field = spec("mass.mass_kg", "kg", 1.0, 100.0, true);
        assert_eq!(
            TuningSchema::new(vec![field, field]),
            Err(TuningSchemaError::DuplicateName {
                name: "mass.mass_kg"
            })
        );

        // `PermittedRange` itself is the reason a reversed bound is unusable.
        assert_eq!(
            TuningFieldSpec::new("mass.mass_kg", "kg", 10.0, 1.0, true),
            Err(TuningSchemaError::Range {
                field: "mass.mass_kg",
                detail: RangeError::Reversed {
                    min: 10.0,
                    max: 1.0,
                },
            })
        );
        assert_eq!(
            TuningFieldSpec::new("  ", "kg", 0.0, 1.0, true),
            Err(TuningSchemaError::EmptyName)
        );
    }

    /// The declared synthetic record is complete, in range and explicitly not
    /// original; its values are the cs_sim synthetic fixture's.
    #[test]
    fn accept_f24_a_declared_synthetic_airframe_is_complete_and_synthetic() {
        let schema = TuningSchema::fixed_wing();
        let record = declared_synthetic_airframe();
        assert_eq!(record.validate(&schema), Ok(()));
        assert_eq!(record.origin, Origin::SyntheticFixture);
        assert!(!record.origin.is_original());
        assert!(record.missing_required(&schema).is_empty());
        assert_eq!(record.model_kind, "fixed_wing");
        assert_eq!(record.profile, "fidelity");
        assert_eq!(record.known_value("mass.mass_kg"), Some(1200.0));
        assert_eq!(record.known_value("reference_area_m2"), Some(21.0));
        assert_eq!(record.values.len(), schema.len(), "every field is stated");
    }

    /// A required field that is missing is refused, and a required field that
    /// is explicitly unknown is refused instead of being read as zero.
    #[test]
    fn accept_f24_a_missing_or_unknown_required_field_is_refused_not_zeroed() {
        let schema = TuningSchema::fixed_wing();

        let mut missing = declared_synthetic_airframe();
        missing.values.retain(|value| value.field != "mass.mass_kg");
        assert_eq!(
            missing.validate(&schema),
            Err(DeclaredTuningError::MissingField {
                name: "mass.mass_kg"
            })
        );
        assert_eq!(missing.known_value("mass.mass_kg"), None);

        let mut unknown = declared_synthetic_airframe();
        for value in &mut unknown.values {
            if value.field == "mass.mass_kg" {
                value.value = Resolved::unknown(claim("f24a.test.unknown-mass"), "not measured")
                    .expect("a reason is present");
            }
        }
        assert_eq!(
            unknown.validate(&schema),
            Err(DeclaredTuningError::RequiredFieldUnknown {
                name: "mass.mass_kg"
            })
        );
        assert_eq!(
            unknown.known_value("mass.mass_kg"),
            None,
            "unknown is not zero"
        );

        // An optional field may stay explicitly unknown and still validate.
        let mut no_boost = declared_synthetic_airframe();
        for value in &mut no_boost.values {
            if value.field == "boost.thrust_n" {
                value.value =
                    Resolved::unknown(claim("f24a.test.no-boost"), "no boost equipment measured")
                        .expect("a reason is present");
            }
        }
        assert_eq!(no_boost.validate(&schema), Ok(()));
        assert_eq!(no_boost.known_value("boost.thrust_n"), None);
    }

    /// A corrupt known value is refused by name, never clamped; an undeclared
    /// field is refused; a repeated field is refused.
    #[test]
    fn accept_f24_a_corrupt_or_undeclared_values_are_refused_by_name() {
        let schema = TuningSchema::fixed_wing();

        let mut out_of_range = declared_synthetic_airframe();
        for value in &mut out_of_range.values {
            if value.field == "stall.residual_fraction" {
                value.value = Resolved::Known(cs_types::content::Known::new(
                    1.5,
                    Provenance::designed(claim("f24a.test.oob")),
                ));
            }
        }
        assert_eq!(
            out_of_range.validate(&schema),
            Err(DeclaredTuningError::OutOfRange {
                name: "stall.residual_fraction".to_owned(),
                value: 1.5,
                min: 0.0,
                max: 1.0,
            })
        );

        let mut non_finite = declared_synthetic_airframe();
        for value in &mut non_finite.values {
            if value.field == "reference_area_m2" {
                value.value = Resolved::Known(cs_types::content::Known::new(
                    f64::NAN,
                    Provenance::designed(claim("f24a.test.nan")),
                ));
            }
        }
        assert_eq!(
            non_finite.validate(&schema),
            Err(DeclaredTuningError::NonFinite {
                name: "reference_area_m2".to_owned()
            })
        );

        let mut undeclared = declared_synthetic_airframe();
        undeclared.values.push(DeclaredTuningValue::known(
            "warp.drive_power",
            1.0,
            Provenance::designed(claim("f24a.test.undeclared")),
        ));
        assert_eq!(
            undeclared.validate(&schema),
            Err(DeclaredTuningError::UnknownField {
                name: "warp.drive_power".to_owned()
            })
        );

        let mut repeated = declared_synthetic_airframe();
        let mass = repeated
            .values
            .iter()
            .find(|value| value.field == "mass.mass_kg")
            .expect("the fixture states the mass")
            .clone();
        repeated.values.push(mass);
        assert_eq!(
            repeated.validate(&schema),
            Err(DeclaredTuningError::DuplicateField {
                name: "mass.mass_kg".to_owned()
            })
        );
    }

    /// A record with an empty identity is refused before its values matter.
    #[test]
    fn accept_f24_a_empty_identity_is_refused() {
        let schema = TuningSchema::fixed_wing();
        let mut record = declared_synthetic_airframe();
        record.id = "  ".to_owned();
        assert_eq!(record.validate(&schema), Err(DeclaredTuningError::EmptyId));
        record.id = "fixture".to_owned();
        record.model_kind.clear();
        assert_eq!(
            record.validate(&schema),
            Err(DeclaredTuningError::EmptyModelKind)
        );
        record.model_kind = "fixed_wing".to_owned();
        record.profile.clear();
        assert_eq!(
            record.validate(&schema),
            Err(DeclaredTuningError::EmptyProfile)
        );
    }

    /// F24-C: profile selection is explicit and never falls back to fidelity.
    /// The fidelity and improved records are distinct, both validate, and an
    /// unknown label is refused instead of silently selecting the fidelity
    /// record (F24 non-negotiable behavior 5).
    #[test]
    fn accept_f24_c_profile_selection_is_explicit_and_never_a_fallback() {
        let schema = TuningSchema::fixed_wing();

        let fidelity = declared_synthetic_airframe_for(FIDELITY_PROFILE)
            .expect("the fidelity profile is declared");
        assert_eq!(fidelity.profile, FIDELITY_PROFILE);
        assert!(!fidelity.assists_enabled);
        assert_eq!(fidelity.validate(&schema), Ok(()));

        let improved = declared_synthetic_airframe_for(IMPROVED_PROFILE)
            .expect("the improved profile is declared");
        assert_eq!(improved.profile, IMPROVED_PROFILE);
        assert!(improved.assists_enabled);
        assert_eq!(improved.validate(&schema), Ok(()));

        assert_ne!(
            fidelity, improved,
            "the named profiles must be different records"
        );
        assert_ne!(
            fidelity.known_value("angular.rate_gain_per_s"),
            improved.known_value("angular.rate_gain_per_s"),
            "the improved profile changes the controller, not only its label"
        );
        assert_eq!(
            declared_synthetic_airframe_for("autogyro"),
            None,
            "an undeclared profile is refused, never the fidelity fallback"
        );
    }

    /// F24-C: the improved record keeps the synthetic airframe's fixtures
    /// (mass, engine, area) and changes only the controller and assist, so a
    /// profile swap can never smuggle in different airframe data.
    #[test]
    fn accept_f24_c_improved_profile_changes_only_handling() {
        let schema = TuningSchema::fixed_wing();
        let fidelity = declared_synthetic_airframe();
        let improved = declared_synthetic_improved_airframe();

        for field in [
            "mass.mass_kg",
            "mass.inertia_kg_m2[0]",
            "engine.max_thrust_n",
            "drag.zero_lift_coefficient",
            "lift.lift_slope_per_rad",
            "reference_area_m2",
        ] {
            assert_eq!(
                fidelity.known_value(field),
                improved.known_value(field),
                "{field} must not change between profiles"
            );
        }
        assert_eq!(improved.validate(&schema), Ok(()));
        assert_eq!(improved.origin, Origin::SyntheticFixture);
        assert!(!improved.origin.is_original());
        assert_eq!(improved.values.len(), schema.len(), "every field is stated");
    }
}

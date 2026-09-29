//! Quantity normalization with units and permitted ranges (F14-B).
//!
//! Spec F14 non-negotiable behavior 3 ("All referenced quantities have units
//! and permitted ranges. A missing critical value is an error, not
//! `Default::default()`") and the `IDENTITY-CONTENT` numeric contract
//! ("Normalization creates meters, seconds, radians, kilograms or explicitly
//! documented game-weight units … Unknown original units are
//! `Resolved::Unknown`, not assumed SI"). This module is the smallest
//! production path that turns a raw declared number into a canonical
//! [`NormalizedField`]:
//!
//! * a [`QuantityRule`] declares the canonical [`Unit`], the approved
//!   [`PermittedRange`] and how the raw number's unit is known
//!   ([`UnitScale`]): already canonical, a declared source conversion with an
//!   evidence-carrying factor, or unrecognized;
//! * [`normalize_field`] refuses a missing value, an unrecognized unit, a
//!   non-finite value or conversion factor and an out-of-range value, each
//!   naming the field and quoting the offending value. It never substitutes a
//!   zero, a default or an assumed SI unit;
//! * [`normalize_element`] runs the declared rules over an element's raw
//!   fields in canonical name order and produces the element's
//!   [`NormalizeState`]: `Normalized` only when every field normalized, and
//!   `Failed` with the first failure's diagnostic otherwise. Parsing is a
//!   different state and is deliberately untouched here (non-negotiable
//!   behavior 1).
//!
//! Nothing here is derived from original game data. A declared conversion is
//! an authored (or measured) declaration carrying its own provenance; an
//! undeclared unit is a refusal, never a guess.

use std::collections::BTreeMap;
use std::fmt;

use cs_types::content::{NormalizeState, PermittedRange, Provenance, Resolved, Unit};

/// How a raw number's unit is known relative to its canonical [`Unit`].
#[derive(Clone, Debug, PartialEq)]
pub enum UnitScale {
    /// The raw number is already expressed in the canonical unit.
    Canonical,
    /// The raw number is in a declared source unit; multiply by
    /// `canonical_per_source` to reach the canonical unit. The factor is a
    /// claim with its own provenance (a measured or authored conversion),
    /// never an assumed SI ratio.
    Declared {
        /// The source unit's spelling, kept for the diagnostic.
        source_label: String,
        /// How many canonical units one source unit is worth. It must be
        /// finite and strictly positive.
        canonical_per_source: f64,
        /// Where the conversion factor came from.
        provenance: Provenance,
    },
    /// The bytes carry a number but no recognized unit. The numeric contract
    /// refuses to assume SI, so this is a normalization failure.
    Unknown {
        /// The unrecognized unit spelling.
        source_label: String,
    },
}

/// The declared rule for one normalized field: its canonical unit, its
/// approved inclusive range and how its raw unit is known.
#[derive(Clone, Debug, PartialEq)]
pub struct QuantityRule {
    /// The canonical unit the normalized value is expressed in.
    pub unit: Unit,
    /// The approved inclusive range of the normalized value.
    pub range: PermittedRange,
    /// How the raw number's unit is known.
    pub scale: UnitScale,
}

impl QuantityRule {
    /// A rule for a value already in the canonical unit.
    pub fn canonical(unit: Unit, range: PermittedRange) -> Self {
        Self {
            unit,
            range,
            scale: UnitScale::Canonical,
        }
    }

    /// A rule for a value in a declared source unit.
    pub fn converted(
        unit: Unit,
        range: PermittedRange,
        source_label: &str,
        canonical_per_source: f64,
        provenance: Provenance,
    ) -> Self {
        Self {
            unit,
            range,
            scale: UnitScale::Declared {
                source_label: source_label.to_owned(),
                canonical_per_source,
                provenance,
            },
        }
    }

    /// A rule whose raw unit is unrecognized; normalization will refuse it.
    pub fn unknown_unit(unit: Unit, range: PermittedRange, source_label: &str) -> Self {
        Self {
            unit,
            range,
            scale: UnitScale::Unknown {
                source_label: source_label.to_owned(),
            },
        }
    }
}

/// One raw field before normalization: its name and its value, which is
/// either known with provenance or explicitly unknown.
#[derive(Clone, Debug, PartialEq)]
pub struct FieldInput {
    /// The field name, unique within an element.
    pub name: String,
    /// The raw value, resolved through the F01/F14 schema.
    pub value: Resolved<f64>,
}

/// A value normalized into its canonical unit, with provenance.
#[derive(Clone, Debug, PartialEq)]
pub struct NormalizedField {
    /// The field name.
    pub name: String,
    /// The canonical unit.
    pub unit: Unit,
    /// The normalized value, finite and inside the rule's range.
    pub value: f64,
    /// Where the value came from.
    pub provenance: Provenance,
}

/// Why a field could not be normalized.
#[derive(Clone, Debug, PartialEq)]
pub enum NormalizeError {
    /// The value is explicitly unknown, and it is a critical field.
    MissingValue {
        /// The field name.
        field: String,
        /// Why the value is unknown.
        reason: String,
    },
    /// The raw number's unit is not recognized and no SI unit may be assumed.
    UnknownUnit {
        /// The field name.
        field: String,
        /// The unrecognized unit spelling.
        source_label: String,
    },
    /// The value (or the result of its conversion) was NaN or infinite.
    NonFinite {
        /// The field name.
        field: String,
        /// The offending value.
        value: f64,
    },
    /// The declared conversion factor was not finite and strictly positive.
    InvalidConversionFactor {
        /// The field name.
        field: String,
        /// The declared factor.
        factor: f64,
    },
    /// The normalized value fell outside the approved inclusive range.
    OutOfRange {
        /// The field name.
        field: String,
        /// The normalized value.
        value: f64,
        /// The approved range.
        range: PermittedRange,
        /// The canonical unit.
        unit: Unit,
    },
    /// Two raw fields shared one name.
    DuplicateField {
        /// The repeated field name.
        field: String,
    },
    /// No declared rule covered a raw field.
    MissingRule {
        /// The undeclared field name.
        field: String,
    },
}

impl fmt::Display for NormalizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::MissingValue { field, reason } => {
                write!(f, "critical field {field:?} is unknown: {reason}")
            }
            Self::UnknownUnit {
                field,
                source_label,
            } => write!(
                f,
                "field {field:?} has unrecognized unit {source_label:?}; refusing to assume SI"
            ),
            Self::NonFinite { field, value } => {
                write!(f, "field {field:?} is not finite: {value}")
            }
            Self::InvalidConversionFactor { field, factor } => write!(
                f,
                "field {field:?} declares conversion factor {factor}, which is not finite and positive"
            ),
            Self::OutOfRange {
                field,
                value,
                range,
                unit,
            } => write!(
                f,
                "field {field:?} value {value} {unit} is outside the approved range {range}"
            ),
            Self::DuplicateField { field } => {
                write!(f, "field {field:?} is declared more than once")
            }
            Self::MissingRule { field } => {
                write!(f, "field {field:?} has no declared quantity rule")
            }
        }
    }
}

impl std::error::Error for NormalizeError {}

/// Normalizes one raw field against its declared rule.
///
/// # Errors
///
/// [`NormalizeError::MissingValue`] for an explicitly unknown critical value,
/// [`NormalizeError::UnknownUnit`] when the raw unit is unrecognized,
/// [`NormalizeError::InvalidConversionFactor`] for a non-positive or
/// non-finite declared factor, [`NormalizeError::NonFinite`] for a NaN or
/// infinite value and [`NormalizeError::OutOfRange`] for a value outside the
/// approved range.
pub fn normalize_field(
    input: &FieldInput,
    rule: &QuantityRule,
) -> Result<NormalizedField, NormalizeError> {
    let known = match &input.value {
        Resolved::Unknown { reason, .. } => {
            return Err(NormalizeError::MissingValue {
                field: input.name.clone(),
                reason: reason.clone(),
            });
        }
        Resolved::Known(known) => known,
    };
    let value = match &rule.scale {
        UnitScale::Canonical => known.value,
        UnitScale::Declared {
            canonical_per_source,
            ..
        } => {
            if !canonical_per_source.is_finite() || *canonical_per_source <= 0.0 {
                return Err(NormalizeError::InvalidConversionFactor {
                    field: input.name.clone(),
                    factor: *canonical_per_source,
                });
            }
            known.value * canonical_per_source
        }
        UnitScale::Unknown { source_label } => {
            return Err(NormalizeError::UnknownUnit {
                field: input.name.clone(),
                source_label: source_label.clone(),
            });
        }
    };
    if !value.is_finite() {
        return Err(NormalizeError::NonFinite {
            field: input.name.clone(),
            value,
        });
    }
    if !rule.range.contains(value) {
        return Err(NormalizeError::OutOfRange {
            field: input.name.clone(),
            value,
            range: rule.range,
            unit: rule.unit,
        });
    }
    Ok(NormalizedField {
        name: input.name.clone(),
        unit: rule.unit,
        value,
        provenance: known.provenance.clone(),
    })
}

/// The result of normalizing one element's declared fields.
///
/// `state` is `Normalized` only when every field normalized, and `fields` is
/// then in canonical field-name order. A failure yields
/// `NormalizeState::Failed` carrying the first failure's diagnostic (a failure
/// is never a silently defaulted value).
#[derive(Clone, Debug, PartialEq)]
pub struct ElementNormalization {
    /// The element's normalization state.
    pub state: NormalizeState,
    /// The normalized fields; empty on failure.
    pub fields: Vec<NormalizedField>,
}

fn failed(error: NormalizeError) -> ElementNormalization {
    ElementNormalization {
        state: NormalizeState::Failed {
            diagnostic: error.to_string(),
        },
        fields: Vec::new(),
    }
}

/// Normalizes an element's raw fields against its declared rules.
///
/// Fields are processed in canonical name order so the produced
/// [`NormalizedField`] list and the diagnostic of the first failure are
/// independent of the caller's enumeration order.
pub fn normalize_element(
    fields: &[FieldInput],
    rules: &BTreeMap<String, QuantityRule>,
) -> ElementNormalization {
    let mut by_name: BTreeMap<&str, &FieldInput> = BTreeMap::new();
    for input in fields {
        if by_name.insert(input.name.as_str(), input).is_some() {
            return failed(NormalizeError::DuplicateField {
                field: input.name.clone(),
            });
        }
    }
    let mut normalized = Vec::with_capacity(by_name.len());
    for (name, input) in &by_name {
        let Some(rule) = rules.get(*name) else {
            return failed(NormalizeError::MissingRule {
                field: (*name).to_owned(),
            });
        };
        match normalize_field(input, rule) {
            Ok(field) => normalized.push(field),
            Err(error) => return failed(error),
        }
    }
    ElementNormalization {
        state: NormalizeState::Normalized,
        fields: normalized,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use cs_types::content::{Known, Resolved};
    use cs_types::evidence::{ClaimId, ClaimStatus};

    fn claim(id: &str) -> ClaimId {
        ClaimId::new(id).expect("test claim id is valid")
    }

    fn designed() -> Provenance {
        Provenance::designed(claim("f14_b.test.conversion"))
    }

    fn known(value: f64) -> Resolved<f64> {
        Resolved::Known(Known::new(value, designed()))
    }

    fn field(name: &str, value: Resolved<f64>) -> FieldInput {
        FieldInput {
            name: name.to_owned(),
            value,
        }
    }

    fn range(min: f64, max: f64) -> PermittedRange {
        PermittedRange::new(min, max).expect("test range is valid")
    }

    /// F14-B units/ranges: a canonical value inside its range normalizes, and
    /// a declared conversion is applied with its evidence-carrying factor.
    #[test]
    fn accept_f14_b_normalize_canonical_and_converted_values() {
        let rule = QuantityRule::canonical(Unit::Meters, range(0.0, 100.0));
        let normalized =
            normalize_field(&field("wingspan", known(12.5)), &rule).expect("12.5 m is in range");
        assert_eq!(normalized.unit, Unit::Meters);
        assert_eq!(normalized.value, 12.5);
        assert_eq!(normalized.provenance.class, ClaimStatus::Designed);

        // 10 feet converted by the declared 0.3048 m/ft factor.
        let feet =
            QuantityRule::converted(Unit::Meters, range(0.0, 100.0), "feet", 0.3048, designed());
        let normalized =
            normalize_field(&field("wingspan", known(10.0)), &feet).expect("3.048 m is in range");
        assert!((normalized.value - 3.048).abs() < 1e-12);
    }

    /// F14-B refusal paths: every way a critical value can be missing,
    /// unlabeled or out of range is a named error, never a default.
    #[test]
    fn accept_f14_b_normalize_refuses_missing_unknown_and_out_of_range() {
        let rule = QuantityRule::canonical(Unit::Seconds, range(0.0, 10.0));

        assert_eq!(
            normalize_field(
                &field(
                    "warmup",
                    Resolved::unknown(claim("f14_b.unknown.warmup"), "no observed value")
                        .expect("reason is present")
                ),
                &rule
            ),
            Err(NormalizeError::MissingValue {
                field: "warmup".to_owned(),
                reason: "no observed value".to_owned(),
            })
        );

        assert!(matches!(
            normalize_field(&field("warmup", known(f64::NAN)), &rule),
            Err(NormalizeError::NonFinite { value, .. }) if value.is_nan()
        ));

        assert!(matches!(
            normalize_field(&field("warmup", known(11.0)), &rule),
            Err(NormalizeError::OutOfRange { .. })
        ));

        let undeclared_unit = QuantityRule::unknown_unit(Unit::Seconds, range(0.0, 10.0), "ticks");
        assert_eq!(
            normalize_field(&field("warmup", known(3.0)), &undeclared_unit),
            Err(NormalizeError::UnknownUnit {
                field: "warmup".to_owned(),
                source_label: "ticks".to_owned(),
            })
        );

        let zero_factor =
            QuantityRule::converted(Unit::Seconds, range(0.0, 10.0), "ticks", 0.0, designed());
        assert!(matches!(
            normalize_field(&field("warmup", known(3.0)), &zero_factor),
            Err(NormalizeError::InvalidConversionFactor { .. })
        ));
    }

    /// F14-B element normalization: canonical name order, all-or-failed, and
    /// the parsing state is not touched by normalization.
    #[test]
    fn accept_f14_b_normalize_element_is_ordered_and_all_or_failed() {
        let mut rules = BTreeMap::new();
        rules.insert(
            "alpha".to_owned(),
            QuantityRule::canonical(Unit::Meters, range(0.0, 10.0)),
        );
        rules.insert(
            "beta".to_owned(),
            QuantityRule::canonical(Unit::Seconds, range(0.0, 10.0)),
        );

        // Inputs enumerated out of canonical order still normalize in order.
        let outcome = normalize_element(
            &[field("beta", known(2.0)), field("alpha", known(1.0))],
            &rules,
        );
        assert_eq!(outcome.state, NormalizeState::Normalized);
        assert_eq!(
            outcome
                .fields
                .iter()
                .map(|field| field.name.as_str())
                .collect::<Vec<_>>(),
            vec!["alpha", "beta"]
        );

        let failure = normalize_element(&[field("beta", known(99.0))], &rules);
        assert!(matches!(failure.state, NormalizeState::Failed { .. }));
        assert!(failure.fields.is_empty());

        let duplicate = normalize_element(
            &[field("alpha", known(1.0)), field("alpha", known(2.0))],
            &rules,
        );
        assert_eq!(
            duplicate.state,
            NormalizeState::Failed {
                diagnostic: NormalizeError::DuplicateField {
                    field: "alpha".to_owned()
                }
                .to_string()
            }
        );

        let undeclared = normalize_element(&[field("gamma", known(1.0))], &rules);
        assert!(matches!(undeclared.state, NormalizeState::Failed { .. }));
    }
}

//! Acceptance scenario F14-B through quantity normalization.
//!
//! These tests exercise production code only:
//! `cs_content::catalog::normalize::{normalize_field, normalize_element}` over
//! `cs_types::content` values. Removing the unit/range/refusal behaviour makes
//! them fail.

use std::collections::BTreeMap;

use cs_content::catalog::normalize::{
    FieldInput, NormalizeError, QuantityRule, normalize_element, normalize_field,
};
use cs_types::content::{Known, NormalizeState, PermittedRange, Provenance, Resolved, Unit};
use cs_types::evidence::{ClaimId, ClaimStatus};

fn claim(id: &str) -> ClaimId {
    ClaimId::new(id).expect("test claim id is valid")
}

fn designed() -> Provenance {
    Provenance::designed(claim("f14_b.normalize.design"))
}

fn range(min: f64, max: f64) -> PermittedRange {
    PermittedRange::new(min, max).expect("test range is valid")
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

/// Units are canonical and conversions are declared, not assumed; the
/// normalized value carries the raw value's provenance.
#[test]
fn accept_f14_b_normalization_converts_declared_units_with_provenance() {
    let known_value = Resolved::Known(Known::new(
        10.0,
        Provenance::new(
            claim("f14_b.normalize.wingspan"),
            ClaimStatus::ObservedTool,
            None,
        )
        .expect("observed without a span is allowed"),
    ));
    let feet = QuantityRule::converted(Unit::Meters, range(0.0, 100.0), "feet", 0.3048, designed());
    let normalized = normalize_field(&field("wingspan", known_value), &feet)
        .expect("10 ft is 3.048 m, inside the range");
    assert_eq!(normalized.unit, Unit::Meters);
    assert!((normalized.value - 3.048).abs() < 1e-12);
    assert_eq!(normalized.provenance.class, ClaimStatus::ObservedTool);

    let seconds = QuantityRule::canonical(Unit::Seconds, range(0.0, 60.0));
    let normalized =
        normalize_field(&field("warmup", known(30.0)), &seconds).expect("30 s is in range");
    assert_eq!(normalized.value, 30.0);
    assert_eq!(normalized.unit, Unit::Seconds);

    assert_eq!(Unit::from_label("radians"), Some(Unit::Radians));
    assert_eq!(Unit::from_label("furlongs"), None);
}

/// Every refusal path is named and nothing is defaulted.
#[test]
fn accept_f14_b_normalization_refuses_missing_unknown_nonfinite_and_out_of_range() {
    let rule = QuantityRule::canonical(Unit::Kilograms, range(1.0, 500.0));

    assert_eq!(
        normalize_field(
            &field(
                "mass",
                Resolved::unknown(claim("f14_b.unknown.mass"), "not observed")
                    .expect("reason is present")
            ),
            &rule
        ),
        Err(NormalizeError::MissingValue {
            field: "mass".to_owned(),
            reason: "not observed".to_owned(),
        }),
        "a missing critical value is an error, never a default"
    );

    assert_eq!(
        normalize_field(&field("mass", known(f64::INFINITY)), &rule),
        Err(NormalizeError::NonFinite {
            field: "mass".to_owned(),
            value: f64::INFINITY,
        })
    );

    let out_of_range = normalize_field(&field("mass", known(0.5)), &rule)
        .expect_err("0.5 kg is below the approved range");
    assert_eq!(
        out_of_range,
        NormalizeError::OutOfRange {
            field: "mass".to_owned(),
            value: 0.5,
            range: range(1.0, 500.0),
            unit: Unit::Kilograms,
        }
    );

    let unknown_unit = QuantityRule::unknown_unit(Unit::Kilograms, range(0.0, 500.0), "stones");
    assert_eq!(
        normalize_field(&field("mass", known(3.0)), &unknown_unit),
        Err(NormalizeError::UnknownUnit {
            field: "mass".to_owned(),
            source_label: "stones".to_owned(),
        }),
        "an unrecognized unit is refused, never assumed to be SI"
    );

    let bad_factor = QuantityRule::converted(
        Unit::Kilograms,
        range(0.0, 500.0),
        "stones",
        -1.0,
        designed(),
    );
    assert!(matches!(
        normalize_field(&field("mass", known(3.0)), &bad_factor),
        Err(NormalizeError::InvalidConversionFactor { .. })
    ));
}

/// Element normalization is deterministic and all-or-failed: canonical field
/// order, one failure fails the element, and duplicates are refused.
#[test]
fn accept_f14_b_element_normalization_is_deterministic_and_all_or_failed() {
    let mut rules = BTreeMap::new();
    rules.insert(
        "alpha".to_owned(),
        QuantityRule::canonical(Unit::Meters, range(0.0, 10.0)),
    );
    rules.insert(
        "beta".to_owned(),
        QuantityRule::canonical(Unit::Seconds, range(0.0, 10.0)),
    );

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
        vec!["alpha", "beta"],
        "normalized fields are in canonical name order"
    );

    let failed = normalize_element(
        &[field("alpha", known(1.0)), field("beta", known(99.0))],
        &rules,
    );
    assert_eq!(
        failed.state,
        NormalizeState::Failed {
            diagnostic: NormalizeError::OutOfRange {
                field: "beta".to_owned(),
                value: 99.0,
                range: range(0.0, 10.0),
                unit: Unit::Seconds,
            }
            .to_string()
        }
    );
    assert!(
        failed.fields.is_empty(),
        "a failed element stores no fields"
    );

    let missing_rule = normalize_element(&[field("gamma", known(1.0))], &rules);
    assert!(matches!(missing_rule.state, NormalizeState::Failed { .. }));
}

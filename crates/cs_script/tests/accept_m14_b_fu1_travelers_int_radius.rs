//! M14-B-FU1: `TRAVELERS` reads its radius by type tag, so an int is valid.
//!
//! The original's parse at `0x467a70` copies a real child3's bits and
//! converts an int one with `fild dword; fstp dword` (`0x467cb6`-`0x467cbe`);
//! text and a list are rejected. M14's `OBJECTIVE25` spells `1000`. Nothing
//! here is an original run.

use cs_script::conditions::{BlockDirective, ConditionRefusal, lower_block_condition};
use cs_script::ir::{Condition, Value};

fn s(text: &str) -> Value {
    Value::Str(text.to_owned())
}

fn site(subject: Value, radius: Value) -> Vec<BlockDirective> {
    vec![BlockDirective::new(
        "TRAVELERS",
        vec![subject, s("APPROACHING"), s("anchor"), radius],
    )]
}

fn leaf(spelled: Vec<BlockDirective>) -> Result<Condition, ConditionRefusal> {
    lower_block_condition("OBJECTIVE25", 0, &spelled).map(|condition| match condition {
        Condition::All(mut items) if items.len() == 2 => items.pop().unwrap(),
        other => other,
    })
}

#[test]
fn accept_m14_b_fu1_an_int_radius_lowers_in_subject_mode() {
    let Condition::Travelers { radius, .. } =
        leaf(site(s("player"), Value::Int(1000))).expect("an int radius lowers")
    else {
        panic!("a text child0 takes subject mode");
    };
    assert_eq!(radius, 1000.0);
}

#[test]
fn accept_m14_b_fu1_an_int_radius_lowers_in_counting_mode() {
    let Condition::TravelersCount { radius, group, .. } =
        leaf(site(Value::Int(2), Value::Int(200))).expect("an int radius lowers")
    else {
        panic!("a numeric child0 takes counting mode");
    };
    assert_eq!((group, radius), (2, 200.0));
}

#[test]
fn accept_m14_b_fu1_an_int_radius_equals_the_float_of_the_same_value() {
    for value in [0, 1, 1000, -5, 16_777_216] {
        assert_eq!(
            leaf(site(s("player"), Value::Int(value))).unwrap(),
            leaf(site(s("player"), Value::Float(f64::from(value)))).unwrap(),
            "{value}"
        );
    }
    // `fild` then `fstp dword` rounds to f32: 16777217 is not representable.
    let Condition::Travelers { radius, .. } =
        leaf(site(s("player"), Value::Int(16_777_217))).unwrap()
    else {
        panic!("subject mode");
    };
    assert_eq!(radius, 16_777_216.0);
}

#[test]
fn accept_m14_b_fu1_text_list_and_non_finite_radii_are_still_refused() {
    for radius in [
        s("1000"),
        Value::List(vec![Value::Int(1)]),
        Value::Float(f64::NAN),
        Value::Float(f64::INFINITY),
    ] {
        for subject in [s("player"), Value::Int(2)] {
            assert!(
                matches!(
                    leaf(site(subject, radius.clone())),
                    Err(ConditionRefusal::BadOperands { .. })
                ),
                "{radius:?}"
            );
        }
    }
}

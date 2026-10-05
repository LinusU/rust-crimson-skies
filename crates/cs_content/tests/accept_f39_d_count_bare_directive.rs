//! F39-D-COUNT: a bare directive must not swallow the key after it.
//!
//! The original spells a no-argument directive (`INSTANTWIN`, `INSTANTLOSS`) by
//! leaving the next key beside it. The flat (text, value) pairing paired the
//! bare key with the next key's spelling and skipped that key, undercounting it.

use cs_content::objectives::{OBJECTIVE_DORMANT_KEY, measure_dormant_declarations};
use cs_content::stunts::{
    ZrdValue, objective_state_machine, zrd_directive_fields, zrd_flat_fields, zrd_is_bare_argument,
};

fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

/// `OBJECTIVE1 [INSTANTWIN BEGIN_DORMANT [-1.0] IDENTITY [PRIMARY]]`: a bare
/// directive, then the keyed directive it used to swallow, then another.
fn document() -> ZrdValue {
    ZrdValue::List(vec![
        text("OBJECTIVE1"),
        ZrdValue::List(vec![
            text("INSTANTWIN"),
            text(OBJECTIVE_DORMANT_KEY),
            ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
            text("IDENTITY"),
            ZrdValue::List(vec![text("PRIMARY")]),
        ]),
    ])
}

fn block(document: &ZrdValue) -> &ZrdValue {
    &document.as_list().expect("a list")[1]
}

#[test]
fn accept_f39_d_count_the_key_after_a_bare_directive_is_kept() {
    let document = document();
    let fields = zrd_directive_fields(block(&document));
    let keys: Vec<&str> = fields.iter().map(|(key, _)| *key).collect();
    assert_eq!(keys, ["INSTANTWIN", OBJECTIVE_DORMANT_KEY, "IDENTITY"]);
    assert!(zrd_is_bare_argument(fields[0].1));
    assert!(!zrd_is_bare_argument(fields[1].1));

    // The flat walk is what dropped it: `INSTANTWIN` paired with the key's
    // spelling, `BEGIN_DORMANT` gone.
    let flat: Vec<&str> = zrd_flat_fields(block(&document))
        .iter()
        .map(|(key, _)| *key)
        .collect();
    assert!(!flat.contains(&OBJECTIVE_DORMANT_KEY));

    let machine = objective_state_machine(&document);
    let count = |key: &str| {
        machine
            .keys()
            .iter()
            .find(|(name, _)| name == key)
            .map(|(_, count)| *count)
    };
    assert_eq!(count(OBJECTIVE_DORMANT_KEY), Some(1));
    assert_eq!(count("INSTANTWIN"), Some(1));
    assert_eq!(count("IDENTITY"), Some(1));
}

#[test]
fn accept_f39_d_count_the_dormant_reader_sees_the_follower() {
    let document = ZrdValue::List(vec![
        text("OBJECTIVE1"),
        ZrdValue::List(vec![
            text("INSTANTLOSS"),
            text(OBJECTIVE_DORMANT_KEY),
            ZrdValue::List(vec![ZrdValue::Float(-1.0)]),
        ]),
    ]);
    let measured = measure_dormant_declarations(&document).expect("the block reads");
    assert_eq!(measured.len(), 1);
    assert!(measured[0].begins_dormant());
}

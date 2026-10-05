//! Task #513: the campaign `dzones.zrd` member's grammar and refusals.
//!
//! Every byte here is authored for this file; nothing is derived from original
//! data. The grammar under test was measured over all 23 retail members by the
//! `accept_t427_dzones_retail_*` tests in `cs_app`. Task test prefix:
//! `accept_t427_dzones_`.

use cs_formats::zbd::detection_zones::{
    DetectionZoneKey, DetectionZonesError, KeyMeaning, read_detection_zones,
};
use cs_types::evidence::ClaimStatus;

fn int(value: u32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn text(value: &str) -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

/// A list of `children`, stored with the measured word: children plus one.
fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

fn names(zones: &[&str]) -> Vec<u8> {
    list(&zones.iter().map(|zone| text(zone)).collect::<Vec<_>>())
}

fn member() -> Vec<u8> {
    list(&[
        text("disable"),
        names(&["dzpath5", "dzpath6"]),
        text("nosnapshot"),
        names(&["dzpath9"]),
        text("objective_numbers"),
        list(&[
            list(&[text("dzpath1"), int(18)]),
            list(&[text("dzpath2"), int(19)]),
        ]),
    ])
}

#[test]
fn accept_t427_dzones_a_list_word_is_its_child_count_plus_one() {
    let declared = read_detection_zones(&member()).expect("the authored member decodes");
    assert_eq!(
        declared.keys(),
        [
            DetectionZoneKey::Disable,
            DetectionZoneKey::NoSnapshot,
            DetectionZoneKey::ObjectiveNumbers
        ]
    );
    assert_eq!(declared.disable().unwrap(), ["dzpath5", "dzpath6"]);
    assert_eq!(declared.no_snapshot().unwrap(), ["dzpath9"]);
    assert_eq!(
        declared.objective_numbers().unwrap(),
        [("dzpath1".to_owned(), 18), ("dzpath2".to_owned(), 19)]
    );
    assert_eq!(declared.named_zones().len(), 5);

    // A member that states only some keys is a member that does not state the
    // others, not an error and not an empty list.
    let only = list(&[text("nosnapshot"), names(&["dzpath4"])]);
    let declared = read_detection_zones(&only).expect("one key decodes");
    assert!(declared.disable().is_none());
    assert!(declared.objective_numbers().is_none());
    assert_eq!(declared.no_snapshot().unwrap(), ["dzpath4"]);
}

#[test]
fn accept_t427_dzones_every_refusal_is_named_with_its_offset() {
    let good = member();

    // Truncation, at every possible length.
    for cut in 0..good.len() {
        let error = read_detection_zones(&good[..cut]).expect_err("a truncated member is refused");
        assert!(
            matches!(
                error,
                DetectionZonesError::Truncated { .. }
                    | DetectionZonesError::LengthDoesNotFit { .. }
            ),
            "cut at {cut}: {error}"
        );
    }

    // An undefined tag, including the general grammar's float tag.
    for tag in [0u32, 2, 5, 0xFFFF_FFFF] {
        let mut bytes = good.clone();
        bytes[0..4].copy_from_slice(&tag.to_le_bytes());
        assert_eq!(
            read_detection_zones(&bytes),
            Err(DetectionZonesError::UndefinedTag { offset: 0, tag })
        );
    }

    // A string length that does not fit.
    let mut bytes = list(&[text("disable"), names(&[])]);
    bytes[12..16].copy_from_slice(&1000u32.to_le_bytes());
    assert!(matches!(
        read_detection_zones(&bytes),
        Err(DetectionZonesError::LengthDoesNotFit { declared: 1000, .. })
    ));

    // A list word whose children cannot fit.
    let mut bytes = good.clone();
    bytes[4..8].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
    assert!(matches!(
        read_detection_zones(&bytes),
        Err(DetectionZonesError::LengthDoesNotFit { .. })
    ));

    // A list word of zero.
    let mut bytes = good.clone();
    bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        read_detection_zones(&bytes),
        Err(DetectionZonesError::ZeroListWord { offset: 0 })
    );

    // Trailing bytes.
    let mut bytes = good.clone();
    bytes.extend([0, 0, 0]);
    assert_eq!(
        read_detection_zones(&bytes),
        Err(DetectionZonesError::TrailingBytes {
            offset: good.len() as u64,
            count: 3
        })
    );

    // Not UTF-8.
    let mut bytes = text("x");
    *bytes.last_mut().unwrap() = 0xFF;
    let bytes = list(&[bytes, names(&[])]);
    assert!(matches!(
        read_detection_zones(&bytes),
        Err(DetectionZonesError::InvalidText { .. })
    ));
}

#[test]
fn accept_t427_dzones_a_record_outside_the_measured_shape_is_refused() {
    // The root is not a list.
    assert_eq!(
        read_detection_zones(&int(1)),
        Err(DetectionZonesError::NotAKeyedRecord { offset: 0 })
    );
    // An odd number of children: a key without a value.
    assert_eq!(
        read_detection_zones(&list(&[text("disable")])),
        Err(DetectionZonesError::NotAKeyedRecord { offset: 0 })
    );
    // A key that is not a string.
    assert_eq!(
        read_detection_zones(&list(&[int(1), names(&[])])),
        Err(DetectionZonesError::NotAKeyedRecord { offset: 0 })
    );
    // A key the 23 members never use.
    assert_eq!(
        read_detection_zones(&list(&[text("enable"), names(&[])])),
        Err(DetectionZonesError::UnknownKey {
            key: "enable".to_owned()
        })
    );
    // A key stated twice.
    assert_eq!(
        read_detection_zones(&list(&[
            text("disable"),
            names(&[]),
            text("disable"),
            names(&[])
        ])),
        Err(DetectionZonesError::DuplicateKey { key: "disable" })
    );
    // A value of the wrong shape: an integer where names belong, a name where a
    // pair belongs.
    assert_eq!(
        read_detection_zones(&list(&[text("disable"), int(3)])),
        Err(DetectionZonesError::WrongValueShape { key: "disable" })
    );
    assert_eq!(
        read_detection_zones(&list(&[
            text("objective_numbers"),
            list(&[text("dzpath1")])
        ])),
        Err(DetectionZonesError::WrongValueShape {
            key: "objective_numbers"
        })
    );
}

#[test]
fn accept_t427_dzones_what_a_key_means_is_recorded_as_unknown() {
    for key in DetectionZoneKey::ALL {
        assert_eq!(key.meaning(), KeyMeaning::Unknown);
        assert_eq!(key.meaning().evidence(), ClaimStatus::Unknown);
        assert_eq!(DetectionZoneKey::from_spelling(key.spelling()), Some(key));
        assert!(!key.value_shape().is_empty());
    }
}

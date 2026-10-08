//! Acceptance tests for `M01-LC-PLACEZEPS-FIELDS` (task #791): the
//! `placezeps.zrd` placement member's field grammar.
//!
//! Prefix: `accept_m01_lc_placezeps_fields_`. The synthetic tests hold the
//! decoder to its stated grammar (a signed integer, a float, an unmodelled key
//! kept with its claim, every refusal); the retail tests read M01's member
//! through the production discovery and the production binding and assert the
//! measured field census. Retail tests need `CS_GAME_DIR` and fail loudly
//! without it.

use cs_app::animation::{
    MeasuredPlacement, PLACEMENT_ROTATION_AXIS_CLAIM, PLACEMENT_STATE_CLAIM, bind_mission_animation,
};
use cs_assets::install;
use cs_formats::script_raw::discover_container;
use cs_formats::zbd::placezeps::{
    DEGREES_TO_RADIANS, PLACEZEPS_MEMBER, PlacezepsError, RESET_TIME_CLAIM, StateKind, StateNumber,
    UNMODELLED_FIELD_CLAIM, read_placezeps_member,
};
use cs_types::content::Resolved;
use cs_types::install::RelativePath;

fn int(value: u32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn float(value: f32) -> Vec<u8> {
    [2u32.to_le_bytes(), value.to_bits().to_le_bytes()].concat()
}

fn text(value: &str) -> Vec<u8> {
    let mut out = 3u32.to_le_bytes().to_vec();
    out.extend((value.len() as u32).to_le_bytes());
    out.extend(value.as_bytes());
    out
}

fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

fn keyed(key: &str, value: Vec<u8>) -> Vec<Vec<u8>> {
    vec![text(key), value]
}

fn statement(node: &str, state: [Vec<u8>; 3], extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut children = keyed("NAME", list(&[text(node)]));
    children.extend(keyed("STATE", list(&state)));
    children.extend(extra);
    list(&children)
}

fn member(sequence_extra: Vec<Vec<u8>>, definition_extra: Vec<Vec<u8>>) -> Vec<u8> {
    let mut sequence = keyed("NAME", list(&[text("placement")]));
    sequence.extend(keyed(
        "OBJECT_TRANSLATE_STATE",
        statement("ship", [int(0xffff_f200), float(1.5), int(7)], Vec::new()),
    ));
    sequence.extend(keyed(
        "OBJECT_ROTATE_STATE",
        statement("ship", [int(0), int(180), int(0)], Vec::new()),
    ));
    sequence.extend(sequence_extra);
    let mut definition = keyed("NAME", list(&[text("ship")]));
    definition.extend(keyed("ANIMATION_NAME", list(&[text("placeship")])));
    definition.extend(keyed("ACTIVATION", list(&[text("ON_STARTUP")])));
    definition.extend(keyed("RESET_TIME", list(&[int(u32::MAX)])));
    definition.extend(keyed("SEQUENCE_DEFINITION", list(&sequence)));
    definition.extend(definition_extra);
    let animations = keyed("ANIMATION_DEFINITION", list(&definition));
    let body = list(&[text("ANIMATION_LIST"), list(&animations)]);
    list(&[list(&[text("ANIMATION_DEFINITIONS"), body])])
}

#[test]
fn accept_m01_lc_placezeps_fields_a_state_number_is_a_signed_integer_or_a_float() {
    let decoded = read_placezeps_member(&member(Vec::new(), Vec::new())).expect("decodes");
    let [definition] = decoded.definitions() else {
        panic!("one definition");
    };
    assert_eq!(definition.names(), ["ship"]);
    assert_eq!(definition.animation_name(), "placeship");
    assert_eq!(definition.activation(), "ON_STARTUP");
    assert_eq!(definition.reset_time(), Some(u32::MAX));
    let translate = definition.sequence().translate().expect("translate");
    assert_eq!(translate.kind(), StateKind::Translate);
    assert_eq!(translate.node(), "ship");
    // 0xfffff200 is -3584 as a signed word: the image converts with `fild`.
    assert_eq!(
        translate.stored(),
        [
            StateNumber::Int(-3584),
            StateNumber::Float(1.5),
            StateNumber::Int(7)
        ]
    );
    assert_eq!(translate.state(), [-3584.0, 1.5, 7.0]);
    assert_eq!(translate.parsed(), translate.state());
    let rotate = definition.sequence().rotate().expect("rotate");
    assert_eq!(rotate.state(), [0.0, 180.0, 0.0]);
    let radians = rotate.parsed();
    assert!((f64::from(radians[1]) - 180.0 * DEGREES_TO_RADIANS).abs() < 1e-6);
    assert!(definition.unknown_fields().is_empty());
}

#[test]
fn accept_m01_lc_placezeps_fields_a_key_outside_the_vocabulary_is_kept_not_ignored() {
    let bytes = member(
        keyed("OBJECT_SCALE_STATE", list(&[int(1)])),
        keyed("MYSTERY", list(&[int(2)])),
    );
    let decoded = read_placezeps_member(&bytes).expect("decodes");
    let definition = &decoded.definitions()[0];
    let keys: Vec<&str> = definition
        .unknown_fields()
        .iter()
        .chain(definition.sequence().unknown_fields())
        .map(|field| {
            assert_eq!(field.claim_id, UNMODELLED_FIELD_CLAIM);
            assert!(field.range.end > field.range.start);
            field.key.as_str()
        })
        .collect();
    assert_eq!(keys, ["MYSTERY", "OBJECT_SCALE_STATE"]);
}

#[test]
fn accept_m01_lc_placezeps_fields_a_member_outside_the_grammar_is_refused_with_its_offset() {
    let good = member(Vec::new(), Vec::new());
    assert!(matches!(
        read_placezeps_member(&good[..good.len() - 1]),
        Err(PlacezepsError::Truncated { .. } | PlacezepsError::LengthDoesNotFit { .. })
    ));
    let mut trailing = good.clone();
    trailing.push(0);
    assert!(matches!(
        read_placezeps_member(&trailing),
        Err(PlacezepsError::TrailingBytes { count: 1, .. })
    ));
    // A STATE of two numbers is not the measured shape.
    let two = {
        let mut sequence = keyed("NAME", list(&[text("placement")]));
        sequence.extend(keyed(
            "OBJECT_TRANSLATE_STATE",
            list(&[
                text("NAME"),
                list(&[text("ship")]),
                text("STATE"),
                list(&[int(1), int(2)]),
            ]),
        ));
        let mut definition = keyed("NAME", list(&[text("ship")]));
        definition.extend(keyed("ANIMATION_NAME", list(&[text("a")])));
        definition.extend(keyed("ACTIVATION", list(&[text("ON_STARTUP")])));
        definition.extend(keyed("SEQUENCE_DEFINITION", list(&sequence)));
        let body = list(&[
            text("ANIMATION_LIST"),
            list(&[text("ANIMATION_DEFINITION"), list(&definition)]),
        ]);
        list(&[list(&[text("ANIMATION_DEFINITIONS"), body])])
    };
    assert!(matches!(
        read_placezeps_member(&two),
        Err(PlacezepsError::WrongShape { key, .. }) if key == "STATE"
    ));
    // Not the ANIMATION_DEFINITIONS frame at all.
    assert!(matches!(
        read_placezeps_member(&list(&[list(&[text("OTHER"), list(&[])])])),
        Err(PlacezepsError::NotAMember { .. })
    ));
}

fn retail_root() -> std::path::PathBuf {
    std::path::PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("a retail test needs CS_GAME_DIR to be set"),
    )
}

/// The bytes of one reader-archive member of M01's scope, read through the
/// production discovery.
fn m01_member() -> (Vec<u8>, u64) {
    let found = install::discover(&retail_root()).expect("the installation is discovered");
    let key = "zbd/c1c/m01/zrdr.zbd";
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == key)
        .expect("M01 has a reader archive");
    let spelling = record.relative_spelling.as_str().to_owned();
    let bytes = std::fs::read(found.manifest.host_root.join(&spelling)).expect("archive reads");
    let path = RelativePath::new(&spelling.to_lowercase()).expect("a relative path");
    let discovery = discover_container(key, &path, &bytes);
    discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|member| member.eq_ignore_ascii_case(PLACEZEPS_MEMBER))
        })
        .map(|program| (program.bytes().to_vec(), program.locator().span().offset))
        .expect("M01 carries placezeps.zrd")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_placezeps_fields_retail_m01_census() {
    let (bytes, offset) = m01_member();
    assert_eq!(bytes.len(), 1535);
    assert_eq!(offset, 49213);
    let decoded = read_placezeps_member(&bytes).expect("M01's member decodes with no byte left");
    assert_eq!(decoded.byte_len(), 1535);
    assert!(decoded.list_fields().is_empty());

    struct Expected {
        node: &'static str,
        animation: &'static str,
        sequence: &'static str,
        translate: [f32; 3],
        rotate: Option<[f32; 3]>,
    }
    let expected = [
        Expected {
            node: "piratezep",
            animation: "placepiratezep",
            sequence: "placement",
            translate: [-3584.0, 1360.0, -8704.0],
            rotate: Some([0.0, 180.0, 0.0]),
        },
        Expected {
            node: "workersvoyagezep",
            animation: "placeworkersvoyagezep",
            sequence: "up_down",
            translate: [-5972.0, 1460.0, -7680.0],
            rotate: Some([0.0, 180.0, 0.0]),
        },
        Expected {
            node: "blackswanzep",
            animation: "placeblackswanzep",
            sequence: "up_down",
            translate: [-6656.0, 1960.0, -5632.0],
            rotate: None,
        },
    ];
    assert_eq!(decoded.definitions().len(), expected.len());
    for (definition, expected) in decoded.definitions().iter().zip(&expected) {
        assert_eq!(definition.names(), [expected.node]);
        assert_eq!(definition.animation_name(), expected.animation);
        assert_eq!(definition.activation(), "ON_STARTUP");
        assert_eq!(definition.reset_time(), Some(u32::MAX));
        assert_eq!(definition.sequence().name(), Some(expected.sequence));
        assert!(definition.unknown_fields().is_empty());
        assert!(definition.sequence().unknown_fields().is_empty());
        let translate = definition.sequence().translate().expect("translate");
        assert_eq!(translate.node(), expected.node);
        assert_eq!(translate.state(), expected.translate);
        assert!(translate.unknown_fields().is_empty());
        assert!(
            translate
                .stored()
                .iter()
                .all(|number| matches!(number, StateNumber::Int(_))),
            "M01 stores its states as integers"
        );
        match (definition.sequence().rotate(), expected.rotate) {
            (Some(rotate), Some(state)) => {
                assert_eq!(rotate.node(), expected.node);
                assert_eq!(rotate.state(), state);
            }
            (None, None) => {}
            other => panic!("{}: rotate mismatch {other:?}", expected.node),
        }
    }
}

fn known<T: Clone + std::fmt::Debug>(resolved: &Resolved<T>) -> (T, String, u64, u64) {
    let Resolved::Known(known) = resolved else {
        panic!("expected a known value, found {resolved:?}");
    };
    let source = known.provenance.source.as_ref().expect("a source span");
    (
        known.value.clone(),
        known.provenance.claim_id.as_str().to_owned(),
        source.offset(),
        source.length(),
    )
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_lc_placezeps_fields_retail_m01_binding_carries_the_measured_fields() {
    let (bytes, member_offset) = m01_member();
    let decoded = read_placezeps_member(&bytes).expect("decodes");
    let binding = bind_mission_animation(&retail_root(), "zbd/c1c/m01").expect("M01 binds");
    let measured: Vec<&MeasuredPlacement> = binding.measured_placements().iter().collect();
    assert_eq!(measured.len(), 3);
    assert_eq!(binding.placements().len(), 3);
    for (measured, definition) in measured.iter().zip(decoded.definitions()) {
        assert_eq!(measured.definition, definition.index());
        let (node, claim, ..) = known(&measured.node);
        assert_eq!(node, definition.names()[0]);
        assert_eq!(claim, PLACEMENT_STATE_CLAIM);

        let translate = definition.sequence().translate().expect("translate");
        let (value, claim, offset, len) =
            known(measured.translation.as_ref().expect("translation"));
        assert_eq!(value, translate.state());
        assert_eq!(claim, PLACEMENT_STATE_CLAIM);
        // The source span is the STATE list's own byte range in the archive.
        assert_eq!(offset, member_offset + translate.state_range().start);
        assert_eq!(
            len,
            translate.state_range().end - translate.state_range().start
        );

        match (&measured.rotation_degrees, definition.sequence().rotate()) {
            (Some(degrees), Some(rotate)) => {
                assert_eq!(known(degrees).0, rotate.state());
                assert_eq!(
                    known(measured.rotation_radians.as_ref().expect("radians")).0,
                    rotate.parsed()
                );
                // The heading is not read out of the middle component.
                let Some(Resolved::Unknown { claim_id, .. }) = &measured.yaw_degrees else {
                    panic!("yaw must stay unknown");
                };
                assert_eq!(claim_id.as_str(), PLACEMENT_ROTATION_AXIS_CLAIM);
            }
            (None, None) => assert!(measured.yaw_degrees.is_none()),
            other => panic!("rotation presence disagrees: {other:?}"),
        }
        let Some(Resolved::Unknown { claim_id, .. }) = &measured.reset_time else {
            panic!("RESET_TIME stays unknown");
        };
        assert_eq!(claim_id.as_str(), RESET_TIME_CLAIM);
        assert!(measured.unmodelled_fields.is_empty());
    }
}

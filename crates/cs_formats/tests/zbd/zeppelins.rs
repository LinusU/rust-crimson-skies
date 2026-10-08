//! Task #574: the mission `zeppelins.zrd` member's grammar and refusals.
//!
//! Every byte here is authored for this file; nothing is derived from
//! original data. The grammar under test was measured over all 50 retail
//! members by the `accept_t574_retail_*` tests in `cs_content`. Task test
//! prefix: `accept_t574_`.

use cs_formats::zbd::zeppelins::{
    KeyMeaning, MeasuredTeam, ZeppelinKey, ZeppelinsError, read_zeppelins_member,
};
use cs_types::evidence::ClaimStatus;

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

/// A list of `children`, stored with the measured word: children plus one.
fn list(children: &[Vec<u8>]) -> Vec<u8> {
    let mut out = 4u32.to_le_bytes().to_vec();
    out.extend((children.len() as u32 + 1).to_le_bytes());
    for child in children {
        out.extend(child);
    }
    out
}

fn one_text(value: &str) -> Vec<u8> {
    list(&[text(value)])
}

fn one_int(value: u32) -> Vec<u8> {
    list(&[int(value)])
}

fn one_float(value: f32) -> Vec<u8> {
    list(&[float(value)])
}

fn text_list(values: &[&str]) -> Vec<u8> {
    list(&values.iter().map(|v| text(v)).collect::<Vec<_>>())
}

/// The 16 key/value pairs every measured record states, with authored values.
fn required_pairs(node: &str) -> Vec<(&'static str, Vec<u8>)> {
    vec![
        ("node", one_text(node)),
        ("position", list(&[float(1.0), float(2.0), float(3.0)])),
        ("yaw", one_float(90.0)),
        ("pitch", one_float(0.0)),
        ("max_speed", one_float(5.0)),
        ("max_accel", one_float(4.47)),
        ("accel_pitch", one_float(0.5)),
        ("accel_yaw", one_float(0.5)),
        ("max_rate_yaw", one_float(5.0)),
        ("max_rate_pitch", one_float(5.0)),
        ("min_pitch", one_float(-30.0)),
        ("max_pitch", one_float(30.0)),
        ("net", one_text("TestNet")),
        ("healthy", list(&[list(&[text("gasbag1"), text("panels")])])),
        ("num_healthy_required", one_int(2)),
        ("engines", text_list(&["engine1"])),
    ]
}

/// A record list node holding `pairs` in the order given.
fn keyed_record(pairs: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut children = Vec::with_capacity(pairs.len() * 2);
    for (key, value) in pairs {
        children.push(text(key));
        children.push(value.clone());
    }
    list(&children)
}

/// A record stating the required keys and `extra`, minus any key in `omit`.
fn record_with(extra: &[(&'static str, Vec<u8>)], omit: &[&str]) -> Vec<u8> {
    let mut pairs: Vec<(&'static str, Vec<u8>)> = required_pairs("testzep")
        .into_iter()
        .filter(|(key, _)| !omit.contains(key))
        .collect();
    pairs.extend(extra.iter().cloned());
    keyed_record(&pairs)
}

fn member(records: &[Vec<u8>]) -> Vec<u8> {
    list(&[list(records)])
}

#[test]
fn accept_t574_zeppelins_a_member_decodes_its_authored_records() {
    let sparse = record_with(&[], &[]);
    let full = record_with(
        &[
            ("deactivated", one_int(1)),
            ("team", one_text("ally")),
            ("targets", text_list(&["testzep", "player"])),
            (
                "gasbags",
                list(&[
                    list(&[text("gasbag1"), float(120.0), one_text("gb1torpedo")]),
                    list(&[
                        text("gasbag2"),
                        float(400.0),
                        one_text("gb2torpedo"),
                        text("panels"),
                    ]),
                ]),
            ),
            ("cannon_fire_delay", one_float(10.0)),
            ("cannon_fire_range", one_float(1500.0)),
            ("cannon_inaccuracy", one_float(6.0)),
            (
                "left_cannons",
                list(&[list(&[
                    text("lbroad1"),
                    text("deploy_lbroad1"),
                    text("retract_lbroad1"),
                ])]),
            ),
            (
                "right_cannons",
                list(&[list(&[
                    text("rbroad1"),
                    text("deploy_rbroad1"),
                    text("retract_rbroad1"),
                ])]),
            ),
            (
                "cannon_health",
                list(&[list(&[
                    text("lbroad1"),
                    text("gunback"),
                    text("frame"),
                    text("gasbag1"),
                    float(200.0),
                    one_text("destroy_lbroad1"),
                    list(&[
                        list(&[float(0.6), text("60_lbroad1")]),
                        list(&[float(0.3), text("30_lbroad1")]),
                    ]),
                ])]),
            ),
        ],
        &[],
    );
    let decoded =
        read_zeppelins_member(&member(&[sparse, full])).expect("the authored member decodes");
    assert_eq!(decoded.len(), 2);

    let first = &decoded.records()[0];
    assert_eq!(first.node(), "testzep");
    assert_eq!(first.position(), [1.0, 2.0, 3.0]);
    assert_eq!(first.yaw(), 90.0);
    assert_eq!(first.pitch(), 0.0);
    assert_eq!(first.max_speed(), 5.0);
    assert_eq!(first.max_accel(), 4.47);
    assert_eq!(first.accel_pitch(), 0.5);
    assert_eq!(first.accel_yaw(), 0.5);
    assert_eq!(first.max_rate_yaw(), 5.0);
    assert_eq!(first.max_rate_pitch(), 5.0);
    assert_eq!(first.min_pitch(), -30.0);
    assert_eq!(first.max_pitch(), 30.0);
    assert_eq!(first.net(), "TestNet");
    assert_eq!(first.healthy().len(), 1);
    assert_eq!(first.healthy()[0].node(), "gasbag1");
    assert_eq!(first.healthy()[0].attachment(), "panels");
    assert_eq!(first.num_healthy_required(), 2);
    assert_eq!(first.engines(), ["engine1"]);
    // The optional keys are absent, not defaulted: present-but-optional
    // stays a distinct state from never-stated.
    assert_eq!(first.deactivated(), None);
    assert_eq!(first.team(), None);
    assert_eq!(first.measured_team(), None);
    assert_eq!(first.targets(), None);
    assert_eq!(first.gasbags(), None);
    assert_eq!(first.cannon_fire_delay(), None);
    assert_eq!(first.cannon_fire_range(), None);
    assert_eq!(first.cannon_inaccuracy(), None);
    assert_eq!(first.left_cannons(), None);
    assert_eq!(first.right_cannons(), None);
    assert_eq!(first.cannon_health(), None);
    assert_eq!(first.keys().len(), 16);

    let second = &decoded.records()[1];
    assert_eq!(second.deactivated(), Some(1));
    assert_eq!(second.team(), Some("ally"));
    assert_eq!(second.measured_team(), Some(MeasuredTeam::Ally));
    assert_eq!(second.targets().unwrap(), ["testzep", "player"]);
    let gasbags = second.gasbags().expect("gasbags stated");
    assert_eq!(gasbags.len(), 2);
    assert_eq!(gasbags[0].node(), "gasbag1");
    assert_eq!(gasbags[0].value(), 120.0);
    assert_eq!(gasbags[0].torpedo(), "gb1torpedo");
    assert_eq!(gasbags[0].attachment(), None);
    assert_eq!(gasbags[1].attachment(), Some("panels"));
    assert_eq!(second.cannon_fire_delay(), Some(10.0));
    assert_eq!(second.cannon_fire_range(), Some(1500.0));
    assert_eq!(second.cannon_inaccuracy(), Some(6.0));
    let left = second.left_cannons().expect("left cannons stated");
    assert_eq!(left[0].node(), "lbroad1");
    assert_eq!(left[0].deploy(), "deploy_lbroad1");
    assert_eq!(left[0].retract(), "retract_lbroad1");
    assert_eq!(second.right_cannons().unwrap()[0].node(), "rbroad1");
    let health = second.cannon_health().expect("cannon health stated");
    assert_eq!(health[0].node(), "lbroad1");
    assert_eq!(health[0].mount(), "gunback");
    assert_eq!(health[0].frame(), "frame");
    assert_eq!(health[0].gasbag(), "gasbag1");
    assert_eq!(health[0].value(), 200.0);
    assert_eq!(health[0].destroy(), "destroy_lbroad1");
    assert_eq!(
        health[0].states(),
        &[
            (0.6, "60_lbroad1".to_owned()),
            (0.3, "30_lbroad1".to_owned())
        ]
    );
    assert_eq!(second.keys().len(), 26);
}

#[test]
fn accept_t574_zeppelins_an_empty_record_list_is_a_member_with_no_records() {
    // The measured `c*/mp1`/`c*/mp2` state: the member exists and holds zero
    // records. That decodes to an empty member, not an error.
    let decoded = read_zeppelins_member(&member(&[])).expect("the empty member decodes");
    assert!(decoded.is_empty());
    assert_eq!(decoded.len(), 0);

    // An empty `targets` list is likewise a stated key with zero rows —
    // distinct from the key being absent.
    let sparse = record_with(&[("targets", list(&[]))], &[]);
    let decoded = read_zeppelins_member(&member(&[sparse])).expect("an empty targets list decodes");
    assert_eq!(decoded.records()[0].targets(), Some([].as_slice()));
}

#[test]
fn accept_t574_zeppelins_every_refusal_is_named_with_its_offset() {
    let good = member(&[record_with(&[], &[])]);

    // Truncation, at every possible length.
    for cut in 0..good.len() {
        let error = read_zeppelins_member(&good[..cut]).expect_err("a truncated member is refused");
        assert!(
            matches!(
                error,
                ZeppelinsError::Truncated { .. } | ZeppelinsError::LengthDoesNotFit { .. }
            ),
            "cut at {cut}: {error}"
        );
    }

    // An undefined tag.
    for tag in [0u32, 5, 0xFFFF_FFFF] {
        let mut bytes = good.clone();
        bytes[0..4].copy_from_slice(&tag.to_le_bytes());
        assert_eq!(
            read_zeppelins_member(&bytes),
            Err(ZeppelinsError::UndefinedTag { offset: 0, tag })
        );
    }

    // A list word of zero.
    let mut bytes = good.clone();
    bytes[4..8].copy_from_slice(&0u32.to_le_bytes());
    assert_eq!(
        read_zeppelins_member(&bytes),
        Err(ZeppelinsError::ZeroListWord { offset: 0 })
    );

    // A list word whose children cannot fit.
    let mut bytes = good.clone();
    bytes[4..8].copy_from_slice(&0x00FF_FFFFu32.to_le_bytes());
    assert!(matches!(
        read_zeppelins_member(&bytes),
        Err(ZeppelinsError::LengthDoesNotFit { .. })
    ));

    // Trailing bytes.
    let mut bytes = good.clone();
    bytes.extend([0, 0, 0]);
    assert_eq!(
        read_zeppelins_member(&bytes),
        Err(ZeppelinsError::TrailingBytes {
            offset: good.len() as u64,
            count: 3
        })
    );

    // Not UTF-8: corrupt the `node` value's text body.
    let mut bytes = record_with(&[], &[]);
    let needle = b"testzep";
    let at = bytes
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("the node spelling is in the record");
    bytes[at] = 0xFF;
    assert!(matches!(
        read_zeppelins_member(&member(&[bytes])),
        Err(ZeppelinsError::InvalidText { .. })
    ));

    // Nesting deeper than the measured grammar needs.
    let mut deep = list(&[float(1.0)]);
    for _ in 0..9 {
        deep = list(&[deep]);
    }
    let bytes = list(&[list(&[list(&[text("node"), deep])])]);
    assert!(matches!(
        read_zeppelins_member(&bytes),
        Err(ZeppelinsError::DepthExceeded { .. })
            | Err(ZeppelinsError::WrongValueShape { .. })
            | Err(ZeppelinsError::MissingKey { .. })
    ));
}

#[test]
fn accept_t574_zeppelins_a_record_outside_the_measured_shape_is_refused() {
    // The root is not a list.
    assert_eq!(
        read_zeppelins_member(&int(1)),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "the root must be a list"
        })
    );
    // The root's only child is not the record list.
    assert_eq!(
        read_zeppelins_member(&list(&[text("node")])),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "the root must hold exactly one record list"
        })
    );
    // A record that is not a list.
    assert_eq!(
        read_zeppelins_member(&list(&[list(&[text("node")])])),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "a record must be a list"
        })
    );
    // A record with an odd child count: a key without a value.
    assert_eq!(
        read_zeppelins_member(&member(&[list(&[text("node")])])),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "a record's children must alternate a key and a value"
        })
    );
    // A key that is not a string.
    assert_eq!(
        read_zeppelins_member(&member(&[list(&[int(1), one_text("x")])])),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "a record key must be a text node"
        })
    );
    // A value that is not a list.
    assert_eq!(
        read_zeppelins_member(&member(&[keyed_record(&[("node", text("x"))])])),
        Err(ZeppelinsError::NotAMember {
            offset: 0,
            reason: "a record value must be a list node"
        })
    );
    // A key the 50 members never use.
    assert_eq!(
        read_zeppelins_member(&member(&[record_with(&[("pilot", one_text("ace"))], &[])])),
        Err(ZeppelinsError::UnknownKey {
            key: "pilot".to_owned()
        })
    );
    // A key stated twice.
    let mut pairs = required_pairs("testzep");
    pairs.push(("node", one_text("other")));
    assert_eq!(
        read_zeppelins_member(&member(&[keyed_record(&pairs)])),
        Err(ZeppelinsError::DuplicateKey { key: "node" })
    );
    // A required key absent — one case per required key.
    for key in ZeppelinKey::ALL {
        if !key.required() {
            continue;
        }
        assert_eq!(
            read_zeppelins_member(&member(&[record_with(&[], &[key.spelling()])])),
            Err(ZeppelinsError::MissingKey {
                key: key.spelling()
            }),
            "omitting {} must refuse",
            key.spelling()
        );
    }
    // A value of the wrong shape: an int list for `node`, a two-text list
    // for `net`, a single float for `position`, a text list for `yaw`.
    assert_eq!(
        read_zeppelins_member(&member(&[record_with(&[("node", one_int(3))], &["node"])])),
        Err(ZeppelinsError::WrongValueShape { key: "node" })
    );
    assert_eq!(
        read_zeppelins_member(&member(&[record_with(
            &[("net", text_list(&["a", "b"]))],
            &["net"]
        )])),
        Err(ZeppelinsError::WrongValueShape { key: "net" })
    );
    assert_eq!(
        read_zeppelins_member(&member(&[record_with(
            &[("position", one_float(1.0))],
            &["position"]
        )])),
        Err(ZeppelinsError::WrongValueShape { key: "position" })
    );
    assert_eq!(
        read_zeppelins_member(&member(&[record_with(
            &[("yaw", one_text("ninety"))],
            &["yaw"]
        )])),
        Err(ZeppelinsError::WrongValueShape { key: "yaw" })
    );
}

#[test]
fn accept_t574_zeppelins_nested_row_shapes_are_enforced() {
    // `healthy` rows must be two texts.
    let bad = record_with(
        &[("healthy", list(&[list(&[text("gasbag1"), float(1.0)])]))],
        &["healthy"],
    );
    assert_eq!(
        read_zeppelins_member(&member(&[bad])),
        Err(ZeppelinsError::WrongValueShape { key: "healthy" })
    );
    // `left_cannons` rows must be three texts.
    let bad = record_with(
        &[("left_cannons", list(&[list(&[text("c1"), text("d1")])]))],
        &[],
    );
    assert_eq!(
        read_zeppelins_member(&member(&[bad])),
        Err(ZeppelinsError::WrongValueShape {
            key: "left_cannons"
        })
    );
    // `gasbags` rows: a missing torpedo list and a fifth element both refuse.
    let bad = record_with(
        &[("gasbags", list(&[list(&[text("g1"), float(1.0)])]))],
        &[],
    );
    assert_eq!(
        read_zeppelins_member(&member(&[bad])),
        Err(ZeppelinsError::WrongValueShape { key: "gasbags" })
    );
    let bad = record_with(
        &[(
            "gasbags",
            list(&[list(&[
                text("g1"),
                float(1.0),
                one_text("t1"),
                text("panels"),
                text("extra"),
            ])]),
        )],
        &[],
    );
    assert_eq!(
        read_zeppelins_member(&member(&[bad])),
        Err(ZeppelinsError::WrongValueShape { key: "gasbags" })
    );
    // `cannon_health` rows must be the measured seven elements.
    let bad = record_with(
        &[(
            "cannon_health",
            list(&[list(&[
                text("c1"),
                text("gunback"),
                text("frame"),
                text("gasbag1"),
                float(200.0),
                one_text("destroy_c1"),
            ])]),
        )],
        &[],
    );
    assert_eq!(
        read_zeppelins_member(&member(&[bad])),
        Err(ZeppelinsError::WrongValueShape {
            key: "cannon_health"
        })
    );
}

#[test]
fn accept_t574_zeppelins_what_a_key_means_is_recorded_as_unknown() {
    let mut spellings = Vec::new();
    for key in ZeppelinKey::ALL {
        assert_eq!(key.meaning(), KeyMeaning::Unknown);
        assert_eq!(key.meaning().evidence(), ClaimStatus::Unknown);
        assert_eq!(ZeppelinKey::from_spelling(key.spelling()), Some(key));
        assert!(!key.value_shape().is_empty());
        spellings.push(key.spelling());
    }
    spellings.dedup();
    assert_eq!(spellings.len(), ZeppelinKey::ALL.len());
    assert_eq!(ZeppelinKey::from_spelling("pilot"), None);
    // The measured team vocabulary resolves; anything else stays unclaimed.
    assert_eq!(
        MeasuredTeam::from_spelling("ally"),
        Some(MeasuredTeam::Ally)
    );
    assert_eq!(
        MeasuredTeam::from_spelling("enemy"),
        Some(MeasuredTeam::Enemy)
    );
    assert_eq!(MeasuredTeam::from_spelling("neutral"), None);
}

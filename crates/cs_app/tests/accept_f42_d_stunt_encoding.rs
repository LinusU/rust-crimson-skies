//! Task #463 acceptance tests: the original stunt encoding and gate geometry.
//!
//! Spec: `specs/F42-stunts-fame-photos-and-optional-achievement-events.md`,
//! stage `### F42-D`. Required capabilities: retail. Task test prefix:
//! `accept_f42_d_`.
//!
//! The original does not author a stunt gate's volume in a mission. It names a
//! **detection zone** of the world the scenario is set in, and the zone's box
//! lives in the world container (task #427 measured those). This file pins the
//! other half — the scenario-side encoding:
//!
//! * an instant-action scenario's reader-archive member `ia.zrd` carries the
//!   scenario's `mission_type` (measured `stunt_flying` for four of the eight)
//!   and a `dzones` list binding a scenario-local label to the world node it
//!   stands for;
//! * the same scenario's `targets.zrd` declares one objective per target, and a
//!   fly-through danger-zone target carries `category_label = MSG_OBJ_DZ`,
//!   `help_label = MSG_OBJ_FLYTHROUGH` and `nodes = [<label>]`.
//!
//! The unignored tests author every byte (the `.zrd` grammar, a reader archive,
//! a world container) and run the production decoder, the production discovery
//! and the production survey, so the mechanics are falsifiable in CI. The
//! `#[ignore]`d tests read the owner's installation through the same production
//! entry point and state the measured corpus.
//!
//! Nothing here is `verified_original`: no original run happened, and reading
//! the installation's files is not evidence of how the game behaves. A
//! direction rule, a clearance rule, a payout and a repeat policy are **not**
//! in the scenario bytes, so the survey reports them unmeasured and no test here
//! fills them in.

use std::collections::BTreeSet;
use std::fs;
use std::path::PathBuf;

use cs_app::stunts::{StuntEncodingSurveyError, survey_retail_stunt_encoding};
use cs_content::stunts::{
    FLY_THROUGH_CATEGORY_LABEL, FLY_THROUGH_HELP_LABEL, SCENARIO_MEMBER, SCENARIO_TARGETS_MEMBER,
    STUNT_MISSION_TYPE, ZrdValue, decode_zrd, scenario_fly_through_targets, scenario_mission_type,
    scenario_zone_bindings,
};
use cs_content::world::WorldId;

// ------------------------------------------------------------- .zrd writer ---

/// A `.zrd` integer node: tag `1` and the value.
fn zrd_int(value: u32) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8);
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&value.to_le_bytes());
    bytes
}

/// A `.zrd` text node: tag `3`, the byte length and the bytes.
fn zrd_text(text: &str) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + text.len());
    bytes.extend_from_slice(&3_u32.to_le_bytes());
    bytes.extend_from_slice(&(text.len() as u32).to_le_bytes());
    bytes.extend_from_slice(text.as_bytes());
    bytes
}

/// A `.zrd` list node: tag `4`, then **`children.len() + 1`** as the count, then
/// the children. The profiler stores one more than the child count; the decoder
/// is built on that measured grammar and this writer is independent of it.
fn zrd_list(children: Vec<Vec<u8>>) -> Vec<u8> {
    let mut bytes = Vec::with_capacity(8 + children.iter().map(Vec::len).sum::<usize>());
    bytes.extend_from_slice(&4_u32.to_le_bytes());
    bytes.extend_from_slice(&((children.len() as u32) + 1).to_le_bytes());
    for child in children {
        bytes.extend_from_slice(&child);
    }
    bytes
}

/// A key/value `.zrd` map (`["k", v, "k2", v2]`), the shape both members use.
fn zrd_map(entries: Vec<(&str, Vec<u8>)>) -> Vec<u8> {
    let mut children = Vec::with_capacity(entries.len() * 2);
    for (key, value) in entries {
        children.push(zrd_text(key));
        children.push(value);
    }
    zrd_list(children)
}

/// An authored `ia.zrd`: a scenario mode and the `dzones` bindings.
///
/// The measured shape: a flat alternating key/value root whose `mission_type`
/// value is a one-element list, exactly as the original writes it.
fn scenario_document(mission_type: &str, bindings: &[(&str, &str)]) -> Vec<u8> {
    let zones = zrd_list(
        bindings
            .iter()
            .map(|(world_node, label)| zrd_list(vec![zrd_text(world_node), zrd_text(label)]))
            .collect(),
    );
    zrd_map(vec![
        ("mission_type", zrd_list(vec![zrd_text(mission_type)])),
        ("dzones", zones),
    ])
}

/// One authored `targets.zrd` objective.
///
/// The measured shape: a list of two-element `[key, value]` pairs in the
/// authored order the retail corpus uses (`description`, `nodes`,
/// `category_label`, `help_label`).
fn target_document(
    category_label: &str,
    help_label: &str,
    description: &str,
    nodes: &[&str],
) -> Vec<u8> {
    zrd_list(vec![
        zrd_list(vec![zrd_text("description"), zrd_text(description)]),
        zrd_list(vec![
            zrd_text("nodes"),
            zrd_list(nodes.iter().map(|node| zrd_text(node)).collect()),
        ]),
        zrd_list(vec![zrd_text("category_label"), zrd_text(category_label)]),
        zrd_list(vec![zrd_text("help_label"), zrd_text(help_label)]),
    ])
}

/// An authored `targets.zrd` root: one child per objective.
fn targets_document(targets: Vec<Vec<u8>>) -> Vec<u8> {
    zrd_list(targets)
}

// ------------------------------------------------------- reader-archive writer ---

/// A version-one reader archive holding `members` in order: the member data,
/// then one 148-byte index entry each (u32 start, u32 length, a 64-byte
/// NUL-padded name and 76 bytes), then the u32 version `1` and u32 count.
///
/// Written independently of the production reader: the reader's own accepted
/// shape is what the test proves.
fn reader_archive(members: &[(&str, Vec<u8>)]) -> Vec<u8> {
    let mut bytes = Vec::new();
    let mut entries = Vec::with_capacity(members.len());
    for (name, member) in members {
        let start = bytes.len() as u32;
        bytes.extend_from_slice(member);
        entries.push((start, member.len() as u32, *name));
    }
    for (start, length, name) in &entries {
        bytes.extend_from_slice(&start.to_le_bytes());
        bytes.extend_from_slice(&length.to_le_bytes());
        let name = name.as_bytes();
        assert!(name.len() < 64, "a fixture member name fits its field");
        let mut field = [0_u8; 64];
        field[..name.len()].copy_from_slice(name);
        bytes.extend_from_slice(&field);
        bytes.extend_from_slice(&[0_u8; 76]);
    }
    bytes.extend_from_slice(&1_u32.to_le_bytes());
    bytes.extend_from_slice(&(members.len() as u32).to_le_bytes());
    bytes
}

// ----------------------------------------------------- world-container writer ---

/// One authored detection-zone node: its name, its mesh slot and the two stored
/// box corners task #427 measured the box out of (`unk140`).
struct ZoneNode<'a> {
    name: &'a str,
    mesh_index: i32,
    corners: [[f32; 3]; 2],
}

/// A synthetic CS GameZ container holding exactly the authored nodes, so the
/// survey's geometry join resolves against a real file. The layout is the one
/// task #427's test authored and the production reader accepted; only the
/// `object3d`-identity shape is written.
fn world_container(nodes: &[ZoneNode<'_>]) -> Vec<u8> {
    const SIGNATURE: u32 = 43_455_010;
    const VERSION: u32 = 42;
    const NODES_OFFSET: u32 = 512;
    const SLOT: usize = 212;
    const OBJECT3D_BYTES: usize = 144;
    const OBJECT3D: u32 = 5;
    const IDENTITY: u32 = 40;

    let data_offset = NODES_OFFSET as usize + SLOT * nodes.len();
    let mut offsets = Vec::with_capacity(nodes.len());
    let mut cursor = data_offset;
    for _ in nodes {
        offsets.push(cursor);
        cursor += OBJECT3D_BYTES;
    }

    let mut bytes = vec![0_u8; cursor];
    let word = |bytes: &mut Vec<u8>, at: usize, value: u32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };
    let half = |bytes: &mut Vec<u8>, at: usize, value: u16| {
        bytes[at..at + 2].copy_from_slice(&value.to_le_bytes());
    };
    let float = |bytes: &mut Vec<u8>, at: usize, value: f32| {
        bytes[at..at + 4].copy_from_slice(&value.to_le_bytes());
    };

    for (field, value) in [
        (0_usize, SIGNATURE),
        (4, VERSION),
        (8, 0x1234_5678),
        (12, 1),
        (16, 40),
        (20, 248),
        (24, 256),
        (28, nodes.len() as u32),
        (32, 0),
        (36, NODES_OFFSET),
    ] {
        word(&mut bytes, field, value);
    }

    for (index, node) in nodes.iter().enumerate() {
        let at = NODES_OFFSET as usize + SLOT * index;
        let name = node.name.as_bytes();
        assert!(
            name.len() < 36,
            "the fixture's names fit their 36-byte field"
        );
        bytes[at..at + name.len()].copy_from_slice(name);
        word(&mut bytes, at + 36, 0x0180_001c);
        word(&mut bytes, at + 44, 1);
        word(&mut bytes, at + 48, 255);
        word(&mut bytes, at + 52, OBJECT3D);
        word(&mut bytes, at + 56, offsets[index] as u32);
        word(&mut bytes, at + 60, node.mesh_index as u32);
        word(&mut bytes, at + 68, 1);
        word(&mut bytes, at + 196, 160);
        word(&mut bytes, at + 208, 0x0200_0000 | index as u32);
        half(&mut bytes, at + 84, 0);
        half(&mut bytes, at + 86, 0);
        for (axis, value) in node.corners[0].iter().enumerate() {
            float(&mut bytes, at + 140 + 4 * axis, *value);
        }
        for (axis, value) in node.corners[1].iter().enumerate() {
            float(&mut bytes, at + 140 + 12 + 4 * axis, *value);
        }
        // The object record: the identity flags with an identity rotation,
        // scale, matrix and translation, the one shape every measured zone has.
        let data = offsets[index];
        word(&mut bytes, data, IDENTITY);
        for axis in 0..3 {
            float(&mut bytes, data + 36 + 4 * axis, 1.0);
        }
        for axis in 0..3 {
            float(&mut bytes, data + 48 + 12 * axis, 1.0);
            float(&mut bytes, data + 48 + 4 * axis + 4, 0.0);
            float(&mut bytes, data + 48 + 4 * axis + 8, 0.0);
        }
    }
    bytes
}

/// A throwaway installation tree holding one world group, its world container
/// and an instant-action scenario archive.
struct TempInstallation {
    root: PathBuf,
}

impl TempInstallation {
    fn new(label: &str) -> Self {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("the system clock is after the Unix epoch")
            .as_nanos();
        let root = std::env::temp_dir().join(format!(
            "crimson-t463-{label}-{}-{nanos}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("the fixture root is created");
        Self { root }
    }

    fn write(&self, spelling: &str, bytes: &[u8]) {
        let path = self.root.join(spelling);
        fs::create_dir_all(path.parent().expect("a fixture spelling has a parent"))
            .expect("the fixture directories are created");
        fs::write(&path, bytes).expect("the fixture bytes are written");
    }
}

impl Drop for TempInstallation {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

// --------------------------------------------- the decoder and the extraction ---

/// The `.zrd` grammar task #463 relies on: tag `1` int, `2` float, `3` text,
/// `4` list with **`count - 1`** children. The count-1 rule is the measured one
/// (F09); a decoder that read `count` children would run past the member and be
/// refused by its own trailing-bytes check.
#[test]
fn accept_f42_d_the_zrd_grammar_is_the_measured_count_minus_one_tree() {
    // A list of three children is stored with a count of four.
    let root = decode_zrd(&zrd_list(vec![
        zrd_text("mission_type"),
        zrd_text("stunt_flying"),
        zrd_int(7),
    ]))
    .expect("an authored list decodes");
    let children = root.as_list().expect("the root is a list");
    assert_eq!(
        children.len(),
        3,
        "a stored count of four carries three children"
    );

    // The count word is not the child count: the same three children with a
    // count of three, decoded as `count - 1`, leave one child trailing and the
    // member is refused rather than silently agreed with.
    let mut miscounted = Vec::new();
    miscounted.extend_from_slice(&4_u32.to_le_bytes());
    miscounted.extend_from_slice(&3_u32.to_le_bytes());
    miscounted.extend_from_slice(&zrd_text("mission_type"));
    miscounted.extend_from_slice(&zrd_text("stunt_flying"));
    miscounted.extend_from_slice(&zrd_int(7));
    let error = decode_zrd(&miscounted).expect_err("a wrong count leaves trailing bytes");
    assert_eq!(error.code(), "trailing_bytes");

    assert_eq!(decode_zrd(&zrd_int(9)).expect("an int").as_int(), Some(9));
    assert_eq!(
        decode_zrd(&zrd_text("x")).expect("text").as_text(),
        Some("x")
    );
    assert_eq!(
        decode_zrd(&zrd_text("x")).expect("text").as_list(),
        None,
        "a text node is not a list"
    );

    // Every refusal is named: an unknown tag, a truncated word, a text body that
    // is not UTF-8, an impossible list count and an empty member.
    assert_eq!(
        decode_zrd(&[9, 0, 0, 0, 0, 0, 0, 0])
            .expect_err("tag 9 is unknown")
            .code(),
        "unknown_tag"
    );
    assert_eq!(
        decode_zrd(&[1, 0, 0]).expect_err("a short int").code(),
        "truncated"
    );
    assert_eq!(
        decode_zrd(&{
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&3_u32.to_le_bytes());
            bytes.extend_from_slice(&1_u32.to_le_bytes());
            bytes.push(0xff);
            bytes
        })
        .expect_err("0xff is not UTF-8")
        .code(),
        "invalid_text"
    );
    assert_eq!(
        decode_zrd(&{
            let mut bytes = Vec::new();
            bytes.extend_from_slice(&4_u32.to_le_bytes());
            bytes.extend_from_slice(&1_000_000_u32.to_le_bytes());
            bytes
        })
        .expect_err("a count past the member")
        .code(),
        "count_exceeds_bytes"
    );
    assert_eq!(
        ZrdValue::List(Vec::new()).as_text(),
        None,
        "only text answers as text"
    );
}

/// The extraction reads the scenario mode and the label bindings, and selects
/// fly-through targets by their own category/help pair rather than by position,
/// so an ordinary objective in the same file is never mistaken for a stunt gate.
#[test]
fn accept_f42_d_the_scenario_extraction_reads_the_mode_bindings_and_only_fly_through_targets() {
    let scenario = decode_zrd(&scenario_document(
        "stunt_flying",
        &[("dzpath1", "dz1"), ("dzpath2", "dz2")],
    ))
    .expect("the scenario decodes");
    assert_eq!(scenario_mission_type(&scenario), Some("stunt_flying"));
    assert_eq!(
        scenario_zone_bindings(&scenario),
        vec![("dzpath1", "dz1"), ("dzpath2", "dz2")],
        "the bindings keep their authored order and direction (world node, label)"
    );

    // A scenario with no mode is not a stunt scenario, and the extraction says
    // so instead of inventing a mode.
    let unnamed = decode_zrd(&zrd_map(vec![(
        "dzones",
        zrd_list(vec![zrd_list(vec![zrd_text("dzpath1"), zrd_text("dz1")])]),
    )]))
    .expect("the scenario decodes");
    assert_eq!(scenario_mission_type(&unnamed), None);

    let targets = decode_zrd(&targets_document(vec![
        target_document(
            FLY_THROUGH_CATEGORY_LABEL,
            FLY_THROUGH_HELP_LABEL,
            "MSG_OBJ_DESCRIPTION_1",
            &["dz1"],
        ),
        // An ordinary objective, neither a danger zone nor a fly-through: it
        // must not be selected.
        target_document(
            "MSG_OBJ_ZEPPELIN",
            "MSG_OBJ_DESTROY",
            "MSG_DESC_2",
            &["zeppelin"],
        ),
        // A fly-through named only by its help key: still selected, because the
        // selector reads both halves of the measured pair.
        target_document(
            "MSG_OBJ_OTHER",
            FLY_THROUGH_HELP_LABEL,
            "MSG_DESC_3",
            &["dz2"],
        ),
    ]))
    .expect("the targets decode");
    let selected = scenario_fly_through_targets(&targets);
    assert_eq!(
        selected.len(),
        2,
        "the zeppelin objective is not a stunt gate"
    );
    assert_eq!(selected[0].zone_label, "dz1");
    assert_eq!(selected[0].category_label, FLY_THROUGH_CATEGORY_LABEL);
    assert_eq!(selected[0].help_label, FLY_THROUGH_HELP_LABEL);
    assert_eq!(selected[0].description, "MSG_OBJ_DESCRIPTION_1");
    assert_eq!(selected[1].zone_label, "dz2");
}

// -------------------------------------------------- the survey over a file ----

/// The whole production survey over an authored installation: production
/// discovery, production reader-archive discovery, the `.zrd` decoder and
/// task #427's own production node survey, joined into one row per fly-through
/// target.
///
/// This is where the two measurements meet. The scenario says *which* zone
/// (`dz1`); the `dzones` list binds the label to a world node (`dzpath1`); and
/// #427's survey supplies that node's box. A target whose label binds no world
/// node, or whose world node carries no measured box, is a **reported gap**, not
/// a dropped row — which is what `unresolved_gates` exists for.
#[test]
fn accept_f42_d_the_survey_joins_each_scenario_target_to_the_measured_world_box() {
    let install = TempInstallation::new("join");
    install.write(
        "ZBD/C5/gamez.zbd",
        &world_container(&[
            ZoneNode {
                name: "dzpath1",
                mesh_index: 949,
                corners: [[-10.0, 4.0, -20.0], [-2.0, 9.5, -1.0]],
            },
            ZoneNode {
                name: "dzpath2",
                mesh_index: 950,
                corners: [[0.0, 0.0, 0.0], [40.0, 12.0, 90.0]],
            },
            ZoneNode {
                name: "hangar",
                mesh_index: 12,
                corners: [[-500.0, 0.0, 0.0], [500.0, 60.0, 0.0]],
            },
        ]),
    );
    install.write(
        "ZBD/C5/IA1/zrdr.zbd",
        &reader_archive(&[
            (
                SCENARIO_MEMBER,
                scenario_document(
                    STUNT_MISSION_TYPE,
                    // `dz1` binds a measured node, `dz2` binds a measured node,
                    // `dz3` binds a node the world does not carry and `dz4` is
                    // bound by no entry at all.
                    &[("dzpath1", "dz1"), ("dzpath2", "dz2"), ("dzpath9", "dz3")],
                ),
            ),
            (
                SCENARIO_TARGETS_MEMBER,
                targets_document(vec![
                    target_document(
                        FLY_THROUGH_CATEGORY_LABEL,
                        FLY_THROUGH_HELP_LABEL,
                        "MSG_OBJ_DESCRIPTION_1",
                        &["dz1"],
                    ),
                    target_document(
                        FLY_THROUGH_CATEGORY_LABEL,
                        FLY_THROUGH_HELP_LABEL,
                        "MSG_OBJ_DESCRIPTION_2",
                        &["dz2"],
                    ),
                    // Binds a node the world container does not carry.
                    target_document(
                        FLY_THROUGH_CATEGORY_LABEL,
                        FLY_THROUGH_HELP_LABEL,
                        "MSG_OBJ_DESCRIPTION_3",
                        &["dz3"],
                    ),
                    // Binds no world node at all.
                    target_document(
                        FLY_THROUGH_CATEGORY_LABEL,
                        FLY_THROUGH_HELP_LABEL,
                        "MSG_OBJ_DESCRIPTION_4",
                        &["dz4"],
                    ),
                    target_document(
                        "MSG_OBJ_ZEPPELIN",
                        "MSG_OBJ_DESTROY",
                        "MSG_DESC",
                        &["hangar"],
                    ),
                ]),
            ),
        ]),
    );

    let survey = survey_retail_stunt_encoding(&install.root)
        .unwrap_or_else(|error| panic!("the fixture installation surveys: {error}"));

    assert_eq!(
        survey.len(),
        4,
        "the four fly-through targets, not the zeppelin"
    );
    assert_eq!(
        survey.install_sha256().len(),
        64,
        "the installation fingerprint"
    );
    assert!(!survey.is_complete(), "two targets name no measured box");
    assert_eq!(survey.unresolved_gates().count(), 2);

    let world = WorldId::from_key("c5").expect("a valid world key");
    let zones: Vec<Option<&str>> = survey
        .gates()
        .iter()
        .map(|gate| gate.world_zone())
        .collect();
    assert_eq!(
        zones,
        vec![Some("dzpath1"), Some("dzpath2"), Some("dzpath9"), None],
        "every row keeps its binding, resolved or not"
    );
    for gate in survey.gates() {
        assert_eq!(gate.world(), &world, "the scenario is set in c5");
        assert_eq!(gate.mission_type(), STUNT_MISSION_TYPE);
        assert_eq!(gate.category_label(), FLY_THROUGH_CATEGORY_LABEL);
        assert_eq!(gate.help_label(), FLY_THROUGH_HELP_LABEL);
        // Provenance: the container key, its file digest and the member's own
        // span, so each number can be traced back to the bytes.
        assert_eq!(gate.span().container(), "zbd/c5/ia1/zrdr.zbd");
        assert_eq!(gate.span().member(), SCENARIO_TARGETS_MEMBER);
        assert_eq!(gate.span().container_sha256().len(), 64);
        assert!(gate.span().length() > 0);
    }

    // The first two resolved all the way to #427's box.
    let first = &survey.gates()[0];
    assert!(first.is_resolved());
    let geometry = first.geometry().expect("dz1 resolved to dzpath1");
    assert_eq!(geometry.zone(), "dzpath1");
    assert_eq!(geometry.world(), &world);
    assert_eq!(geometry.volume().min(), [-10.0, 4.0, -20.0]);
    assert_eq!(geometry.volume().max(), [-2.0, 9.5, -1.0]);
    assert_eq!(geometry.mesh_index(), Some(949));
    assert_eq!(survey.gates()[1].geometry().expect("dz2").zone(), "dzpath2");

    // The two gaps are distinct states: one names a world node the container
    // does not carry, the other names no world node.
    assert_eq!(survey.gates()[2].world_zone(), Some("dzpath9"));
    assert!(survey.gates()[2].geometry().is_none());
    assert_eq!(survey.gates()[3].world_zone(), None);
    assert!(survey.gates()[3].geometry().is_none());

    // What the encoding does **not** carry: a direction rule, a clearance rule,
    // a payout and a repeat policy. The survey refuses to look as though it
    // measured them.
    assert!(!survey.direction_rule_is_measured());
    assert!(!survey.clearance_rule_is_measured());
    assert!(!survey.reward_is_measured());
    assert!(!survey.repeat_is_measured());
}

/// The survey's own refusals, over files: a scenario container that lacks one of
/// the two members, a member that is not decodable and an installation with no
/// world group at all are each a **named** refusal rather than a shorter list.
#[test]
fn accept_f42_d_every_survey_refusal_names_the_container_and_member_it_could_not_read() {
    // A world group whose scenario archive carries `ia.zrd` but no targets.
    let missing = TempInstallation::new("missing");
    missing.write(
        "ZBD/C5/gamez.zbd",
        &world_container(&[ZoneNode {
            name: "dzpath1",
            mesh_index: 949,
            corners: [[0.0; 3], [8.0, 8.0, 8.0]],
        }]),
    );
    missing.write(
        "ZBD/C5/ia1/zrdr.zbd",
        &reader_archive(&[(SCENARIO_MEMBER, zrd_int(1))]),
    );
    match survey_retail_stunt_encoding(&missing.root) {
        Err(StuntEncodingSurveyError::MissingMember { container, member }) => {
            assert_eq!(container, "zbd/c5/ia1/zrdr.zbd");
            assert_eq!(member, SCENARIO_TARGETS_MEMBER);
        }
        other => panic!("a container without its targets member must be refused, got {other:?}"),
    }

    // A member whose bytes are not a `.zrd` document.
    let garbage = TempInstallation::new("garbage");
    garbage.write(
        "ZBD/C5/gamez.zbd",
        &world_container(&[ZoneNode {
            name: "dzpath1",
            mesh_index: 949,
            corners: [[0.0; 3], [8.0, 8.0, 8.0]],
        }]),
    );
    garbage.write(
        "ZBD/C5/ia1/zrdr.zbd",
        &reader_archive(&[
            (SCENARIO_MEMBER, vec![0_u8; 12]),
            (SCENARIO_TARGETS_MEMBER, zrd_int(1)),
        ]),
    );
    match survey_retail_stunt_encoding(&garbage.root) {
        Err(StuntEncodingSurveyError::Decode {
            container,
            member,
            code,
            ..
        }) => {
            assert_eq!(container, "zbd/c5/ia1/zrdr.zbd");
            assert_eq!(member, SCENARIO_MEMBER);
            assert_eq!(code, "unknown_tag");
        }
        other => panic!("a non-.zrd member must be refused, got {other:?}"),
    }

    // No world group at all.
    let bare = TempInstallation::new("bare");
    bare.write("ZBD/planes.zbd", b"authored fixture");
    assert!(
        matches!(
            survey_retail_stunt_encoding(&bare.root),
            Err(StuntEncodingSurveyError::NoWorldGroups)
        ),
        "an installation with no world group has no scenario to measure"
    );
}

// ------------------------------------------------------------------ retail ---

/// The retail root, or a loud failure. The tests that call this are `#[ignore]`d;
/// a test that skipped itself here would report a pass it never earned.
fn retail_root() -> PathBuf {
    PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set for a retail test"))
}

/// The installation fingerprint of the owner's installation, recorded in
/// `docs/findings/2026-10-02-t463-stunt-encoding-and-gate-geometry.md`.
const RETAIL_INSTALL_SHA256: &str =
    "b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978";

/// The measured corpus, through the production survey.
///
/// Measured over the owner's installation: **54** fly-through danger-zone
/// targets across the six world groups that author any (`c1` 5, `c1b` 5, `c2` 9,
/// `c3` 4, `c4` 14, `c5` 17), every one resolved to a `dzpath<N>` node's
/// measured box. **45** of them sit in a scenario the original marks
/// `stunt_flying` (`c1b`, `c2`, `c4`, `c5`); `c1` and `c3` author fly-through
/// objectives under another mode.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_f42_d_retail_every_fly_through_target_resolves_to_its_measured_world_box() {
    let survey = survey_retail_stunt_encoding(&retail_root()).expect("the retail corpus surveys");

    assert_eq!(
        survey.install_sha256(),
        RETAIL_INSTALL_SHA256,
        "the installation fingerprint the measurement was taken over"
    );
    assert_eq!(
        survey.len(),
        54,
        "54 fly-through danger-zone targets across the six world groups that author any"
    );
    assert_eq!(
        survey.stunt_flying_gates().count(),
        45,
        "45 targets sit in a scenario the original marks `stunt_flying`"
    );
    assert!(
        survey.is_complete(),
        "every measured target resolved to a world node #427 measured a box for"
    );
    assert_eq!(survey.unresolved_gates().count(), 0);

    for gate in survey.gates() {
        assert_eq!(gate.category_label(), FLY_THROUGH_CATEGORY_LABEL);
        assert_eq!(gate.help_label(), FLY_THROUGH_HELP_LABEL);
        assert!(
            gate.span().member() == SCENARIO_TARGETS_MEMBER,
            "every row's provenance is the targets member: {}",
            gate.span().member()
        );
        assert_eq!(gate.span().container_sha256().len(), 64);
        assert!(gate.span().length() > 0);
    }
    let worlds: Vec<(String, usize)> = survey
        .gates()
        .iter()
        .map(|gate| gate.world().key().to_owned())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .map(|name| {
            let count = survey
                .gates()
                .iter()
                .filter(|gate| gate.world().key() == name)
                .count();
            (name, count)
        })
        .collect();
    assert_eq!(
        worlds,
        vec![
            ("c1".to_owned(), 5),
            ("c1b".to_owned(), 5),
            ("c2".to_owned(), 9),
            ("c3".to_owned(), 4),
            ("c4".to_owned(), 14),
            ("c5".to_owned(), 17),
        ],
        "the per-world corpus, in world order"
    );

    // The four modes, stated as the measurement: only the two worlds the
    // original does not mark `stunt_flying` fall outside the mode's set.
    let stunt_worlds: BTreeSet<String> = survey
        .stunt_flying_gates()
        .map(|gate| gate.world().key().to_owned())
        .collect();
    assert_eq!(
        stunt_worlds,
        ["c1b", "c2", "c4", "c5"]
            .iter()
            .map(|world| (*world).to_owned())
            .collect::<BTreeSet<_>>(),
        "the original marks four of the eight instant-action scenarios `stunt_flying`"
    );

    // A named row, so the label→node join is asserted and not only counted:
    // `c2`'s `sghangar` objective binds the world node `dzpath1`.
    let hangar = survey
        .gates()
        .iter()
        .find(|gate| gate.zone_label() == "sghangar")
        .expect("c2 authors a `sghangar` fly-through target");
    assert_eq!(hangar.world().key(), "c2");
    assert_eq!(hangar.world_zone(), Some("dzpath1"));
    assert_eq!(
        hangar.geometry().expect("bound and resolved").zone(),
        "dzpath1"
    );

    // Nothing this encoding carries decides direction, clearance, payout or a
    // repeat policy, and the survey says so rather than filling a guess.
    assert!(!survey.direction_rule_is_measured());
    assert!(!survey.clearance_rule_is_measured());
    assert!(!survey.reward_is_measured());
    assert!(!survey.repeat_is_measured());
}

//! Acceptance suite M01-PLAY-TRIGGERS-TURRET-CLASS (#1491): what M01's nine
//! `workersvoyagezep` turret nodes (`utur1`…`utur4`, `ctur1`…`ctur4`,
//! `maingun9`) are, measured from the shared `ai.zrd`'s `TURRET` table.
//!
//! #1481 gave every part-state chain a damage node, but could not classify the
//! nine turret chains: no `zeppelins.zrd` key declares them. The `TURRET` list of
//! `zbd/zrdr.zbd`'s `ai.zrd` does — `NODES` entries `[mount, part]` with a
//! `WEAPON`, `PARTS` and (records 28 and 31) a `HEALTH` — so
//! `mission_session::parts` now classes exactly those nodes as weapon mounts.
//!
//! The legs prove: the nine chains resolve to the three records the finding
//! names, with their weapon, parts and health; none of the thirty engine chains
//! is a turret; each mount's destruction disables the F29 `Weapon` system on
//! that mount's own node (so one loss never silences the others); and a
//! synthetic table pins the reader's pattern rule and its skips without retail.
//!
//! Nothing here is `verified_original`: no original executable ran, and whether
//! destroying a mount clears the in-play bit stays untraced.

use std::path::PathBuf;

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionStage, TurretRecord, classed_part_node, pattern_matches, plan_part_states,
    read_turret_records, stage_for, turret_record_for,
};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::script_raw::discovery::discover_container;
use cs_sim::damage::{DamageNodeKey, DamageNodeKind, SystemKind};

use crate::campaign::common::label;

/// The retail chains under `workersvoyagezep` the `TURRET` table declares, with
/// the record index, weapon and `HEALTH` each resolves to.
const TURRET_CHAINS: [(&str, usize, &str, Option<f64>); 9] = [
    ("utur1", 31, "wep_29", Some(10.0)),
    ("utur2", 31, "wep_29", Some(10.0)),
    ("utur3", 31, "wep_29", Some(10.0)),
    ("utur4", 31, "wep_29", Some(10.0)),
    ("ctur1", 28, "wep_29", Some(10.0)),
    ("ctur2", 28, "wep_29", Some(10.0)),
    ("ctur3", 28, "wep_29", Some(10.0)),
    ("ctur4", 28, "wep_29", Some(10.0)),
    ("maingun9", 27, "wep_27", None),
];

fn game_dir() -> PathBuf {
    PathBuf::from(std::env::var_os("CS_GAME_DIR").expect("CS_GAME_DIR must be set"))
}

/// The shared `ai.zrd`, decoded through the production readers.
fn retail_ai_zrd() -> ZrdValue {
    let root = game_dir();
    let found = cs_assets::install::discover(&root).expect("the installation is discoverable");
    let record = found
        .manifest
        .files
        .iter()
        .find(|record| record.relative_spelling.logical_key() == "zbd/zrdr.zbd")
        .expect("zbd/zrdr.zbd is in the manifest");
    let bytes = std::fs::read(root.join(record.relative_spelling.as_str())).expect("zrdr.zbd");
    let discovery = discover_container("zbd/zrdr.zbd", &record.relative_spelling, &bytes);
    let program = discovery
        .programs()
        .iter()
        .find(|program| {
            program
                .locator()
                .member()
                .is_some_and(|name| name.eq_ignore_ascii_case("ai.zrd"))
        })
        .expect("zrdr.zbd holds ai.zrd");
    decode_zrd(program.bytes()).expect("ai.zrd decodes")
}

fn key(name: &str) -> DamageNodeKey {
    DamageNodeKey::new(name).expect("a node key")
}

fn retail_stage() -> MissionStage {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    stage_for(&root, &plan).expect("M01's stage reads through the production readers")
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_turret_class_the_nine_chains_are_declared_weapon_mounts() {
    let records = read_turret_records(&retail_ai_zrd());
    // 42 records, of which the 16 standalone ones (0..=15) state no `NODES`.
    assert_eq!(records.len(), 26, "the records that state NODES");
    assert_eq!(records.first().map(|r| r.index), Some(16));
    assert_eq!(records.last().map(|r| r.index), Some(41));
    for (node, index, weapon, health) in TURRET_CHAINS {
        let record = turret_record_for(&records, "workersvoyagezep", node)
            .unwrap_or_else(|| panic!("{node} is declared by no TURRET record"));
        assert_eq!(record.index, index, "{node}'s record");
        assert_eq!(record.weapon.as_deref(), Some(weapon), "{node}'s weapon");
        assert_eq!(record.health, health, "{node}'s HEALTH");
        assert_eq!(
            record.parts,
            ["turret", "gun", "firepoint"],
            "{node}'s PARTS"
        );
        let graph_node = classed_part_node(&records, "workersvoyagezep", &key(node));
        assert_eq!(graph_node.kind(), DamageNodeKind::WeaponMount, "{node}");
        assert_eq!(
            graph_node.disables(),
            Some(SystemKind::Weapon),
            "{node} disables the F29 weapon system"
        );
    }
    // The titles name the three mount families.
    let title = |index: usize| {
        records
            .iter()
            .find(|record| record.index == index)
            .and_then(|record| record.title.as_deref())
    };
    assert_eq!(title(27), Some("MSG_TUR_MAIN"));
    assert_eq!(title(28), Some("MSG_TUR_BELLY"));
    assert_eq!(title(31), Some("MSG_TUR_TOP"));
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_triggers_turret_class_exactly_the_nine_of_the_39_chains_are_mounts() {
    let records = read_turret_records(&retail_ai_zrd());
    let stage = retail_stage();
    let plan = plan_part_states(&stage);
    let mut mounts = Vec::new();
    let mut others = 0;
    for (chain, node) in plan.values().flatten() {
        let actor = chain.first().expect("a chain has an actor").as_str();
        let built = classed_part_node(&records, actor, node);
        if built.kind() == DamageNodeKind::WeaponMount {
            mounts.push((actor.to_owned(), node.as_str().to_owned()));
            // The leaf and the record's PARTS resolve under the same node.
            for part in ["healthy", "turret", "gun", "firepoint"] {
                let mut path = chain[..2].to_vec();
                path.push(part.to_owned());
                assert!(
                    stage.host.resolver.resolve(&path).is_some(),
                    "{path:?} resolves under the mount"
                );
            }
        } else {
            others += 1;
            assert_eq!(built.kind(), DamageNodeKind::InternalStructure);
        }
    }
    mounts.sort();
    let mut expected: Vec<(String, String)> = TURRET_CHAINS
        .iter()
        .map(|(node, ..)| ("workersvoyagezep".to_owned(), (*node).to_owned()))
        .collect();
    expected.sort();
    assert_eq!(
        mounts, expected,
        "the mounts are exactly the nine turret chains"
    );
    assert_eq!(others, 30, "the 18 + 12 engine chains stay unclassed");
}

/// One decoded-shape record: `NODES`, then optional `HEALTH` and `WEAPON`.
fn text(value: &str) -> ZrdValue {
    ZrdValue::Text(value.to_owned())
}

fn record(nodes: &[&[&str]], health: Option<f32>, weapon: &str) -> ZrdValue {
    let mut fields = vec![
        text("NODES"),
        ZrdValue::List(
            nodes
                .iter()
                .map(|chain| ZrdValue::List(chain.iter().map(|part| text(part)).collect()))
                .collect(),
        ),
        text("TITLE"),
        ZrdValue::List(vec![text("MSG_SYNTH")]),
    ];
    if let Some(health) = health {
        fields.extend([
            text("HEALTH"),
            ZrdValue::List(vec![ZrdValue::Float(health)]),
        ]);
    }
    fields.extend([
        text("PARTS"),
        ZrdValue::List(vec![text("turret"), text("gun")]),
        text("WEAPON"),
        ZrdValue::List(vec![text("NAME"), ZrdValue::List(vec![text(weapon)])]),
    ]);
    ZrdValue::List(fields)
}

fn synthetic_table() -> Vec<TurretRecord> {
    let no_nodes = ZrdValue::List(vec![text("TITLE"), ZrdValue::List(vec![text("MSG_SKIP")])]);
    let ai = ZrdValue::List(vec![
        ZrdValue::List(vec![text("OTHER"), ZrdValue::List(vec![])]),
        ZrdValue::List(vec![
            text("TURRET"),
            ZrdValue::List(vec![
                no_nodes,
                record(
                    &[&["synthzep", "gun*"], &["otherzep", "gun*"]],
                    Some(4.0),
                    "wep_a",
                ),
                record(&[&["synthzep", "gun*"]], Some(99.0), "wep_late"),
                record(&[&["synthzep", "deep*", "leaf"]], None, "wep_deep"),
            ]),
        ]),
    ]);
    read_turret_records(&ai)
}

#[test]
fn accept_m01_play_triggers_turret_class_a_synthetic_table_classes_only_what_it_declares() {
    let table = synthetic_table();
    // The record without NODES is skipped, not guessed into a record.
    assert_eq!(table.len(), 3);
    assert_eq!(table[0].index, 1, "indexes are the list positions");

    let declared = turret_record_for(&table, "synthzep", "gun3").expect("gun3 is declared");
    assert_eq!(declared.index, 1, "the earliest record wins");
    assert_eq!(declared.health, Some(4.0));
    assert_eq!(declared.weapon.as_deref(), Some("wep_a"));
    assert_eq!(declared.parts, ["turret", "gun"]);

    for (actor, part) in [
        ("synthzep", "gun"),
        ("synthzep", "gun12"),
        ("synthzep", "engine1"),
        ("elsezep", "gun3"),
        ("synthzep", "leaf"),
        ("synthzep", "deep1"),
    ] {
        assert!(
            turret_record_for(&table, actor, part).is_none(),
            "{actor}/{part} is declared by no two-element NODES entry"
        );
    }

    let mount = classed_part_node(&table, "synthzep", &key("gun3"));
    assert_eq!(mount.kind(), DamageNodeKind::WeaponMount);
    assert_eq!(mount.disables(), Some(SystemKind::Weapon));
    let engine = classed_part_node(&table, "synthzep", &key("engine1"));
    assert_eq!(engine.kind(), DamageNodeKind::InternalStructure);
    assert_eq!(
        engine.disables(),
        None,
        "an unclassed part declares no system"
    );
}

#[test]
fn accept_m01_play_triggers_turret_class_the_pattern_star_is_exactly_one_digit() {
    assert!(pattern_matches("ctur*", "ctur1"));
    assert!(pattern_matches("maingun*", "maingun9"));
    assert!(!pattern_matches("ctur*", "ctur"));
    assert!(!pattern_matches("ctur*", "ctur01"));
    assert!(!pattern_matches("ctur*", "ctura"));
    assert!(!pattern_matches("ctur*", "utur1"));
    assert!(pattern_matches("workersvoyagezep", "workersvoyagezep"));
    assert!(!pattern_matches("workersvoyagezep", "workersvoyagezep2"));
}

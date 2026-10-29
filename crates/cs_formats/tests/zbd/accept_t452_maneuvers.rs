//! Task #2204 (`T452-REG-MANEUVER-CHOOSER`): the `maneuvers.zrd` catalog.
//! Task test prefix: `accept_t452_maneuver_`.
//!
//! The synthetic test authors every byte it reads. The retail test reads
//! `$CS_GAME_DIR` (never writes it) and fails loudly without it. It asserts
//! counts, key names and structure only; no original value is committed.

use cs_formats::zbd::{
    DEFAULT_NATURAL_TOUCH, ManeuverError, read_maneuver_nodes, read_reader_nodes,
};

use super::accept_parity2_dmg_fx_a::with_zrdr;

fn int(value: i32) -> Vec<u8> {
    [1u32.to_le_bytes(), value.to_le_bytes()].concat()
}

fn float(value: f32) -> Vec<u8> {
    [2u32.to_le_bytes(), value.to_le_bytes()].concat()
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

fn authored() -> Vec<u8> {
    let nitro_record = list(&[
        text("autogyro_allowed"),
        list(&[]),
        text("nitro"),
        list(&[int(1)]),
        text("natural_touch"),
        list(&[int(0)]),
        text("steps"),
        list(&[list(&[float(6.0), float(0.0), float(0.0), float(0.0)])]),
    ]);
    let plain_record = list(&[
        text("bias"),
        list(&[float(-0.5)]),
        text("relative"),
        list(&[]),
        text("steps"),
        list(&[
            list(&[float(1.0), float(2.0), float(3.0), float(4.0)]),
            list(&[float(5.0), float(6.0), float(7.0), float(8.0)]),
        ]),
    ]);
    list(&[
        text("nitro_evade"),
        nitro_record,
        text("climb"),
        plain_record,
    ])
}

#[test]
fn accept_t452_maneuver_nodes_lower_flags_by_presence_and_defaults() {
    let bytes = authored();
    let nodes = read_reader_nodes(&bytes).expect("decodes");
    let lowered = read_maneuver_nodes(&nodes).expect("lowers");
    assert_eq!(lowered.len(), 2);

    let nitro = &lowered[0];
    assert_eq!(nitro.name, "nitro_evade");
    assert!(nitro.nitro && nitro.autogyro_allowed && !nitro.relative);
    assert!(nitro.has_natural_touch);
    assert_eq!((nitro.natural_touch, nitro.bias), (0, 0.0));
    assert_eq!(nitro.steps, vec![vec![6.0, 0.0, 0.0, 0.0]]);

    let plain = &lowered[1];
    assert!(!plain.nitro && !plain.autogyro_allowed && plain.relative);
    assert!(!plain.has_natural_touch);
    assert_eq!(plain.natural_touch, DEFAULT_NATURAL_TOUCH);
    assert_eq!(plain.bias, -0.5);
    assert_eq!(plain.steps.len(), 2);
}

#[test]
fn accept_t452_maneuver_nodes_refuse_the_wrong_shape() {
    let bytes = [int(1), int(2)].concat();
    let nodes = read_reader_nodes(&bytes).expect("decodes");
    assert_eq!(
        read_maneuver_nodes(&nodes),
        Err(ManeuverError::NotNamedRecords)
    );
    let bad = list(&[
        text("roll"),
        list(&[text("natural_touch"), list(&[text("two")])]),
    ]);
    let nodes = read_reader_nodes(&bad).expect("decodes");
    assert!(matches!(
        read_maneuver_nodes(&nodes),
        Err(ManeuverError::BadValue {
            key: "natural_touch",
            ..
        })
    ));
}

/// The retail member: seventeen maneuvers, each step row 4 or 7 numbers wide.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t452_maneuver_retail_catalog_has_seventeen_nodes() {
    with_zrdr(|members| {
        let (_, bytes) = members
            .iter()
            .find(|(name, _)| name == "maneuvers.zrd")
            .expect("zrdr.zbd lists maneuvers.zrd");
        let nodes = read_reader_nodes(bytes).expect("decodes with no byte left");
        let lowered = read_maneuver_nodes(&nodes).expect("lowers");
        assert_eq!(lowered.len(), 17);
        let mut names: Vec<&str> = lowered.iter().map(|m| m.name).collect();
        names.sort_unstable();
        names.dedup();
        assert_eq!(names.len(), 17, "names are unique");
        for expected in [
            "roll",
            "rudder_turn",
            "bank_turn",
            "climb",
            "dive",
            "jinking",
            "scissors",
            "rolling_scissors",
            "snap_roll",
            "immelman",
            "loop",
            "split_s",
            "lag_pursuit_roll",
            "high_yo_yo",
            "nitro_evade",
            "barrel_roll",
            "spiral_dive",
        ] {
            assert!(names.contains(&expected), "{expected} is in the file");
        }
        for maneuver in &lowered {
            // Step rows are kept unnamed; their measured widths are 4 and 7.
            // A node may carry no `steps` key at all.
            assert!(
                maneuver.steps.iter().all(|row| matches!(row.len(), 4 | 7)),
                "{} step widths",
                maneuver.name
            );
        }
        // Which nodes carry `nitro` is original data; only that the gate has
        // something to act on is asserted.
        assert!(lowered.iter().any(|m| m.nitro));
        assert!(lowered.iter().any(|m| !m.nitro));
    });
}

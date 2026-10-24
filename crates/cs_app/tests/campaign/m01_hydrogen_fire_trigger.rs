//! What starts `hydrogen_fire` in `ZBD/C3/M01` (#2087). Task test prefix:
//! `accept_m01_hydrogen_fire_`.
//!
//! OBJECTIVE25 of "The Lost Treasure" waits for the animation `hydrogen_fire`
//! to be RUNNING, and OBJECTIVE13 (the last gate before the docking and the
//! `INSTANTWIN`) needs OBJECTIVE26 awake, which only OBJECTIVE25 wakes. These
//! tests read every record of the mission's `mis_anim.zbd` through the
//! production animation reader and event decoder and pin the measured chain:
//! the control program never names the tanks' animations, the four
//! `hydrobombboom<N>` records are the only callers of `hydrogen_fire`, and
//! nothing in the data calls a `hydrobombboom<N>` except the records
//! themselves. What starts the first one is not in the data; see
//! `docs/findings/2026-10-24-m01-hydrogen-fire-trigger.md`.
//!
//! Needs `CS_GAME_DIR`.

use std::collections::BTreeSet;

use cs_app::animation::events::decode_event_stream;
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::stage_for;
use cs_app::world_facts::WorldOperands;
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_animation_index};
use cs_types::install::RelativePath;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

const MIS_ANIM: &str = "ZBD/C3/M01/mis_anim.zbd";
const TANKS: [&str; 4] = [
    "hydrobombboom1",
    "hydrobombboom2",
    "hydrobombboom3",
    "hydrobombboom4",
];

/// One record with every `CALL_ANIMATION` target its sequences spell, per
/// sequence name.
struct Row {
    anim: String,
    object: String,
    root: String,
    activation: u8,
    max_health: f32,
    sequences: Vec<(String, Vec<String>)>,
}

impl Row {
    fn calls(&self) -> impl Iterator<Item = &str> {
        self.sequences
            .iter()
            .flat_map(|(_, calls)| calls.iter().map(String::as_str))
    }

    fn sequence_calls(&self, name: &str) -> Vec<&str> {
        self.sequences
            .iter()
            .filter(|(sequence, _)| sequence == name)
            .flat_map(|(_, calls)| calls.iter().map(String::as_str))
            .collect()
    }
}

fn rows() -> Vec<Row> {
    let data = std::fs::read(game_dir().join(MIS_ANIM)).expect("C3/M01's mis_anim.zbd reads");
    let key = MIS_ANIM.to_lowercase();
    let path = RelativePath::new(&key).expect("a path");
    let mut context = ParseContext::with_defaults(&key);
    let decision =
        dispatch(ZbdProbe::new(&key, &path, &data[..8])).expect("an animation container");
    let index = read_animation_index(&mut context, decision, &data).expect("the index reads");
    let records = index
        .payload()
        .expect("the payload header reads")
        .records()
        .expect("the records walk");
    records
        .iter()
        .map(|record| Row {
            anim: String::from_utf8_lossy(record.anim_name()).into_owned(),
            object: String::from_utf8_lossy(record.object_name()).into_owned(),
            root: String::from_utf8_lossy(record.root_name()).into_owned(),
            activation: record.activation(),
            max_health: record.max_health(),
            sequences: record
                .sequences()
                .iter()
                .map(|sequence| {
                    let events = decode_event_stream(sequence.events())
                        .unwrap_or_else(|e| panic!("{}: {e}", record.index()));
                    let calls = events
                        .iter()
                        .filter(|event| event.statement == "CALL_ANIMATION")
                        .filter_map(|event| event.spelled_name().map(str::to_owned))
                        .collect();
                    (String::from_utf8_lossy(sequence.name()).into_owned(), calls)
                })
                .collect(),
        })
        .collect()
}

fn named<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    let mut found = rows.iter().filter(|row| row.anim == name);
    let row = found
        .next()
        .unwrap_or_else(|| panic!("no record named {name}"));
    assert!(found.next().is_none(), "{name} is stored twice");
    row
}

/// **The chain from the tanks to `hydrogen_fire`.**
///
/// The four `hydrobombboom<N>` records belong to `hydrogentank<N>` (root
/// `healthy`, `HEALTH 15`, no activation mode), each default sequence calls
/// `hydrogen_fire` once, and each `call_others` sequence calls all four. The
/// `hydrogen_fire` record itself is a call-only effect.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_hydrogen_fire_is_called_by_the_four_tank_records() {
    let rows = rows();
    for (n, name) in TANKS.iter().enumerate() {
        let tank = named(&rows, name);
        assert_eq!(tank.object, format!("hydrogentank{}", n + 1));
        assert_eq!(tank.root, "healthy");
        assert_eq!(tank.activation, 0, "{name}: no ON_CALL/start activation");
        assert_eq!(tank.max_health, 15.0, "{name}: HEALTH");
        let default = tank.sequence_calls("");
        assert_eq!(
            default.iter().filter(|c| **c == "hydrogen_fire").count(),
            4,
            "{name}: one hydrogen_fire per burner node"
        );
        assert!(default.contains(&"cargozep1_crash"), "{name}");
        assert_eq!(tank.sequence_calls("call_others"), TANKS, "{name}");
    }
    let fire = named(&rows, "hydrogen_fire");
    assert_eq!(fire.object, "hydrogenfire");
    assert_eq!(fire.activation, 3, "a call-only effect record");
    assert_eq!(fire.calls().count(), 0, "it calls nothing itself");

    let callers: BTreeSet<&str> = rows
        .iter()
        .filter(|row| row.calls().any(|call| call == "hydrogen_fire"))
        .map(|row| row.anim.as_str())
        .collect();
    assert_eq!(callers, TANKS.into(), "only the tank records call it");
}

/// **Nothing outside the tank records calls a tank record**, in the records
/// and in the mission's own files: the first `hydrobombboom<N>` is started by
/// something that is not a spelled call, which the data does not name.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_hydrogen_fire_no_data_calls_the_first_tank_record() {
    let rows = rows();
    let outside: Vec<&str> = rows
        .iter()
        .filter(|row| !TANKS.contains(&row.anim.as_str()))
        .filter(|row| row.calls().any(|call| call.starts_with("hydrobombboom")))
        .map(|row| row.anim.as_str())
        .collect();
    assert!(outside.is_empty(), "callers outside the tanks: {outside:?}");

    // The control program names `hydrogen_fire` and no tank record.
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    let program = &stage.host.control;
    assert!(
        WorldOperands::of(program)
            .animations
            .contains("hydrogen_fire")
    );
    assert!(
        !format!("{program:?}").contains("hydrobombboom"),
        "the control program spells no tank record"
    );

    // Only the group's damage reader and the mission's animation container
    // spell the name.
    let mut holders = Vec::new();
    for file in [
        "ZBD/C3/M01/zrdr.zbd",
        "ZBD/C3/M01/mis_anim.zbd",
        "ZBD/C3/zrdr.zbd",
    ] {
        let bytes = std::fs::read(root.join(file)).expect("a mission file reads");
        if bytes
            .windows(b"hydrobombboom".len())
            .any(|w| w == b"hydrobombboom")
        {
            holders.push(file);
        }
    }
    assert_eq!(holders, ["ZBD/C3/M01/mis_anim.zbd", "ZBD/C3/zrdr.zbd"]);
}

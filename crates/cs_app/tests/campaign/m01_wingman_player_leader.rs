//! M01-WINGMAN-PLAYER-LEADER (#1571): how the original picks a wingman's
//! leader when that leader is the player (finding
//! `docs/findings/2026-10-18-m01-wingman-player-leader.md`).
//!
//! Three members:
//!
//! * the committed `[[ai_player_leader]]` rows (and the player-leader slot
//!   vector among the `[[ai_follow]]` rows) parse under the production loader
//!   and agree with the finding's numbers;
//! * with the owner's image named, every row is re-read from the image's
//!   bytes, and the instruction bytes the finding's leader-selection chain
//!   and "the 80 m push vector is read before any store" claim rest on are
//!   re-read too;
//! * with the installation named, the `aiv.zrd` records of both candidate
//!   M01 archives (`ZBD/C1C/M01` and `ZBD/C3/M01`) and the `vehicle.zrd`
//!   wingman records carry the values the finding states.

use cs_app::engine_facts::engine_facts;
use cs_app::mission_start::{SpawnDispatch, read_aircraft_records};
use cs_content::coordinates::{load_engine_image, original_image_digest};

use super::m01_aiv_layout::member_document;

const TABLE_TEXT: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// The number of `[[ai_player_leader]]` rows the finding commits.
const ROW_COUNT: usize = 7;

struct Row {
    name: String,
    va: u64,
    kind: String,
    value: String,
}

fn rows_of(table: &str) -> Vec<Row> {
    let header = format!("[[{table}]]");
    let mut rows = Vec::new();
    let mut current: Option<Vec<(String, String)>> = None;
    let finish = |entries: Option<Vec<(String, String)>>, rows: &mut Vec<Row>| {
        let Some(entries) = entries else { return };
        let get = |key: &str| -> String {
            entries
                .iter()
                .find(|(k, _)| k == key)
                .unwrap_or_else(|| panic!("a [[{table}]] row lacks `{key}`"))
                .1
                .clone()
        };
        assert_eq!(get("provenance"), "\"static\"");
        let va = get("va");
        rows.push(Row {
            name: get("name").trim_matches('"').to_owned(),
            va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("a hex va"),
            kind: get("kind").trim_matches('"').to_owned(),
            value: get("value"),
        });
    };
    for line in TABLE_TEXT.lines() {
        let line = line.trim();
        if line.starts_with('[') {
            finish(current.take(), &mut rows);
            if line == header {
                current = Some(Vec::new());
            }
        } else if let Some(entries) = current.as_mut()
            && let Some((key, value)) = line.split_once(" = ")
        {
            entries.push((key.to_owned(), value.to_owned()));
        }
    }
    finish(current.take(), &mut rows);
    rows
}

fn named<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("row {name} is committed"))
}

fn as_f64(row: &Row) -> f64 {
    row.value.parse().expect("a float value")
}

#[test]
fn accept_m01_wingman_player_leader_rows_parse_and_agree_with_the_finding() {
    engine_facts().expect("the committed table parses with the player-leader rows in it");
    let rows = rows_of("ai_player_leader");
    assert_eq!(rows.len(), ROW_COUNT);
    assert_eq!(named(&rows, "default_leader_name").value, "\"player\"");
    assert_eq!(as_f64(named(&rows, "stick_unit_scale")), 1.0);
    assert_eq!(
        as_f64(named(&rows, "stick_level_forward_y_max")) as f32,
        0.9
    );
    assert_eq!(as_f64(named(&rows, "stick_recover_forward_y_max")), -0.5);

    // The player-leader station vector lives with the #1563 rows.
    let follow = rows_of("ai_follow");
    let player = |axis: &str| {
        as_f64(named(
            &follow,
            &format!("station_offset_player_leader_{axis}"),
        ))
    };
    assert_eq!((player("x"), player("y"), player("z")), (6.0, 0.0, 18.0));
    // The three stick thresholds the finding names.
    assert_eq!(as_f64(named(&follow, "follow_params_2")) as f32, 0.06);
    assert_eq!(as_f64(named(&follow, "follow_params_3")) as f32, 0.06);
    assert_eq!(as_f64(named(&follow, "follow_params_6")) as f32, 0.35);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_player_leader_rows_and_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    let at = |va: u64| -> usize {
        usize::try_from(facts.image_offset(va).expect("va inside the image")).expect("an offset")
    };

    for row in rows_of("ai_player_leader").iter().chain(
        rows_of("ai_follow")
            .iter()
            .filter(|row| row.name.starts_with("station_offset_player_leader_")),
    ) {
        let offset = at(row.va);
        match row.kind.as_str() {
            "f32" => {
                let bytes: [u8; 4] = image[offset..offset + 4].try_into().expect("four bytes");
                assert_eq!(
                    f32::from_le_bytes(bytes) as f64,
                    row.value.parse::<f32>().expect("an f32") as f64,
                    "{} drifted at {:#x}",
                    row.name,
                    row.va
                );
            }
            "f64" => {
                let bytes: [u8; 8] = image[offset..offset + 8].try_into().expect("eight bytes");
                assert_eq!(
                    f64::from_le_bytes(bytes),
                    as_f64(row),
                    "{} drifted at {:#x}",
                    row.name,
                    row.va
                );
            }
            "cstr" => {
                let want = row.value.trim_matches('"').as_bytes();
                assert_eq!(&image[offset..offset + want.len()], want, "{}", row.name);
                assert_eq!(
                    image[offset + want.len()],
                    0,
                    "{} is NUL-terminated",
                    row.name
                );
            }
            other => panic!("{}: unknown kind {other}", row.name),
        }
    }

    let bytes = |va: u64, want: &[u8], what: &str| {
        let offset = at(va);
        assert_eq!(
            &image[offset..offset + want.len()],
            want,
            "{what} at {va:#x}"
        );
    };
    // Leader selection in the follower: `cmp edi, [0x71c298]` (the player)
    // chooses the station vector 0x61fb88; anything else 0x61fb98 and forces
    // the sub-state.
    bytes(
        0x41e78a,
        &[0x3b, 0x3d, 0x98, 0xc2, 0x71, 0x00],
        "cmp leader, player",
    );
    bytes(0x41e792, &[0xbb, 0x88, 0xfb, 0x61, 0x00], "player vector");
    bytes(
        0x41e7a6,
        &[0xc7, 0x45, 0x08, 0x98, 0xfb, 0x61, 0x00],
        "other vector",
    );
    // The loader's default: mode 4 without a primary target stores "player".
    bytes(
        0x47c6cb,
        &[0x83, 0xb8, 0x7c, 0x06, 0x00, 0x00, 0x04],
        "cmp [+0x67c], 4",
    );
    bytes(0x47c6d4, &[0x68, 0x64, 0x83, 0x62, 0x00], "push \"player\"");
    // The post-spawn pass that resolves +0x2f8 into the +0x2fc link.
    bytes(
        0x47535d,
        &[0xe8, 0xfe, 0xdf, 0x02, 0x00],
        "call the resolver 0x4a3360",
    );
    bytes(
        0x4a3380,
        &[0xb9, 0xb8, 0xda, 0x71, 0x00],
        "mov ecx, vehicle registry",
    );

    // Every reference the follower makes to [ebp-0x40..-0x38]: the first
    // access is a read (`fld [ebp-0x40]` at 0x41ee80) that precedes the first
    // store (`fstp [ebp-0x40]` at 0x41ee85), and nothing stores to the slot
    // earlier in the routine.
    let start = at(0x41e760);
    let body = &image[start..start + (0x41f009 - 0x41e760)];
    let mut references = Vec::new();
    for (index, pair) in body.windows(2).enumerate() {
        if pair[0] & 0xc7 == 0x45 && matches!(pair[1], 0xc0 | 0xc4 | 0xc8) {
            references.push((0x41e760 + index as u64, body[index - 1], pair[1]));
        }
    }
    let spots: Vec<u64> = references.iter().map(|(va, _, _)| *va).collect();
    assert_eq!(
        spots,
        [
            0x41ee75, 0x41ee81, 0x41ee86, 0x41ee89, 0x41ee8e, 0x41ee91, 0x41ee96, 0x41ef3c,
            0x41ef6d
        ],
        "the push vector's references"
    );
    // opcodes: lea (0x8d), fld (0xd9 /0), fstp (0xd9 /3)
    assert_eq!(references[0].1, 0x8d, "first access takes the address");
    assert_eq!(references[1].1, 0xd9, "then reads it");
    assert_eq!(
        body[(0x41ee86 - 0x41e760) as usize] & 0x38,
        0x18,
        "first store is the fstp"
    );
}

fn field<'a>(
    fields: &'a [cs_content::stunts::ZrdValue],
    key: &str,
) -> Option<&'a cs_content::stunts::ZrdValue> {
    fields
        .windows(2)
        .find(|pair| pair[0].as_text() == Some(key))
        .map(|pair| &pair[1])
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_wingman_player_leader_retail_records_match_the_finding() {
    let root =
        std::path::PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set"));

    // ZBD/C1C/M01: no record names the player as a wingman's leader; both
    // wingmen follow a devastator (the non-player branch).
    let (document, source) = member_document(&root, "zbd/c1c/m01/zrdr.zbd", "aiv.zrd");
    let c1c = read_aircraft_records("zbd/c1c/m01", &document, &source).expect("read");
    let wingmen: Vec<_> = c1c
        .iter()
        .filter(|a| a.name.starts_with("wingman"))
        .collect();
    assert_eq!(wingmen.len(), 2);
    for wingman in &wingmen {
        let leader = wingman.primary_target.as_deref().expect("a leader");
        assert!(
            leader.starts_with("devastator_"),
            "{} -> {leader}",
            wingman.name
        );
    }

    // ZBD/C3/M01: Betty is `wingman_1` and names the player.
    let (document, source) = member_document(&root, "zbd/c3/m01/zrdr.zbd", "aiv.zrd");
    let c3 = read_aircraft_records("zbd/c3/m01", &document, &source).expect("read");
    let names: Vec<&str> = c3.iter().map(|a| a.name.as_str()).collect();
    assert_eq!(
        names,
        [
            "player",
            "medkestrel_1",
            "medkestrel_2",
            "wingman_1",
            "medkestrel_3"
        ]
    );
    let player = &c3[0];
    let betty = c3
        .iter()
        .find(|a| a.name == "wingman_1")
        .expect("wingman_1");
    assert_eq!(betty.primary_target.as_deref(), Some("player"));
    assert_eq!(betty.dispatch, SpawnDispatch::WingmanOne);
    assert_eq!(betty.team, player.team, "Betty flies on the player's team");
    assert_eq!(betty.starts_asleep, Some(false));
    let (p, b) = (
        player.stored_pose.as_ref().expect("player pose"),
        betty.stored_pose.as_ref().expect("betty pose"),
    );
    assert_eq!(p.heading, b.heading, "stored on the player's heading");
    let offset = [
        b.position[0] - p.position[0],
        b.position[1] - p.position[1],
        b.position[2] - p.position[2],
    ];
    for (got, want) in offset.iter().zip([48.0_f32, 0.0, 107.08]) {
        assert!((got - want).abs() < 1e-3, "{offset:?}");
    }

    // Every airframe's `w*` record (what `wingman_1` can spawn as) runs the
    // mode-4 follower; so does the Black Swan's `bswingman`.
    let (vehicles, _) = member_document(&root, "zbd/zrdr.zbd", "vehicle.zrd");
    let mut seen = Vec::new();
    for record in vehicles.as_list().unwrap_or_default() {
        for pair in record.as_list().unwrap_or_default().chunks(2) {
            let [name, fields] = pair else { continue };
            let (Some(name), Some(fields)) = (name.as_text(), fields.as_list()) else {
                continue;
            };
            if let Some(mode) = field(fields, "mode")
                .and_then(|v| v.as_list()?.first()?.as_text().map(str::to_owned))
                && mode == "wingman"
            {
                seen.push(name.to_owned());
            }
        }
    }
    seen.sort();
    assert_eq!(
        seen,
        [
            "bswingman",
            "wautogyro",
            "wavenger",
            "wbalmoral",
            "wbloodhawk",
            "wbrigand",
            "wfirebrand",
            "wfury",
            "wingman",
            "wkestrel",
            "wpeacemaker",
            "wwarhawk"
        ]
    );
}

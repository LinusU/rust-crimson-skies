//! Task #2209 (`GROUNDBLOW-SWEEP-DECODE`): the committed
//! `[[groundblow_sweep_fact]]` rows of `data/owner-static/engine-facts.toml`
//! and the `bytes` block of
//! `docs/findings/2026-10-29-groundblow-world-segment-sweep.md`.
//!
//! The rows cite the world segment sweep `0x4c8ec0` / `0x4c8f70`, its hit
//! record and the ground-blow probe `0x48bf60`. The first test needs no
//! original data: it checks that the finding and the committed table name the
//! same addresses with the same bytes, and that the instructions those bytes
//! hold still say what the finding claims (the record stride and cap, the
//! `V+0xcc` polarity, the two constants). The second re-reads every row from
//! the owner's image.
//!
//! Static measurement only; nothing here awards `verified_original`.

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const FINDING: &str =
    include_str!("../../../docs/findings/2026-10-29-groundblow-world-segment-sweep.md");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// Rows the finding's conclusions rest on; each must be cited by both the
/// finding and the committed table.
const REQUIRED: &[&str] = &[
    "probe.elev_axis",
    "probe.self_node",
    "probe.query_call",
    "probe.pick",
    "probe.player_owner",
    "probe.amount",
    "probe.blow",
    "caller.torque",
    "caller.velocity",
    "query.entry",
    "query.nearest",
    "sweep.return",
    "sweep.world_box",
    "sweep.grid_flags",
    "sweep.node_list",
    "sweep.node_flags",
    "sweep.cap",
    "sweep.type_dispatch",
    "sweep.type_table",
    "record.owner",
    "record.sub_object",
    "record.stride",
    "record.distance",
    "record.normal",
    "record.point",
    "owner.aircraft_gate",
    "owner.group_bit",
    "owner.map_walk",
    "veh.cc_set",
    "veh.cc_wake",
    "veh.cc_far",
    "veh.ac_grace",
    "veh.ac_skip",
    "veh.b4_damp",
    "veh.b4_reset",
    "lock.dispatch",
    "lock.bit",
    "const.groundblow",
    "const.zero",
    "const.one",
    "const.two",
    "message.class_type",
];

struct Row {
    name: String,
    va: u64,
    bytes: Vec<u8>,
}

fn hex(text: &str) -> Vec<u8> {
    assert!(
        text.len().is_multiple_of(2) && !text.is_empty(),
        "even hex: {text}"
    );
    (0..text.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&text[i..i + 2], 16).expect("a hex byte"))
        .collect()
}

/// The finding's `bytes` block: `va: hex  # name: note` per line.
fn finding_rows() -> Vec<Row> {
    let block = FINDING
        .split("```bytes\n")
        .nth(1)
        .and_then(|rest| rest.split("```").next())
        .expect("the finding has a `bytes` block");
    block
        .lines()
        .filter(|line| !line.trim().is_empty())
        .map(|line| {
            let (va, rest) = line.split_once(": ").expect("`va: hex  # note`");
            let hex_text = rest.split('#').next().expect("hex").trim();
            let name = rest
                .split('#')
                .nth(1)
                .and_then(|note| note.trim().split(':').next())
                .expect("a row name")
                .trim()
                .to_string();
            Row {
                name,
                va: u64::from_str_radix(va, 16).expect("a hex va"),
                bytes: hex(hex_text),
            }
        })
        .collect()
}

/// The committed `[[groundblow_sweep_fact]]` rows, in file order.
fn table_rows() -> Vec<Row> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a groundblow_sweep_fact row lacks {name}"))
    };
    TABLE
        .split("[[groundblow_sweep_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-29");
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            Row {
                name: field(chunk, "name"),
                va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va"),
                bytes: hex(&field(chunk, "bytes")),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("the {name} row"))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

fn f32_row(rows: &[Row], name: &str, at: usize) -> f32 {
    let bytes = &row(rows, name).bytes;
    f32::from_le_bytes(bytes[at..at + 4].try_into().expect("four bytes"))
}

/// The finding and the committed table are the same citations, and the bytes
/// still say what the finding claims.
#[test]
fn accept_groundblow_sweep_rows_match_the_finding_and_the_table() {
    let finding = finding_rows();
    let table = table_rows();
    assert_eq!(
        finding.len(),
        table.len(),
        "the finding cites {} windows, the table {}",
        finding.len(),
        table.len()
    );
    for (cited, committed) in finding.iter().zip(&table) {
        assert_eq!(cited.name, committed.name, "row order");
        assert_eq!(cited.va, committed.va, "{} va", cited.name);
        assert_eq!(cited.bytes, committed.bytes, "{} bytes", cited.name);
    }
    for name in REQUIRED {
        row(&finding, name);
        row(&table, name);
    }

    // The record is 0x2c bytes and the query keeps at most 0x20 of them.
    assert!(
        contains(&row(&finding, "record.stride").bytes, &[0x83, 0xc2, 0x2c]),
        "`add edx, 0x2c` advances the record pointer"
    );
    assert!(
        contains(&row(&finding, "sweep.cap").bytes, &[0xb3, 0x20]),
        "`mov bl, 0x20` is the 32-contact cap"
    );
    // The nearest-contact field is a squared distance: 0x4c8ec0 calls
    // 0x538880, not 0x5388d0.
    assert!(
        contains(
            &row(&finding, "record.distance").bytes,
            &[0xe8, 0x6c, 0xf9, 0x06, 0x00]
        ),
        "0x4c8f0f calls 0x538880 (squared distance)"
    );
    // The hit-owner gate keeps the hit when `V+0xcc` is non-zero: the bytes are
    // `test cl, cl; jne 0x48c0e6`, so the fall-through is the discarding one.
    assert!(
        contains(
            &row(&finding, "probe.player_owner").bytes,
            &[0x84, 0xc9, 0x75, 0x30]
        ),
        "`test cl,cl; jne` keeps the hit for a non-zero vehicle+0xcc"
    );
    // The amount is `(elev - d) / elev * sqrt(dot)`, gated on dot > 0.
    assert!(
        contains(
            &row(&finding, "probe.amount").bytes,
            &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]
        ),
        "`fcomp [0x6032c8]` is the 0.0 threshold"
    );
    // The collision grace is `now + 1.0`.
    assert!(
        contains(
            &row(&finding, "veh.ac_grace").bytes,
            &[0xd8, 0x05, 0xdc, 0x32, 0x60, 0x00]
        ),
        "`fadd [0x6032dc]` is the 1.0-second grace"
    );

    // The constants the finding quotes are the cited dwords.
    assert!((f32_row(&finding, "const.groundblow", 0) - 0.15).abs() < 1e-7);
    assert!((f32_row(&finding, "const.groundblow", 4) + 0.05).abs() < 1e-7);
    assert_eq!(f32_row(&finding, "const.zero", 0), 0.0);
    assert_eq!(f32_row(&finding, "const.one", 0), 1.0);
    assert_eq!(f32_row(&finding, "const.two", 0), 2.0);

    // The unintersectable class types are named by this string.
    let message = &row(&finding, "message.class_type").bytes;
    assert!(
        message.starts_with(b"Unrecognized node class type:  node = %s"),
        "the class-type message"
    );
}

/// Every cited window is in the owner's decrypted executable.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_groundblow_sweep_bytes_match_the_engine_image() {
    let loaded = cs_content::coordinates::load_engine_image()
        .expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(
        loaded.digest,
        cs_content::coordinates::original_image_digest(),
        "the loader only returns the measured image"
    );
    let image = loaded.bytes.as_slice();
    let mut checked = 0usize;
    for row in table_rows() {
        let offset = usize::try_from(row.va - IMAGE_BASE).expect("an offset");
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
        checked += 1;
    }
    assert_eq!(checked, table_rows().len());
    assert!(checked >= 40, "the table cites {checked} windows");
}

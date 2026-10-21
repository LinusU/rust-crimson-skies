//! Acceptance for Rally #1819 `FLAK-A-TURRET-UPDATE-LAW`: the static trace of
//! the original turret update (`0x4aabb0..0x4ab53b`), its target search and
//! scorer, the aim step, the generic weapon fire and the rocket burst rules,
//! recorded as cited rows of `data/owner-static/engine-facts.toml` (finding
//! `docs/findings/2026-10-21-flak-a-turret-update-law.md`).
//!
//! The committed table is parsed by the production reader; the ignored test
//! re-reads every cited instruction and constant from the owner's decrypted
//! image so a row cannot drift from the bytes it cites.

use cs_app::engine_facts::engine_facts;

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");
const IMAGE_BASE: usize = 0x40_0000;

/// One `[[turret_update_fact]]` row.
struct Row {
    name: String,
    va: usize,
    bytes: Vec<u8>,
    fact: String,
}

fn field(chunk: &str, name: &str) -> String {
    chunk
        .lines()
        .find_map(|line| {
            let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
            Some(rest.trim().trim_matches('"').to_string())
        })
        .unwrap_or_else(|| panic!("turret_update_fact lacks {name}"))
}

fn rows() -> Vec<Row> {
    TABLE
        .split("[[turret_update_fact]]")
        .skip(1)
        .map(|chunk| {
            // A row ends at the next table header or the end of the file.
            let chunk = chunk.split("\n[").next().unwrap_or(chunk);
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-21");
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            let va = usize::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            Row {
                name: field(chunk, "name"),
                va,
                bytes,
                fact: field(chunk, "fact"),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == name)
        .unwrap_or_else(|| panic!("no turret_update_fact named {name}"))
}

#[test]
fn accept_flak_a_the_committed_table_still_parses_with_the_turret_rows() {
    // The production reader refuses a table outside its strict subset; the
    // new rows must not break the launch's binding.
    let facts = engine_facts().expect("the committed table parses");
    assert_eq!(facts.image(), "crimson.decrypted.exe");
    let rows = rows();
    assert!(rows.len() >= 80, "only {} rows", rows.len());
    let mut names: Vec<&str> = rows.iter().map(|row| row.name.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), rows.len(), "row names are unique");
    for row in &rows {
        assert!(!row.bytes.is_empty(), "{} cites no bytes", row.name);
        assert!(!row.fact.is_empty(), "{} states no fact", row.name);
        // Every cited instruction is inside the image's code or data.
        let offset = facts
            .image_offset(row.va as u64)
            .unwrap_or_else(|error| panic!("{}: {error}", row.name));
        assert!(
            offset < 0x30_0000,
            "{} va {:#x} is past the image",
            row.name,
            row.va
        );
    }
}

#[test]
fn accept_flak_a_the_recorded_update_law_names_each_stage_of_the_task() {
    let rows = rows();
    // Target step: the team test, detection range, line of sight, limits,
    // state machine, fire rate, inaccuracy, the fire call and the burst.
    for name in [
        "target.filter_ctor",
        "target.filter_range",
        "target.accept_vehicle",
        "target.accept_turret",
        "target.score_final",
        "field.team_default",
        "los.ray",
        "aim.pitch_limited",
        "aim.yaw_limited",
        "aim.aligned_cos",
        "timer.bored_end",
        "timer.attack_end",
        "fire.rate_draw",
        "aim.inaccuracy",
        "fire.weapon_fire",
        "rocket.proximity_burst",
        "rocket.splash_radius",
    ] {
        row(&rows, name);
    }
    // The registered values the doc quotes.
    assert!(row(&rows, "field.team_default").fact.contains("team 2"));
    assert_eq!(row(&rows, "fire.weapon_fire").bytes[0], 0xe8, "a call");
    assert_eq!(row(&rows, "aim.aligned_cos").va, 0x4aba51);
}

fn f32_at(image: &[u8], va: usize) -> f32 {
    let offset = va - IMAGE_BASE;
    f32::from_le_bytes(image[offset..offset + 4].try_into().expect("four bytes"))
}

fn f64_at(image: &[u8], va: usize) -> f64 {
    let offset = va - IMAGE_BASE;
    f64::from_le_bytes(image[offset..offset + 8].try_into().expect("eight bytes"))
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_flak_a_cited_rows_match_the_engine_image() {
    let path = std::env::var("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must name the image");
    let image = std::fs::read(path).expect("read the engine image");
    for row in rows() {
        let offset = row.va - IMAGE_BASE;
        assert_eq!(
            &image[offset..offset + row.bytes.len()],
            row.bytes.as_slice(),
            "{} at {:#x}",
            row.name,
            row.va
        );
    }
    // Constants the trace reads (all in .rdata).
    assert_eq!(
        f32_at(&image, 0x603598).to_bits(),
        0x3800_0100,
        "1/32767, the rand01 scale"
    );
    assert_eq!(f32_at(&image, 0x6032dc), 1.0);
    assert_eq!(f32_at(&image, 0x6032e0), 0.5);
    assert_eq!(f32_at(&image, 0x6034fc), 0.2);
    assert_eq!(f32_at(&image, 0x603534), 1200.0);
    assert_eq!(f32_at(&image, 0x608b6c), 37.5);
    assert_eq!(f32_at(&image, 0x603514), 4.0);
    assert_eq!(f32_at(&image, 0x603580), 400.0);
    assert!((f32_at(&image, 0x608854) - 199_844.73).abs() < 0.01);
    assert!(
        (f64_at(&image, 0x608b70) - 0.965_926).abs() < 1e-9,
        "cos 15 degrees"
    );
    assert!((f64_at(&image, 0x603d58) - std::f64::consts::TAU).abs() < 1e-9);
    // The wrapper classes the target search keeps apart (RTTI names).
    let name_at = |va: usize| {
        let word = |at: usize| {
            u32::from_le_bytes(
                image[at - IMAGE_BASE..at - IMAGE_BASE + 4]
                    .try_into()
                    .unwrap(),
            ) as usize
        };
        let locator = word(va - 4);
        let descriptor = word(locator + 12);
        let start = descriptor + 8 - IMAGE_BASE;
        let end = image[start..].iter().position(|&b| b == 0).unwrap() + start;
        String::from_utf8_lossy(&image[start..end]).into_owned()
    };
    assert_eq!(name_at(0x60886c), ".?AVTargetVehicle@@");
    assert_eq!(name_at(0x6088cc), ".?AVTargetTurret@@");
    assert_eq!(name_at(0x608920), ".?AVTargetProjectile@@");
    assert_eq!(name_at(0x60364c), ".?AVTargetStruct@@");
    assert_eq!(name_at(0x608b68), ".?AVTurretRate@TargetRate@@");
}

//! Task #2204 (`T452-REG-MANEUVER-CHOOSER`): the committed `[[maneuver_fact]]`
//! rows of `data/owner-static/engine-facts.toml` that the runtime chooser
//! (`cs_app::ai::maneuver`) is built from. Task test prefix:
//! `accept_t452_maneuver_`. Finding:
//! `docs/findings/2026-10-29-t452-maneuver-chooser.md`.
//!
//! Static measurement only; the ignored image test re-reads every window from
//! `$CS_ENGINE_IMAGE`. No test awards `verified_original`.

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

struct Row {
    name: String,
    va: u64,
    bytes: Vec<u8>,
}

fn rows() -> Vec<Row> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a maneuver_fact row lacks {name}"))
    };
    TABLE
        .split("[[maneuver_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            assert_eq!(field(chunk, "provenance"), "static");
            let va = field(chunk, "va");
            let hex = field(chunk, "bytes");
            Row {
                name: field(chunk, "name"),
                va: u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va"),
                bytes: (0..hex.len() / 2)
                    .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                    .collect(),
            }
        })
        .collect()
}

fn row<'a>(rows: &'a [Row], name: &str) -> &'a Row {
    rows.iter()
        .find(|row| row.name == format!("maneuver_chooser.{name}"))
        .unwrap_or_else(|| panic!("the {name} row"))
}

fn float(rows: &[Row], name: &str) -> f32 {
    f32::from_le_bytes(
        row(rows, name)
            .bytes
            .as_slice()
            .try_into()
            .expect("4 bytes"),
    )
}

/// The constants the runtime chooser uses are the ones the rows record.
#[test]
fn accept_t452_maneuver_rows_hold_the_constants_the_chooser_uses() {
    let rows = rows();
    assert_eq!(rows.len(), 14);
    assert_eq!(float(&rows, "const_one"), 1.0);
    assert_eq!(float(&rows, "const_zero"), 0.0);
    assert_eq!(float(&rows, "const_current"), 0.1);
    assert_eq!(float(&rows, "const_previous"), 0.2);
    assert_eq!(float(&rows, "const_preferred"), 6.0);
    assert!((f64::from(float(&rows, "const_draw_scale")) - 1.0 / 32_767.0).abs() < 1e-10);
    assert_eq!(
        row(&rows, "const_draw_scale").bytes,
        0x3800_0100u32.to_le_bytes()
    );

    // Defaults: `mov dword [esi], 0x63`, then the three flag bytes cleared.
    let defaults = &row(&rows, "record_defaults").bytes;
    assert_eq!(defaults[..6], [0xc7, 0x06, 0x63, 0x00, 0x00, 0x00]);
    assert_eq!(defaults[6..10], [0xc6, 0x46, 0x04, 0x00]);
    assert_eq!(defaults[10..14], [0xc6, 0x46, 0x05, 0x00]);

    // The negative-weight floor is the dword 0x3dcccccd (0.1f).
    let floor = &row(&rows, "weight_floor").bytes;
    assert_eq!(floor[floor.len() - 4..], 0x3dcc_cccdu32.to_le_bytes());
    assert_eq!(f32::from_bits(0x3dcc_cccd), 0.1);

    // The gate rows name the offsets the runtime reads.
    let gates = &row(&rows, "skill_gates").bytes;
    assert!(gates.windows(4).any(|w| w == [0x1c, 0x02, 0x00, 0x00])); // [ecx+0x21c]
    assert!(gates.windows(4).any(|w| w == [0x58, 0x09, 0x00, 0x00])); // [esi+0x958]
}

/// Every cited window is in the owner's decrypted executable, and the key strings
/// the constructor looks up are the ones the runtime lowering reads.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_maneuver_image_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    // Sections are laid out with file offset == RVA for the headers' first page
    // only, so map the virtual address through the PE section table.
    let pe = u32::from_le_bytes(image[0x3c..0x40].try_into().unwrap()) as usize;
    let sections = u16::from_le_bytes(image[pe + 6..pe + 8].try_into().unwrap()) as usize;
    let optional = u16::from_le_bytes(image[pe + 20..pe + 22].try_into().unwrap()) as usize;
    let word = |at: usize| u32::from_le_bytes(image[at..at + 4].try_into().unwrap()) as u64;
    let offset_of = |va: u64| -> usize {
        let rva = va - IMAGE_BASE;
        for index in 0..sections {
            let header = pe + 24 + optional + index * 40;
            let (size, start, raw, raw_at) = (
                word(header + 8),
                word(header + 12),
                word(header + 16),
                word(header + 20),
            );
            if (start..start + size.max(raw)).contains(&rva) {
                return usize::try_from(raw_at + rva - start).expect("offset");
            }
        }
        panic!("0x{va:x} is in no section");
    };
    let at = |va: u64, len: usize| -> &[u8] {
        let offset = offset_of(va);
        &image[offset..offset + len]
    };

    let rows = rows();
    assert_eq!(rows.len(), 14);
    for row in &rows {
        assert_eq!(
            at(row.va, row.bytes.len()),
            row.bytes.as_slice(),
            "{}",
            row.name
        );
    }
    for (va, key) in [
        (0x62_5878u64, "bias"),
        (0x62_5880, "natural_touch"),
        (0x62_5890, "relative"),
        (0x62_589c, "nitro"),
        (0x62_58a4, "autogyro_allowed"),
        (0x62_58b8, "steps"),
    ] {
        let mut want = key.as_bytes().to_vec();
        want.push(0);
        assert_eq!(at(va, want.len()), want.as_slice(), "key {key}");
    }
}

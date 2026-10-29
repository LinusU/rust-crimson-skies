//! Task #2179 (T452-REG-TANGLER-EFFECT): what a zero-damage `TANGLER` does.
//!
//! Task test prefix: `accept_t452_tangler_`. Finding:
//! `docs/findings/2026-10-28-t452-tangler-effect.md`.
//!
//! The committed rows carry the addresses and bytes of every measured site;
//! the image test re-reads each byte window from `$CS_ENGINE_IMAGE` (static
//! analysis, never original-run behavior).

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// `(name, va, bytes, fact)` of every committed `[[tangler_fact]]` row.
fn committed_rows() -> Vec<(String, u64, Vec<u8>, String)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a tangler_fact row lacks {name}"))
    };
    TABLE
        .split("[[tangler_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            assert_eq!(field(chunk, "provenance"), "static");
            (field(chunk, "name"), va, bytes, field(chunk, "fact"))
        })
        .collect()
}

/// The rows name every site the tangle effect is measured at, and the two
/// bytes that decide it: the `TANGLER` flag test and the flag-2 force test.
#[test]
fn accept_t452_tangler_the_committed_rows_cover_the_measured_sites() {
    let rows = committed_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "tangler.hit_branch",
            "tangler.hit_duration",
            "tangler.status_store",
            "tangler.status_timer",
            "tangler.force_zero",
            "tangler.countdown_force",
            "tangler.countdown_other",
            "tangler.boost_refuse",
            "tangler.ai_gate",
        ]
    );
    let get = |name: &str| rows.iter().find(|r| r.0 == name).expect("committed row");
    // `test ecx, 0x40000`: the TANGLER flag the parser sets at 0x4ba859.
    assert_eq!(get("tangler.hit_branch").1, 0x004b_9ded);
    assert_eq!(
        get("tangler.hit_branch").2[..6],
        [0xf7, 0xc1, 0x00, 0x00, 0x04, 0x00]
    );
    // `test byte ptr [esi+0x2dc], 2` followed by the force routine's zeroing.
    assert_eq!(get("tangler.force_zero").1, 0x0048_fdd0);
    assert!(get("tangler.force_zero").3.contains("0.0"));
}

/// Every cited byte window is in the owner's decrypted executable.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_tangler_image_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    let rows = committed_rows();
    assert_eq!(rows.len(), 9);
    for (name, va, bytes, _) in rows {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at 0x{va:x}"
        );
    }
}

/// `(name, va, bytes, fact)` of every committed `[[tangler_origin_fact]]` row.
fn origin_rows() -> Vec<(String, u64, Vec<u8>, String)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a tangler_origin_fact row lacks {name}"))
    };
    TABLE
        .split("[[tangler_origin_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            assert_eq!(field(chunk, "provenance"), "static");
            (field(chunk, "name"), va, bytes, field(chunk, "fact"))
        })
        .collect()
}

/// Task #2201: the rows name the caller's three constructor arguments and the
/// one site that invokes the hook.
#[test]
fn accept_t452_tangler_origin_rows_name_the_caller_sources() {
    let rows = origin_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "tangler_origin.hook_arguments",
            "tangler_origin.owner_lookup",
            "tangler_origin.centre_argument",
            "tangler_origin.hook_site",
        ]
    );
    let get = |name: &str| rows.iter().find(|r| r.0 == name).expect("committed row");
    // `add edi, 0x48` before `call 0x4b94e0`: the centre is rocket+0x48.
    let centre = get("tangler_origin.centre_argument");
    assert_eq!(centre.2[3..6], [0x83, 0xc7, 0x48]);
    assert_eq!(centre.2[centre.2.len() - 5], 0xe8);
    // `mov ecx, [edi+4]`: the owner id is rocket+4.
    assert_eq!(
        get("tangler_origin.owner_lookup").2[4..7],
        [0x8b, 0x4f, 0x04]
    );
    // `call eax` loaded from `[esi+0x20c]`, in the burst function.
    let site = get("tangler_origin.hook_site");
    assert_eq!(site.2[4..10], [0x8b, 0x86, 0x0c, 0x02, 0x00, 0x00]);
    assert_eq!(site.2[site.2.len() - 2..], [0xff, 0xd0]);
}

/// The call `0x4ba6d1` is the one and only direct caller of `0x4b94e0` in the
/// image, every cited window is in the image, and the hook site is a call
/// inside the burst function's range.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t452_tangler_origin_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    for (name, va, bytes, _) in origin_rows() {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at 0x{va:x}"
        );
    }
    // Closed inventory of direct `call`/`jmp rel32` sites targeting 0x4b94e0.
    let target = 0x4b_94e0_i64;
    let mut callers = Vec::new();
    for offset in 0x1000..image.len().min(0x20_2000) - 5 {
        if matches!(image[offset], 0xe8 | 0xe9) {
            let rel = i32::from_le_bytes(image[offset + 1..offset + 5].try_into().unwrap());
            let va = offset as i64 + IMAGE_BASE as i64;
            if va + 5 + i64::from(rel) == target {
                callers.push(va);
            }
        }
    }
    assert_eq!(callers, [0x4b_a6d1]);
}

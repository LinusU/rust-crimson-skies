//! Task #2208 (HIT-RECORD-CONTACT-POINT): what fills the hit record's
//! position `[record+0xc]` on a projectile contact, and what the point is on
//! the target's collision volume.
//!
//! Task test prefix: `accept_hit_record_`. Finding:
//! `docs/findings/2026-10-29-hit-record-contact-point.md`.
//!
//! The committed rows carry the addresses and bytes of every measured site;
//! the image test re-reads each byte window from `$CS_ENGINE_IMAGE` (static
//! analysis, never original-run behavior).

const TABLE: &str = include_str!("../../../data/owner-static/engine-facts.toml");

/// The image base the executable's virtual addresses are relative to.
const IMAGE_BASE: u64 = 0x40_0000;

/// `(name, va, bytes, fact)` of every committed `[[hit_record_fact]]` row.
fn committed_rows() -> Vec<(String, u64, Vec<u8>, String)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a hit_record_fact row lacks {name}"))
    };
    TABLE
        .split("[[hit_record_fact]]")
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

/// The rows name every site the record position is measured at: the swept
/// segment's construction, both query call sites, the class dispatch, the two
/// shape tests, the point/normal writes, the surface-object handling, the
/// distance sort and the pick.
#[test]
fn accept_hit_record_the_committed_rows_cover_the_measured_sites() {
    let rows = committed_rows();
    let names: Vec<&str> = rows.iter().map(|r| r.0.as_str()).collect();
    assert_eq!(
        names,
        [
            "hit_record.segment_built",
            "hit_record.query_call",
            "hit_record.query_call_slow",
            "hit_record.class_dispatch",
            "hit_record.jump_table",
            "hit_record.node_surface_write",
            "hit_record.tail_advance",
            "hit_record.box_test_call",
            "hit_record.soup_call",
            "hit_record.poly_point_write",
            "hit_record.poly_normal_write",
            "hit_record.first_face_wins",
            "hit_record.box_surface_clear",
            "hit_record.mesh_surf_write",
            "hit_record.world_xform",
            "hit_record.distance_fill",
            "hit_record.nearest_index",
            "hit_record.effect_only_check",
            "hit_record.nearest_pick",
        ]
    );
    let get = |name: &str| rows.iter().find(|r| r.0 == name).expect("committed row");

    // The writer: `fstp dword [esi+0xc]` opens the point store in 0x55d6c0.
    assert_eq!(get("hit_record.poly_point_write").1, 0x0055_d7d8);
    assert_eq!(
        get("hit_record.poly_point_write").2[..3],
        [0xd9, 0x5e, 0x0c],
        "`fstp [esi+0xc]` is the record-position write the task asked for"
    );
    assert!(
        get("hit_record.poly_point_write")
            .3
            .contains("seg_start + t*(seg_end-seg_start)"),
        "the row states the position is the segment's own plane-crossing point"
    );

    // The query: `add esi,0x48; push esi; push eax; call 0x4c8ec0` — the
    // swept segment starts at rocket+0x48 and the callee is the scene query.
    assert_eq!(get("hit_record.query_call").1, 0x005b_047e);
    assert_eq!(
        get("hit_record.query_call").2[..3],
        [0x83, 0xc6, 0x48],
        "`add esi,0x48` passes &rocket+0x48 as the segment start"
    );

    // The dispatch: `jmp [ecx*4+0x4c9de0]` selects one of ten node classes.
    assert_eq!(get("hit_record.class_dispatch").1, 0x004c_9ac8);
    assert_eq!(
        &get("hit_record.class_dispatch").2[0x13..0x17],
        [0xe0, 0x9d, 0x4c, 0x00],
        "the indirect jump names the class-handler table at 0x4c9de0"
    );

    // Both volume kinds appear in the rows: the bound-box face test and the
    // polygon-soup test, each ending in the same 0x55d6c0 writer.
    assert_eq!(get("hit_record.box_test_call").1, 0x004c_c1c4);
    assert!(
        get("hit_record.box_test_call")
            .3
            .contains("eight stored bound corners"),
        "the box row names the tested volume"
    );
    assert_eq!(get("hit_record.soup_call").1, 0x004c_a238);
    assert!(
        get("hit_record.soup_call").3.contains("[node+0x3c]"),
        "the soup row names the node's face list"
    );

    // The pick: the list head carries the nearest element's index and the
    // rescan skips effect-only surfaces.
    assert_eq!(get("hit_record.nearest_index").1, 0x004c_8f5b);
    assert_eq!(get("hit_record.effect_only_check").1, 0x005a_d367);
    assert!(
        get("hit_record.effect_only_check")
            .3
            .contains("effect-only"),
        "the row states the solid-surface filter"
    );
}

/// Every cited byte window is in the owner's decrypted executable.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_hit_record_image_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    let rows = committed_rows();
    assert_eq!(rows.len(), 19);
    for (name, va, bytes, _) in rows {
        let offset = usize::try_from(va - IMAGE_BASE).expect("offset");
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#08x} no longer matches the image bytes"
        );
    }
}

/// The image test's call-target bytes land where the finding says they land:
/// the query calls 0x4c8ec0, the class dispatch jumps through 0x4c9de0, and
/// the two shape tests call their face workers.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_hit_record_image_call_targets_match_the_finding() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this measurement needs the owner-supplied decrypted \
         executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    let rel32_at = |va: u64| -> u64 {
        let at = usize::try_from(va - IMAGE_BASE).expect("offset");
        assert_eq!(
            image[at], 0xe8,
            "call site at {va:#08x} is not a rel32 call"
        );
        let rel = i32::from_le_bytes(image[at + 1..at + 5].try_into().unwrap());
        (va + 5).wrapping_add(rel as i64 as u64)
    };
    let get = |name: &str| {
        committed_rows()
            .into_iter()
            .find(|r| r.0 == name)
            .expect("committed row")
    };
    // `call 0x4c8ec0` is the last five bytes of both query-call windows.
    let (.., va, bytes, _) = get("hit_record.query_call");
    assert_eq!(rel32_at(va + bytes.len() as u64 - 5), 0x004c_8ec0);
    let (.., va, bytes, _) = get("hit_record.query_call_slow");
    assert_eq!(rel32_at(va + bytes.len() as u64 - 5), 0x004c_8ec0);
    // The box path calls 0x55d270; the soup path calls 0x55c9c0.
    let (.., va, bytes, _) = get("hit_record.box_test_call");
    assert_eq!(rel32_at(va + bytes.len() as u64 - 5), 0x0055_d270);
    let (.., va, bytes, _) = get("hit_record.soup_call");
    assert_eq!(rel32_at(va + bytes.len() as u64 - 5), 0x0055_c9c0);
    // The distance fill calls the length helper 0x538880.
    let (.., va, _, _) = get("hit_record.distance_fill");
    assert_eq!(rel32_at(va + 5), 0x0053_8880);
    // The class table's own entries point inside the per-node test.
    let (.., va, _, _) = get("hit_record.jump_table");
    let at = usize::try_from(va - IMAGE_BASE).expect("offset");
    for i in 0..10 {
        let target = u32::from_le_bytes(image[at + 4 * i..at + 4 * i + 4].try_into().unwrap());
        assert!(
            (0x004c_9a00..0x004c_9e10).contains(&target),
            "jump-table entry {i} target {target:#08x} left the per-node test"
        );
    }
}

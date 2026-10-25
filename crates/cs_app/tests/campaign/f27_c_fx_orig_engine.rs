//! Does the engine play the weapon row's FIRE ANIMATION? (#2060)
//! Task test prefix: `accept_f27_c_fx_orig_engine_`.
//!
//! Rows are `[[weapon_fire_animation_fact]]` of
//! `data/owner-static/engine-facts.toml`, described in
//! `docs/findings/2026-10-25-f27-c-fx-orig-engine-fire-animation.md`.

const TABLE: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[weapon_fire_animation_fact]]` row.
fn rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a fire animation row lacks {name}"))
    };
    TABLE
        .split("[[weapon_fire_animation_fact]]")
        .skip(1)
        .map(|chunk| chunk.split("\n[").next().unwrap_or(chunk))
        .map(|chunk| {
            let va = field(chunk, "va");
            let va = u64::from_str_radix(va.trim_start_matches("0x"), 16).expect("hex va");
            assert_eq!(field(chunk, "provenance"), "static");
            assert_eq!(field(chunk, "claim"), "OWNER-STATIC-2026-10-25");
            let hex = field(chunk, "bytes");
            let bytes = (0..hex.len() / 2)
                .map(|i| u8::from_str_radix(&hex[2 * i..2 * i + 2], 16).expect("hex byte"))
                .collect();
            (field(chunk, "name"), va, bytes)
        })
        .collect()
}

#[test]
fn accept_f27_c_fx_orig_engine_rows_name_the_fire_animation_path() {
    let rows = rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "fire.block_parse",
            "fire.block_animation_key",
            "fire.effect_precedes_animation",
            "fire.animation_gate",
            "fire.animation_slot",
            "fire.animation_attached_branch",
            "anim.event_offset_parse",
            "anim.call_animation_at_node",
        ]
    );
    let get = |name: &str| &rows.iter().find(|(n, _, _)| n == name).expect("a row").2;
    // push 0x63cfd8 ("FIRE"); lea edx,[ebx+0x94].
    assert_eq!(get("fire.block_parse")[..5], [0x68, 0xd8, 0xcf, 0x63, 0x00]);
    assert_eq!(get("fire.block_parse")[5..11], [0x8d, 0x93, 0x94, 0, 0, 0]);
    // push 0x63d318 ("ANIMATION").
    assert_eq!(
        get("fire.block_animation_key")[..5],
        [0x68, 0x18, 0xd3, 0x63, 0x00]
    );
    // mov eax,[esi+0xb8] (EFFECT); mov eax,[esi+0xec] (fire index).
    assert_eq!(
        get("fire.effect_precedes_animation")[..3],
        [0x8b, 0x86, 0xb8]
    );
    assert_eq!(get("fire.animation_gate")[..3], [0x8b, 0x86, 0xec]);
    // lea edx,[esi+ecx*4+0x98].
    assert_eq!(get("fire.animation_slot")[..3], [0x8d, 0x94, 0x8e]);
    assert_eq!(get("fire.animation_slot")[3], 0x98);
    // mov edx,[esi+0x94]; test dl,2.
    assert_eq!(get("fire.animation_attached_branch")[..2], [0x8b, 0x96]);
    assert_eq!(
        get("fire.animation_attached_branch")[6..9],
        [0xf6, 0xc2, 0x02]
    );
    // fild dword [eax+0x14]: an integer EVENT_OFFSET is converted, not scaled.
    assert_eq!(get("anim.event_offset_parse")[..3], [0xdb, 0x40, 0x14]);
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_f27_c_fx_orig_engine_rows_match_the_engine_image() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect("CS_ENGINE_IMAGE must be set");
    let image = std::fs::read(path).expect("the engine image reads");
    let facts = cs_app::engine_facts::engine_facts().expect("the committed table parses");
    for (name, va, bytes) in rows() {
        let offset = facts
            .image_offset(va)
            .unwrap_or_else(|error| panic!("{name}: {error}")) as usize;
        assert_eq!(
            &image[offset..offset + bytes.len()],
            bytes.as_slice(),
            "{name} at {va:#x}"
        );
    }
}

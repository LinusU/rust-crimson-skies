//! Does the hit dispatcher consult a team relation? (#2102)
//! Task test prefix: `accept_m01_hit_dispatch_team_`.
//!
//! The static trace of `0x5abcf0` and the gun-round path (finding
//! `docs/findings/2026-10-25-m01-hit-dispatch-team-trace.md`) found no team,
//! side or owner compare between a round's impact and the node's registered
//! weapon-hit handler. The rows are `[[hit_dispatch_fact]]` of
//! `data/owner-static/engine-facts.toml`; these tests pin them to the owner's
//! decrypted image (`CS_ENGINE_IMAGE`) and to the designed rule this finding
//! leaves unmeasured.

use cs_sim::targeting::Allegiance;
use cs_sim::weapons::guns::FriendlyFireRule;

const TABLE: &str = include_str!("../../../../data/owner-static/engine-facts.toml");

/// `(name, va, bytes)` of every `[[hit_dispatch_fact]]` row.
fn rows() -> Vec<(String, u64, Vec<u8>)> {
    let field = |chunk: &str, name: &str| {
        chunk
            .lines()
            .find_map(|line| {
                let rest = line.strip_prefix(name)?.trim_start().strip_prefix('=')?;
                Some(rest.trim().trim_matches('"').to_string())
            })
            .unwrap_or_else(|| panic!("a hit dispatch row lacks {name}"))
    };
    TABLE
        .split("[[hit_dispatch_fact]]")
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

/// **The committed rows name the gates the dispatcher has, and only those.**
#[test]
fn accept_m01_hit_dispatch_team_the_committed_rows_name_the_gates() {
    let rows = rows();
    let names: Vec<&str> = rows.iter().map(|(name, _, _)| name.as_str()).collect();
    assert_eq!(
        names,
        [
            "hit.dispatch_entry",
            "hit.dispatch_call",
            "hit.surface_index_is_not_team",
            "hit.gun_round_mask",
            "hit.best_contact_filter",
            "hit.gun_round_dispatch",
            "hit.damage_pair_weapon_only",
        ]
    );
    let get = |name: &str| rows.iter().find(|(n, _, _)| n == name).expect("a row");
    // mov eax,[esi+0x24]; cmp eax,ebx; je ...; mov edi,[eax+0xbc]; cmp edi,ebx; je ...
    let entry = &get("hit.dispatch_entry").2;
    assert_eq!(entry[..3], [0x8b, 0x46, 0x24]);
    assert_eq!(entry[11..17], [0x8b, 0xb8, 0xbc, 0x00, 0x00, 0x00]);
    // call [edi+4] with (handler, weapon, record, damage pair).
    assert_eq!(get("hit.dispatch_call").2[6..], [0xff, 0x57, 0x04]);
    // push 0x40000 before the ray query: a node-flag mask.
    assert_eq!(
        get("hit.gun_round_mask").2[..5],
        [0x68, 0x00, 0x00, 0x04, 0x00]
    );
    // The contact filter compares [node+0x20] with 1.
    assert_eq!(
        get("hit.best_contact_filter").2[11..],
        [0x8b, 0x40, 0x20, 0x83, 0xf8, 0x01, 0x0f, 0x85, 0xa0]
    );
    // The gun round dispatches on [node+0xbc] != 0 alone.
    assert_eq!(
        get("hit.gun_round_dispatch").2[3..9],
        [0x8b, 0x82, 0xbc, 0x00, 0x00, 0x00]
    );
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_hit_dispatch_team_the_rows_match_the_engine_image() {
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

/// **The designed `HostileOnly` rule is stricter than anything the dispatcher
/// measures**: it refuses a neutral relation that the engine path never reads.
/// Pinned so a later change to the rule is a conscious edit of this finding.
#[test]
fn accept_m01_hit_dispatch_team_the_designed_rule_refuses_what_the_engine_does_not() {
    assert!(!FriendlyFireRule::HostileOnly.allows(Some(Allegiance::Neutral)));
    assert!(!FriendlyFireRule::HostileOnly.allows(None));
    assert!(FriendlyFireRule::NonFriendly.allows(Some(Allegiance::Neutral)));
}

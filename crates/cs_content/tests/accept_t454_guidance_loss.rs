//! Task #454: what each retail guided rocket does once its target is lost.
//!
//! Task test prefix: `accept_t454_`. Findings:
//! `docs/findings/2026-10-25-f28-guidance-loss-per-weapon-audit.md`.
//!
//! Two probes, independent of each other: the **data** probe reads every
//! `ROCKET` record through the production reader and derives the per-weapon
//! lost-target rule; the **code** probe scans the owner-supplied decrypted
//! executable (`$CS_ENGINE_IMAGE`, base `0x400000`) for every store to
//! offset `0x70` inside the projectile module, so a writer that could
//! re-acquire a target would show up as an address this test does not name.

use std::path::PathBuf;

use cs_content::ordnance::{
    OriginalLostTargetRule, OriginalRangeEnd, OriginalRocketBlast, read_original_rocket_blasts,
};

const IMAGE_BASE: u64 = 0x40_0000;

fn rockets() -> Vec<OriginalRocketBlast> {
    let dir = std::env::var_os("CS_GAME_DIR").expect(
        "CS_GAME_DIR is not set: this audit needs the original installation (capability `retail`)",
    );
    let dir = PathBuf::from(dir);
    assert!(
        dir.is_dir(),
        "CS_GAME_DIR {} is not a directory",
        dir.display()
    );
    read_original_rocket_blasts(&dir).expect("the retail weapon records read")
}

fn rocket(name: &str, keys: &[&str], lock_on: Option<f64>) -> OriginalRocketBlast {
    OriginalRocketBlast {
        record: name.to_owned(),
        impact_proximity: None,
        detonation_distance: Some(30.0),
        detonation_time_s: Some(4.0),
        armor_damage: None,
        health_damage: None,
        lock_on,
        range: Some(900.0),
        range_minimum: None,
        range_minimum_blast_scale: None,
        keys: keys.iter().map(|key| (*key).to_owned()).collect(),
    }
}

/// The derivation, on records that need no installation: an unguided rocket
/// has no rule, a guided one loses its proximity trigger and keeps its timer
/// and the end of its range.
#[test]
fn accept_t454_a_guided_rocket_keeps_only_the_timer_and_the_range_end() {
    assert_eq!(rocket("u", &["ROCKET"], None).lost_target_rule(), None);
    let rule = rocket("g", &["ROCKET", "LOCK_ON"], Some(3.0))
        .lost_target_rule()
        .expect("a guided rocket has a rule");
    assert_eq!(rule.proximity_trigger_lost_m, Some(30.0));
    assert_eq!(rule.timer_s, Some(4.0));
    assert_eq!(rule.range_m, Some(900.0));
    assert_eq!(rule.range_end, OriginalRangeEnd::Detonates);

    let expiring = rocket("e", &["ROCKET", "LOCK_ON", "EXPIRES"], Some(3.0))
        .lost_target_rule()
        .expect("rule");
    assert_eq!(
        expiring.range_end,
        OriginalRangeEnd::DetonatesOnlyWithDetonateAtRange
    );
    let both = rocket(
        "b",
        &["ROCKET", "LOCK_ON", "EXPIRES", "DETONATE_AT_RANGE"],
        Some(3.0),
    )
    .lost_target_rule()
    .expect("rule");
    assert_eq!(both.range_end, OriginalRangeEnd::Detonates);
}

/// The data probe: the per-weapon table of the installation.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_t454_retail_guided_rockets_lose_the_proximity_trigger_and_nothing_else() {
    let rules: Vec<OriginalLostTargetRule> = rockets()
        .iter()
        .filter_map(OriginalRocketBlast::lost_target_rule)
        .collect();
    for rule in &rules {
        eprintln!("{rule:?}");
    }
    assert_eq!(
        rules.len(),
        12,
        "the installation declares 12 guided rockets"
    );
    for rule in &rules {
        assert_eq!(
            rule.range_end,
            OriginalRangeEnd::Detonates,
            "{}: no retail record declares EXPIRES",
            rule.record
        );
        // wep_15 is the one record that declares no RANGE, only its timer.
        assert_eq!(rule.range_m.is_none(), rule.record == "wep_15");
    }
    let timers: Vec<_> = rules.iter().filter(|r| r.timer_s.is_some()).collect();
    assert_eq!(timers.len(), 1, "only one guided rocket keeps a timer");
    assert_eq!(timers[0].record, "wep_15");
    assert_eq!(timers[0].timer_s, Some(2.0));
}

/// Every store of a 32-bit register or immediate to `[reg + 0x70]` in
/// `[from, to)`: the opcode `0x89 /r disp8` (mod 01, no SIB) and
/// `0xc7 /0 disp8`.
fn stores_to_0x70(image: &[u8], from: u64, to: u64) -> Vec<u64> {
    let lo = usize::try_from(from - IMAGE_BASE).expect("offset");
    let hi = usize::try_from(to - IMAGE_BASE).expect("offset");
    let mut found = Vec::new();
    for at in lo..hi - 2 {
        let modrm = image[at + 1];
        let register_store = image[at] == 0x89 && (0x40..0x80).contains(&modrm) && modrm & 7 != 4;
        let immediate_store = image[at] == 0xc7 && (0x40..0x48).contains(&modrm);
        if (register_store || immediate_store) && image[at + 2] == 0x70 {
            found.push(IMAGE_BASE + at as u64);
        }
    }
    found
}

/// The code probe: the only stores to a projectile's `+0x70` (the target
/// pointer) in the projectile module are the launch zeroing and the
/// removed-object sweep's clear. No store assigns a target there, and none
/// re-acquires.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_t454_image_has_no_target_store_in_the_projectile_module() {
    let path = std::env::var_os("CS_ENGINE_IMAGE").expect(
        "CS_ENGINE_IMAGE is not set: this audit needs the owner-supplied decrypted executable",
    );
    let image = std::fs::read(path).expect("the engine image reads");
    // The module spans the removed-object sweep (0x5ac0c0), the launch
    // (0x5aef40) and the step (0x5af720..0x5b0330).
    let stores = stores_to_0x70(&image, 0x5a_b000, 0x5b_2000);
    // 0x5ac0f1: the sweep's clear; 0x5ad95e: an unrelated record's constant;
    // 0x5af610: the launch zeroing.
    assert_eq!(stores, [0x5a_c0f1, 0x5a_d95e, 0x5a_f610]);
    let at = |address: u64, length: usize| {
        let offset = usize::try_from(address - IMAGE_BASE).expect("offset");
        &image[offset..offset + length]
    };
    // The launch zeroing: `mov [edi+0x70], ebx` with ebx = 0.
    assert_eq!(at(0x5a_f610, 3), [0x89, 0x5f, 0x70]);
    // The steering call is guarded by the target pointer: `cmp [esi+0x70], ebx; je`.
    assert_eq!(at(0x5a_f7f9, 5), [0x39, 0x5e, 0x70, 0x74, 0x0a]);
}

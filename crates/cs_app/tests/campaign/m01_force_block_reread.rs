//! FORCE-BLOCK-REREAD (#2071): the static re-read of the fixed-wing force
//! block `0x48c470..0x48ff88` against `OriginalFlightModel`
//! (finding `docs/findings/2026-10-25-force-block-reread.md`).
//!
//! * the finding's `bytes` block parses, covers the sites its conclusions rest
//!   on, and the production constants equal the cited float bytes;
//! * with the owner's image named, every row is re-read from the image;
//! * the retail `player.zrd` carries the ground-blow and pitch-fade keys the
//!   finding reports;
//! * the production law is flown from the hand-over states the finding tabulates.

use std::path::PathBuf;

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};
use cs_sim::flight::original as law;

use super::m01_handover_dynamics_step::{fly, speed};

const FINDING: &str = include_str!("../../../../docs/findings/2026-10-25-force-block-reread.md");

/// Sites the finding's conclusions rest on; each must be in the block.
const REQUIRED_SITES: &[u64] = &[
    0x48c4ba, 0x48c4e9, 0x48c50f, 0x48c5a0, 0x48c746, 0x48c77b, 0x48c7f7, 0x48c81e, 0x48c82e,
    0x48c883, 0x48c919, 0x48c9ae, 0x48cb5a, 0x48cc56, 0x48cd3e, 0x48cf7e, 0x48cf9d, 0x48d05f,
    0x48d0e1, 0x48d10d, 0x48fcb6, 0x48fd14, 0x48fd4d, 0x48fdf3, 0x48ff85, 0x41abde, 0x41ac55,
    0x41ada0, 0x48bdd0, 0x48be22, 0x53fcf5, 0x53fcff, 0x539087, 0x48c3da, 0x48bfa3, 0x48e6da,
    0x48e9d3, 0x43e5d3, 0x43e61e, 0x4753e1, 0x4b08c5, 0x6036bc, 0x6289f8, 0x6289fc, 0x6379c0,
    0x6298d4, 0x622ee8, 0x622eec,
];

struct Site {
    va: u64,
    bytes: Vec<u8>,
}

fn sites() -> Vec<Site> {
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
            let hex = rest.split('#').next().expect("hex").trim();
            assert!(hex.len() % 2 == 0 && !hex.is_empty(), "hex of {va}");
            Site {
                va: u64::from_str_radix(va, 16).expect("a hex va"),
                bytes: (0..hex.len())
                    .step_by(2)
                    .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("a hex byte"))
                    .collect(),
            }
        })
        .collect()
}

fn site(va: u64) -> Vec<u8> {
    sites()
        .into_iter()
        .find(|s| s.va == va)
        .unwrap_or_else(|| panic!("the finding's block lacks {va:#x}"))
        .bytes
}

fn f32_at(va: u64) -> f64 {
    let bytes = site(va);
    f64::from(f32::from_le_bytes(
        bytes[..4].try_into().expect("four bytes"),
    ))
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

#[test]
fn accept_force_block_reread_block_parses_and_the_production_constants_are_the_cited_bytes() {
    engine_facts().expect("the committed table parses");
    let all = sites();
    for want in REQUIRED_SITES {
        assert!(
            all.iter().any(|s| s.va == *want),
            "the finding's block lacks {want:#x}"
        );
    }
    // The approximate root is `sar eax, 1; add eax, 0x1fc00000` at all three
    // sites the law's `approx_sqrt` stands for.
    for va in [0x48c81e_u64, 0x53fcf5, 0x539087] {
        let bytes = site(va);
        assert!(contains(&bytes, &[0xd1, 0xf8]), "sar eax at {va:#x}");
        assert!(
            contains(&bytes, &[0x05, 0x00, 0x00, 0xc0, 0x1f]),
            "add eax, 0x1fc00000 at {va:#x}"
        );
    }
    assert_eq!(law::approx_sqrt(2.0), 1.5);

    // Production constants against the cited floats.
    assert_eq!(f32_at(0x6036bc), law::FAKE_AI_SPEED_BONUS_MPS);
    assert!((f32_at(0x6289f8) - law::FALL_OFF).abs() < 1e-7);
    assert!((f32_at(0x6289fc) - law::BANK_OFF).abs() < 1e-7);
    assert!((f32_at(0x603470) - law::LOAD_COMMAND_WINDOW[1]).abs() < 1e-7);
    assert!((f32_at(0x60346c) - law::LOAD_COMMAND_WINDOW[0]).abs() < 1e-7);
    assert!((f32_at(0x60347c) - law::LIFT_COEFFICIENT_WINDOW[1]).abs() < 1e-7);
    assert!((f32_at(0x603478) - law::LIFT_COEFFICIENT_WINDOW[0]).abs() < 1e-7);
    assert!((f32_at(0x6080d4) - law::FORCE_TO_ACCEL).abs() < 1e-6);
    assert!((f32_at(0x60811c) - law::LIFT_MIN_SPEED_MPS).abs() < 1e-6);
    // The fallback normal is `mov [ebp-0x1c], 0x3f333333` twice.
    let fallback = site(0x48c7f7);
    assert!(contains(&fallback, &[0x33, 0x33, 0x33, 0x3f]));
    let as_f32 = f32::from_bits(0x3f33_3333);
    assert!((f64::from(as_f32) - law::LIFT_NORMAL_FALLBACK[0]).abs() < 1e-7);
    assert_eq!(law::LIFT_NORMAL_FALLBACK[0], law::LIFT_NORMAL_FALLBACK[1]);
    // World up for the stall's `r` is (0, 1, 0).
    assert_eq!(site(0x6379c0), [0, 0, 0, 0, 0, 0, 0x80, 0x3f, 0, 0, 0, 0]);
    // The writers of `veh+0x384` the finding names: the 'fd' and 'ifon'
    // commands and the '-fd' option.
    assert_eq!(site(0x622ee8), b"fd\0");
    assert_eq!(site(0x622eec), b"ifon\0");
    assert_eq!(site(0x6298d4), b"-fd\0");
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_force_block_reread_instructions_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    let image = loaded.bytes.as_slice();
    for site in sites() {
        let offset = usize::try_from(facts.image_offset(site.va).expect("va inside the image"))
            .expect("an offset");
        assert_eq!(
            &image[offset..offset + site.bytes.len()],
            site.bytes.as_slice(),
            "instruction bytes at {:#x}",
            site.va
        );
    }
}

/// The retail `player.zrd` carries the keys the 2026-10-08 finding did not
/// list: the ground blow is on (400 / 10 / 0.5) and the pitch fade is out of
/// reach (1000 / 1001 mph).
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_force_block_reread_player_zrd_carries_the_ground_blow_and_pitch_fade_keys() {
    let root = PathBuf::from(std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR"));
    let docs = cs_content::original_airframe::OriginalDocuments::read(&root)
        .expect("the retail documents read");
    let document = docs
        .player()
        .root()
        .as_list()
        .and_then(|outer| outer.first())
        .expect("player.zrd opens with a list");
    let list = document.as_list().expect("the first entry is a list");
    let numbers = |key: &str| -> Vec<f64> {
        let index = list
            .iter()
            .position(|node| node.as_text() == Some(key))
            .unwrap_or_else(|| panic!("player.zrd has no {key}"));
        list[index + 1]
            .as_list()
            .expect("a list of numbers")
            .iter()
            .map(|node| node.as_scalar_f64().expect("a number"))
            .collect()
    };
    assert_eq!(numbers("groundblow_elev"), [400.0]);
    assert_eq!(numbers("groundblow_mag"), [10.0]);
    assert_eq!(numbers("ai_groundblow"), [0.5]);
    let fade = numbers("high_speed_pitch_fade");
    assert_eq!(fade, [1000.0, 1001.0]);
    // 1000 mph is above any speed the law reaches (finding section 4.2).
    assert!(fade[0] * law::MPH_TO_MPS > 440.0);
}

/// Section 5 of the finding: the law flown from the states the image can
/// produce. Throttle 1.0 from the placement speed lands near the capture
/// (113.5 m/s, 5.6 m/s sink); with 55 m/s to start the law never stalls and
/// holds the level equilibrium.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_force_block_reread_flights_match_the_finding_table() {
    let settled = |throttle: f64, forward: f64| {
        let samples = fly(throttle, forward, 20);
        let last = &samples[20];
        (speed(last.velocity_mps), -last.velocity_mps[1])
    };

    let (v, sink) = settled(1.0, 20.0);
    assert!((v - 113.2).abs() < 0.3, "1.0 from 20 m/s: {v}");
    assert!((sink - 3.8).abs() < 0.3, "sink {sink}");

    let (v, sink) = settled(0.4, 20.0);
    assert!((v - 83.0).abs() < 0.5, "0.4 from 20 m/s: {v}");
    assert!((sink - 9.73).abs() < 0.3, "sink {sink}");

    let (v, sink) = settled(1.0, 0.0);
    assert!((v - 117.9).abs() < 0.5, "1.0 from rest: {v}");
    assert!((sink - 26.3).abs() < 0.5, "sink {sink}");

    for (throttle, level) in [(1.0, 112.29), (0.4, 78.77)] {
        let (v, sink) = settled(throttle, 55.0);
        assert!((v - level).abs() < 0.1, "{throttle} from 55 m/s: {v}");
        assert!(sink.abs() < 0.05, "level flight sinks {sink}");
    }
}

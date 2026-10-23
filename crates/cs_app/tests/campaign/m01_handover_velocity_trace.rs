//! M01-HANDOVER-VELOCITY-TRACE (#1978): the static trace of what clears
//! `[0x71c50c]` / `veh+0x91d` and writes the player's throttle and velocity
//! (finding `docs/findings/2026-10-23-m01-handover-velocity-trace.md`).
//!
//! * the finding's `bytes` block parses and covers the claims' sites;
//! * with the owner's image named, every row is re-read from the image.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};

const FINDING: &str =
    include_str!("../../../../docs/findings/2026-10-23-m01-handover-velocity-trace.md");

/// Sites the finding's conclusions rest on; each must be in the block.
const REQUIRED_SITES: &[u64] = &[
    0x4897f3, 0x47e2e6, 0x47e0bc, 0x4a02c8, 0x4898f9, 0x47f28b, 0x47f313, 0x47f31e, 0x48049e,
    0x4804d9, 0x470a91, 0x535c8b, 0x487c8a,
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

#[test]
fn accept_m01_handover_velocity_trace_block_parses_and_covers_the_claims() {
    engine_facts().expect("the committed table parses");
    let sites = sites();
    assert!(sites.len() >= REQUIRED_SITES.len());
    for want in REQUIRED_SITES {
        assert!(
            sites.iter().any(|site| site.va == *want),
            "the finding's block lacks {want:#x}"
        );
    }
    // The finding's central claim: throttle 0.4 is the f32 0x3ecccccd.
    let throttle = sites.iter().find(|s| s.va == 0x47f28b).expect("0x47f28b");
    assert_eq!(&throttle.bytes[6..], &0.4f32.to_le_bytes());
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_handover_velocity_trace_instructions_match_the_image() {
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

//! M01-WINGMAN-HILLSIDE-DIVERGENCE (#2262): the instruction windows behind
//! `docs/findings/2026-11-02-m01-wingman-hillside-divergence.md`.
//!
//! Task test prefix: `accept_m01_wingman_hillside_`. The finding's answer is
//! static: the original's AI step runs the follower with no ground-blow or
//! collision-grace gate, the stick axes the follower writes are the roll,
//! pitch and yaw inputs of the force block that also carries the ground blow,
//! and the blow's AI branch is `ai_groundblow * groundblow_mag` with no dot
//! test. Each window is re-read from the owner's image so the claim fails
//! loudly if the image or the citation drifts.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::{load_engine_image, original_image_digest};

/// (virtual address, bytes as hex, what the window shows).
const WINDOWS: &[(u64, &str, &str)] = &[
    // The no-op stub the AI step calls for modes 0, 4 and 1 (`0x41c2d5`).
    (0x41afd0, "c3", "0x41afd0 is a bare ret"),
    // The AI step head: skip only for `V+0xcc`, then the route refresh.
    (
        0x41c270,
        "568bf18a86cc00000084c00f85e0000000e88a3b00008b864809000085c0752783bef002000001751e8b86f402",
        "AI step 0x41c270: gated by V+0xcc only",
    ),
    (
        0x41c2b7,
        "8b867c06000085c0740a83f804740583f801751283be58030000037d0956e8f6ecffff83c40483be7c0600",
        "AI step: mode 0/4/1 with V+0x358 < 3 calls the 0x41afd0 stub",
    ),
    (
        0x41c2dd,
        "83be7c0600000474728b86f002000083e800741348740548740d5ec3",
        "AI step: mode 4 jumps to the follower, others dispatch on V+0x2f0",
    ),
    (
        0x41c358,
        "56e80224000083c4045ec3",
        "AI step: mode 4 calls the follower 0x41e760 and returns",
    ),
    // The follower's altitude clamp on the goal (`+0x314` ceiling, `0x71c3f0` floor).
    (
        0x41b57e,
        "d945d08b11d89e140300008945d48b41048b49088945dcdfe08955d8f6c441894de07520d945dc8b9614030000d81dc83260008955d0dfe0f6c4417507c745dc00000000d945d0d81df0c37100dfe0f6c401741fd945dca1f0c37100d81dc83260008945d0dfe0f6c4017407c745dc00000000",
        "0x41b560: goal y limited to V+0x314 and raised to [0x71c3f0]",
    ),
    // The stick gains, and the `now < V+0xb4` attenuation.
    (
        0x41bfc8,
        "a04cee6400d9867409000084c07406d805f0346000d90570c47100d89eb4000000dfe0f6c4017406d80da8346000d9c0d88e00010000d9",
        "0x41b560: stick gain x0x6034a8 while now < V+0xb4",
    ),
    // The three stick words copied to the inputs the flight law reads.
    (
        0x41c01e,
        "8b8e00010000680000803f680000003f51e85c480400d99e140100008b9608010000680000803f680000003f52e840480400d99e1c0100008b860c010000680000803f680000003f50e824480400d99e2001000083c424",
        "0x41b560: +0x100 -> +0x114, +0x108 -> +0x11c, +0x10c -> +0x120",
    ),
    // The force block's reads of those inputs: roll, pitch, yaw.
    (
        0x48caea,
        "d9861c010000d81dc8326000dfe0f6c4400f859f000000d945ec8d55a8",
        "0x48c470: roll reads veh+0x114 (and, below, level-off reads +0x114 and +0x11c)",
    ),
    (
        0x48cb07,
        "d88e480600008d86800100008955cc8945dc",
        "0x48c470: pitch term reads veh+0x11c",
    ),
    (
        0x48cbb7,
        "d9864c0600008d4da8d88e200100008d868c010000894dcc8945dcd84df0",
        "0x48c470: yaw term reads veh+0x120",
    ),
    // The blow's gates and the two seats' factors.
    (
        0x48c22b,
        "83be5803000004570f842b0200003b3598c271007417d90570c47100d89eac000000dfe0f6c4010f850c020000",
        "0x48c220: skip for V+0x358 == 4; a non-player skipped while now < V+0xac",
    ),
    (
        0x48c317,
        "d90528c47100d80d24c47100d90570c47100d89eb4000000dfe0f6c4017412d80da8366000d94508d80da8366000d95d08",
        "0x48c220: AI factor = ai_groundblow * groundblow_mag, no dot test",
    ),
];

fn from_hex(text: &str) -> Vec<u8> {
    (0..text.len())
        .step_by(2)
        .map(|at| u8::from_str_radix(&text[at..at + 2], 16).expect("hex"))
        .collect()
}

#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_wingman_hillside_windows_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let facts = engine_facts().expect("the committed table parses");
    for (va, hex, what) in WINDOWS {
        let want = from_hex(hex);
        let at = usize::try_from(facts.image_offset(*va).expect("va inside the image"))
            .expect("an offset");
        assert_eq!(
            &loaded.bytes[at..at + want.len()],
            want.as_slice(),
            "{what} at {va:#x}"
        );
    }
}

/// No window cited is empty or odd-length: a typo in a citation would
/// otherwise compare fewer bytes than the finding claims.
#[test]
fn accept_m01_wingman_hillside_windows_are_well_formed() {
    let mut seen = std::collections::BTreeSet::new();
    for (va, hex, what) in WINDOWS {
        assert!(!hex.is_empty() && hex.len() % 2 == 0, "{what}");
        assert!(seen.insert(*va), "{what}: duplicate window {va:#x}");
    }
}

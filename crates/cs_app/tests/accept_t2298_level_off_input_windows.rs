//! Task #2298: the image windows behind the player's **clear-on-stick-input**
//! rule for the Level-Off assist (`vehicle+0x12c`).
//!
//! This re-reads the owner image directly, so it is `#[ignore]`d
//! (`requires CS_ENGINE_IMAGE`) and fails loudly when the variable is unset.
//! Static evidence only: claim `OWNER-STATIC-2026-10-29`, nothing
//! `verified_original`. The reading and every window below are written up in
//! `docs/findings/2026-11-03-2298-level-off-clears-on-stick-input.md`.
//!
//! The windows:
//!
//! * `0x487d3d`..`0x487d77` — the player input routine loads the three raw
//!   stick words `+0x100`/`+0x108`/`+0x10c`, compares each against the image's
//!   `0.0` (`0x6032c8`) and, on the first non-zero one (`test ah, 0x40` /
//!   `je 0x487d96`), jumps to the clear.
//! * `0x487d96` — `c6 86 2c 01 00 00 00`, `mov byte [esi+0x12c], 0`.
//! * `0x460890` and the `fstp` stores after it — the identity response curve
//!   that copies those raw words into the force block's `+0x114`/`+0x11c`/
//!   `+0x120`, which is what the law's `OriginalInput` carries.
//! * `0x488640`..`0x48866e` — the player-vehicle toggle
//!   (`mov ecx,[0x71c298]`, `sete al`, `mov [ecx+0x12c], al`), the only writer
//!   that **sets** the byte.
//! * `0x4b0645` — the constructor/reset clearing it.
//! * `0x48ce53`/`0x48ce6a` — the assist's own gate, which stays
//!   `+0x114 == 0 && +0x11c == 0`.
//!
//! The acceptance test that drives the behaviour is
//! `crates/cs_sim/tests/accept_t2298_level_off_clears_on_stick_input.rs`.

use cs_app::engine_facts::engine_facts;
use cs_content::coordinates::load_engine_image;

/// Every window the clear-on-input rule rests on, byte for byte.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_t2298_level_off_clear_and_toggle_windows_match_the_image() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    let facts = engine_facts().expect("the committed table parses");
    let at = |va: u64| usize::try_from(facts.image_offset(va).expect("va in image")).expect("off");
    let window = |va: u64, bytes: &[u8]| {
        let i = at(va);
        assert_eq!(
            &loaded.bytes[i..i + bytes.len()],
            bytes,
            "window at {va:#x}"
        );
    };

    // 1. The comparison's zero, and the three raw stick words with their
    //    `fcomp 0.0` / `test ah, 0x40` / `je 0x487d96` pairs.
    window(0x6032c8, &[0x00, 0x00, 0x00, 0x00]);
    //    Roll `+0x100`: load, compare, test, jump clear when non-zero.
    window(0x487d3d, &[0xd9, 0x86, 0x00, 0x01, 0x00, 0x00]);
    window(0x487d46, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]);
    window(0x487d4e, &[0xf6, 0xc4, 0x40]);
    window(0x487d51, &[0x74, 0x43]);
    //    Pitch `+0x108`.
    window(0x487d53, &[0xd9, 0x86, 0x08, 0x01, 0x00, 0x00]);
    window(0x487d59, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]);
    window(0x487d61, &[0xf6, 0xc4, 0x40]);
    window(0x487d64, &[0x74, 0x30]);
    //    Yaw `+0x10c`.
    window(0x487d66, &[0xd9, 0x86, 0x0c, 0x01, 0x00, 0x00]);
    window(0x487d6c, &[0xd8, 0x1d, 0xc8, 0x32, 0x60, 0x00]);
    window(0x487d74, &[0xf6, 0xc4, 0x40]);
    window(0x487d77, &[0x74, 0x1d]);
    //    The clear itself: `mov byte [esi+0x12c], 0`.
    window(0x487d96, &[0xc6, 0x86, 0x2c, 0x01, 0x00, 0x00, 0x00]);

    // 2. Those raw words reach the force block through the identity curve, so
    //    a non-zero `OriginalInput` word means a non-zero raw word.
    window(0x460890, &[0xd9, 0x44, 0x24, 0x04, 0xc3]);
    window(0x487db0, &[0xd9, 0x9e, 0x14, 0x01, 0x00, 0x00]);
    window(0x487dcc, &[0xd9, 0x9e, 0x1c, 0x01, 0x00, 0x00]);
    window(0x487de8, &[0xd9, 0x96, 0x20, 0x01, 0x00, 0x00]);

    // 3. The only writer that sets the byte: the player-vehicle toggle. Its
    //    handle is the player global `0x71c298`, and its value is `!old`
    //    (`sete al`).
    window(0x488640, &[0x8b, 0x0d, 0x98, 0xc2, 0x71, 0x00]);
    window(0x488663, &[0x8a, 0x81, 0x2c, 0x01, 0x00, 0x00]);
    window(0x48866b, &[0x0f, 0x94, 0xc0]);
    window(0x48866e, &[0x88, 0x81, 0x2c, 0x01, 0x00, 0x00]);
    //    And the constructor/reset clear, beside the whole control block it
    //    clears.
    window(0x4b0645, &[0x88, 0x9e, 0x2c, 0x01, 0x00, 0x00]);

    // 4. The assist's own gate is unchanged: `+0x114 == 0 && +0x11c == 0`.
    window(0x48ce45, &[0x8a, 0x86, 0x2c, 0x01, 0x00, 0x00]);
    window(0x48ce53, &[0xd9, 0x86, 0x14, 0x01, 0x00, 0x00]);
    window(0x48ce64, &[0x0f, 0x84, 0x14, 0x01, 0x00, 0x00]);
    window(0x48ce6a, &[0xd9, 0x86, 0x1c, 0x01, 0x00, 0x00]);
    window(0x48ce7b, &[0x0f, 0x84, 0xfd, 0x00, 0x00, 0x00]);
}

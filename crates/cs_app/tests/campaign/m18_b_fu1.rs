//! M18-B-FU1 (#1254): what the original does with the sound group M18's
//! `STOP_QUEUED_SOUNDS` sites name and no shipped record declares.
//!
//! Blocks 12 and 50 of `ZBD/C4/M03/zrdr.zbd :: objectives.zrd` stop
//! `snd_c4-RM-m3_BlackSwan_27`. The answer is read statically from the owner's
//! decrypted engine image (`$CS_ENGINE_IMAGE`) and recorded in
//! `docs/findings/2026-10-19-m18-b-fu1-stop-queued-sounds-operand.md`; no
//! original run was made, so nothing here is `verified_original`.

use cs_assets::install::sha256;
use cs_content::coordinates::{load_engine_image, original_image_digest};

/// The decrypted image is flat from `0x400000`.
const IMAGE_VA_BASE: usize = 0x40_0000;
const SOUND: &str = "snd_c4-RM-m3_BlackSwan_27";

/// SHA-256 of the 84-byte queued-sound sweep `STOP_QUEUED_SOUNDS` calls at `0x5920f0`.
const SWEEP_SHA256: &str = "5a3a81162eb17f4f380d666fc600ab8153e3ad7c06d93fcd40b0a6a1bfc6cbef";

fn at(image: &[u8], va: usize, len: usize) -> &[u8] {
    &image[va - IMAGE_VA_BASE..va - IMAGE_VA_BASE + len]
}

fn count(haystack: &[u8], needle: &[u8]) -> usize {
    haystack
        .windows(needle.len())
        .filter(|window| *window == needle)
        .count()
}

/// The operand is compared, byte for byte, with the name of each queued sound;
/// it is not resolved against a declaration table. A name no queued sound
/// carries ends the sweep silently, and the name is in no file but M18's own
/// reader archive and not in the image.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m18_b_fu1_an_undeclared_stop_operand_is_compared_with_queued_names_only() {
    let loaded = load_engine_image().expect("CS_ENGINE_IMAGE names the measured image");
    assert_eq!(loaded.digest, original_image_digest());
    let image = loaded.bytes;

    let sweep = at(&image, 0x59_20f0, 84);
    assert_eq!(sha256(sweep).to_hex(), SWEEP_SHA256);
    // Operand (`[esp+0xc]` -> edi) is handed to the search `0x5791a0` with the
    // comparison callback `0x592090`; a null answer returns.
    assert_eq!(
        sweep[0x0b..0x0f],
        [0x8b, 0x7c, 0x24, 0x0c],
        "mov edi,[esp+0xc]"
    );
    assert_eq!(
        sweep[0x10..0x15],
        [0x68, 0x90, 0x20, 0x59, 0x00],
        "push 0x592090"
    );
    assert_eq!(sweep[0x22..0x24], [0x74, 0x2d], "no match returns");

    // The callback walks entry->[+4]->[+8] and the operand in step, comparing
    // byte pairs until they differ or the NUL: an exact, case-sensitive match.
    let matcher = at(&image, 0x59_2090, 96);
    assert_eq!(sha256(matcher).to_hex(), MATCHER_SHA256);
    assert_eq!(matcher[0x1d..0x23], [0x8a, 0x10, 0x8a, 0x1e, 0x8a, 0xca]);
    assert_eq!(matcher[0x23..0x25], [0x3a, 0xd3], "cmp dl, bl");

    assert_eq!(count(&image, SOUND.as_bytes()), 0, "not in the image");

    let game = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR is set");
    let archive = std::fs::read(std::path::Path::new(&game).join("ZBD/C4/M03/zrdr.zbd"))
        .expect("M18's reader archive");
    // Two STOP_QUEUED_SOUNDS sites: blocks 12 and 50.
    assert_eq!(count(&archive, SOUND.as_bytes()), 2);
}

/// SHA-256 of the 96-byte name-comparison callback at `0x592090`.
const MATCHER_SHA256: &str = "6b684f5cb19e83b168ada9a2b1c26c2a33b6857cf847d27ccbb200b79d0f12bb";

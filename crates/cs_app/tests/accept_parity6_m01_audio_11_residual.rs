//! PARITY6-M01-AUDIO-RESIDUAL-B (#2002): the 33,864 B loop has no archive member
//! under any length reading tried so far.
//!
//! Task test prefix: `accept_parity6_m01_audio_11_`. Finding:
//! `docs/findings/2026-10-24-m01-audio-residual-b-33864.md`. The retail scenario
//! reads the installation named by `CS_GAME_DIR` and fails loudly without it.

use crate::accept_parity6_m01_audio_01_sample_identity::{archive_rows, game_dir};

/// The Drydock buffer length of the owner's 11,025 Hz loop (owner log, t = 15.0 s).
const LOOP_BYTES: u64 = 33_864;

/// Readings of a member's decoded length that could plausibly be logged:
/// as is, halved (8-bit read as 16-bit), doubled, and with one 16-bit sample
/// per byte of a partial last block dropped.
fn readings(decoded: u64) -> [u64; 4] {
    [decoded, decoded / 2, decoded * 2, decoded.saturating_sub(2)]
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_11_33864_matches_no_member_under_any_length_reading() {
    let root = game_dir();
    let mut checked = 0usize;
    for container in ["ZBD/soundsh.zbd", "ZBD/soundsl.zbd"] {
        let rows = archive_rows(&root, container);
        assert!(rows.len() > 2000, "{container}: {} members", rows.len());
        for (name, _tag, _channels, _rate, decoded) in rows {
            checked += 1;
            assert!(
                !readings(decoded).contains(&LOOP_BYTES),
                "{container}/{name} reads as {LOOP_BYTES} B ({decoded} B decoded)"
            );
        }
    }
    assert!(checked > 4000, "only {checked} members were checked");
}

#[test]
fn accept_parity6_m01_audio_11_readings_include_the_decoded_length_itself() {
    assert!(readings(LOOP_BYTES).contains(&LOOP_BYTES));
    assert!(readings(LOOP_BYTES * 2).contains(&LOOP_BYTES));
    assert!(readings(LOOP_BYTES / 2).contains(&LOOP_BYTES));
}

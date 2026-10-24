//! Task #1964: the intro gate, engine callback 2151. Task test prefix:
//! `accept_parity5_intro_2151_`.
//!
//! The rule is the one read statically from the engine image (see
//! `docs/findings/2026-10-24-intro-callback-2151-static.md`); the tests drive
//! the production functions. What they do not prove: that the original's
//! flags are written the way a profile here would write them, or that the
//! original ran this way (`static`, never `verified_original`).

use cs_app::ui::front_end::{
    GATE_CALLBACK, MISSIONS_PER_CHAPTER, MissionFlags, chapter_gate, gate_record,
};

#[test]
fn accept_parity5_intro_2151_a_fresh_pilot_gets_its_chapter_and_the_intro_plays() {
    assert_eq!(GATE_CALLBACK, 2151);
    assert_eq!(chapter_gate(1, MissionFlags::default()), 1);
}

#[test]
fn accept_parity5_intro_2151_either_flag_on_the_first_mission_suppresses_the_intro() {
    for flags in [
        MissionFlags {
            word_04: 1,
            word_58: 0,
        },
        MissionFlags {
            word_04: 0,
            word_58: 1,
        },
        MissionFlags {
            word_04: 1,
            word_58: 1,
        },
    ] {
        assert_eq!(chapter_gate(3, flags), 0);
    }
}

#[test]
fn accept_parity5_intro_2151_chapter_zero_is_zero_and_reads_no_record() {
    assert_eq!(chapter_gate(0, MissionFlags::default()), 0);
    assert_eq!(gate_record(0), None);
}

#[test]
fn accept_parity5_intro_2151_the_record_read_is_the_chapters_first_mission() {
    assert_eq!(MISSIONS_PER_CHAPTER, 5);
    let records: Vec<_> = (1..=5).map(gate_record).collect();
    assert_eq!(records, [Some(0), Some(5), Some(10), Some(15), Some(20)]);
}

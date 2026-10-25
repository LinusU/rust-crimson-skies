//! Task #2077: the intro gate's progress, per profile. Task test prefix:
//! `accept_parity5_intro_gate_2077_`.
//!
//! The rule, the mission-to-chapter table and the meaning of the record words
//! are read statically from the engine image
//! (`docs/findings/2026-10-25-intro-gate-flag-writer-static.md`); the tests
//! drive the production functions over real profile documents. The two
//! ignored tests re-read the cited bytes of the owner's installation and
//! image so the table cannot drift. Nothing is `verified_original`: no
//! original run was observed.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

use cs_app::profile::ProfileSession;
use cs_app::ui::front_end::pilot_progress::{
    Attempt, current_mission, gate_flags, intro_chapter, mission_label, open_session,
    record_attempt,
};
use cs_app::ui::front_end::{
    AREA_CHAPTERS, CAMPAIGN_MISSIONS, MissionFlags, chapter_of_area, chapter_of_mission,
};

static NEXT: AtomicUsize = AtomicUsize::new(0);

struct Root(PathBuf);

impl Root {
    fn new() -> Self {
        let dir = std::env::temp_dir().join(format!(
            "cs-intro-gate-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("the temp directory is created");
        Self(dir)
    }

    fn session(&self) -> ProfileSession {
        open_session(Some(&self.0)).expect("the population opens")
    }
}

impl Drop for Root {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn new_pilot(root: &Root, name: &str) {
    let mut session = root.session();
    session.create(name).expect("the pilot is created");
}

#[test]
fn accept_parity5_intro_gate_2077_the_table_maps_missions_to_chapters() {
    let chapters: Vec<u32> = (0..CAMPAIGN_MISSIONS).map(chapter_of_mission).collect();
    assert_eq!(chapters.len(), 24);
    assert_eq!(&chapters[0..5], &[1; 5]);
    assert_eq!(&chapters[5..10], &[2; 5]);
    assert_eq!(&chapters[10..15], &[3; 5]);
    assert_eq!(&chapters[15..20], &[4; 5]);
    assert_eq!(&chapters[20..24], &[5; 4]);
    assert_eq!(chapter_of_mission(24), 0);
    assert_eq!(chapter_of_area("HAWAII"), 1);
    assert_eq!(chapter_of_area("MANHATTAN"), 5);
    assert_eq!(chapter_of_area("ATLANTIS"), 0);
}

#[test]
fn accept_parity5_intro_gate_2077_a_fresh_pilot_sees_the_first_chapter_intro() {
    let root = Root::new();
    new_pilot(&root, "Eve");
    assert_eq!(intro_chapter(&root.session()), Some(1));
}

#[test]
fn accept_parity5_intro_gate_2077_an_attempt_at_the_first_mission_ends_the_intro() {
    for attempt in [Attempt::Failed, Attempt::Abandoned, Attempt::Completed] {
        let root = Root::new();
        new_pilot(&root, "Eve");
        record_attempt(&mut root.session(), "M01", attempt).expect("recorded");
        // Reopened, as the next process would: the progress is persisted.
        // Even a pass leaves the current mission in chapter 1 (M02), whose
        // first mission already has its record set.
        assert_eq!(intro_chapter(&root.session()), Some(0), "{attempt:?}");
    }
}

#[test]
fn accept_parity5_intro_gate_2077_each_new_chapter_plays_its_own_intro_once() {
    let root = Root::new();
    new_pilot(&root, "Eve");
    {
        let mut session = root.session();
        for index in 0..5 {
            record_attempt(&mut session, &mission_label(index), Attempt::Completed)
                .expect("recorded");
        }
    }
    let records = root.session().document().expect("a pilot").records.clone();
    assert_eq!(current_mission(&records), 5);
    // Chapter 2's first mission (M06) has no attempt yet: its intro plays.
    assert_eq!(intro_chapter(&root.session()), Some(2));
    record_attempt(&mut root.session(), "M06", Attempt::Failed).expect("recorded");
    assert_eq!(intro_chapter(&root.session()), Some(0));
}

#[test]
fn accept_parity5_intro_gate_2077_progress_belongs_to_the_pilot() {
    let root = Root::new();
    let (first, second) = {
        let mut session = root.session();
        let first = session.create("Eve").expect("created");
        record_attempt(&mut session, "M01", Attempt::Failed).expect("recorded");
        let second = session.create("Bob").expect("created");
        (first, second)
    };
    let mut session = root.session();
    session.select(second).expect("selected");
    assert_eq!(intro_chapter(&session), Some(1), "a new pilot is fresh");
    session.select(first).expect("selected");
    assert_eq!(intro_chapter(&session), Some(0), "the first pilot played");
}

#[test]
fn accept_parity5_intro_gate_2077_no_pilot_no_gate_and_second_word_stays_zero() {
    let root = Root::new();
    assert_eq!(intro_chapter(&root.session()), None);
    new_pilot(&root, "Eve");
    record_attempt(&mut root.session(), "M01", Attempt::Failed).expect("recorded");
    let records = root.session().document().expect("a pilot").records.clone();
    assert_eq!(
        gate_flags(&records, 1),
        MissionFlags {
            word_04: 1,
            word_58: 0
        }
    );
    assert_eq!(gate_flags(&records, 2), MissionFlags::default());
    assert_eq!(gate_flags(&records, 0), MissionFlags::default());
}

fn rva_to_offset(image: &[u8], va: u32) -> usize {
    let u16_at = |at: usize| usize::from(u16::from_le_bytes([image[at], image[at + 1]]));
    let u32_at = |at: usize| {
        u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]]) as usize
    };
    let pe = u32_at(0x3c);
    let sections = u16_at(pe + 6);
    let optional = u16_at(pe + 20);
    let rva = va as usize - 0x40_0000;
    (0..sections)
        .find_map(|index| {
            let header = pe + 24 + optional + 40 * index;
            let (size, start, raw_size, raw) = (
                u32_at(header + 8),
                u32_at(header + 12),
                u32_at(header + 16),
                u32_at(header + 20),
            );
            (start..start + size.max(raw_size))
                .contains(&rva)
                .then(|| raw + rva - start)
        })
        .expect("the address is inside a section")
}

fn c_str(image: &[u8], va: u32) -> String {
    let start = rva_to_offset(image, va);
    let end = image[start..].iter().position(|b| *b == 0).expect("NUL");
    String::from_utf8_lossy(&image[start..start + end]).into_owned()
}

/// The engine image's own table: the area strings the mission loader compares
/// (`0x6271c8..`), the codes it stores (`0x46c04..0x46c0b4`) and the jump
/// table of `0x419ce0` (`0x419d38`) that turns a code into a chapter.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_parity5_intro_gate_2077_image_table_matches_the_engine() {
    let Some(path) = std::env::var_os("CS_ENGINE_IMAGE") else {
        panic!("CS_ENGINE_IMAGE is unset: this run needs the engine image");
    };
    let image = std::fs::read(Path::new(&path)).expect("the image reads");
    // (string address, stored code) in the loader's comparison order.
    let loader = [
        (0x62_71c8_u32, "HAWAII", 3_u32),
        (0x62_71d0, "NORTHWEST", 1),
        (0x62_71dc, "HOLLYWOOD", 2),
        (0x62_71e8, "COLORADO", 4),
        (0x62_71f4, "MANHATTAN", 5),
    ];
    let table = rva_to_offset(&image, 0x41_9d38);
    for (address, area, code) in loader {
        assert_eq!(c_str(&image, address), area);
        let at = table + 4 * (code as usize - 1);
        let target = u32::from_le_bytes([image[at], image[at + 1], image[at + 2], image[at + 3]]);
        // The handler is `mov esi, <chapter>` (BE imm32).
        let code_at = rva_to_offset(&image, target);
        assert_eq!(image[code_at], 0xbe, "{area}: mov esi, imm32");
        let chapter = u32::from_le_bytes([
            image[code_at + 1],
            image[code_at + 2],
            image[code_at + 3],
            image[code_at + 4],
        ]);
        assert_eq!(chapter_of_area(area), chapter, "{area}");
        assert!(AREA_CHAPTERS.contains(&(area, chapter)));
    }
}

/// The installation's `cm_sequence.zrd`: 24 missions, areas in chapter order.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity5_intro_gate_2077_retail_sequence_lists_the_areas_in_chapter_order() {
    let Some(dir) = std::env::var_os("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is unset: this run needs the original installation");
    };
    let archive = std::fs::read(Path::new(&dir).join("ZBD").join("zrdr.zbd")).expect("reads");
    let find = |needle: &[u8], from: usize| {
        archive[from..]
            .windows(needle.len())
            .position(|window| window == needle)
            .map(|at| at + from)
    };
    let start = find(b"Hawaii mission 1", 0).expect("the sequence member");
    let mut areas = Vec::new();
    let mut at = start;
    while areas.len() < CAMPAIGN_MISSIONS {
        let next = AREA_CHAPTERS
            .iter()
            .filter_map(|(name, _)| find(name.as_bytes(), at).map(|found| (found, *name)))
            .min()
            .expect("an area string");
        areas.push(next.1);
        at = next.0 + next.1.len();
    }
    for (index, area) in areas.iter().enumerate() {
        assert_eq!(chapter_of_area(area), chapter_of_mission(index), "{index}");
    }
    // The 25th entry the table would hold is absent: the next area string is
    // beyond the member (5192 bytes).
    assert!(find(b"MANHATTAN", at).is_none_or(|found| found > start + 5192));
}

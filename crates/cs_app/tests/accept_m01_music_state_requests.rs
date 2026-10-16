//! Acceptance: M01's music state requests and the seven `music_*_sg` groups
//! they name (Rally #1463, `M01-PLAY-MUSIC`).
//!
//! A `WAKEUP_SOUND_GROUP` naming one of the executable's seven built-in
//! `music_*_sg` groups is a **music state request**, not an ordinary cue
//! (`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`),
//! and [`MissionRadio::plan`] answers such a cue with
//! [`cs_app::mission_session::RadioLineOutcome::Music`] instead of a radio
//! line. This suite pins, through production code, both halves of what a
//! music state request resolves to:
//!
//! * the **retail half** — what `sounds.zrd`'s own group table expands each
//!   state to (the tracks it names), read by [`MissionRadio::read`] from the
//!   owner's installation, plus the five of the seven states M01's own
//!   program can cue;
//! * the **image half** — the seven built-in track tables the executable's
//!   music dispatcher (`0x5954f0`) indexes, read from `$CS_ENGINE_IMAGE`
//!   through `cs_formats`' own PE layout reader.
//!
//! The two halves are measured separately and cross-checked: for six of the
//! seven states the executable's table and `sounds.zrd` name the same tracks
//! in the same order, and `music_prebattle_sg` is the measured exception
//! (three distinct names repeated to fill six slots in the image, six
//! distinct tracks in the retail data) — pinned here so neither reading can
//! drift into the other.
//!
//! What is **not** claimed: nothing here mints a music content id, asks
//! [`cs_sim::audio_events::MusicDirector`] for a cue, or plays anything.
//! `MusicDirector::request` refuses a non-`Music`
//! [`cs_types::content::ContentId`], F14-D.7 records that no container family
//! states a cue class, and **no owner ruling for a music identity exists**,
//! so this suite derives no id and the routing half of #1463's acceptance
//! waits for that ruling
//! (`docs/findings/2026-10-16-m01-play-music-state-requests.md`). Nothing
//! here is `verified_original`: no original executable ran and no person
//! heard anything.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionRadio, declared_engine_objectives, declared_sound_groups};
use cs_app::objectives::recover_retail_objectives;
use cs_content::campaign_bindings::MissionLabel;
use cs_formats::ParseContext;
use cs_formats::pe_resources::{PeLayout, RvaSpan, read_pe_layout};

/// The seven music state groups the executable names, with the virtual
/// address of that name, the virtual address of the state's built-in track
/// table, and the table's entries **as the image spells them**. Every address
/// and every spelling was measured in `$CS_ENGINE_IMAGE` and is re-read by
/// [`accept_m01_music_state_requests_match_the_executable_track_tables`].
///
/// Address convention: virtual address, image base `0x400000`.
const STATES: [(&str, u32, u32, &[&str]); 7] = [
    (
        "music_battlesuccess_sg",
        0x63a4a8,
        0x63a088,
        &[
            "music_battlesuccess1.wav",
            "music_battlesuccess2.wav",
            "music_battlesuccess3.wav",
            "music_battlesuccess4.wav",
            "music_battlesuccess5.wav",
            "music_battlesuccess6.wav",
        ],
    ),
    (
        "music_missionsuccess_sg",
        0x63a4c0,
        0x63a058,
        &[
            "music_missionsuccess1.wav",
            "music_missionsuccess2.wav",
            "music_missionsuccess3.wav",
            "music_missionsuccess4.wav",
            "music_missionsuccess5.wav",
            "music_missionsuccess6.wav",
        ],
    ),
    (
        "music_tertiaryobj_sg",
        0x63a4d8,
        0x63a038,
        &["music_tertiaryobj1.wav", "music_tertiaryobj2.wav"],
    ),
    (
        "music_primaryobj_sg",
        0x63a4f0,
        0x63a028,
        &["music_primaryobj1.wav", "music_primaryobj2.wav"],
    ),
    (
        "music_secondaryobj_sg",
        0x63a504,
        0x63a030,
        &["music_secondaryobj1.wav", "music_secondaryobj2.wav"],
    ),
    // Measured exception: the image's prebattle table repeats its three
    // distinct names to fill the six slots.
    (
        "music_prebattle_sg",
        0x63a51c,
        0x63a070,
        &[
            "music_prebattle1.wav",
            "music_prebattle2.wav",
            "music_prebattle3.wav",
            "music_prebattle1.wav",
            "music_prebattle2.wav",
            "music_prebattle3.wav",
        ],
    ),
    (
        "music_battle_sg",
        0x63a530,
        0x63a040,
        &[
            "music_battle1.wav",
            "music_battle2.wav",
            "music_battle3.wav",
            "music_battle4.wav",
            "music_battle5.wav",
            "music_battle6.wav",
        ],
    ),
];

/// The tracks `sounds.zrd` expands each state to, measured through
/// [`MissionRadio::read`]. The order is the spelled order the expansion
/// preserves, and it is the candidate set a music request draws from — the
/// *rule* that picks one is the executable's and is recorded in
/// `docs/findings/2026-10-16-m01-play-music-state-requests.md`, not here.
const RETAIL_TRACKS: [(&str, &[&str]); 7] = [
    (
        "music_battlesuccess_sg",
        &[
            "music_battlesuccess1.wav",
            "music_battlesuccess2.wav",
            "music_battlesuccess3.wav",
            "music_battlesuccess4.wav",
            "music_battlesuccess5.wav",
            "music_battlesuccess6.wav",
        ],
    ),
    (
        "music_missionsuccess_sg",
        &[
            "music_missionsuccess1.wav",
            "music_missionsuccess2.wav",
            "music_missionsuccess3.wav",
            "music_missionsuccess4.wav",
            "music_missionsuccess5.wav",
            "music_missionsuccess6.wav",
        ],
    ),
    (
        "music_tertiaryobj_sg",
        &["music_tertiaryobj1.wav", "music_tertiaryobj2.wav"],
    ),
    (
        "music_primaryobj_sg",
        &["music_primaryobj1.wav", "music_primaryobj2.wav"],
    ),
    (
        "music_secondaryobj_sg",
        &["music_secondaryobj1.wav", "music_secondaryobj2.wav"],
    ),
    (
        "music_prebattle_sg",
        &[
            "music_prebattle1.wav",
            "music_prebattle2.wav",
            "music_prebattle3.wav",
            "music_prebattle4.wav",
            "music_prebattle5.wav",
            "music_prebattle6.wav",
        ],
    ),
    (
        "music_battle_sg",
        &[
            "music_battle1.wav",
            "music_battle2.wav",
            "music_battle3.wav",
            "music_battle4.wav",
            "music_battle5.wav",
            "music_battle6.wav",
        ],
    ),
];

/// The five states M01's own program cues, measured: M01 spells five of the
/// seven over six `WAKEUP_SOUND_GROUP` sites
/// (`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`).
const M01_MUSIC_GROUPS: [&str; 5] = [
    "music_battlesuccess_sg",
    "music_missionsuccess_sg",
    "music_prebattle_sg",
    "music_primaryobj_sg",
    "music_secondaryobj_sg",
];

/// The installation root this suite reads. Fails loudly when the capability
/// is absent, the way every retail member of the suite does.
fn game_dir() -> PathBuf {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is not set: this test reads the installation's sounds.zrd");
    };
    PathBuf::from(game_dir)
}

/// The owner's decrypted engine image, read-only.
fn engine_image() -> Vec<u8> {
    let Ok(image) = std::env::var("CS_ENGINE_IMAGE") else {
        panic!("CS_ENGINE_IMAGE is not set: this test reads the music dispatcher's tables");
    };
    std::fs::read(&image)
        .unwrap_or_else(|error| panic!("the engine image {image} does not read: {error}"))
}

/// The file offset of one virtual address, through `cs_formats`' own PE
/// layout reader. Image base `0x400000`, measured from this image's optional
/// header.
fn offset_of(layout: &PeLayout, va: u32) -> usize {
    let rva = va
        .checked_sub(0x0040_0000)
        .expect("the VA is inside the image");
    let RvaSpan {
        file_offset,
        available,
    } = layout
        .map_rva(rva)
        .unwrap_or_else(|| panic!("rva {va:#x} maps to no file span"));
    assert!(available > 0, "rva {va:#x} has no bytes on disk");
    usize::try_from(file_offset).expect("the offset fits usize")
}

/// The NUL-terminated text one virtual address holds.
fn text_at(layout: &PeLayout, bytes: &[u8], va: u32) -> String {
    let start = offset_of(layout, va);
    let end = bytes[start..]
        .iter()
        .position(|byte| *byte == 0)
        .unwrap_or_else(|| panic!("{va:#x} holds no NUL-terminated text"))
        + start;
    String::from_utf8(bytes[start..end].to_vec()).expect("the image spells text as ASCII")
}

/// The little-endian `u32` one virtual address holds.
fn word_at(layout: &PeLayout, bytes: &[u8], va: u32) -> u32 {
    let start = offset_of(layout, va);
    u32::from_le_bytes(
        bytes[start..start + 4]
            .try_into()
            .expect("four bytes of the word are on disk"),
    )
}

/// **Every music state group resolves — through the production group table —
/// to the `MUSIC` leaf plus the tracks it names, and M01's own program cues
/// five of the seven.**
///
/// Every step is production code: the launch plan, the retail objective
/// recovery, the engine lifecycle model, and `MissionRadio::read`'s read of
/// `sounds.zrd`. A music group's tracks are never prepared as radio lines —
/// a music state request plays no dialogue.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_music_state_requests_resolve_to_their_tracks() {
    let root = game_dir();

    // M01's own program: which of the seven states it can cue.
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("M01 is a valid mission label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(&root, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let declared = declared_sound_groups(&engine);
    let declared_music: BTreeSet<&str> = M01_MUSIC_GROUPS
        .into_iter()
        .filter(|group| declared.contains(*group))
        .collect();
    assert_eq!(
        declared_music,
        BTreeSet::from(M01_MUSIC_GROUPS),
        "M01's program cues five music states; it holds {:?}",
        declared
    );
    for group in MUSIC_STATES_NOT_CUED {
        assert!(
            !declared.contains(group),
            "M01 spells five of the seven music states, so it must not cue {group}",
        );
    }

    // The seven states, read through the production group table.
    let radio = MissionRadio::read(
        &root,
        RETAIL_TRACKS.iter().map(|(group, _)| (*group).to_owned()),
    )
    .expect("the seven music state groups read from sounds.zrd");
    for (group, expected) in RETAIL_TRACKS {
        let resolved = radio
            .resolved(group)
            .unwrap_or_else(|| panic!("{group} must resolve"));
        assert!(
            resolved.music,
            "{group} spelled the MUSIC leaf, so it is a music state request: {resolved:?}"
        );
        let members: Vec<&str> = resolved.members.iter().map(String::as_str).collect();
        assert_eq!(
            members.as_slice(),
            expected,
            "{group} reaches exactly the tracks sounds.zrd names, in the spelled order"
        );
        let unresolved: Vec<&str> = resolved.unresolved.iter().map(String::as_str).collect();
        assert_eq!(
            unresolved,
            ["DYNAMIC_WEIGHTS"],
            "{group} spells the DYNAMIC_WEIGHTS property, which resolves to no member and is \
             named rather than dropped"
        );
    }

    // A music state's tracks never become radio lines: the prepare step
    // decodes members only for groups that are not music.
    for line in radio.prepared() {
        assert!(
            !line.member.starts_with("music_"),
            "a music track is not a spoken line: {line:?}"
        );
    }
}

/// The five music states M01 does **not** cue: the seven built-ins minus
/// M01's five.
const MUSIC_STATES_NOT_CUED: [&str; 2] = ["music_tertiaryobj_sg", "music_battle_sg"];

/// **The executable's music dispatcher indexes seven built-in track tables,
/// and each state's name sits where the finding says it does.**
///
/// Read through `cs_formats`' own PE layout reader: the name at the measured
/// virtual address, every table entry as a pointer to the track name it
/// spells, and each table's own address present in `.text` — which is what
/// ties the tables to the dispatcher rather than to this document.
#[test]
#[ignore = "requires CS_ENGINE_IMAGE"]
fn accept_m01_music_state_requests_match_the_executable_track_tables() {
    let bytes = engine_image();
    let mut context = ParseContext::with_defaults("crimson.decrypted.exe");
    let layout: PeLayout = read_pe_layout(&mut context, &bytes)
        .unwrap_or_else(|error| panic!("the engine image does not read as a PE: {error}"));

    let text = layout
        .sections()
        .iter()
        .find(|section| section.name == ".text")
        .expect("the image has a .text section");
    let text_start = offset_of(&layout, text.virtual_address + 0x0040_0000);
    let text_len = usize::try_from(text.raw_size).expect("the section length fits usize");
    let code = &bytes[text_start..text_start + text_len];

    for (state, name_va, table_va, tracks) in STATES {
        assert_eq!(
            text_at(&layout, &bytes, name_va),
            state,
            "the dispatcher's name table spells {state} at {name_va:#x}"
        );
        for (index, expected) in tracks.iter().enumerate() {
            let entry_va = table_va + 4 * u32::try_from(index).expect("the index fits u32");
            let pointer = word_at(&layout, &bytes, entry_va);
            assert_eq!(
                text_at(&layout, &bytes, pointer),
                *expected,
                "{state} slot {index} spells the track it plays"
            );
        }
        assert!(
            code.windows(4).any(|window| u32::from_le_bytes(
                window.try_into().expect("four bytes")
            ) == table_va),
            "{state}'s table at {table_va:#x} is indexed by code in .text"
        );
    }

    // The four direct music clips the dispatcher answers by name are their
    // own thing — no state, no table: the image spells each name, and that is
    // all this task claims about them.
    for name in [
        "music_splash.wav",
        "music_instantaction.wav",
        "music_spicyairtales.wav",
        "music_airtales.wav",
    ] {
        assert!(
            bytes
                .windows(name.len())
                .any(|window| window == name.as_bytes()),
            "the image spells {name} in its music string cluster"
        );
    }
}

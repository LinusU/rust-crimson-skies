//! Acceptance: M01's music state requests and the seven `music_*_sg` groups
//! they name (Rally #1463, `M01-PLAY-MUSIC`).
//!
//! A `WAKEUP_SOUND_GROUP` naming one of the executable's seven built-in
//! `music_*_sg` groups is a **music state request**, not an ordinary cue
//! (`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`).
//! [`MissionRadio::plan`] answers such a cue with
//! [`cs_app::mission_session::RadioLineOutcome::Music`] instead of a radio
//! line, and [`offer_cues`] turns that classification into the request it
//! is: the selector's measured pick, the labelled container choice's archive,
//! the owner-ruled track-level [`ContentKind::Music`] id, and the audio
//! session's [`MusicDirector`] verdict.
//!
//! This suite pins, through production code, every half of that chain:
//!
//! * the **measured rule** — the weighted draw's post-draw `DYNAMIC_WEIGHTS`
//!   rescale and the three objective states' shared alternating counter,
//!   driven by an injected seed (the ruling's designed RNG);
//! * the **identity** — `music_member_id`, one `ContentKind::Music` id per
//!   track over `install_file_key("<container>/<member>")`, the cue's id and
//!   any published id minted through that one function, and
//!   `MusicDirector`'s refusal of a non-`Music` id;
//! * the **retail half** — what `sounds.zrd`'s own group table expands each
//!   state to (the tracks it names), read by [`MissionRadio::read`] from the
//!   owner's installation, the spelled `DYNAMIC_WEIGHTS` factor, plus the
//!   five of the seven states M01's own program can cue, ending in the real
//!   mission opening's music requests reaching `MusicDirector` under those
//!   ids;
//! * the **image half** — the seven built-in track tables the executable's
//!   music dispatcher (`0x5954f0`) indexes, read from `$CS_ENGINE_IMAGE`
//!   through `cs_formats`' own PE layout reader, pinned against the
//!   production [`IMAGE_MUSIC_STATES`] table.
//!
//! The two measured halves are cross-checked: for six of the seven states
//! the executable's table and `sounds.zrd` name the same tracks in the same
//! order, and `music_prebattle_sg` is the measured exception (three distinct
//! names repeated to fill six slots in the image, six distinct tracks in the
//! retail data) — the image's table is followed, as the owner ruled.
//!
//! What is **not** claimed: nothing here plays anything. Whether a device
//! outputs the requested cue stays the owner's F41-D listening gate, and
//! nothing here is `verified_original`: no original executable ran and no
//! person heard anything.

use std::collections::BTreeSet;
use std::path::PathBuf;

use cs_app::audio::{
    AudioSession, IMAGE_MUSIC_STATES, MusicArm, MusicSelector, MusicStateRule, music_member_id,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionRadio, RadioLineOutcome, declared_engine_objectives, declared_sound_groups, offer_cues,
};
use cs_app::objectives::recover_retail_objectives;
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::SceneGeneration;
use cs_content::campaign_bindings::MissionLabel;
use cs_formats::ParseContext;
use cs_formats::pe_resources::{PeLayout, RvaSpan, read_pe_layout};
use cs_sim::audio_events::{AudioAssetSpec, AudioRouter, MusicCue, MusicDirector, MusicOutcome};
use cs_types::content::ContentKind;
use cs_types::net::SessionId;

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
        MissionLabel::new("M06").expect("M01 is a valid mission label"),
        "The Red Menace",
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

    // The production table is the one pinned against the image: every state
    // the selector serves, its rule and its slots must be the dispatcher's
    // own, at the measured addresses.
    for spec in IMAGE_MUSIC_STATES {
        let state = spec.group;
        assert_eq!(
            text_at(&layout, &bytes, spec.name_va),
            state,
            "the dispatcher's name table spells {state} at {:#x}",
            spec.name_va
        );
        for (index, expected) in spec.tracks.iter().enumerate() {
            let entry_va = spec.table_va + 4 * u32::try_from(index).expect("the index fits u32");
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
            ) == spec.table_va),
            "{state}'s table at {:#x} is indexed by code in .text",
            spec.table_va
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

/// **The selector reproduces the measured rule — the weighted draw's
/// post-draw `DYNAMIC_WEIGHTS` rescale and the objective states' shared
/// counter — under an injected seed, and `MusicDirector` serves a request
/// only under a `Music` id.**
///
/// This is the synthetic leg the owner ruling asks for: the arm is the shape
/// `sounds.zrd` spells (six children, the `0.5` factor), the seed is the
/// designed stream's own, and every pinned number derives from the measured
/// arithmetic — nothing here touches the installation.
#[test]
fn accept_m01_music_state_requests_selects_by_the_measured_rules() {
    let arm = |group: &str| MusicArm {
        group: group.to_owned(),
        children: 6,
        factor: Some(0.5),
    };
    let mut selector = MusicSelector::launch(0, [arm("music_battle_sg")]);

    // The weighted pick is the injected stream's: the slot names its table
    // track, and the post-draw rescale is the measured arithmetic on the
    // live weights — chosen × 0.5, 0.001 floor, renormalized to sum 100.
    let first = selector
        .select("music_battle_sg")
        .expect("an armed state selects");
    let battle = IMAGE_MUSIC_STATES
        .iter()
        .find(|spec| spec.group == "music_battle_sg")
        .expect("a built-in state");
    assert_eq!(battle.rule, MusicStateRule::WeightedDraw);
    assert_eq!(first.member, battle.tracks[first.slot]);
    let weights = selector
        .weights("music_battle_sg")
        .expect("an armed weighted state");
    let mut expected = [1.0f32; 6];
    expected[first.slot] = 0.5;
    let expected_sum: f32 = expected.iter().sum();
    for (index, weight) in weights.iter().enumerate() {
        let expected = expected[index] * (100.0 / expected_sum);
        assert!(
            (weight - expected).abs() < 1e-4,
            "slot {index}'s weight is the measured decay then renormalize: {weights:?}"
        );
    }
    assert!(
        (weights.iter().sum::<f32>() - 100.0).abs() < 1e-3,
        "the weights renormalize to the measured 100.0: {weights:?}"
    );

    // The injected stream is deterministic: two selectors on the same seed
    // replay the identical pick sequence from the first draw.
    let mut stream_a = MusicSelector::launch(0, [arm("music_battle_sg")]);
    let mut stream_b = MusicSelector::launch(0, [arm("music_battle_sg")]);
    for _ in 0..8 {
        assert_eq!(
            stream_a.select("music_battle_sg"),
            stream_b.select("music_battle_sg"),
            "one seed replays one pick sequence exactly"
        );
    }

    // A group no built-in state serves selects nothing — never a guessed
    // track.
    assert_eq!(selector.select("music_not_a_state_sg"), None);

    // The objective states share one counter: secondary reads it `% 2` and
    // advances it, primary reads it `% 2` without advancing, and tertiary
    // reads it raw (bounded by the table's two slots — the designed bound on
    // the measured oddity).
    let mut objectives = MusicSelector::empty(0);
    let member =
        |selector: &mut MusicSelector, group: &str| selector.select(group).map(|pick| pick.member);
    assert_eq!(
        member(&mut objectives, "music_secondaryobj_sg").as_deref(),
        Some("music_secondaryobj1.wav"),
        "counter 0 reads slot 0"
    );
    assert_eq!(objectives.objective_counter(), 1);
    assert_eq!(
        member(&mut objectives, "music_secondaryobj_sg").as_deref(),
        Some("music_secondaryobj2.wav"),
        "counter 1 reads slot 1"
    );
    assert_eq!(objectives.objective_counter(), 2);
    let primary = objectives
        .select("music_primaryobj_sg")
        .expect("an objective state selects");
    assert_eq!(primary.slot, 0, "counter 2 masks to its % 2 index");
    assert_eq!(
        primary.member, "music_primaryobj1.wav",
        "primary reads the shared counter's % 2 without advancing it"
    );
    assert_eq!(
        objectives.objective_counter(),
        2,
        "the primary branch does not advance the counter"
    );
    assert_eq!(
        member(&mut objectives, "music_tertiaryobj_sg").as_deref(),
        Some("music_tertiaryobj1.wav"),
        "tertiary reads the raw counter, bounded by its table"
    );

    // The measured `[group+8]` gate: a group that spells no
    // `DYNAMIC_WEIGHTS` skips the whole post-draw block — its weights never
    // move.
    let mut unspelled = MusicSelector::launch(
        0,
        [MusicArm {
            group: "music_battle_sg".to_owned(),
            children: 6,
            factor: None,
        }],
    );
    unspelled
        .select("music_battle_sg")
        .expect("an armed state selects");
    assert_eq!(
        unspelled.weights("music_battle_sg"),
        Some(&[1.0; 6][..]),
        "an unspelled factor leaves every weight at its initial value"
    );

    // The owner-ruled identity: one Music id per track, the container part
    // of the key.
    let asset = music_member_id("ZBD/soundsh.zbd", "music_battle1.wav")
        .expect("a track member derives an id");
    assert_eq!(asset.kind(), ContentKind::Music);
    assert_ne!(
        asset,
        music_member_id("ZBD/soundsl.zbd", "music_battle1.wav")
            .expect("the same member under the other container"),
        "the container is part of the identity"
    );

    // The director serves the request only under a `Music` id: the refusal
    // the ruling makes impossible to hide.
    let session = SessionId::new(7).expect("a nonzero session");
    let mut director = MusicDirector::new(session);
    let mut sequence = 0;
    let mut id = |producer: u32| {
        sequence += 1;
        cs_sim::audio_events::AudioEventId {
            session,
            tick: cs_types::Tick(0),
            producer,
            sequence,
        }
    };
    let not_music =
        cs_types::content::ContentId::from_source(ContentKind::Dialogue, "synthetic.not-music")
            .expect("a synthetic id");
    assert_eq!(
        director.request(MusicCue {
            id: id(0),
            asset: not_music
        }),
        MusicOutcome::RefusedNotMusic {
            kind: ContentKind::Dialogue
        },
        "a non-Music asset is refused by kind"
    );
    assert_eq!(
        director.request(MusicCue {
            id: id(0),
            asset: asset.clone()
        }),
        MusicOutcome::Started {
            to: asset.clone(),
            audible: true
        },
        "the same request under the owner-ruled id starts the cue"
    );
    assert_eq!(director.current().map(|cue| &cue.asset), Some(&asset));
}

/// **M01's music state requests reach `MusicDirector` under the owner-ruled
/// track-level identity, through the production sound step.**
///
/// The drive is the mission's own cue chain: `ObjectiveLifecycles::engine`
/// emits the `WAKEUP_SOUND_GROUP` cues M01's lifecycle transitions spell,
/// `offer_cues` — the host's whole sound step — resolves each one, the armed
/// selector picks the track by the state's measured rule, and the request
/// carries a [`ContentKind::Music`] id minted through [`music_member_id`]
/// over the labelled container's member. M01's music blocks wake only as
/// `WAKE_OBJECTIVE_WHEN_I_COMPLETE` effects of other blocks, so the test
/// completes exactly those blocks — `complete(index)` is the entry point
/// the host calls when a block's conditions hold; the emitted cues, the
/// effect cascade, the resolution and the request are the model's own.
/// Whether a device plays the cue is the F41-D gate's question, not this
/// test's.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_music_state_requests_reach_the_music_director() {
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M06").expect("M01 is a valid mission label"),
        "The Red Menace",
    )
    .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(&root, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let radio = MissionRadio::read(&root, declared_sound_groups(&engine))
        .expect("M01's sound groups read from sounds.zrd");

    // The music-state blocks M01's objective record declares: each cues its
    // state on `WAKEUP_SOUND_GROUP` and wakes only through another block's
    // `WAKE` effect.
    let mut music_blocks = Vec::new();
    let mut declared_groups = std::collections::BTreeSet::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if let Some(group) = objective
            .declaration
            .wakeup_sound_group
            .as_deref()
            .filter(|group| cs_app::audio::music_state_spec(group).is_some())
        {
            music_blocks.push(index);
            declared_groups.insert(group.to_owned());
        }
    }
    assert!(
        !music_blocks.is_empty(),
        "M01's objective record declares music-state blocks"
    );
    // Every block whose own `WAKE` list reaches a music block — completing
    // it is what the mission's record says wakes that block.
    let mut completers = Vec::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if objective
            .declaration
            .wake
            .targets()
            .iter()
            .any(|target| music_blocks.contains(target))
        {
            completers.push(index);
        }
    }

    let mut lifecycles = cs_app::mission_session::ObjectiveLifecycles::engine(engine);
    let mut music = MusicSelector::launch(0, radio.music_arms());
    let session = SessionId::new(9).expect("a nonzero session");
    let mut audio = AudioSession::new(
        AudioRouter::new(session),
        SceneGeneration(1),
        std::iter::empty::<AudioAssetSpec>(),
    );

    let elapsed = 1.0 / f64::from(BASELINE_FIXED_HZ);
    let mut requested: Vec<(
        String,
        usize,
        String,
        cs_types::content::ContentId,
        MusicOutcome,
    )> = Vec::new();
    let mut music_refusals = Vec::new();
    let serve = |lifecycles: &mut cs_app::mission_session::ObjectiveLifecycles,
                 music: &mut MusicSelector,
                 audio: &mut AudioSession,
                 requested: &mut Vec<(
        String,
        usize,
        String,
        cs_types::content::ContentId,
        MusicOutcome,
    )>,
                 music_refusals: &mut Vec<String>| {
        for outcome in offer_cues(&lifecycles.take_cues(), &radio, music, Some(audio)) {
            match outcome {
                RadioLineOutcome::MusicRequested {
                    group,
                    slot,
                    member,
                    asset,
                    outcome,
                } => requested.push((group, slot, member, asset, outcome)),
                RadioLineOutcome::Refused { group, refusal }
                    if matches!(
                        refusal,
                        cs_app::mission_session::RadioRefusal::MusicStateUnknown { .. }
                            | cs_app::mission_session::RadioRefusal::MusicMemberAbsent { .. }
                            | cs_app::mission_session::RadioRefusal::MusicIdentity { .. }
                    ) =>
                {
                    music_refusals.push(format!("{group:?}: {refusal}"));
                }
                _ => {}
            }
        }
    };

    // The opening: the mission clock's own wakes, then the completions that
    // cascade the music blocks' wakes.
    for _ in 0..4 * BASELINE_FIXED_HZ {
        lifecycles.advance(elapsed);
        serve(
            &mut lifecycles,
            &mut music,
            &mut audio,
            &mut requested,
            &mut music_refusals,
        );
    }
    for index in completers {
        lifecycles.complete(index as u32);
        serve(
            &mut lifecycles,
            &mut music,
            &mut audio,
            &mut requested,
            &mut music_refusals,
        );
    }

    assert!(
        music_refusals.is_empty(),
        "every music state request resolves a track and an identity: {music_refusals:?}"
    );
    // Every music state M01 declares was served: the groups the music
    // blocks cue are exactly the groups the director was asked for.
    let served: std::collections::BTreeSet<&String> =
        requested.iter().map(|(group, ..)| group).collect();
    assert!(
        !requested.is_empty(),
        "M01's opening emits at least one music state request"
    );
    assert_eq!(
        served,
        declared_groups
            .iter()
            .collect::<std::collections::BTreeSet<&String>>(),
        "every music state M01's record declares was requested"
    );
    assert_eq!(
        requested.len(),
        music_blocks.len(),
        "one request per declared music block: {requested:?}"
    );
    for (group, slot, member, asset, outcome) in &requested {
        let spec = cs_app::audio::music_state_spec(group)
            .unwrap_or_else(|| panic!("{group} is a built-in state"));
        assert_eq!(
            member.as_str(),
            spec.tracks[slot % spec.tracks.len()],
            "{group} slot {slot} names the image table's track"
        );
        let container = radio
            .music_container(member)
            .unwrap_or_else(|| panic!("{member} resolves to a container"));
        assert_eq!(
            asset,
            &music_member_id(container, member).expect("the track derives an id"),
            "the request carries the owner-ruled identity over {container}"
        );
        assert!(
            matches!(
                outcome,
                MusicOutcome::Started { .. }
                    | MusicOutcome::Transition { .. }
                    | MusicOutcome::Unchanged
            ),
            "the director accepted the request: {outcome:?}"
        );
    }
    // The session's director ends on the last requested cue — its current
    // state is what the mission asked for, under the same identity.
    let last = requested.last().expect("at least one request");
    assert_eq!(
        audio
            .music
            .current()
            .map(|cue| (&cue.asset, cue.id.session)),
        Some((&last.3, session)),
        "the director's current cue is the last track the mission requested"
    );
}

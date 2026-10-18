//! `M01-PLAY-RADIO` (#1438): the opening conversation lines are queued when
//! the mission program fires them.
//!
//! M01's record spells `BEGIN_DORMANT [2]` on `OBJECTIVE1` with
//! `WAKEUP_SOUND_GROUP snd_NW1Start`, so the measured lifecycle model wakes
//! that block two mission-clock seconds in and emits its cue
//! (`docs/findings/2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md`,
//! `docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`).
//! `snd_NW1Start` is a `SOUND_GROUPS` record whose children reach two set
//! rows in `sounds.zrd`, and those name the two voice members of the opening
//! crew conversation.
//!
//! Two retail members hold that, at two depths:
//!
//! * [`accept_m01_play_radio_the_opening_conversation_is_queued_at_mission_start`]
//!   (needs `CS_GAME_DIR` only) reads M01's record through
//!   [`recover_retail_objectives`], builds the engine lifecycle model with
//!   [`declared_engine_objectives`], prepares [`MissionRadio`] from the
//!   groups that model can cue, and drives the model's own mission clock the
//!   way the host drives it — [`ObjectiveLifecycles::advance`] then
//!   [`ObjectiveLifecycles::take_cues`] then [`offer_cues`] — into a real
//!   [`AudioSession`]. Every step is production code.
//! * [`accept_m01_play_radio_the_composition_queues_the_opening_conversation`]
//!   (needs `CS_GAME_DIR` **and** `CS_ENGINE_IMAGE`, because M01's player
//!   start is only known from the owner's decrypted image) runs the whole
//!   [`build_headless`] composition and reads the answers off
//!   [`MissionHostReport`], which is where the host's own sound step
//!   ([`MissionHost::step`]) publishes them.
//!
//! What is **not** claimed: nothing here is audible. The mixer does hand a
//! queued line to the device (`M01-RADIO-DEVICE` #1461,
//! `docs/findings/2026-10-15-m01-radio-device.md`), and
//! `M01-RADIO-SAMPLE-WIRE` (#1466,
//! `docs/findings/2026-10-15-m01-radio-sample-wire.md`) now publishes the
//! members this prepare step decoded into the device library under the
//! dialogue id a line names, so the device resolves them instead of refusing
//! `sample_unavailable`; whether a person hears them stays the owner's F41-D
//! listening test. There is
//! no subtitle text in the data this reads (see
//! `crates/cs_app/src/mission_session/radio.rs` and
//! `docs/findings/2026-10-15-m01-play-radio-dialogue.md`); and no original
//! executable ran, so nothing here is `verified_original`.
//!
//! The synthetic members of this suite live beside them: the
//! `accept_m01_play_radio_*` unit tests inside
//! `crates/cs_app/src/mission_session/radio.rs` hold the pure productions
//! (the speaker the member's own name spells, the dialogue id it derives, the
//! group expansion) and run in CI.

use std::collections::BTreeSet;

use cs_app::audio::{AudioSession, MusicSelector};
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHostReport, MissionRadio, ObjectiveLifecycles, RADIO_SOUND_CONTAINER, RadioLineOutcome,
    RadioRefusal, build_headless, declared_engine_objectives, declared_sound_groups,
    dialogue_content_id, offer_cues, stage_for, teardown,
};
use cs_app::objectives::recover_retail_objectives;
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::SceneGeneration;
use cs_content::objectives_engine::{CueEmission, CueKind};
use cs_sim::audio_events::{AudioAssetSpec, AudioRouter};
use cs_types::content::ContentKind;
use cs_types::net::SessionId;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::game_dir;

/// The group `OBJECTIVE1`'s measured wake spells.
const OPENING_GROUP: &str = "snd_NW1Start";

/// The two voice members that group reaches through `sounds.zrd`.
const OPENING_MEMBERS: [&str; 2] = [
    "VO_c2-NW-m1_WorkersVoyage_1.wav",
    "VO_c2-NW-m1_Zachary_2.wav",
];

/// One queued line, as the assertions read it back.
#[derive(Debug, Clone)]
struct Queued {
    group: String,
    member: String,
    speaker: String,
    duration_ticks: u64,
}

/// Splits the composed answers into the queued lines, the refusals and the
/// cues that reached no audio session.
fn collect(outcomes: &[RadioLineOutcome]) -> (Vec<Queued>, Vec<String>, usize) {
    let mut queued = Vec::new();
    let mut refused = Vec::new();
    let mut no_audio = 0usize;
    for outcome in outcomes {
        match outcome {
            RadioLineOutcome::Queued {
                group,
                member,
                speaker,
                duration_ticks,
            } => queued.push(Queued {
                group: group.clone(),
                member: member.clone(),
                speaker: speaker.clone(),
                duration_ticks: *duration_ticks,
            }),
            RadioLineOutcome::Refused { group, refusal } => refused.push(format!(
                "{}: {refusal}",
                group.as_deref().unwrap_or("<unnamed>")
            )),
            RadioLineOutcome::NoAudioSession { .. } => no_audio += 1,
            RadioLineOutcome::Music { .. } | RadioLineOutcome::MusicRequested { .. } => {}
        }
    }
    (queued, refused, no_audio)
}

/// Holds one drive's queued opening lines to the measured facts.
fn assert_opening(queued: &[Queued], refused: &[String], no_audio: usize) {
    assert_eq!(
        no_audio, 0,
        "every cue must reach the audio session, never be dropped for want of one"
    );
    let opening: Vec<&Queued> = queued
        .iter()
        .filter(|line| line.group == OPENING_GROUP)
        .collect();
    assert_eq!(
        opening.len(),
        2,
        "the opening group cues exactly two lines, got {} (queued: {queued:?}, refused: \
         {refused:?}, cues with no audio session: {no_audio})",
        opening.len()
    );
    let members = BTreeSet::from(OPENING_MEMBERS);
    let queued_members: BTreeSet<&str> = opening.iter().map(|line| line.member.as_str()).collect();
    assert_eq!(
        queued_members, members,
        "the opening conversation is the two members sounds.zrd maps the group to"
    );
    for line in &opening {
        assert!(
            line.duration_ticks > 0,
            "a queued line carries its measured length: {line:?}"
        );
        assert!(
            !line.speaker.is_empty(),
            "a queued line reads its speaker: {line:?}"
        );
    }
    assert!(
        refused.iter().all(|entry| !entry.contains(OPENING_GROUP)),
        "the opening group must not be refused: {refused:?}"
    );
}

/// Holds the queue's **active** line to the measured facts and answers how
/// many lines wait behind it.
///
/// The active line must be the **first** member `sounds.zrd` spelled for the
/// group: the table's child order is the order the conversation plays, so the
/// helper pins the spelled order rather than accepting either member.
fn assert_active_line(audio: &AudioSession) -> usize {
    let active = audio
        .radio
        .active()
        .expect("the first opening line starts the moment it is queued");
    assert_eq!(
        active.asset.kind(),
        ContentKind::Dialogue,
        "the queued line plays a dialogue id, got {}",
        active.asset
    );
    let first = dialogue_content_id(RADIO_SOUND_CONTAINER, OPENING_MEMBERS[0])
        .expect("the archive spells the member");
    assert_eq!(
        active.asset, first,
        "the group's spelled child order is the order the conversation plays: {} must be the \
         line already running, got {}",
        OPENING_MEMBERS[0], active.asset
    );
    assert!(
        !active.speaker.is_empty(),
        "the active line carries the speaker its member's name spells, got {:?}",
        active.speaker
    );
    assert_eq!(
        active.subtitle, None,
        "the installation holds no caption for a voice line, so none is invented"
    );
    audio.radio.pending_len()
}

/// **M01's program wakes `OBJECTIVE1` at mission start and the opening
/// conversation's two lines reach a real audio session's radio queue.**
///
/// Every step is production code: the retail record's recovery, the engine
/// lifecycle model, the group table read from `sounds.zrd`, the cue
/// hand-off and the audio session's own queue.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_radio_the_opening_conversation_is_queued_at_mission_start() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);

    let recovery = recover_retail_objectives(&root, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let radio = MissionRadio::read(&root, declared_sound_groups(&engine))
        .expect("M01's sound groups read from sounds.zrd and their members decode");

    // The prepared radio names the group this test is about, and every line
    // it prepared carries the measured facts a queue needs.
    assert!(
        radio.groups().any(|group| group == OPENING_GROUP),
        "the prepared radio must hold the group OBJECTIVE1 cues; it holds {:?}",
        radio.groups().collect::<Vec<_>>()
    );
    for line in radio.prepared() {
        assert!(
            line.duration_ticks > 0,
            "every prepared line carries a measured length: {line:?}"
        );
        assert_eq!(
            line.asset.kind(),
            ContentKind::Dialogue,
            "a spoken line's identity is a dialogue id: {line:?}"
        );
        assert!(
            !line.speaker.is_empty(),
            "a spoken line reads its speaker from the member's own name: {line:?}"
        );
    }

    let mut lifecycles = ObjectiveLifecycles::engine(engine);
    let mut audio = AudioSession::new(
        AudioRouter::new(SessionId::new(9).expect("a nonzero session")),
        SceneGeneration(1),
        std::iter::empty::<AudioAssetSpec>(),
    );
    // The same armed selector the host carries: music cues this drive emits
    // reach the session's MusicDirector, and this leg's assertions ignore
    // them as it always has.
    let mut music = MusicSelector::launch(0, radio.music_arms());

    // Mission start, driven the way the host drives it: one fixed tick's
    // elapsed per pass, the model's own clock inside it, then the cues it
    // emitted, offered to the audio session. `OBJECTIVE1` wakes at 2.0 s, so
    // three seconds of the mission clock covers it.
    let elapsed = 1.0 / f64::from(BASELINE_FIXED_HZ);
    let passes = usize::try_from(3 * BASELINE_FIXED_HZ).expect("three seconds of ticks fit usize");
    let mut outcomes: Vec<RadioLineOutcome> = Vec::new();
    for _ in 0..passes {
        lifecycles.advance(elapsed);
        let cues = lifecycles.take_cues();
        if cues.is_empty() {
            continue;
        }
        outcomes.extend(offer_cues(&cues, &radio, &mut music, Some(&mut audio)));
        if !outcomes.is_empty() {
            break;
        }
    }

    let (queued, refused, no_audio) = collect(&outcomes);
    assert_opening(&queued, &refused, no_audio);
    assert_eq!(
        assert_active_line(&audio),
        1,
        "the second opening line waits behind the first"
    );
}

/// **The composed M01 run publishes the same two lines on the host's own
/// sound step.**
///
/// This member needs `CS_ENGINE_IMAGE` as well: M01's player start is only
/// known from the owner's decrypted image, so without it the launch plan
/// reports the gap and the test fails loudly rather than skipping.
///
/// The composition is pumped **one committed fixed tick per world update**
/// (the frame pinned to the fixed timestep, the way
/// `PhysicsSession::pump_frame` pumps it): an unpinned 1/60 s frame commits
/// two ticks over the 120 Hz loop, and the composed entry's report for the
/// frame's first tick is overwritten before this test can read it — which
/// would lose OBJECTIVE1's own wake tick, the one tick that carries the
/// conversation.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_play_radio_the_composition_queues_the_opening_conversation() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    assert!(
        plan.launchable(),
        "every launch surface must be satisfied before the composition runs: {}",
        plan.gaps()
            .map(|gap| format!("{}: {}", gap.surface.label(), gap.verdict.describe()))
            .collect::<Vec<_>>()
            .join("; ")
    );
    let stage = stage_for(&root, &plan).expect("M01's stage reads through the production readers");
    drop(plan);

    assert!(
        stage
            .host
            .radio
            .groups()
            .any(|group| group == OPENING_GROUP),
        "the stage's radio must hold the group OBJECTIVE1 cues; it holds {:?}",
        stage.host.radio.groups().collect::<Vec<_>>()
    );

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let mut outcomes: Vec<RadioLineOutcome> = Vec::new();
    let mut seen = 0u64;
    let mut tick = 0u64;
    // A composed step that refuses answers nothing: the refusal itself is
    // the reason, so it is carried out of the loop and quoted by the
    // assertion instead of leaving an empty answer nobody can explain.
    let mut answer_error: Option<String> = None;
    // One headless frame is 1/60 s over the 120 Hz fixed loop, so an
    // unpinned frame commits **two** ticks and the composed entry overwrites
    // the first one's report before this test can read it — which is exactly
    // what `PhysicsSession::pump_frame` prevents by pumping one fixed tick
    // per world update. OBJECTIVE1 wakes two mission seconds in, so the
    // report of that very tick has to be readable: the frame is pinned to
    // one fixed tick the same way here.
    let timestep = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep();
    // The wake fires at tick 240 (two seconds of the 120 Hz mission clock);
    // 600 is the guard that fails loudly instead of spinning if it never does.
    for _ in 0..200_000 {
        *app.world_mut()
            .resource_mut::<bevy::time::TimeUpdateStrategy>() =
            bevy::time::TimeUpdateStrategy::ManualDuration(timestep);
        app.update();
        let Some(report) = app.world().get_resource::<MissionHostReport>() else {
            continue;
        };
        tick = report.tick.0;
        if tick == seen {
            continue;
        }
        seen = tick;
        match &report.answer {
            Ok(produced) => outcomes.extend(produced.radio.iter().cloned()),
            Err(error) => {
                answer_error = Some(format!("composed step at tick {tick} refused: {error}"));
                break;
            }
        }
        if tick >= 600 {
            break;
        }
        if tick >= 245 && outcomes.iter().any(|outcome| {
            matches!(outcome, RadioLineOutcome::Queued { group, .. } if group == OPENING_GROUP)
        }) {
            break;
        }
    }
    assert!(
        answer_error.is_none(),
        "{}",
        answer_error.unwrap_or_default()
    );
    assert!(
        tick >= 245,
        "the composed run must pass OBJECTIVE1's two-second wake; it stopped at tick {tick}"
    );

    let (queued, refused, no_audio) = collect(&outcomes);
    assert_opening(&queued, &refused, no_audio);
    let audio = app
        .world()
        .get_resource::<AudioSession>()
        .expect("the composition installs the audio session for the launch sound");
    assert!(
        assert_active_line(audio) >= 1,
        "the second opening line waits behind the first"
    );
    teardown(&mut app);
}

/// The voice groups M01's record can cue and the members each reaches, in the
/// spelled order, as measured on the owner's installation
/// (`docs/findings/2026-10-18-m01-radio-cue-inventory.md`).
const VOICE_GROUPS: &[(&str, &[&str])] = &[
    (
        "snd_NW1BSwan",
        &[
            "VO_c2-NW-m1_BlackSwan_25.wav",
            "VO_c2-NW-m1_Zachary_26.wav",
            "VO_c2-NW-m1_BlackSwan_27.wav",
            "VO_c2-NW-m1_BigJohn_28.wav",
            "VO_c2-NW-m1_Zachary_29.wav",
        ],
    ),
    (
        "snd_NW1Fass",
        &[
            "VO_c2-NW-m1_Jack_30.wav",
            "VO_c2-NW-m1_Zachary_31.wav",
            "VO_c2-NW-m1_Jack_31-1.wav",
            "VO_c2-NW-m1_Zachary_31-2.wav",
            "VO_c2-NW-m1_Jack_32.wav",
            "VO_c2-NW-m1_Zachary_33.wav",
        ],
    ),
    (
        "snd_NW1FirstHook",
        &["VO_c2-NW-m1_Jack_22.wav", "VO_c2-NW-m1_Zachary_23.wav"],
    ),
    (
        "snd_NW1Prim2",
        &[
            "VO_c2-NW-m1_WorkersVoyage_8.wav",
            "VO_c2-NW-m1_WorkersVoyage_9.wav",
            "VO_c2-NW-m1_Tex_10.wav",
        ],
    ),
    (
        "snd_NW1Prim5Suc",
        &["VO_c2-NW-m1_Zachary_38.wav", "VO_c2-NW-m1_Sparks_39.wav"],
    ),
    (
        "snd_NW1Sec1Suc",
        &[
            "VO_c2-NW-m1_BlackSwan_41.wav",
            "VO_c2-NW-m1_Zachary_42.wav",
            "VO_c2-NW-m1_Zachary_43.wav",
        ],
    ),
    (
        "snd_NW1Start",
        &[
            "VO_c2-NW-m1_WorkersVoyage_1.wav",
            "VO_c2-NW-m1_Zachary_2.wav",
        ],
    ),
    ("snd_c2-NW-m1_Jack_11", &["VO_c2-NW-m1_Jack_11.wav"]),
    ("snd_c2-NW-m1_Jack_21", &["VO_c2-NW-m1_Jack_21.wav"]),
    ("snd_c2-NW-m1_Jack_34", &["VO_c2-NW-m1_Jack_34.wav"]),
    ("snd_c2-NW-m1_Jack_44", &["VO_c2-NW-m1_Jack_44.wav"]),
    ("snd_c2-NW-m1_Jack_7", &["VO_c2-NW-m1_Jack_7.wav"]),
    ("snd_c2-NW-m1_Sparks_24", &["VO_c2-NW-m1_Sparks_24.wav"]),
    ("snd_c2-NW-m1_Sparks_3", &["VO_c2-NW-m1_Sparks_3.wav"]),
    ("snd_c2-NW-m1_Sparks_4", &["VO_c2-NW-m1_Sparks_4.wav"]),
    ("snd_c2-NW-m1_Sparks_40", &["VO_c2-NW-m1_Sparks_40.wav"]),
    ("snd_c2-NW-m1_Sparks_6", &["VO_c2-NW-m1_Sparks_6.wav"]),
    ("snd_c2-NW-m1_Tex_17", &["VO_c2-NW-m1_Tex_17.wav"]),
    ("snd_c2-NW-m1_Tex_18", &["VO_c2-NW-m1_Tex_18.wav"]),
    ("snd_c2-NW-m1_Tex_35", &["VO_c2-NW-m1_Tex_35.wav"]),
    ("snd_c2-NW-m1_Tex_5", &["VO_c2-NW-m1_Tex_5.wav"]),
    (
        "snd_c2-NW-m1_WorkersVoyage_12",
        &["VO_c2-NW-m1_WorkersVoyage_12.wav"],
    ),
    (
        "snd_c2-NW-m1_WorkersVoyage_13",
        &["VO_c2-NW-m1_WorkersVoyage_13.wav"],
    ),
    (
        "snd_c2-NW-m1_WorkersVoyage_15",
        &["VO_c2-NW-m1_WorkersVoyage_15.wav"],
    ),
    (
        "snd_c2-NW-m1_WorkersVoyage_19",
        &["VO_c2-NW-m1_WorkersVoyage_19.wav"],
    ),
    ("snd_c2-NW-m1_Zachary_14", &["VO_c2-NW-m1_Zachary_14.wav"]),
    ("snd_c2-NW-m1_Zachary_20", &["VO_c2-NW-m1_Zachary_20.wav"]),
    ("snd_c2-NW-m1_Zachary_37", &["VO_c2-NW-m1_Zachary_37.wav"]),
    ("snd_c2-NW-m1_Zachary_45", &["VO_c2-NW-m1_Zachary_45.wav"]),
];

/// The music state requests M01's record can cue: the group, the stem its
/// track members share, and how many tracks (`<stem>1.wav` ..
/// `<stem><n>.wav`) the group reaches.
const MUSIC_GROUPS: &[(&str, &str, usize)] = &[
    ("music_battlesuccess_sg", "music_battlesuccess", 6),
    ("music_missionsuccess_sg", "music_missionsuccess", 6),
    ("music_prebattle_sg", "music_prebattle", 6),
    ("music_primaryobj_sg", "music_primaryobj", 2),
    ("music_secondaryobj_sg", "music_secondaryobj", 2),
];

/// **Every sound group M01's record can cue resolves to what the table
/// says: voice members in the spelled order, or a music state request.**
///
/// Drives the production path the first member drives, then asks
/// [`MissionRadio::plan`] about a cue for each group
/// [`declared_sound_groups`] returns. Nothing may be refused, nothing may be
/// [`RadioRefusal::GroupNotPrepared`], and the declared set must equal the
/// recorded inventory exactly, so a group appearing or vanishing fails here.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_radio_every_declared_group_resolves_to_the_recorded_inventory() {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(&root, &mission_dir)
        .expect("M01's objective record reads through the production recovery");
    let engine = declared_engine_objectives(&recovery)
        .expect("M01's engine lifecycle model builds from its own declarations");
    let declared = declared_sound_groups(&engine);
    let radio = MissionRadio::read(&root, declared.clone())
        .expect("M01's sound groups read from sounds.zrd and their members decode");

    let recorded: BTreeSet<String> = VOICE_GROUPS
        .iter()
        .map(|(group, _)| (*group).to_owned())
        .chain(MUSIC_GROUPS.iter().map(|(group, _, _)| (*group).to_owned()))
        .collect();
    assert_eq!(
        declared, recorded,
        "the groups M01's record can cue changed from the recorded inventory"
    );
    assert_eq!(
        radio.failures().collect::<Vec<_>>(),
        Vec::<(&str, &RadioRefusal)>::new(),
        "no declared group or member may refuse to prepare"
    );

    for wakeup in [CueKind::Wakeup, CueKind::Completed] {
        for (group, members) in VOICE_GROUPS {
            let outcomes = radio.plan(&CueEmission {
                objective: 0,
                kind: wakeup,
                group: Some((*group).to_owned()),
            });
            let lines: Vec<&str> = outcomes
                .iter()
                .map(|outcome| match outcome {
                    RadioLineOutcome::Queued {
                        group: queued_group,
                        member,
                        speaker,
                        duration_ticks,
                    } => {
                        assert_eq!(queued_group, group);
                        assert!(*duration_ticks > 0, "{member} has no measured length");
                        assert!(!speaker.is_empty(), "{member} has no speaker");
                        member.as_str()
                    }
                    other => panic!("{group} must queue voice lines only, got {other:?}"),
                })
                .collect();
            assert_eq!(
                &lines, members,
                "{group}: the member list changed from the recorded table"
            );
        }
        for (group, stem, count) in MUSIC_GROUPS {
            let outcomes = radio.plan(&CueEmission {
                objective: 0,
                kind: wakeup,
                group: Some((*group).to_owned()),
            });
            assert_eq!(
                outcomes,
                vec![RadioLineOutcome::Music {
                    group: (*group).to_owned()
                }],
                "{group} is a music state request, not a voice line"
            );
            let resolved = radio
                .resolved(group)
                .unwrap_or_else(|| panic!("{group} must be prepared"));
            assert!(resolved.music, "{group} spells the MUSIC leaf");
            let tracks: Vec<String> = (1..=*count).map(|n| format!("{stem}{n}.wav")).collect();
            assert_eq!(
                resolved.members, tracks,
                "{group}: the track list changed from the recorded table"
            );
        }
    }
}

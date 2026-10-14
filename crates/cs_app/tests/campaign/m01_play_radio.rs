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
//! crew conversation — this module's retail member drives the real M01 stage
//! headlessly and holds the audio session's own radio queue to those two
//! lines.
//!
//! What is **not** claimed: nothing here is audible — no mixer or device
//! consumes a radio event yet (`docs/findings/2026-10-01-f41-c-radio-queue-music-and-device-loss.md`),
//! so the owner's listening test waits on that work; there is no subtitle
//! text in the data this reads (see `crates/cs_app/src/mission_session/radio.rs`);
//! and no original executable ran, so nothing here is `verified_original`.
//!
//! The synthetic members of this suite live beside the retail one: the
//! `accept_m01_play_radio_*` unit tests inside
//! `crates/cs_app/src/mission_session/radio.rs` hold the pure productions
//! (the speaker the member's own name spells, the dialogue id it derives, the
//! group expansion) and run in CI.

use std::collections::BTreeSet;

use cs_app::audio::AudioSession;
use cs_app::mission_launch::{MissionLaunchPlan, plan_mission_launch};
use cs_app::mission_session::{
    MissionHostReport, RadioLineOutcome, build_headless, stage_for, teardown,
};
use cs_types::content::ContentKind;

use crate::common::label;
use crate::vs_m01_rt_window::game_dir;

/// The group `OBJECTIVE1`'s measured wake spells.
const OPENING_GROUP: &str = "snd_NW1Start";

/// The two voice members that group reaches through `sounds.zrd`.
const OPENING_MEMBERS: [&str; 2] = [
    "VO_c2-NW-m1_WorkersVoyage_1.wav",
    "VO_c2-NW-m1_Zachary_2.wav",
];

/// One host tick's own radio answer, as the assertion reads it back.
#[derive(Debug, Clone)]
struct Queued {
    group: String,
    member: String,
    speaker: String,
    duration_ticks: u64,
}

/// What one drive of the real composition answered, as the assertions read
/// it back.
#[derive(Debug)]
struct Drive {
    /// Every line the composed entry queued, in order.
    queued: Vec<Queued>,
    /// Every cue the composed entry refused, as the refusal displays.
    refused: Vec<String>,
    /// Cues that reached no audio session (must stay zero).
    no_audio: usize,
    /// The last committed tick the drive reached.
    tick: u64,
    /// The queue's active line: its identity, speaker and caption.
    active: Option<(ContentKind, String, String, Option<String>)>,
    /// How many lines wait behind the active one.
    pending: usize,
}

/// Drives M01's real composition until the mission clock passes
/// `OBJECTIVE1`'s two-second wake, collecting every radio answer the
/// composed entry produced on the way.
fn drive_opening() -> Drive {
    let root = game_dir();
    let plan: MissionLaunchPlan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
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

    // The prepared radio is part of the stage: the group this test is about
    // must be one the mission's own declarations spelled.
    assert!(
        stage
            .host
            .radio
            .groups()
            .any(|group| group == OPENING_GROUP),
        "the prepared radio must hold the group OBJECTIVE1 cues; it holds {:?}",
        stage.host.radio.groups().collect::<Vec<_>>()
    );
    for line in stage.host.radio.prepared() {
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

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let mut queued: Vec<Queued> = Vec::new();
    let mut refused: Vec<String> = Vec::new();
    let mut no_audio = 0usize;
    let mut seen = 0u64;
    let mut tick = 0u64;
    // One host tick covers `BASELINE_FIXED_HZ`'s worth of mission clock, so
    // the wake fires at tick 240; the budget is generous because a headless
    // frame commits however many fixed ticks its elapsed covers.
    for _ in 0..200_000 {
        app.update();
        let Some(report) = app.world().get_resource::<MissionHostReport>() else {
            continue;
        };
        tick = report.tick.0;
        if tick == seen {
            continue;
        }
        seen = tick;
        let Ok(produced) = &report.answer else {
            continue;
        };
        for outcome in &produced.radio {
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
                RadioLineOutcome::Music { .. } => {}
            }
        }
        if tick >= 245 && !queued.is_empty() {
            break;
        }
    }

    let active = app
        .world()
        .get_resource::<AudioSession>()
        .and_then(|audio| {
            audio.radio.active().map(|line| {
                (
                    line.asset.kind(),
                    line.asset.key().to_owned(),
                    line.speaker.clone(),
                    line.subtitle.clone(),
                )
            })
        });
    let pending = app
        .world()
        .get_resource::<AudioSession>()
        .map_or(0, |audio| audio.radio.pending_len());
    teardown(&mut app);
    Drive {
        queued,
        refused,
        no_audio,
        tick,
        active,
        pending,
    }
}

/// **The opening conversation reaches the audio session's radio queue at
/// mission start: both lines, their speakers and their measured lengths.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_play_radio_the_opening_conversation_is_queued_at_mission_start() {
    let drive = drive_opening();

    assert!(
        drive.tick >= 245,
        "the composed run must pass OBJECTIVE1's two-second wake; it stopped at tick {}",
        drive.tick
    );
    assert_eq!(
        drive.no_audio, 0,
        "every cue must reach the audio session, never be dropped for want of one"
    );
    let opening: Vec<&Queued> = drive
        .queued
        .iter()
        .filter(|line| line.group == OPENING_GROUP)
        .collect();
    assert_eq!(
        opening.len(),
        2,
        "the opening group cues exactly two lines, got {opening:?} (all answers: {:?})",
        drive.queued
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
        drive
            .refused
            .iter()
            .all(|entry| !entry.contains(OPENING_GROUP)),
        "the opening group must not be refused: {:?}",
        drive.refused
    );

    // The queue itself: one line active, the other waiting behind it.
    let (kind, key, speaker, subtitle) = drive
        .active
        .expect("the first opening line starts the moment it is queued");
    assert_eq!(
        kind,
        ContentKind::Dialogue,
        "the queued line plays a dialogue id, got {key}"
    );
    assert!(
        OPENING_MEMBERS.iter().any(|member| key.contains(*member)),
        "the active line is one of the two opening members, got {key}"
    );
    assert!(
        !speaker.is_empty(),
        "the active line carries the speaker its member's name spells, got {speaker:?}"
    );
    assert_eq!(
        subtitle, None,
        "the installation holds no caption for a voice line, so none is invented for {key}"
    );
    assert_eq!(
        drive.pending, 1,
        "the second opening line waits behind the first"
    );
}

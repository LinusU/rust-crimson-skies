//! `M01-RADIO-SAMPLE-WIRE` (#1466): the radio prepare step's decoded voice
//! members reach the device's sample library under the exact dialogue id a
//! `RadioLine` names.
//!
//! #1438 (`M01-PLAY-RADIO`) resolves each sound-group cue to the voice members
//! `sounds.zrd` names, decodes each one's length and queues a line per member;
//! #1465 (`M01-RADIO-SAMPLE-IDENTITY`) landed the identity and the publish API.
//! Nothing called it, so the device refused every line by name
//! (`sample_unavailable`) instead of resolving it. This suite drives the
//! wiring that closes that gap through production code only:
//!
//! * [`accept_m01_radio_wire_a_the_composition_publishes_a_prepared_member_into_the_library`]
//!   (runs in CI, no original data) composes the real headless composition over
//!   a synthetic stage that carries the audio stack (`launch_sound`) and a
//!   prepare step with one decoded member, runs the load's own population pass,
//!   and reads the device library the composition filled: the member is under
//!   the dialogue id the prepared line names, and the real `AudibleDevice`'s
//!   own lookup finds it. The pass **replaces** the library wholesale, so a
//!   publish ordered before it would be wiped in this same frame — the test is
//!   the ordering contract, not just the insert.
//! * [`accept_m01_radio_wire_b_the_composed_m01_run_delivers_its_opening_members`]
//!   (`#[ignore]`, `CS_GAME_DIR` and `CS_ENGINE_IMAGE`) runs the real M01
//!   composition, pumps it past `OBJECTIVE1`'s measured two-second wake, and
//!   shows the two members of the opening crew conversation in the device
//!   library under their dialogue ids, found by the device's own lookup while
//!   a member nothing published is still refused `sample_unavailable`.
//!
//! What is **not** claimed: nothing here is audible. No scenario opens a
//! speaker to a person, no original executable ran, and the member identities
//! are the owner's designed rule (task #1465 note of 2026-10-15) because the
//! installation states no cue class (F14-D.7) — so nothing here is
//! `verified_original`, and the owner's F41-D listening test stays the gate.
//! Which of `soundsh`/`soundsl` the original opens is unmeasured, so the
//! container is the labelled choice #1438 made.

use std::path::Path;
use std::sync::Arc;

use bevy::app::App;
use cs_app::audio::{
    AudibleDevice, AudioHandoffLog, CODE_DEVICE_CLOSED, CODE_SAMPLE_UNAVAILABLE,
    DeviceSampleLibrary, PcmAudio, SampleLibrary, voice_member_id,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    LaunchSound, MissionHostReport, MissionRadio, RADIO_SOUND_CONTAINER, RadioLineOutcome,
    build_headless, dialogue_content_id, member_content_id, stage_for, teardown,
};
use cs_sim::audio_events::{AudioBus, AudioDevice, RADIO_GAIN, VoiceStart};
use cs_types::content::{ContentId, ContentKind};

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir, synthetic_stage};

/// The container both scenarios read their voice members from: `soundsh`
/// holds the same M01 members as `soundsl`, and which the original opens is
/// unmeasured (F14-D.7) — the labelled choice #1438 made.
const RADIO_CONTAINER: &str = "ZBD/soundsh.zbd";

/// A synthetic voice member: a synthetic spelling, never a retail name, so the
/// CI scenario cannot be mistaken for a measurement of original content. The
/// identity and length derived from it are the production derivations.
const SYNTHETIC_MEMBER: &str = "VO_m1_Wire_1.wav";

/// The two voice members of M01's opening crew conversation, measured by #1438.
const OPENING_MEMBERS: [&str; 2] = [
    "VO_c2-NW-m1_WorkersVoyage_1.wav",
    "VO_c2-NW-m1_Zachary_2.wav",
];

/// The group `OBJECTIVE1`'s measured wake cues.
const OPENING_GROUP: &str = "snd_NW1Start";

/// The archive member no prepare step ever publishes; used to show the
/// device's own lookup telling a delivered member from one it was never given.
const ABSENT_MEMBER: &str = "VO_c2-NW-m1_nobody_99.wav";

/// The radio start the mixer emits for a line: the radio bus at unity gain.
fn start(asset: &ContentId) -> VoiceStart {
    VoiceStart {
        asset: asset.clone(),
        bus: AudioBus::Radio,
        gain: RADIO_GAIN,
        pan: 0.0,
        pitch: 1.0,
    }
}

/// The library the composition's device plays from: the same object the load's
/// population pass fills and the publish step inserts into.
fn composition_library(app: &App) -> Arc<dyn SampleLibrary> {
    Arc::clone(app.world().resource::<DeviceSampleLibrary>().library())
}

/// A hand-built launch sound whose container is a scratch file. Its catalog
/// record is the composition's one declared sound asset (never delivered, so
/// the population pass replaces the library with nothing); its container is
/// only ever mounted, never parsed. This is the synthetic seam that makes the
/// composition install the real audio stack — the library, the plugin and the
/// population pass — without an installation.
fn synthetic_launch_sound(scratch: &Path) -> LaunchSound {
    let container = RADIO_CONTAINER;
    let member = "synthetic.wav";
    let host = scratch.join("launch-source").join(container);
    std::fs::create_dir_all(host.parent().expect("the container has a directory"))
        .expect("the launch source directory is created");
    let bytes = b"synthetic launch container: mounted, never parsed".to_vec();
    std::fs::write(&host, &bytes).expect("the synthetic container is written");
    let container_sha256 = cs_assets::install::sha256(&bytes);
    LaunchSound {
        airframe: "synthetic-airframe".to_owned(),
        group: "snd_synthetic".to_owned(),
        member: member.to_owned(),
        container: container.to_owned(),
        container_sha256,
        host,
        content: member_content_id(container, member).expect("the synthetic member derives an id"),
        bytes: bytes.len() as u64,
    }
}

/// **The real composition publishes the prepare step's decoded member into the
/// device library, on top of the load's own population pass.**
///
/// The stage is synthetic and carries a hand-built launch sound, so the
/// composition installs the real [`AudioPlugin`](cs_app::audio::AudioPlugin),
/// the real `DeviceSampleLibrary` and the real population pass; the prepare
/// step is the production [`MissionRadio`], built from one already-decoded
/// member. Nothing about the retail data is claimed — the wiring is.
#[test]
fn accept_m01_radio_wire_a_the_composition_publishes_a_prepared_member_into_the_library() {
    let scratch = Scratch::new("radio_wire");
    let mut stage = synthetic_stage(scratch.path());
    stage.launch_sound = Some(synthetic_launch_sound(scratch.path()));

    let pcm = PcmAudio::try_new(1, 11_025, vec![0.25; 64]).expect("a valid mono asset");
    let radio = MissionRadio::from_synthetic_members(
        RADIO_CONTAINER,
        [(SYNTHETIC_MEMBER.to_owned(), pcm.clone())],
    )
    .expect("the synthetic member spells a voice line");
    let line = radio
        .prepared()
        .next()
        .expect("the prepare step holds the member it decoded");
    let line_asset = line.asset.clone();
    let expected_id =
        voice_member_id(RADIO_CONTAINER, SYNTHETIC_MEMBER).expect("the member derives an id");
    assert_eq!(
        line_asset, expected_id,
        "the id the prepared line names is the dialogue id over the member's own spelling"
    );
    assert_eq!(line_asset.kind(), ContentKind::Dialogue);
    assert_eq!(line.member, SYNTHETIC_MEMBER);
    let voices = radio.voices();
    assert_eq!(voices.len(), 1, "the prepare step keeps the PCM it decoded");
    assert_eq!(
        voices[0].id(),
        &line_asset,
        "the voice's id is the line's own, derived by the same code path"
    );
    assert_eq!(voices[0].pcm(), &pcm);
    stage.host.radio = radio;

    let mut app = build_headless(&stage).expect("the synthetic stage with audio composes");
    assert!(
        app.world().get_resource::<DeviceSampleLibrary>().is_some(),
        "the launch sound's audible library must be installed by the composition"
    );
    assert!(
        composition_library(&app).pcm(&line_asset).is_none(),
        "nothing is in the library before the load's population pass and the publish run"
    );

    // One frame: the load's own population pass runs (it installs the audio
    // session and replaces the library wholesale), then the publish step runs
    // after it. If the publish had run before the pass, the replace would have
    // discarded this member in the very frame it arrived.
    app.update();
    let handoff = app.world().resource::<AudioHandoffLog>();
    assert!(
        handoff.installed.is_some(),
        "the load's population pass must have installed the session: {:?}",
        handoff.refusals
    );
    let library = composition_library(&app);
    assert_eq!(
        library.pcm(&line_asset),
        Some(pcm.clone()),
        "the prepare step's member is under the exact id its line names, after the population pass"
    );

    // The device's own lookup: the delivered member is resolved (only the
    // absent output refuses), while an id nothing published is refused by name.
    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let delivered = device
        .start_one_shot(start(&line_asset))
        .expect_err("an unopened device grants no output");
    assert_eq!(
        delivered.code, CODE_DEVICE_CLOSED,
        "the device found the published member and refused only the output: {delivered}"
    );
    let absent = voice_member_id(RADIO_CONTAINER, ABSENT_MEMBER)
        .expect("the absent member derives a dialogue id");
    let undelivered = device
        .start_one_shot(start(&absent))
        .expect_err("a member the prepare did not deliver is refused");
    assert_eq!(
        undelivered.code, CODE_SAMPLE_UNAVAILABLE,
        "an id no prepare step published is still refused by name: {undelivered}"
    );

    // The publish is one per load: a second frame neither empties nor changes
    // the library.
    app.update();
    assert_eq!(
        composition_library(&app).pcm(&line_asset),
        Some(pcm),
        "the published member outlives the frame it arrived in"
    );
    teardown(&mut app);
}

/// **The composed M01 run delivers the opening conversation's two members to
/// the device library.**
///
/// The whole retail composition: the real plan, the real stage (its prepare
/// step reads `sounds.zrd` and decodes each member), the real load and its
/// population pass, and the publish step this task wired. `CS_ENGINE_IMAGE` is
/// needed as well because M01's player start is only known from the owner's
/// decrypted image.
#[test]
#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]
fn accept_m01_radio_wire_b_the_composed_m01_run_delivers_its_opening_members() {
    let root = game_dir();
    let plan = plan_mission_launch(&root, label("M01"), "The Lost Treasure")
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

    // The prepare step kept the PCM it decoded for every line it prepared, and
    // each line's id is one voice's id.
    assert!(
        !stage.host.radio.voices().is_empty(),
        "the prepare step must keep the members it decoded"
    );
    for line in stage.host.radio.prepared() {
        let matching: Vec<_> = stage
            .host
            .radio
            .voices()
            .iter()
            .filter(|voice| voice.id() == &line.asset)
            .collect();
        assert_eq!(
            matching.len(),
            1,
            "each prepared line's id is exactly one voice's id: {line:?}"
        );
        assert_eq!(matching[0].member(), line.member.as_str());
        assert!(
            matching[0].pcm().frames() > 0,
            "the member decoded to frames"
        );
    }

    let mut app = build_headless(&stage).expect("M01's stage composes headlessly");
    let (tick, outcomes) = pump_to_opening(&mut app);
    assert!(
        tick >= 245,
        "the composed run must pass OBJECTIVE1's two-second wake; it stopped at tick {tick}"
    );

    // The opening group's two lines were queued by the host's own sound step,
    // exactly the members their ids name.
    let queued: Vec<&str> = outcomes
        .iter()
        .filter_map(|outcome| match outcome {
            RadioLineOutcome::Queued { group, member, .. } if group == OPENING_GROUP => {
                Some(member.as_str())
            }
            _ => None,
        })
        .collect();
    for member in OPENING_MEMBERS {
        assert!(
            queued.contains(&member),
            "the opening group queues {member:?}; it queued {queued:?} (all outcomes: {outcomes:?})"
        );
    }

    let library = composition_library(&app);
    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    for member in OPENING_MEMBERS {
        let id = dialogue_content_id(RADIO_SOUND_CONTAINER, member)
            .expect("the archive spells the member");
        assert_eq!(id.kind(), ContentKind::Dialogue);
        assert!(
            library.pcm(&id).is_some(),
            "the prepare step's member {member:?} must reach the device library under the exact \
             id its line names"
        );
        let refusal = device
            .start_one_shot(start(&id))
            .expect_err("an unopened device grants no output");
        assert_eq!(
            refusal.code, CODE_DEVICE_CLOSED,
            "the device resolved {member:?} and refused only the output: {refusal}"
        );
    }

    // The refusal path is unchanged: a member no prepare step published is
    // still named `sample_unavailable`.
    let absent = dialogue_content_id(RADIO_SOUND_CONTAINER, ABSENT_MEMBER)
        .expect("the absent member derives an id");
    let undelivered = device
        .start_one_shot(start(&absent))
        .expect_err("a member nothing published is refused");
    assert_eq!(
        undelivered.code, CODE_SAMPLE_UNAVAILABLE,
        "an id no prepare step published is still refused by name: {undelivered}"
    );
    teardown(&mut app);
}

/// Pumps the composed run one committed fixed tick per world update — the
/// frame pinned to the fixed timestep, the way
/// `PhysicsSession::pump_frame` pumps it — until the opening group's cue has
/// been offered, and answers the last tick with every cue outcome seen.
///
/// An unpinned 1/60 s frame commits two ticks over the 120 Hz loop, so the
/// composed entry's report for the first of them is overwritten before it can
/// be read; `OBJECTIVE1` wakes at tick 240, and the report of that very tick is
/// what carries the conversation.
fn pump_to_opening(app: &mut App) -> (u64, Vec<RadioLineOutcome>) {
    let timestep = app
        .world()
        .resource::<bevy::time::Time<bevy::time::Fixed>>()
        .timestep();
    let mut outcomes: Vec<RadioLineOutcome> = Vec::new();
    let mut tick = 0u64;
    let mut seen = 0u64;
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
            Err(error) => panic!("the composed step at tick {tick} refused: {error}"),
        }
        if tick >= 600
            || (tick >= 245
                && outcomes.iter().any(|outcome| {
                    matches!(outcome, RadioLineOutcome::Queued { group, .. } if group == OPENING_GROUP)
                }))
        {
            break;
        }
    }
    (tick, outcomes)
}

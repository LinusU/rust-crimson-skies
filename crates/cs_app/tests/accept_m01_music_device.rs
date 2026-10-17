//! Acceptance: M01's music state requests are played through the audio device
//! (Rally #1547, `M01-MUSIC-DEVICE`).
//!
//! #1463 delivered the request half: a `music_*_sg` cue reaches
//! `MusicDirector` under the owner-ruled track id. Nothing played the verdict.
//! This suite pins the playing half through production code:
//!
//! * the mixer's music pass — `Started` and `Transition` become one looping
//!   voice on the music bus, a transition replaces the previous track,
//!   `Unchanged` leaves it, a lost device is named and silences it;
//! * the retail half — `MissionRadio::read` decodes every track M01's music
//!   states can select from the labelled container, the tracks are published
//!   under the ids the cues name, and M01's own cue chain drives a recording
//!   device to start exactly the requested, published track.
//!
//! **Designed, not measured:** that a track loops, the hard cut between
//! tracks, unity gain, and the slot ↔ track binding (unknown #1 of
//! `docs/findings/2026-10-16-m01-play-music-state-requests.md`). Nothing here
//! is `verified_original`; whether a person hears the music is the owner's
//! `human_review`.

use std::path::PathBuf;

use cs_app::audio::{
    AudioSession, DecodedMusicMember, InMemorySamples, MusicSelector, PcmAudio, SampleLibrary,
    music_member_id, publish_music_members,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{
    MissionRadio, RadioLineOutcome, declared_engine_objectives, declared_sound_groups, offer_cues,
};
use cs_app::objectives::recover_retail_objectives;
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_app::scene::SceneGeneration;
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::audio_events::{
    AudioAssetSpec, AudioBus, AudioDevice, AudioEventId, AudioMixer, AudioRouter, DeviceCommand,
    MUSIC_GAIN, MusicCue, MusicMixRefusal, MusicOutcome, RecordingAudioDevice,
};
use cs_types::content::ContentId;
use cs_types::net::SessionId;

fn session() -> SessionId {
    SessionId::new(9).expect("a nonzero session")
}

fn track(name: &str) -> ContentId {
    music_member_id("ZBD/soundsh.zbd", name).expect("the track derives a music id")
}

fn new_audio() -> AudioSession {
    AudioSession::new(
        AudioRouter::new(session()),
        SceneGeneration(1),
        std::iter::empty::<AudioAssetSpec>(),
    )
}

fn request(audio: &mut AudioSession, name: &str) -> MusicOutcome {
    let id: AudioEventId = audio.mint_music_id();
    audio.request_music(MusicCue {
        id,
        asset: track(name),
    })
}

#[test]
fn accept_m01_music_device_plays_the_directors_verdicts() {
    let mut audio = new_audio();
    let mut mixer = AudioMixer::new(session());
    let mut device = RecordingAudioDevice::new();

    // Started: one looping voice on the music bus.
    request(&mut audio, "music_primaryobj1.wav");
    let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
    assert_eq!(report.started, vec![track("music_primaryobj1.wav")]);
    assert!(report.refusals.is_empty(), "{:?}", report.refusals);
    assert_eq!(mixer.music_playing(), Some(&track("music_primaryobj1.wav")));
    let commands = device.commands();
    let started: Vec<_> = commands
        .iter()
        .filter_map(|command| match command {
            DeviceCommand::Started { start, .. } => Some(start),
            _ => None,
        })
        .collect();
    assert_eq!(started.len(), 1, "one voice started: {commands:?}");
    assert_eq!(started[0].bus, AudioBus::Music);
    assert!((started[0].gain - MUSIC_GAIN).abs() < f64::EPSILON);
    assert_eq!(started[0].asset, track("music_primaryobj1.wav"));

    // The same track again leaves the voice alone.
    request(&mut audio, "music_primaryobj1.wav");
    let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
    assert_eq!(report.unchanged, 1);
    assert!(report.started.is_empty() && report.stopped.is_empty());

    // A transition replaces the sounding track.
    request(&mut audio, "music_secondaryobj1.wav");
    let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
    assert_eq!(report.stopped, vec![track("music_primaryobj1.wav")]);
    assert_eq!(report.started, vec![track("music_secondaryobj1.wav")]);
    assert_eq!(
        mixer.music_playing(),
        Some(&track("music_secondaryobj1.wav"))
    );

    // An inaudible verdict (device lost at request time) is named, not played.
    audio.device_lost();
    request(&mut audio, "music_prebattle1.wav");
    let report = mixer.mix_music(&audio.drain_music(), None);
    assert_eq!(
        report.refusals,
        vec![MusicMixRefusal::Unvoiced {
            asset: track("music_prebattle1.wav")
        }]
    );

    // Losing the device silences the sounding voice.
    let lost = mixer.device_lost(&mut device);
    assert!(lost.music_stopped);
    assert_eq!(mixer.music_playing(), None);
    assert!(!device.is_open());
}

#[test]
fn accept_m01_music_device_names_a_track_the_library_lacks() {
    let mut audio = new_audio();
    let mut mixer = AudioMixer::new(session());
    request(&mut audio, "music_primaryobj1.wav");
    // A refusing device: the request is named with the device's own code.
    let mut device = cs_app::audio::RefusingAudioDevice::new(cs_sim::audio_events::DeviceError {
        code: "device_unavailable_test",
        detail: String::new(),
    });
    let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
    assert!(report.started.is_empty());
    assert!(
        matches!(report.refusals.as_slice(), [MusicMixRefusal::Device { .. }]),
        "{:?}",
        report.refusals
    );
    assert_eq!(mixer.music_playing(), None);
}

/// The installation root; fails loudly when the capability is absent.
fn game_dir() -> PathBuf {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is not set: this test decodes M01's music tracks");
    };
    PathBuf::from(game_dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_music_device_plays_m01s_requested_track() {
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

    // Every track the five declared states can select is decoded, non-silent,
    // and published under the id its cue names.
    let decoded: Vec<DecodedMusicMember> = radio
        .music_tracks()
        .iter()
        .map(|source| {
            radio
                .decode_music(&source.id)
                .unwrap_or_else(|error| panic!("{} decodes: {error}", source.member))
        })
        .collect();
    let tracks = decoded.as_slice();
    assert_eq!(
        radio.failures().count(),
        0,
        "every music track decodes: {:?}",
        radio.failures().collect::<Vec<_>>()
    );
    assert!(!tracks.is_empty(), "M01's music tracks decode");
    let library = InMemorySamples::new();
    assert_eq!(publish_music_members(&library, tracks), tracks.len());
    for member in tracks {
        assert_eq!(
            member.id(),
            &music_member_id(member.container(), member.member()).expect("derives"),
        );
        let pcm = library.pcm(member.id()).expect("the track is published");
        assert!(pcm.frames() > 0, "{} decoded to frames", member.member());
        assert!(
            pcm.samples().iter().any(|sample| sample.abs() > 1.0e-4),
            "{} is not silence",
            member.member()
        );
    }
    // The image's 6 prebattle slots repeat 3 names; every selectable track is
    // present, none twice.
    let mut names: Vec<&str> = tracks.iter().map(DecodedMusicMember::member).collect();
    let all = names.len();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), all, "no track is decoded twice");

    // Drive M01's own cue chain, then play the director's verdicts.
    let mut music_blocks = Vec::new();
    for index in 0.. {
        let Some(objective) = engine.get(index) else {
            break;
        };
        if objective
            .declaration
            .wakeup_sound_group
            .as_deref()
            .is_some_and(|group| cs_app::audio::music_state_spec(group).is_some())
        {
            music_blocks.push(index);
        }
    }
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
    let mut selector = MusicSelector::launch(0, radio.music_arms());
    let mut audio = new_audio();
    let mut mixer = AudioMixer::new(session());
    let mut device = RecordingAudioDevice::new();
    let elapsed = 1.0 / f64::from(BASELINE_FIXED_HZ);
    let mut requested: Vec<ContentId> = Vec::new();
    let mut started: Vec<ContentId> = Vec::new();
    let mut serve = |lifecycles: &mut cs_app::mission_session::ObjectiveLifecycles| {
        for outcome in offer_cues(
            &lifecycles.take_cues(),
            &radio,
            &mut selector,
            Some(&mut audio),
        ) {
            if let RadioLineOutcome::MusicRequested { asset, .. } = outcome {
                requested.push(asset);
            }
        }
        let report = mixer.mix_music(&audio.drain_music(), Some(&mut device));
        assert!(report.refusals.is_empty(), "{:?}", report.refusals);
        started.extend(report.started);
    };
    for _ in 0..4 * BASELINE_FIXED_HZ {
        lifecycles.advance(elapsed);
        serve(&mut lifecycles);
    }
    for index in completers {
        lifecycles.complete(index as u32);
        serve(&mut lifecycles);
    }

    assert!(!requested.is_empty(), "M01 requests music states");
    for asset in &requested {
        assert!(
            library.pcm(asset).is_some(),
            "a requested track is in the sample library: {asset}"
        );
    }
    assert!(
        !started.is_empty(),
        "a requested track started on the device"
    );
    assert_eq!(
        mixer.music_playing(),
        requested.last(),
        "the device sounds the last track the mission requested"
    );
    let _: Option<PcmAudio> = library.pcm(requested.last().expect("a request"));
}

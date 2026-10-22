//! PARITY6-M01-AUDIO-TIMELINE.03 (#1863): the M01 launch cinematic's voices —
//! the `generic_intro` `SOUND` cues resolved through `sounds.zrd`, anchored to
//! the measured scene starts, fired on the cinematic's fixed-tick clock and
//! held for the member's decoded length, plus the measured
//! `snd_devastatorengine` `RANGE`/`VOLUME` law the mixer runs on marked
//! engine emitters.
//!
//! Task test prefix: `accept_parity6_m01_audio_03_`. Finding:
//! `docs/findings/2026-10-22-m01-audio-launch-voices.md`; the voice law is
//! `docs/findings/2026-10-22-m01-audio-timeline-voice-laws.md` (.02).
//!
//! The retail scenarios read the installation named by `CS_GAME_DIR` and fail
//! loudly without it. They exercise the production readers
//! (`measure_launch_sound`, `read_launch_cinematic`, `install_launch_audio`)
//! rather than re-deriving the data; the `generic_intro` walk below checks the
//! implementation's cue table against every `SOUND` item the member itself
//! carries, so the table cannot silently drift from the data.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use bevy::app::App;
use bevy::ecs::system::RunSystemOnce;
use bevy::ecs::world::World;
use cs_app::audio::{
    AudioOutput, CapabilityDeclaration, DeviceSampleLibrary, EngineGroupLaw, InMemorySamples,
    LaunchVoicePlan, LaunchVoices, PcmAudio, SceneCue, VoiceListenerPose, distance_gain,
    doppler_factor, drive_launch_voices, measured_mix, projected_position, projection_policy,
};
use cs_app::mission_session::{
    LAUNCH_SOUND_CONTAINER, LaunchCinematic, SoundArchive, install_launch_audio,
    measure_launch_sound, member_content_id, read_launch_cinematic, ticks_for,
};
use cs_app::physics::BASELINE_FIXED_HZ;
use cs_content::replay::{CapabilityClass, DeclaredCapabilities};
use cs_content::stunts::{ZrdValue, decode_zrd};
use cs_formats::io::ParseContext;
use cs_formats::zbd::{ZbdProbe, dispatch, read_reader_archive, read_version_one_index};
use cs_sim::audio_events::{
    AudioDevice, DeviceCommand, Listener, RecordingAudioDevice, spatialize,
};
use cs_types::content::ContentId;

/// The M01 player airframe record and scene root (campaign row 5, the
/// Devastator — `docs/findings/2026-10-18-parity-m01-lc-cinematic-timeline.md`).
const PLAYER_RECORD: &str = "pdevastator";
const PLAYER_ROOT: &str = "player_pfighter";

/// The reader archive holding `sounds.zrd` and `generic_intro.zrd`.
const READER_ARCHIVE: &str = "ZBD/zrdr.zbd";

fn game_dir() -> PathBuf {
    PathBuf::from(
        std::env::var_os("CS_GAME_DIR")
            .expect("CS_GAME_DIR must name the original installation for this test"),
    )
}

/// One member of a version-one reader archive, read through the same
/// production dispatch chain `measure_launch_sound` uses.
fn reader_member(root: &Path, member: &str) -> Vec<u8> {
    let bytes = std::fs::read(root.join(READER_ARCHIVE)).expect("zrdr.zbd readable");
    let spelling = cs_types::install::RelativePath::new(READER_ARCHIVE).expect("relative path");
    let mut context = ParseContext::with_defaults("parity6_audio_03");
    let decision = dispatch(ZbdProbe::new(READER_ARCHIVE, &spelling, &bytes)).expect("dispatch");
    let index = read_version_one_index(&mut context, decision, &bytes).expect("index");
    let table = index.member_table();
    let archive = read_reader_archive(&mut context, &table, index.data()).expect("archive");
    archive
        .entries()
        .find(|e| String::from_utf8_lossy(e.name()).eq_ignore_ascii_case(member))
        .map(|e| e.content().to_vec())
        .unwrap_or_else(|| panic!("{READER_ARCHIVE} has no member {member}"))
}

/// The measured launch sound for M01's player airframe, with the mission's
/// own sound-archive scope spelled the way `content.sound_archives` is.
fn launch_sound(root: &Path) -> cs_app::mission_session::LaunchSound {
    let bytes = std::fs::read(root.join(LAUNCH_SOUND_CONTAINER)).expect("soundsh.zbd readable");
    let archives = [SoundArchive {
        key: LAUNCH_SOUND_CONTAINER.to_owned(),
        sha256: cs_assets::install::sha256(&bytes).to_hex(),
    }];
    measure_launch_sound(root, PLAYER_RECORD, &archives).expect("the launch sound measures")
}

// ------------------------------------------------------------- the driver --

/// A world carrying a launch-voice plan, a cinematic phase and an open
/// recording device — the three resources `drive_launch_voices` reads. The
/// second handle is returned so the test sees what the device was asked.
fn drive_world(plan: LaunchVoicePlan, total_ticks: u64) -> (World, RecordingAudioDevice) {
    let mut world = World::new();
    world.insert_resource(plan);
    world.insert_resource(LaunchCinematic::new(total_ticks).expect("a non-zero phase"));
    let mut device = RecordingAudioDevice::new();
    device.open().expect("the device opens");
    let handle = device.handle();
    world.insert_resource(AudioOutput::new(Box::new(device)));
    (world, handle)
}

/// Runs one fixed tick the way the schedule does: the phase advances, then
/// the launch-voice driver walks its cue list on the new clock
/// (`plugin.rs` schedules `drive_launch_voices` after `advance_phase` in
/// `FixedLast`).
fn tick(world: &mut World) {
    world.resource_mut::<LaunchCinematic>().complete_tick();
    world
        .run_system_once(drive_launch_voices)
        .expect("the driver runs");
}

/// The members a recording device was asked to one-shot, in order.
fn one_shots(device: &RecordingAudioDevice) -> Vec<ContentId> {
    device
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            DeviceCommand::OneShotStarted { start, .. } => Some(start.asset),
            _ => None,
        })
        .collect()
}

/// **A cue fires on the first update whose cinematic clock reaches its
/// anchor, the device one-shot is held for the member's decoded length, and
/// an already-fired group never re-fires.**
#[test]
fn accept_parity6_m01_audio_03_a_cue_fires_on_its_anchor_and_is_held() {
    let member1 = member_content_id(LAUNCH_SOUND_CONTAINER, "introscene1.wav")
        .expect("the member id derives");
    let member3 = member_content_id(LAUNCH_SOUND_CONTAINER, "introscene3.wav")
        .expect("the member id derives");
    // The measured anchors: scene 1 at the cinematic's start, scene 3 at the
    // gi_1stperson start (28.3 s of the 40.15 s phase, 120 Hz fixed ticks).
    let scene3_tick = ticks_for(28.3, BASELINE_FIXED_HZ);
    let plan = LaunchVoicePlan {
        cues: vec![
            SceneCue {
                group: "snd_scene1".to_owned(),
                member: member1.clone(),
                at_tick: 0,
            },
            SceneCue {
                group: "snd_scene3".to_owned(),
                member: member3.clone(),
                at_tick: scene3_tick,
            },
        ],
        refusals: vec!["snd_droplaunch: sounds.zrd declares no group record".to_owned()],
    };
    let (mut world, device) = drive_world(plan, ticks_for(40.15, BASELINE_FIXED_HZ));
    // The device library the load's population pass would have filled:
    // member1 is one second of 11,025 Hz mono (120 ticks) and member3 two
    // (240 ticks) — the held record's life is exactly the decoded length.
    let mut samples = InMemorySamples::new();
    samples.insert(
        member1.clone(),
        PcmAudio::try_new(1, 11_025, vec![0.0; 11_025]).expect("pcm"),
    );
    samples.insert(
        member3.clone(),
        PcmAudio::try_new(1, 11_025, vec![0.0; 22_050]).expect("pcm"),
    );
    world.insert_resource(DeviceSampleLibrary::new(Arc::new(samples)));

    // Tick 0: scene 1's cue is anchored at the phase's first update.
    world
        .run_system_once(drive_launch_voices)
        .expect("the driver runs");
    {
        let voices = world.resource::<LaunchVoices>();
        assert_eq!(voices.held.len(), 1, "{:?}", voices.held);
        assert_eq!(voices.held[0].group, "snd_scene1");
        assert_eq!(voices.held[0].member, member1);
        // 120 ticks of life, minus the firing call's own countdown.
        assert_eq!(voices.held[0].remaining_ticks, 119);
        assert!(voices.refusals.is_empty(), "{:?}", voices.refusals);
    }
    assert_eq!(one_shots(&device), vec![member1.clone()]);

    // The second cue waits for its anchor: scene 3 cannot fire early.
    for _ in 0..scene3_tick {
        tick(&mut world);
    }
    {
        let voices = world.resource::<LaunchVoices>();
        assert_eq!(
            voices.held.len(),
            1,
            "scene 1's one-second voice has run down: {:?}",
            voices.held
        );
        assert_eq!(voices.held[0].group, "snd_scene3");
        assert_eq!(voices.held[0].remaining_ticks, 239);
        assert_eq!(
            voices.fired,
            vec!["snd_scene1".to_owned(), "snd_scene3".to_owned()]
        );
    }
    assert_eq!(one_shots(&device), vec![member1, member3.clone()]);

    // The dedup record keeps a fired group from re-firing, and a refusal the
    // plan could not bind is carried, never played.
    tick(&mut world);
    let started = one_shots(&device);
    assert_eq!(started.len(), 2, "no cue fires twice: {started:?}");
    let plan = world.resource::<LaunchVoicePlan>();
    assert!(
        plan.refusals
            .iter()
            .any(|r| r.starts_with("snd_droplaunch")),
        "the unbound cue stays named: {:?}",
        plan.refusals
    );
}

/// **A world with no `LaunchCinematic` phase — a capture scenario or a
/// no-teleport smoke, which never runs the intro — fires nothing, even a cue
/// anchored at tick 0.**
#[test]
fn accept_parity6_m01_audio_03_no_cinematic_phase_no_launch_voices() {
    let member = member_content_id(LAUNCH_SOUND_CONTAINER, "introscene1.wav")
        .expect("the member id derives");
    let plan = LaunchVoicePlan {
        cues: vec![SceneCue {
            group: "snd_scene1".to_owned(),
            member: member.clone(),
            at_tick: 0,
        }],
        refusals: Vec::new(),
    };
    let mut world = World::new();
    world.insert_resource(plan);
    let mut device = RecordingAudioDevice::new();
    device.open().expect("the device opens");
    let handle = device.handle();
    world.insert_resource(AudioOutput::new(Box::new(device)));
    world
        .run_system_once(drive_launch_voices)
        .expect("the driver runs");
    assert!(
        one_shots(&handle).is_empty(),
        "no cinematic clock, no cinematic voices"
    );
    assert!(
        !world.contains_resource::<LaunchVoices>(),
        "nothing was even attempted"
    );
}

// -------------------------------------------------------- the measured law --

/// **`EngineGroupLaw::try_new` refuses a non-finite or mis-ordered field, so
/// a malformed group record can never install a law.**
#[test]
fn accept_parity6_m01_audio_03_an_engine_group_law_is_validated() {
    assert!(EngineGroupLaw::try_new(0.0, 1020.0, 0.8).is_err());
    assert!(EngineGroupLaw::try_new(1020.0, 130.0, 0.8).is_err());
    assert!(EngineGroupLaw::try_new(130.0, 1020.0, 0.0).is_err());
    assert!(EngineGroupLaw::try_new(130.0, f64::INFINITY, 0.8).is_err());
    assert!(EngineGroupLaw::try_new(130.0, 1020.0, f64::NAN).is_err());
}

/// **The `.02` distance law reproduces its measured table exactly** on the
/// `snd_devastatorengine` `RANGE (130, 1020)`: the flat zone inside
/// `dmin + x` (`x = (dmax − dmin)/8 = 111.25`), the `x/y` rolloff to `dmax`,
/// the `2^(−3 − 7·t)` fade across `dmax .. 1.1·dmax` and the `−10000` mute
/// floor (`2^−10`) beyond — the scale is the engine's `10·log2` dB, so a
/// linear gain is `2^(v/1000)`.
#[test]
fn accept_parity6_m01_audio_03_the_measured_distance_law_matches_the_image() {
    let law = EngineGroupLaw::try_new(130.0, 1020.0, 0.8).expect("the measured group is valid");
    // The flat zone: dmin itself and the knee at 130 + 111.25.
    assert_eq!(distance_gain(&law, 0.0), 1.0);
    assert_eq!(distance_gain(&law, 130.0), 1.0);
    assert_eq!(distance_gain(&law, 241.25), 1.0);
    // The rolloff: y = 2x at 352.5 → x/y = 1/2; y = 8x at dmax → 1/8, where
    // the fade picks up (the pieces join there, per the image's own check).
    assert!((distance_gain(&law, 352.5) - 0.5).abs() < 1e-12);
    assert!((distance_gain(&law, 1020.0) - 0.125).abs() < 1e-12);
    // The fade: halfway across (d = 1071, t = 0.5) → 2^(−6.5); just short
    // of the mute edge the fade nearly reaches its floor.
    assert!((distance_gain(&law, 1071.0) - (-6.5f64).exp2()).abs() < 1e-12);
    assert!((distance_gain(&law, 1121.9) - (-9.993f64).exp2()).abs() < 1e-3);
    // The mute at and past 1.1·dmax = 1122: −10000 is 2^−10, not a stopped
    // voice — the floor the fade ends on, held forever after.
    assert_eq!(distance_gain(&law, 1122.0), (-10.0f64).exp2());
    assert_eq!(distance_gain(&law, 5000.0), (-10.0f64).exp2());
}

/// **The `.02` pan and Doppler terms are the measured ones**: the bearing pan
/// is `dot(src − listener, right)/dist · 0.16` (the image's `· 1600`
/// centi-dB scaled onto the device's ±1) and the Doppler factor is
/// `1 − dot(v_rel, p_rel)/dist/345`.
#[test]
fn accept_parity6_m01_audio_03_the_measured_pan_and_doppler_match_the_image() {
    let pose = VoiceListenerPose {
        position: [0.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0],
        velocity: [0.0, 0.0, 0.0],
    };
    let law = EngineGroupLaw::try_new(130.0, 1020.0, 0.8).expect("the measured group is valid");
    // An emitter 500 m out with 0.8 of its bearing on the right axis,
    // closing at 34.5 m/s along that axis.
    let mix = measured_mix(&law, [400.0, 300.0, 0.0], [-34.5, 0.0, 0.0], &pose);
    // gain at d = 500: y = 370, x = 111.25 → x/y inside the rolloff.
    assert!((mix.gain - 111.25 / 370.0).abs() < 1e-12, "{mix:?}");
    // pan: (400/500) · 0.16 = 0.128.
    assert!((mix.pan - 0.128).abs() < 1e-12, "{mix:?}");
    // doppler: 1 − (−34.5 · 400)/500/345 = 1.08 — closing raises the rate.
    assert!((mix.doppler - 1.08).abs() < 1e-12, "{mix:?}");
    // A coincident emitter has no pan and no factor, never a NaN.
    let here = measured_mix(&law, [0.0, 0.0, 0.0], [0.0, 0.0, 0.0], &pose);
    assert_eq!(here.pan, 0.0);
    assert_eq!(here.doppler, 1.0);
    // The standalone Doppler law agrees on the same geometry.
    assert!((doppler_factor([400.0, 300.0, 0.0], [-34.5, 0.0, 0.0]) - 1.08).abs() < 1e-12);
}

/// **The projected position carries the measured mix through the production
/// `spatialize` unchanged** — `mix_session` hands the device exactly the
/// `.02` distance gain and bearing pan, at any true distance and bearing,
/// including beyond the mute edge.
#[test]
fn accept_parity6_m01_audio_03_the_projection_delivers_the_measured_values() {
    let law = EngineGroupLaw::try_new(130.0, 1020.0, 0.8).expect("the measured group is valid");
    let pose = VoiceListenerPose {
        position: [0.0, 0.0, 0.0],
        right: [1.0, 0.0, 0.0],
        velocity: [0.0, 0.0, 0.0],
    };
    let listener = Listener::try_new(pose.position, pose.right).expect("a valid listener");
    let policy = projection_policy();
    for emitter in [
        [400.0, 300.0, 0.0],  // 500 m, right-forward: rolloff + pan
        [241.25, 0.0, 0.0],   // the flat-zone knee: gain 1, full 0.16 pan
        [-800.0, 0.0, 200.0], // left of the listener, negative pan
        [0.0, 0.0, 5000.0],   // past the mute edge, dead ahead: the floor
    ] {
        let mix = measured_mix(&law, emitter, [0.0; 3], &pose);
        let placed = projected_position(&pose, emitter, &mix);
        let spatial = spatialize(&policy, &listener, placed)
            .unwrap_or_else(|error| panic!("{emitter:?} fails to place: {error}"));
        assert!(
            (spatial.gain - mix.gain).abs() < 1e-9,
            "{emitter:?}: spatialize gain {} != measured {}",
            spatial.gain,
            mix.gain
        );
        assert!(
            (spatial.pan - mix.pan).abs() < 1e-9,
            "{emitter:?}: spatialize pan {} != measured {}",
            spatial.pan,
            mix.pan
        );
    }
}

// ------------------------------------------------------------- the data ----

/// **`measure_launch_sound` resolves every scene `SOUND` cue that has a
/// `sounds.zrd` group and refuses the one that has none — and it reads the
/// engine group's own `RANGE`/`VOLUME` for the measured mix law.**
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_measure_launch_sound_resolves_the_scene_groups() {
    let root = game_dir();
    let launch = launch_sound(&root);
    assert_eq!(launch.group, "snd_devastatorengine");
    assert_eq!(launch.member, "devastator.wav");
    assert_eq!(
        launch.engine_law,
        Some(EngineGroupLaw {
            range_min: 130.0,
            range_max: 1020.0,
            // The member stores VOLUME as an f32; this is that exact value
            // widened, not the f64 literal 0.8.
            volume: f64::from(0.8f32),
        }),
        "the .02 desc terms on the engine group itself"
    );
    // The resolved cues, in cinematic order.
    let resolved: Vec<(&str, &str, &str)> = launch
        .scene_sounds
        .iter()
        .map(|s| (s.group.as_str(), s.member.as_str(), s.animation.as_str()))
        .collect();
    assert_eq!(
        resolved,
        [
            ("snd_scene1", "introscene1.wav", "gi_scene1"),
            ("snd_scene2", "introscene2.wav", "gi_scene2"),
            ("snd_scene3", "introscene3.wav", "gi_1stperson"),
            ("snd_scene4", "introscene4.wav", "gi_playerdrop"),
        ]
    );
    for scene in &launch.scene_sounds {
        assert_eq!(
            scene.content,
            member_content_id(LAUNCH_SOUND_CONTAINER, &scene.member)
                .expect("the member id derives"),
            "{} names its member in {LAUNCH_SOUND_CONTAINER}",
            scene.group
        );
    }
    assert_eq!(
        launch.scene_refusals.len(),
        1,
        "{:?}",
        launch.scene_refusals
    );
    assert!(
        launch.scene_refusals[0].starts_with("snd_droplaunch"),
        "the unbound cue is refused by name: {:?}",
        launch.scene_refusals[0]
    );

    // Each resolved member is declared in the load's catalog — an undeclared
    // member is never decoded into the device library (the population pass
    // decodes only what the catalog names), so this is what lets a cue's
    // `start_one_shot` find its sample. The groups carry no `LOOPED` flag:
    // each plays once.
    let catalog = launch.catalog().expect("the catalog lowers");
    for scene in &launch.scene_sounds {
        let record = catalog
            .get(&scene.content)
            .unwrap_or_else(|| panic!("{} is not declared", scene.member));
        assert_eq!(
            record.playback().mode.clone().known(),
            Some(cs_content::audio::PlaybackMode::OneShot),
            "{}: the group has no LOOPED flag",
            scene.group
        );
        assert_eq!(
            record.playback().bus.clone().known(),
            Some(cs_content::audio::AudioBus::Environment),
            "{}: the one-shot's designed bus matches the driver's VoiceStart",
            scene.group
        );
    }
}

/// **`generic_intro.zrd` carries exactly the `SOUND` cues the table covers —
/// every cue the data declares is resolved or refused by name, and the table
/// claims nothing the data does not carry.** This is the drift guard on
/// `SCENE_SOUND_CUES`.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_generic_intro_declares_exactly_the_covered_cues() {
    let root = game_dir();
    let launch = launch_sound(&root);
    let tree =
        decode_zrd(&reader_member(&root, "generic_intro.zrd")).expect("generic_intro.zrd decodes");
    let mut data: BTreeSet<(String, String)> = BTreeSet::new();
    collect_sound_cues(&tree, &mut data);
    assert_eq!(
        data,
        [
            ("gi_scene1", "snd_scene1"),
            ("gi_scene2", "snd_scene2"),
            ("gi_1stperson", "snd_droplaunch"),
            ("gi_1stperson", "snd_scene3"),
            ("gi_playerdrop", "snd_scene4"),
        ]
        .into_iter()
        .map(|(a, s)| (a.to_owned(), s.to_owned()))
        .collect(),
        "the launch cinematic's whole SOUND cue set"
    );
    let resolved: BTreeSet<(String, String)> = launch
        .scene_sounds
        .iter()
        .map(|s| (s.animation.clone(), s.group.clone()))
        .collect();
    let refused: BTreeSet<String> = launch
        .scene_refusals
        .iter()
        .filter_map(|r| r.split(':').next().map(str::to_owned))
        .collect();
    for (animation, sound) in &data {
        assert!(
            resolved.contains(&(animation.clone(), sound.clone())) || refused.contains(sound),
            "the data's cue {sound} in {animation} is neither resolved nor refused"
        );
    }
    for pair in &resolved {
        assert!(
            data.contains(pair),
            "the cue table resolves {pair:?}, which the data does not carry"
        );
    }
    for name in &refused {
        assert!(
            data.iter().any(|(_, sound)| sound == name),
            "the refusal {name} is not a cue the data carries"
        );
    }
}

/// **The plan anchors each resolved cue at the start tick of the scene
/// animation whose sequence carries it, on the cinematic's own clock** — the
/// .02 wait law, not a guessed wall clock.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_parity6_m01_audio_03_the_plan_anchors_cues_to_the_scene_starts() {
    let root = game_dir();
    let launch = launch_sound(&root);
    let length = read_launch_cinematic(&root, PLAYER_ROOT)
        .expect("M01's launch cinematic measures for the player root");
    assert!((length.seconds - 40.15).abs() < 0.05, "{length:?}");
    let start_of = |animation: &str| -> f64 {
        length
            .scenes
            .iter()
            .find(|span| span.animation == animation)
            .unwrap_or_else(|| panic!("the timeline has no scene {animation}"))
            .started_at
    };
    // The measured scene starts (finding table).
    assert!(start_of("gi_scene1").abs() < 1e-9);
    assert!((start_of("gi_scene2") - 15.9).abs() < 0.05);
    assert!((start_of("gi_1stperson") - 28.3).abs() < 0.05);
    assert!((start_of("gi_playerdrop") - 38.2).abs() < 0.05);

    let mut app = App::new();
    install_launch_audio(
        &mut app,
        &launch,
        &[],
        [0.0; 3],
        &CapabilityDeclaration {
            declared: DeclaredCapabilities::of([CapabilityClass::Retail]),
            malformed: None,
        },
        Some(&length),
    )
    .expect("the launch audio installs");
    let plan = app.world().resource::<LaunchVoicePlan>();
    assert_eq!(
        plan.cues
            .iter()
            .map(|cue| cue.group.as_str())
            .collect::<Vec<_>>(),
        ["snd_scene1", "snd_scene2", "snd_scene3", "snd_scene4"],
        "one cue per resolved scene sound, in cinematic order"
    );
    for cue in &plan.cues {
        let scene = launch
            .scene_sounds
            .iter()
            .find(|s| s.group == cue.group)
            .expect("the cue names a resolved group");
        assert_eq!(
            cue.at_tick,
            ticks_for(start_of(&scene.animation), BASELINE_FIXED_HZ),
            "{} anchors at the {} scene start tick",
            cue.group,
            scene.animation
        );
        assert_eq!(cue.member, scene.content);
    }
    assert_eq!(plan.refusals.len(), 1, "{:?}", plan.refusals);
    assert!(plan.refusals[0].starts_with("snd_droplaunch"));
}

/// The first text in `node`, looking through single-element lists.
fn first_text(node: &ZrdValue) -> Option<String> {
    match node {
        ZrdValue::Text(text) => Some(text.clone()),
        ZrdValue::List(children) => children.first().and_then(first_text),
        _ => None,
    }
}

/// The first text of the value following `key` in a flat `.zrd` field list.
fn field_text(fields: &[ZrdValue], key: &str) -> Option<String> {
    fields
        .iter()
        .position(|child| child.as_text() == Some(key))
        .and_then(|index| fields.get(index + 1))
        .and_then(first_text)
}

/// Every `(animation, sound)` pair `generic_intro.zrd` declares.
///
/// The member's `ANIMATION_LIST` alternates an `ANIMATION_DEFINITION` key
/// with the record's flat field list; inside a record, each
/// `SEQUENCE_DEFINITION`'s `SOUND` item carries `NAME`. The walk is exactly
/// the tree's own shape — nothing is assumed about which scenes carry cues.
fn collect_sound_cues(node: &ZrdValue, out: &mut BTreeSet<(String, String)>) {
    let ZrdValue::List(children) = node else {
        return;
    };
    for (index, child) in children.iter().enumerate() {
        if child.as_text() != Some("ANIMATION_DEFINITION") {
            continue;
        }
        let Some(ZrdValue::List(fields)) = children.get(index + 1) else {
            continue;
        };
        let Some(animation) = field_text(fields, "ANIMATION_NAME") else {
            continue;
        };
        for (at, key) in fields.iter().enumerate() {
            if key.as_text() != Some("SEQUENCE_DEFINITION") {
                continue;
            }
            let Some(ZrdValue::List(sequence)) = fields.get(at + 1) else {
                continue;
            };
            for (pos, item) in sequence.iter().enumerate() {
                if item.as_text() != Some("SOUND") {
                    continue;
                }
                let Some(ZrdValue::List(sound)) = sequence.get(pos + 1) else {
                    continue;
                };
                if let Some(name) = field_text(sound, "NAME") {
                    out.insert((animation.clone(), name));
                }
            }
        }
    }
    for child in children {
        collect_sound_cues(child, out);
    }
}

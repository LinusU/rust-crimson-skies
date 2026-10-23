//! Acceptance stage VS-M01-RT-EVIDENCE-B: M01's original engine loop reaches
//! the `AudioDevice` backend in the launch, and the device's playback is
//! recorded (Rally #1313).
//!
//! The launch's one legitimate sound is the player airframe's engine loop,
//! measured in `docs/findings/2026-10-11-vs-m01-rt-evidence-audio-binding.md`.
//! The synthetic members prove the refusals (a machine without the `audio`
//! capability is a named exit-4 failure, a staged container must match the
//! installation's digest, the teardown stops what the launch started). The
//! retail member needs the installation and the `audio` capability, so it is
//! `#[ignore]` and runs under `--include-ignored`; it writes the playback
//! record under `$CS_EVIDENCE_DIR` or `private/evidence/VS-M01-RT-EVIDENCE-B/`.
//! The record is the **source boundary** (frames pulled by the output stream),
//! never a capture of a speaker: that is the owner's `human_review`.

use std::path::PathBuf;
use std::time::{Duration, Instant};

use bevy::app::App;
use cs_app::audio::{
    AudioBackendKind, AudioBackendLog, AudioBindLog, AudioEmitterRole, AudioHandoffLog,
    AudioMixReport, AudioSession, CODE_CAPABILITY_ABSENT, CapabilityDeclaration,
    EXIT_MISSING_CAPABILITY, SampleLibrary, audibility_exit_code,
};
use cs_app::mission_launch::{MissionLaunchError, plan_mission_launch};
use cs_app::mission_session::{
    LAUNCH_SOUND_CONTAINER, LaunchAudioProbe, LaunchSound, MissionCompositionError, build_headless,
    member_content_id, stage_for, teardown,
};
use cs_content::replay::{CapabilityClass, DeclaredCapabilities};
use cs_types::evidence::ContentHash;

use crate::campaign::common::label;
use crate::campaign::vs_m01_rt_window::{Scratch, game_dir};

fn launch_sound(host: PathBuf, bytes: u64, sha256: ContentHash) -> LaunchSound {
    LaunchSound {
        airframe: "pdevastator".to_owned(),
        group: "snd_devastatorengine".to_owned(),
        member: "devastator.wav".to_owned(),
        container: LAUNCH_SOUND_CONTAINER.to_owned(),
        container_sha256: sha256,
        host,
        content: member_content_id(LAUNCH_SOUND_CONTAINER, "devastator.wav")
            .expect("the member id is valid"),
        bytes,
        engine_law: None,
        engine_rate: None,
        scene_sounds: Vec::new(),
        scene_refusals: Vec::new(),
    }
}

/// **A machine that does not declare `audio` is refused by name, with the
/// missing-capability exit status — never a silent mute.**
#[test]
fn accept_vs_m01_runtime_evidence_audio_a_machine_without_audio_is_refused_with_exit_4() {
    let launch = launch_sound(PathBuf::from("unused"), 1, ContentHash::from_bytes([1; 32]));
    let mut app = App::new();
    cs_app::mission_session::install_launch_audio(
        &mut app,
        &launch,
        &[],
        [0.0; 3],
        &CapabilityDeclaration {
            declared: DeclaredCapabilities::of([CapabilityClass::Retail]),
            malformed: None,
        },
        None,
    )
    .expect("the audio stack installs");
    let backend = app.world().resource::<AudioBackendLog>();
    assert_eq!(backend.kind, AudioBackendKind::Refused, "{backend:?}");
    let refusal = cs_app::mission_session::backend_refusal(&app).expect("the refusal is named");
    assert_eq!(refusal.code, CODE_CAPABILITY_ABSENT, "{refusal}");
    assert_eq!(audibility_exit_code(&refusal), EXIT_MISSING_CAPABILITY);
    assert_eq!(EXIT_MISSING_CAPABILITY, 4);
    // The CLI maps the composition's audio refusal to that same status.
    let launch_error = MissionLaunchError::Composition(MissionCompositionError::Audio(refusal));
    assert_eq!(launch_error.exit_code(), 4);
    assert!(
        launch_error.to_string().contains("audio"),
        "the refusal names the capability: {launch_error}"
    );
}

/// **The staged container must be the installation's bytes.**
#[test]
fn accept_vs_m01_runtime_evidence_audio_the_staged_container_is_checked_against_the_digest() {
    let scratch = Scratch::new("audio_stage");
    let host = scratch.path().join("soundsh.zbd");
    std::fs::write(&host, b"synthetic-sound-container").expect("the host file is written");
    let digest = cs_assets::install::sha256(b"synthetic-sound-container");
    let good = launch_sound(host.clone(), 25, digest);
    let root = cs_app::mission_session::stage_container(&scratch.path().join("cache"), &good)
        .expect("a matching container stages");
    assert_eq!(
        std::fs::read(root.join(LAUNCH_SOUND_CONTAINER)).expect("staged"),
        b"synthetic-sound-container"
    );
    let bad = launch_sound(host, 25, ContentHash::from_bytes([9; 32]));
    let error = cs_app::mission_session::stage_container(&scratch.path().join("cache2"), &bad)
        .expect_err("a container with another digest is refused");
    assert!(error.to_string().contains("digest"), "{error}");
}

/// The SHA-256 of the member's bytes inside the staged container, read
/// through the same content-session and `ZbdContainer` path the load uses.
fn member_digest(cache_root: &std::path::Path, launch: &LaunchSound) -> String {
    let root = cs_app::mission_session::stage_container(cache_root, launch).expect("staged");
    let context = cs_types::asset_id::ResolveContext::new(launch.container_sha256);
    let mut builder = cs_assets::vfs::SessionBuilder::new(context);
    let mount = cs_assets::vfs::MountBuilder::new(
        cs_types::asset_id::MountId::new("digest").expect("id"),
        cs_types::asset_id::MountNamespace::new("install").expect("namespace"),
        cs_types::asset_id::PrecedenceClass::Shared,
        ".",
    )
    .retail();
    builder
        .mount_directory(mount, &root)
        .expect("the staged file mounts");
    let session = builder.open();
    let key = cs_types::asset_id::AssetKey::from_spelling("install", &launch.container, "default")
        .expect("key");
    let container = cs_assets::zbd::ZbdContainer::open(&session, &key).expect("container opens");
    let mut parse = cs_formats::ParseContext::with_defaults(container.label());
    let index = container.index(&mut parse).expect("index");
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .expect("sound assets");
    let member = assets
        .entries()
        .iter()
        .find(|asset| String::from_utf8_lossy(asset.name()).eq_ignore_ascii_case(&launch.member))
        .expect("the member is in the container");
    cs_assets::install::sha256(member.content()).to_hex()
}

fn wait_for(limit: Duration, app: &mut App, done: impl Fn(&App) -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        app.update();
        if done(app) {
            return true;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    done(app)
}

/// **M01's engine loop is decoded from the original archive, started by the
/// composition, pulled by the real output stream, and stopped at teardown.**
#[test]
#[ignore = "requires CS_GAME_DIR, CS_ENGINE_IMAGE and the audio capability"]
fn accept_vs_m01_runtime_evidence_audio_the_launch_plays_the_engine_loop_through_the_device() {
    let root = game_dir();
    assert!(
        CapabilityDeclaration::from_environment().contains(CapabilityClass::Audio),
        "this scenario needs the audio capability in $CS_CAPABILITIES"
    );
    let plan = plan_mission_launch(&root, label("M06"), "The Red Menace")
        .expect("M01's launch closure plans");
    let stage = stage_for(&root, &plan).expect("M01's stage reads");
    drop(plan);
    let launch = stage
        .launch_sound
        .clone()
        .expect("the stage measured a launch sound");
    assert_eq!(launch.member, "devastator.wav");
    assert_eq!(launch.group, "snd_devastatorengine");

    let mut app = build_headless(&stage).expect("M01 composes headlessly");
    let backend = app.world().resource::<AudioBackendLog>().clone();
    assert!(
        backend.is_audible(),
        "the device must be the audible one: {backend:?}"
    );

    let played = wait_for(Duration::from_secs(10), &mut app, |app| {
        app.world().resource::<LaunchAudioProbe>().probe.played()
    });
    let handoff = app.world().resource::<AudioHandoffLog>().clone();
    let install = handoff
        .installed
        .clone()
        .expect("the load installed an audio session");
    // The population pass decodes every audio member the stage delivers
    // through the launch's staged container: the engine member, the player's
    // selected gun member beside it since #1628's measured loadout, the flak
    // shot and burst members, and the launch cinematic's resolved `SOUND`
    // members since #1863. The count is the stage's delivered set, not a
    // fixed number.
    let delivered_audio = 1
        + usize::from(stage.gun_sound.is_some())
        + stage.flak_sound.as_ref().map_or(0, |flak| {
            1 + usize::from(flak.shot.content != flak.burst.content)
        })
        + launch.scene_sounds.len();
    assert_eq!(
        install.samples, delivered_audio,
        "every delivered audio member was decoded into the library: {handoff:?}"
    );
    assert!(played, "the output stream pulled nothing: {handoff:?}");

    let probe = app.world().resource::<LaunchAudioProbe>().clone();
    let pcm = probe
        .library
        .pcm(&launch.content)
        .expect("the library holds the member");
    if let Some(gun) = &stage.gun_sound {
        assert!(
            probe.library.pcm(&gun.content).is_some(),
            "the delivered gun member was not decoded into the library"
        );
    }
    let bound = app
        .world()
        .resource::<AudioSession>()
        .router
        .active_loop_count();
    // Every aircraft in M01 (the player and each wingman and enemy) carries its
    // own emitter playing the same engine loop, so the count is the number of
    // engine emitters the launch bound (nine in M01), not one. Measured on
    // 2026-10-18; every loop must be the launch's engine asset on the engine bus.
    let session = app.world().resource::<AudioSession>();
    assert!(
        bound >= 1,
        "the launch binds at least the player's engine loop"
    );
    for binding in session.router.active_loops() {
        assert_eq!(
            binding.asset, launch.content,
            "only the engine loop is bound"
        );
        assert_eq!(binding.bus, cs_sim::audio_events::AudioBus::Engine);
    }
    let bind_log = app.world().resource::<AudioBindLog>();
    assert!(bind_log.refusals.is_empty(), "{bind_log:?}");
    let engine_emitters = bind_log
        .bound
        .iter()
        .filter(|record| record.role == AudioEmitterRole::Engine)
        .count();
    assert_eq!(
        bind_log.bound.len(),
        engine_emitters,
        "no other role is bound"
    );
    assert_eq!(
        bound, engine_emitters,
        "one running loop per engine emitter"
    );
    let mixed = app.world().resource::<AudioMixReport>().clone();
    assert!(
        mixed.passes > 0 && mixed.last.refusals.is_empty(),
        "{mixed:?}"
    );
    let pulled = probe.probe.pulled();
    let peak = probe.probe.peak();
    assert!(peak > 0.0, "the pulled samples are not silence");

    // The teardown stops the loop the launch started.
    teardown(&mut app);
    assert_eq!(
        app.world()
            .resource::<AudioSession>()
            .router
            .active_loop_count(),
        0
    );
    assert!(
        !app.world()
            .resource::<AudioMixReport>()
            .last
            .stopped
            .is_empty()
    );

    let member_sha256 = member_digest(&stage.cache_root, &launch);
    let dir = std::env::var("CS_EVIDENCE_DIR").map_or_else(
        |_| {
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("../../private/evidence/VS-M01-RT-EVIDENCE-B")
        },
        PathBuf::from,
    );
    std::fs::create_dir_all(&dir).expect("the evidence directory is created");
    let record = format!(
        "{{\n  \"task\": \"VS-M01-RT-EVIDENCE-B\",\n  \"boundary\": \"sample_probe_source_pull\",\n  \
         \"asset\": \"{asset}\",\n  \"airframe_record\": \"{airframe}\",\n  \"sound_group\": \"{group}\",\n  \
         \"archive\": \"{archive}\",\n  \"archive_sha256\": \"{sha}\",\n  \"member\": \"{member}\",\n  \"member_sha256\": \"{member_sha}\",\n  \
         \"container_choice\": \"labelled choice: which sound archive the original opens is unmeasured\",\n  \
         \"channels\": {channels},\n  \"rate_hz\": {rate},\n  \"member_frames\": {frames},\n  \
         \"member_duration_s\": {duration:.4},\n  \"sample_values_pulled\": {pulled},\n  \
         \"frames_pulled\": {frames_pulled},\n  \"peak\": {peak:.4},\n  \"backend\": \"audible\",\n  \
         \"verified_original\": false,\n  \"human_review\": false\n}}\n",
        asset = launch.content,
        airframe = launch.airframe,
        group = launch.group,
        archive = launch.container,
        sha = launch.container_sha256.to_hex(),
        member = launch.member,
        member_sha = member_sha256,
        channels = pcm.channels(),
        rate = pcm.rate_hz(),
        frames = pcm.frames(),
        duration = pcm.duration().as_secs_f64(),
        frames_pulled = pulled / u64::from(pcm.channels()),
    );
    std::fs::write(dir.join("playback.json"), record).expect("the playback record is written");
}

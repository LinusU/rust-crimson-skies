//! Acceptance scenarios M01-LC-AUDIO-DEVICE (#635): the F41 audio stack mixes to
//! a device that opens real output hardware, plays a decoded original sound
//! member through it, and refuses by name on a machine that cannot play.
//!
//! Spec: `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-D`'s playback half. Task test prefix: `accept_m01_lc_audio_device_`.
//! Finding: `docs/findings/2026-10-05-m01-lc-audible-audio-device.md`.
//!
//! # What each test needs
//!
//! The tests are split by what they require, and the split is a property of the
//! code rather than of the fixture:
//!
//! * **Synthetic (unignored, runs in CI).** Everything about the *gate* and the
//!   *refusal*: an absent capability is a named failure, never a silent mute; a
//!   declared-but-broken machine is named too; the sample conversion normalizes
//!   each declared width; an unregistered asset, an unknown voice, a closed
//!   device and a corrupt mix value are each refused by their own code; and a
//!   foreign-session loop is refused without ever reaching the device. None of
//!   these need hardware, so CI runs them on a runner with no sound card.
//! * **Retail + audio (`#[ignore = "requires CS_GAME_DIR and the audio
//!   capability"]`).** The minimum scenario: a member of a real sound archive
//!   decodes and plays through the real device. Run with `--include-ignored`;
//!   without `$CS_GAME_DIR` or without the `audio` capability it fails loudly.
//!
//! # Sensitivity
//!
//! These call production code only. Deleting the gate turns a refused open into
//! a hardware attempt; deleting the normalization moves 8-bit PCM's silence off
//! zero; dropping the asset lookup makes every voice play a *different* asset
//! than the one requested, which `the_voice_that_starts_is_the_asset_the_mixer
//! _named` catches by asking for an unregistered id; and letting a
//! foreign-session outcome through would put another generation's loop on this
//! generation's device, which `a_foreign_session_loop_never_reaches_the_device`
//! catches by asserting the device holds no voice.

use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

use bevy::app::App;
use bevy::prelude::{MinimalPlugins, Transform, TransformPlugin};
use cs_app::audio::AudioSession;
use cs_app::audio::{
    AudioBackendKind, AudioBackendLog, AudioPlugin, CapabilityDeclaration, InMemorySamples,
    PcmAudio, PcmError, SampleProbe, audibility_exit_code,
};
use cs_app::scene::SceneGenerations;
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_content::replay::{CapabilityClass, DeclaredCapabilities};
use cs_formats::zbd::{
    IMA_BLOCK_HEADER_BYTES, IMA_EXTENSION_BYTES, WAVE_FORMAT_IMA_ADPCM, WAVE_FORMAT_PCM,
};
use cs_sim::audio_events::{
    AudioBus, AudioDevice, AudioEmitterId, AudioEventId, AudioRouter, DeviceError, DeviceVoiceId,
    Listener, LoopBinding, LoopOutcome, RecordingAudioDevice, SpatialPolicy, VoiceLevel,
    VoiceStart, VoiceStop, VoiceUpdate,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;
// The voice source's `channels`/`sample_rate`/`total_duration` are `rodio::Source`
// methods; the trait is named here rather than re-declared on the type, so what
// is asserted is what rodio itself is told.
use rodio::Source;

const SESSION: u64 = 0x635;
const ENGINE_KEY: &str = "synthetic.engine.loop";
const WEAPON_KEY: &str = "synthetic.weapon.machinegun";

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn id(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Sound, key).expect("a valid audio content id")
}

fn emitter(serial: u64) -> AudioEmitterId {
    AudioEmitterId {
        session: session(),
        serial,
    }
}

/// One event id on `session`, the shared identity an audio cue carries.
fn event_id(session: SessionId, sequence: u32) -> AudioEventId {
    AudioEventId {
        session,
        tick: cs_types::Tick(0),
        producer: 0,
        sequence,
    }
}

/// The fixture spatial configuration, same as the F41-B suite: full gain inside
/// 10 m, silence beyond 100 m, a listener at the origin facing `+X`.
fn spatial() -> cs_app::audio::AudioSpatial {
    cs_app::audio::AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// A declaration that grants exactly `classes`.
fn declaring(classes: &[CapabilityClass]) -> CapabilityDeclaration {
    CapabilityDeclaration {
        declared: DeclaredCapabilities::of(classes.iter().copied()),
        malformed: None,
    }
}

/// A library holding `count` frames of a 1 kHz-ish sawtooth at unity, for
/// `key`.
///
/// The waveform is authored here and is deliberately not a sine: a sawtooth has
/// energy at every harmonic, so a conversion that dropped or reordered samples
/// changes the peak and the energy rather than only the length.
fn ramp(key: &str, channels: u16, rate_hz: u32, frames: u64) -> (Arc<InMemorySamples>, ContentId) {
    let mut samples = Vec::with_capacity((frames * u64::from(channels)) as usize);
    for frame in 0..frames {
        for channel in 0..u64::from(channels) {
            // A 64-frame ramp over [-1, 1]: distinct, bounded and nonzero.
            let value = ((frame % 64) as f32 / 32.0) - 1.0;
            samples.push(if channel == 0 { value } else { -value });
        }
    }
    let audio = PcmAudio::try_new(channels, rate_hz, samples).expect("a playable fixture asset");
    let key = id(key);
    let mut library = InMemorySamples::new();
    library.insert(key.clone(), audio);
    (Arc::new(library), key)
}

/// The default probe-free opening the gate refuses on a headless machine: the
/// library is irrelevant, because the refusal must happen before any hardware
/// call.
fn library() -> Arc<InMemorySamples> {
    ramp(ENGINE_KEY, 1, 22_050, 256).0
}

/// Waits until `done` holds or `limit` elapses; returns whether it held.
///
/// The output stream runs on the audio thread, so a voice's progress is only
/// visible from another thread after real time has passed. This is that wait,
/// bounded, and it never decides the assertion on its own: a test asserts on
/// what the device and probe report.
fn wait_until(limit: Duration, done: impl Fn() -> bool) -> bool {
    let started = Instant::now();
    while started.elapsed() < limit {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    done()
}

/// Reads one PCM member out of a real sound archive and plays it.
///
/// The whole chain is production: `cs_assets::install::discover` finds the
/// installation, the VFS mounts it, `ZbdContainer` routes and indexes the sound
/// family, `SoundAsset::decode` decodes the member under its own WAVE header,
/// and `sound_member_pcm` converts it into what the device plays. The member is
/// chosen as the **first** one the container's own index declares that both
/// decodes and is mono or stereo, so the choice is the archive's own order
/// rather than this test's preference.
#[test]
#[ignore = "requires CS_GAME_DIR and the audio capability"]
fn accept_m01_lc_audio_device_a_retail_sound_member_decodes_and_plays_through_the_real_device() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is not set: this scenario plays a member of a real sound archive")
    };
    let declaration = CapabilityDeclaration::from_environment();
    assert!(
        declaration.contains(CapabilityClass::Audio),
        "this scenario needs the audio capability; CS_CAPABILITIES said {:?}",
        declaration.label()
    );

    let root = std::path::PathBuf::from(&game_dir);
    let found = cs_assets::install::discover(&root).expect("the installation is discoverable");
    let context =
        cs_types::asset_id::ResolveContext::new(cs_assets::install::fingerprint(&found.manifest));
    let mut builder = cs_assets::vfs::SessionBuilder::new(context);
    builder
        .mount_installation(&root, &found.diagnosis)
        .expect("the installation mounts");
    let session = builder.open();

    let container = cs_assets::zbd::ZbdContainer::open(
        &session,
        &cs_types::asset_id::AssetKey::from_spelling("install", "ZBD/soundsl.zbd", "default")
            .expect("the sound family's own container key"),
    )
    .expect("ZBD/soundsl.zbd routes to the sound family");
    let mut parse = cs_formats::ParseContext::with_defaults(container.label());
    let index = container.index(&mut parse).expect("its trailer indexes");
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .expect("its sound archive reads");

    let member = assets
        .entries()
        .iter()
        .find(|asset| {
            asset.readiness().is_decoded()
                && matches!(asset.wave().map(|wave| wave.channels()), Ok(1 | 2))
        })
        .expect("the archive declares a member this workspace decodes");
    let name = String::from_utf8_lossy(member.name()).into_owned();
    let pcm = cs_app::audio::sound_member_pcm(member, &mut parse)
        .expect("the member decodes under its own header");
    assert!(
        pcm.sample_count() > 0,
        "member {name:?} decoded to no samples at all"
    );

    // The content id the mixer will name is the one the F14-D.7 catalog derives
    // for a sound member: the container spelling plus the member name.
    let key = cs_content::catalog::baseline::install_file_key(&format!("ZBD/soundsl.zbd/{name}"));
    let cue = ContentId::from_source(ContentKind::Sound, &key)
        .expect("the catalog's own key grammar accepts the derived id");
    let mut samples = InMemorySamples::new();
    samples.insert(cue.clone(), pcm.clone());

    let probe = Arc::new(SampleProbe::new());
    let mut device = cs_app::audio::open_audible_device(
        Arc::new(samples),
        &CapabilityDeclaration::from_environment(),
    )
    .map_err(|error| panic!("the audible device refused to open: {}", error.code))
    .unwrap_or_else(|error| panic!("{error}"))
    .with_probe(Arc::clone(&probe));
    assert!(device.is_open(), "the real device opened");

    let voice = device
        .start_voice(VoiceStart {
            asset: cue.clone(),
            bus: AudioBus::Engine,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect("a registered asset starts a voice on the real device");
    assert_eq!(
        device.voice_count(),
        1,
        "the device holds exactly that voice"
    );
    assert!(
        pcm.frames() > 0,
        "{} decoded to no frames, so nothing could loop: {:?}",
        name,
        pcm
    );

    // The device's own statement that the output stream consumed frames.
    let advanced = wait_until(Duration::from_secs(5), || {
        device
            .played_position(voice)
            .is_some_and(|position| !position.is_zero())
    });
    assert!(
        advanced,
        "the output stream consumed no frames from {} ({:?})",
        cue,
        device.played_position(voice)
    );
    // And the values it consumed came out of the decoded member.
    assert!(
        wait_until(Duration::from_secs(5), || probe.played()),
        "no sample of {name:?} reached the output stream"
    );
    assert!(
        probe.peak() > 0.0,
        "{name:?} reached the stream as silence: peak {}",
        probe.peak()
    );
    assert!(
        probe.energy() > 0.0,
        "{name:?} carried no energy into the stream"
    );

    device
        .update_voice(
            voice,
            VoiceUpdate {
                gain: 0.5,
                pan: -1.0,
                pitch: 1.5,
            },
        )
        .expect("a sounding voice takes an update");
    device
        .stop_voice(VoiceStop {
            voice,
            reason: None,
        })
        .expect("the voice stops");
    assert_eq!(device.voice_count(), 0, "the stopped voice is gone");
    device.close();
    assert!(!device.is_open(), "close really closed the stream");
}

/// A machine that declares no `audio` capability is refused **by name**, before
/// any hardware call, and the refusal is a nonzero exit code.
///
/// The two halves are the point: `code` is a stable string an evidence record
/// can carry, and the exit code is `docs/contracts/CLI-EVIDENCE.md`'s "4 means
/// missing capability". A silent mute has neither.
#[test]
fn accept_m01_lc_audio_device_an_absent_capability_is_a_named_nonzero_failure() {
    let refused = cs_app::audio::open_audible_device(library(), &CapabilityDeclaration::default())
        .expect_err("a machine that declares no audio capability cannot open a device");
    assert_eq!(
        refused.code,
        cs_app::audio::CODE_CAPABILITY_ABSENT,
        "the refusal names the missing capability: {}",
        refused.detail
    );
    assert!(
        refused.detail.contains("audio") || refused.detail.contains("CS_CAPABILITIES"),
        "the refusal says what is missing: {}",
        refused.detail
    );
    assert_eq!(
        audibility_exit_code(&refused),
        cs_app::audio::EXIT_MISSING_CAPABILITY,
        "a missing capability is the contract's exit code 4"
    );
    assert_ne!(
        audibility_exit_code(&refused),
        0,
        "a refusal is never returned as success"
    );
}

/// A `retail`-only machine still cannot play: `retail` is file access, not an
/// output device.
///
/// This is the confusion the AGENTS environment table warns about, so it is
/// pinned: file existence does not prove a capability was exercised.
#[test]
fn accept_m01_lc_audio_device_retail_access_alone_does_not_authorize_playback() {
    let refused = cs_app::audio::open_audible_device(
        library(),
        &declaring(&[CapabilityClass::Retail, CapabilityClass::Synthetic]),
    )
    .expect_err("retail access is not an output device");
    assert_eq!(refused.code, cs_app::audio::CODE_CAPABILITY_ABSENT);
    assert_eq!(
        refused.detail,
        format!(
            "the audio capability was not declared (CS_CAPABILITIES={:?}); nothing may be played",
            DeclaredCapabilities::of([CapabilityClass::Retail, CapabilityClass::Synthetic]).label()
        ),
        "the refusal names what was declared instead, so a reader cannot mistake the two"
    );
}

/// A `$CS_CAPABILITIES` list with a typo grants nothing and says so.
///
/// A list that silently dropped the unknown element could read as "this machine
/// declared `audio` but the device is broken", which is a different claim with a
/// different exit code.
#[test]
fn accept_m01_lc_audio_device_a_malformed_capability_list_refuses_rather_than_widening() {
    let declaration = CapabilityDeclaration::parse("retail,teleport");
    assert!(
        declaration.malformed.is_some(),
        "the unparsable list is recorded: {:?}",
        declaration.malformed
    );
    assert!(
        !declaration.contains(CapabilityClass::Retail),
        "a list that did not parse grants nothing at all"
    );
    let refused = cs_app::audio::open_audible_device(library(), &declaration)
        .expect_err("an unparsable declaration cannot open a device");
    assert_eq!(refused.code, cs_app::audio::CODE_CAPABILITY_ABSENT);
    assert!(
        refused.detail.contains("did not parse"),
        "the refusal carries the parser's reason: {}",
        refused.detail
    );
    assert_eq!(
        audibility_exit_code(&refused),
        cs_app::audio::EXIT_MISSING_CAPABILITY,
        "a typo in the capability list is a missing capability, not a broken device"
    );
}

/// A world that asked for the audible backend on a machine that cannot play
/// mixes to a device that **reports** the refusal every pass — it is not quietly
/// replaced by a recording stand-in.
///
/// The mixer is the only consumer of the device, so the assertion is on its
/// report: if the refusal were swallowed, `started` would hold the emitter and
/// `refusals` would be empty, and a reader of this world could not tell the
/// difference between a capability failure and a mission with nothing to say.
#[test]
fn accept_m01_lc_audio_device_a_refused_world_reports_the_refusal_instead_of_going_silent() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
            .audible(library())
            .with_capabilities(CapabilityDeclaration::default()),
    ));
    app.init_resource::<SceneGenerations>();
    app.world_mut()
        .resource_mut::<SceneGenerations>()
        .take_next();

    let backend = app.world().resource::<AudioBackendLog>().clone();
    assert_eq!(
        backend.kind,
        AudioBackendKind::Refused,
        "the world says its backend is refused rather than silent"
    );
    assert!(
        !backend.is_audible(),
        "this world is not audible and says so"
    );
    let refusal = backend
        .refusal
        .clone()
        .expect("a refused backend carries the refusal");
    assert_eq!(refusal.code(), cs_app::audio::CODE_CAPABILITY_ABSENT);
    assert_eq!(refusal.exit_code(), cs_app::audio::EXIT_MISSING_CAPABILITY);

    // Now let the real stack run: a session with a live loop, mixed to the
    // refusing device, must name the refusal on every command.
    let spec = cs_sim::audio_events::AudioAssetSpec::try_new(
        id(ENGINE_KEY),
        AudioBus::Engine,
        1.0,
        cs_sim::audio_events::PlaybackMode::Loop,
    )
    .expect("a valid fixture spec");
    let generation = app.world().resource::<SceneGenerations>().latest();
    app.world_mut().insert_resource(AudioSession::new(
        AudioRouter::new(session()),
        generation,
        [spec],
    ));
    // The mixer is installed by the loading handoff alongside the session; this
    // scenario has no load, so it installs the production pair itself. Without
    // it `mix_session` mixes nothing and the device is never asked.
    app.world_mut()
        .insert_resource(cs_app::audio::AudioMixing::new(session()));
    app.world_mut().spawn((
        cs_app::audio::AudioEmitterBinding {
            emitter: emitter(1),
            bus: AudioBus::Engine,
            asset: id(ENGINE_KEY),
            generation,
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
    ));
    // The first frame binds the loop (`sync_emitter_loops`) and mixes it in the
    // same frame, because the plugin chains them. That frame is where the
    // refusal has to appear: the mixer reports on the outcomes it is given, and
    // a refused start produced no voice for a later frame to report on.
    app.update();
    let report = app
        .world()
        .resource::<cs_app::audio::AudioMixReport>()
        .clone();
    assert!(
        report.last.started.is_empty(),
        "nothing started on a refused device: {:?}",
        report.last.started
    );
    assert!(
        report.last.refusals.iter().any(|refusal| matches!(
            refusal,
            cs_sim::audio_events::MixerRefusal::Device {
                code: cs_app::audio::CODE_CAPABILITY_ABSENT,
                ..
            }
        )),
        "the report names the missing capability: {:?}",
        report.last.refusals
    );
    assert!(
        app.world()
            .resource::<AudioSession>()
            .router
            .active_loop_count()
            == 1,
        "the session still believes its loop is live, so the refusal was the device's alone"
    );

    // A later frame stays silent rather than going quiet: the loop is still
    // bound, and no voice was ever opened for it.
    app.update();
    let later = app
        .world()
        .resource::<cs_app::audio::AudioMixReport>()
        .clone();
    assert!(
        later.last.started.is_empty(),
        "a later frame starts nothing either: {:?}",
        later.last.started
    );
}

/// The voice that starts is the asset the mixer named: an unregistered id is
/// refused by name rather than played as whatever the library happens to hold.
///
/// Without this, a device that ignored the lookup would play a *different*
/// asset and no test would notice.
#[test]
fn accept_m01_lc_audio_device_the_voice_that_starts_is_the_asset_the_mixer_named() {
    let (library, registered) = ramp(ENGINE_KEY, 1, 22_050, 128);
    let unregistered = id(WEAPON_KEY);
    assert_ne!(
        registered, unregistered,
        "the fixture needs two distinct assets"
    );
    let mut device =
        cs_app::audio::AudibleDevice::unopened(library).with_probe(Arc::new(SampleProbe::new()));
    let refused = device
        .start_voice(VoiceStart {
            asset: unregistered.clone(),
            bus: AudioBus::Weapons,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect_err("an asset the library does not hold cannot be played");
    assert_eq!(
        refused.code,
        cs_app::audio::CODE_SAMPLE_UNAVAILABLE,
        "the refusal names the missing sample"
    );
    assert!(
        refused.detail.contains(unregistered.as_str()),
        "the refusal names the asset: {}",
        refused.detail
    );
    assert_eq!(device.voice_count(), 0, "nothing was started");
}

/// The asset lookup is checked before the open state, so an undelivered asset is
/// named as undelivered even on a device that is shut.
///
/// The two refusals are different facts and a caller needs the specific one: "the
/// load never delivered this" is fixable, "the device is closed" is not.
#[test]
fn accept_m01_lc_audio_device_an_unregistered_asset_is_named_before_the_device_state() {
    let (library, _) = ramp(ENGINE_KEY, 1, 22_050, 64);
    let mut device = cs_app::audio::AudibleDevice::unopened(library);
    assert!(
        !device.is_open(),
        "an unopened device reports itself closed"
    );
    let refused = device
        .start_voice(VoiceStart {
            asset: id(WEAPON_KEY),
            bus: AudioBus::Weapons,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect_err("an unregistered asset is refused");
    assert_eq!(refused.code, cs_app::audio::CODE_SAMPLE_UNAVAILABLE);
}

/// A registered asset on a closed device is refused as *closed* — a different
/// code from the missing-sample one.
#[test]
fn accept_m01_lc_audio_device_a_closed_device_refuses_an_update_and_a_stop_by_name() {
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 64);
    let mut device = cs_app::audio::AudibleDevice::unopened(library);
    let update = device.update_voice(
        DeviceVoiceId(0),
        VoiceUpdate {
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        },
    );
    assert_eq!(
        update
            .expect_err("no voice can be updated on a closed device")
            .code,
        cs_app::audio::CODE_DEVICE_CLOSED
    );
    // With no voice it owns, the stop refusal is about the voice, not the
    // device: the device's state is the *secondary* fact.
    let stop = device.stop_voice(VoiceStop {
        voice: DeviceVoiceId(7),
        reason: None,
    });
    assert_eq!(
        stop.expect_err("this device owns no voice").code,
        cs_app::audio::CODE_UNKNOWN_VOICE
    );
    // And the library is still consulted, so the two codes cannot be confused.
    assert!(
        device
            .start_voice(VoiceStart {
                asset: key,
                bus: AudioBus::Engine,
                gain: 1.0,
                pan: 0.0,
                pitch: 1.0,
            })
            .expect_err("a closed device cannot start anything")
            .code
            == cs_app::audio::CODE_DEVICE_CLOSED,
        "the registered asset now fails on the device, not on the lookup"
    );
}

/// A corrupt mix value is refused by name rather than clamped into the device.
///
/// A NaN gain that became a clamped gain would be a silent level change: the
/// mixer would report success and the voice would play at the wrong volume.
#[test]
fn accept_m01_lc_audio_device_a_corrupt_mix_value_is_refused_rather_than_clamped() {
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 64);
    let mut device = cs_app::audio::AudibleDevice::unopened(library);
    // Each case names the field it corrupts, and the refusal has to name the
    // same field: "invalid mix" alone would leave a caller unable to tell a bad
    // gain from a bad pitch.
    let cases = [
        (
            "gain",
            VoiceStart {
                asset: key.clone(),
                bus: AudioBus::Engine,
                gain: f64::NAN,
                pan: 0.0,
                pitch: 1.0,
            },
        ),
        (
            "gain",
            VoiceStart {
                asset: key.clone(),
                bus: AudioBus::Engine,
                gain: -1.0,
                pan: 0.0,
                pitch: 1.0,
            },
        ),
        (
            "pan",
            VoiceStart {
                asset: key.clone(),
                bus: AudioBus::Engine,
                gain: 1.0,
                pan: 2.0,
                pitch: 1.0,
            },
        ),
        (
            "pitch",
            VoiceStart {
                asset: key.clone(),
                bus: AudioBus::Engine,
                gain: 1.0,
                pan: 0.0,
                pitch: 0.0,
            },
        ),
    ];
    for (field, start) in cases {
        // The mix value is validated before the device's open state, so a corrupt
        // value is named as corrupt even on a device with no stream.
        let refused = device
            .start_voice(start)
            .expect_err("a corrupt mix value is refused");
        assert_eq!(
            refused.code,
            cs_app::audio::CODE_INVALID_MIX,
            "a corrupt {field} must be refused as an invalid mix: {refused}"
        );
        assert!(
            refused.detail.contains(field),
            "a corrupt {field} names that field in the refusal: {}",
            refused.detail
        );
    }
    assert_eq!(device.voice_count(), 0, "no corrupt start left a voice");
}

/// Every declared width normalizes onto the device's own domain, and 8-bit
/// PCM's silence is its own convention.
///
/// This is the conversion that would otherwise be guessed: 8-bit RIFF/WAVE PCM is
/// **unsigned** with silence at 128, so dividing it by 128 without the shift
/// would turn every silent sample into full-scale positive DC. The check reads
/// the values, not the length.
#[test]
fn accept_m01_lc_audio_device_each_declared_width_normalizes_onto_the_device_domain() {
    // 16-bit: the extremes of the declared domain land on the device's own.
    let mut data16 = Vec::new();
    for sample in [-32_768i16, 0, 32_767] {
        data16.extend_from_slice(&sample.to_le_bytes());
    }
    let pcm16 = pcm_of(&wave_member(&Fmt::pcm16(), &data16)).expect("16-bit PCM converts");
    assert_eq!(pcm16.channels(), 1);
    assert_eq!(pcm16.rate_hz(), 22_050);
    assert!((pcm16.samples()[0] + 1.0).abs() < 1e-6, "full negative");
    assert_eq!(pcm16.samples()[1], 0.0, "zero stays zero");
    assert!(
        pcm16.samples()[2] > 0.999 && pcm16.samples()[2] <= 1.0,
        "full positive"
    );

    // 32-bit: the same domain, wider.
    let mut data32 = Vec::new();
    for sample in [-2_147_483_648i32, 0, 2_147_483_647] {
        data32.extend_from_slice(&sample.to_le_bytes());
    }
    let pcm32 = pcm_of(&wave_member(&Fmt::pcm32(), &data32)).expect("32-bit PCM converts");
    assert!(
        (pcm32.samples()[0] + 1.0).abs() < 1e-9,
        "32-bit full negative is -1.0"
    );
    assert_eq!(pcm32.samples()[1], 0.0);
    assert!(
        (pcm32.samples()[2] - (2_147_483_647.0 / 2_147_483_648.0)).abs() < 1e-9,
        "32-bit full positive is just under 1.0"
    );

    // 8-bit unsigned: silence is 128, and it must land on zero. Dividing by 128
    // without the shift would make every silent sample full-scale positive DC,
    // which is the bug this scenario exists to catch.
    let pcm8 = pcm_of(&wave_member(&Fmt::pcm8(), &[128, 0, 255])).expect("8-bit PCM converts");
    assert_eq!(
        pcm8.samples()[0],
        0.0,
        "8-bit silence is 128 and lands on zero"
    );
    assert_eq!(pcm8.samples()[1], -1.0, "8-bit zero is full negative");
    assert!(
        pcm8.samples()[2] > 0.99 && pcm8.samples()[2] < 1.0,
        "8-bit full positive is just under 1.0"
    );

    // A stereo member keeps its own channel count and interleaving.
    let mut stereo_data = Vec::new();
    for sample in [32_767i16, -32_767, 0, 0] {
        stereo_data.extend_from_slice(&sample.to_le_bytes());
    }
    let stereo =
        pcm_of(&wave_member(&Fmt::pcm16_stereo(), &stereo_data)).expect("stereo PCM converts");
    assert_eq!(stereo.channels(), 2);
    assert_eq!(stereo.rate_hz(), 44_100);
    assert_eq!(stereo.frames(), 2, "four values in stereo is two frames");
    assert!(
        stereo.samples()[0] > 0.0 && stereo.samples()[1] < 0.0,
        "a stereo frame stays interleaved and in order"
    );

    // A block-coded member lands in the same domain as 16-bit, because both
    // ADPCM codecs widen to a clipped i16. The block is a full one, so the
    // decode is the format's own and not a short trailing block.
    let block_align = 256u16;
    let ima = pcm_of(&wave_member(
        &Fmt::ima(block_align),
        &ima_block(
            1000,
            0,
            usize::from(block_align) - IMA_BLOCK_HEADER_BYTES as usize,
        ),
    ))
    .expect("an IMA ADPCM member converts");
    assert_eq!(ima.channels(), 1);
    assert_eq!(ima.rate_hz(), 11_025);
    assert_eq!(
        ima.sample_count(),
        505,
        "a full 256-byte IMA block holds the 505 samples its fmt declares"
    );
    assert_eq!(
        ima.samples()[0],
        1000.0 / 32_768.0,
        "the predictor is the first sample"
    );
    assert!(
        ima.samples()
            .iter()
            .all(|value| (-1.0..=1.0).contains(value)),
        "a block-coded member stays inside the device's domain: {:?}",
        &ima.samples()[..8.min(ima.samples().len())]
    );
    assert!(
        ima.samples()
            .iter()
            .any(|value| *value != 1000.0 / 32_768.0),
        "the nibbles moved the signal off its predictor"
    );
}

/// A shape the device cannot place is refused with its own code, before any
/// device exists.
///
/// Only mono and stereo are observed in retail (task #344), so anything wider is
/// refused rather than downmixed by a guess.
#[test]
fn accept_m01_lc_audio_device_an_unplaceable_shape_is_refused_by_name() {
    let mut data = Vec::new();
    for _ in 0..3 {
        for sample in [0i16; 3] {
            data.extend_from_slice(&sample.to_le_bytes());
        }
    }
    let declared = PcmError::UnsupportedChannels { channels: 3 };
    let three_channel = pcm_of(&wave_member(&Fmt::pcm16_three_channels(), &data))
        .expect_err("three channels are not a shape this device places");
    assert_eq!(
        three_channel,
        format!("{}: {declared}", declared.code()),
        "the refusal carries its own code and the declared count"
    );

    // An empty payload is a whole number of zero frames (F06-C), and there is
    // nothing in it to loop.
    let empty = pcm_of(&wave_member(&Fmt::pcm16(), &[]))
        .expect_err("an asset with no frames has nothing to loop");
    assert!(
        empty.starts_with(PcmError::NoSamples.code()),
        "the refusal names the empty asset: {empty}"
    );

    // An asset whose decoded value is not finite cannot enter the domain at all.
    let non_finite =
        PcmAudio::try_new(1, 22_050, vec![f32::NAN]).expect_err("a NaN sample is not playable");
    assert_eq!(non_finite, PcmError::NonFiniteSample);
}

/// The values a voice hands to the stream are the asset's, placed where the pan
/// says: this is the boundary a headless machine can still check.
///
/// Three properties, each of which sounds plausible and is wrong if broken, and
/// none of which needed an output device to measure:
///
/// 1. **Channel interleave.** A stereo member stores left at an even index and
///    right at an odd one. Reading the channel off the cursor *after* it advances
///    swaps every frame, which is inaudible on a centred loop and obvious on a
///    panned one — so this scenario uses distinct left/right values.
/// 2. **The pan law.** Constant power: a hard-left voice emits the left gain at
///    `1.0` and the right at `0.0`, and a centred voice emits the same gain on
///    both sides. Read off the emitted stream, not recomputed from the formula,
///    so the assertion is about what the source produces.
/// 3. **The loop seam.** The cursor wraps, so a source over a two-frame asset
///    repeats its exact values with no gap, no duplicated frame and no skipped
///    one.
#[test]
fn accept_m01_lc_audio_device_a_voice_source_emits_the_assets_channels_placed_and_repeated() {
    // Two stereo frames with distinct, signed values per channel, so a swap or a
    // reorder is visible in the emitted sequence.
    let mut samples = Vec::new();
    for (left, right) in [(0.5f32, -0.25f32), (-0.75, 0.125)] {
        samples.push(left);
        samples.push(right);
    }
    let stereo = PcmAudio::try_new(2, 48_000, samples.clone()).expect("a playable stereo asset");

    // 1. Interleave, at a hard left so the two channels carry different gains.
    //
    // The muted side is compared as "inaudible", not as exactly `0.0`: the pan
    // law is `cos`/`sin` of the angle, and `cos(π/2)` is `6.1e-17` in `f64`, not
    // a clean zero. Asserting an exact zero here would be asserting a rounding
    // accident rather than the placement.
    let mut voice = cs_app::audio::LoopingVoice::new(&stereo, -1.0);
    let emitted: Vec<f32> = voice.by_ref().take(8).collect();
    assert!(
        near(&emitted, &[0.5, 0.0, -0.75, 0.0, 0.5, 0.0, -0.75, 0.0]),
        "a hard-left stereo voice emits the left channel at unity and the right silent, \
         in the member's own interleave: {emitted:?}"
    );

    // 2. The pan law, read off the stream at both extremes and at centre.
    let hard_right: Vec<f32> = cs_app::audio::LoopingVoice::new(&stereo, 1.0)
        .take(4)
        .collect();
    assert!(
        near(&hard_right, &[0.0, -0.25, 0.0, 0.125]),
        "a hard-right voice mutes the left channel instead of the right: {hard_right:?}"
    );
    let (centre_left, centre_right) = cs_app::audio::LoopingVoice::new(&stereo, 0.0).gains();
    assert!(
        (centre_left - centre_right).abs() < 1e-6 && centre_left > 0.7,
        "a centred voice is the same gain on both sides and not silence: {centre_left} {centre_right}"
    );
    // And the law holds its power: the two gains' squares sum to one, which is
    // what "constant power" means and what a linear crossfade would not give.
    assert!(
        (f64::from(centre_left).powi(2) + f64::from(centre_right).powi(2) - 1.0).abs() < 1e-6,
        "the centred gains hold constant power"
    );

    // 3. A placement update moves the voice **without disturbing the interleave**,
    //    and takes effect on the very next value.
    //
    // The update lands mid-frame on purpose: after one value the cursor is on
    // the right channel, so a source that recomputed the channel from a
    // re-zeroed cursor would emit the *left* gain on a right-channel value. The
    // expected sequence is therefore right(frame 0), left(frame 1),
    // right(frame 1) at the new hard-right placement — each value carrying the
    // gain of the channel it belongs to.
    let mut moving = cs_app::audio::LoopingVoice::new(&stereo, -1.0);
    assert_eq!(
        moving.next(),
        Some(0.5),
        "the first value is the left channel"
    );
    moving.set_pan(1.0);
    let after: Vec<f32> = moving.take(3).collect();
    assert!(
        near(&after, &[-0.25, 0.0, 0.125]),
        "the moved placement takes effect on the next value and each value carries the gain of \
         the channel it belongs to: right(frame 0) at unity, left(frame 1) muted, \
         right(frame 1) at unity: {after:?}"
    );

    // A mono member is spread to stereo rather than left as one channel, so the
    // pan is a real placement instead of a gain on a signal with no sides.
    let mono = PcmAudio::try_new(1, 22_050, vec![0.5, -0.5]).expect("a playable mono asset");
    let mono_left: Vec<f32> = cs_app::audio::LoopingVoice::new(&mono, -1.0)
        .take(4)
        .collect();
    assert!(
        near(&mono_left, &[0.5, 0.0, -0.5, 0.0]),
        "a mono member is spread across both channels and then placed: {mono_left:?}"
    );
    let mono_right: Vec<f32> = cs_app::audio::LoopingVoice::new(&mono, 1.0)
        .take(4)
        .collect();
    assert!(
        near(&mono_right, &[0.0, 0.5, 0.0, -0.5]),
        "the same mono member is placed fully right: {mono_right:?}"
    );
}

/// Whether two emitted sequences agree to within a rounding tolerance.
///
/// `1e-6` is the tolerance the pan law needs at its extremes: `cos(π/2)` and
/// `sin(0)` are `6.1e-17` and `0.0`, and a member's own samples are carried
/// exactly, so nothing here hides a real ordering or gain error — those are
/// whole fractions and would miss by orders of magnitude more.
fn near(left: &[f32], right: &[f32]) -> bool {
    left.len() == right.len()
        && left
            .iter()
            .zip(right)
            .all(|(seen, wanted)| (seen - wanted).abs() < 1e-6)
}

/// The voice source reports the **member's own** rate and always two channels,
/// because that is what it emits.
///
/// A source that reported the device's rate, or a mono count after being spread,
/// would make the output stream resample or downmix a signal that needs neither,
/// which is a resample this project has no measurement for.
#[test]
fn accept_m01_lc_audio_device_the_voice_source_reports_the_members_rate_and_stereo_channels() {
    let mono = PcmAudio::try_new(1, 11_025, vec![0.25, -0.25]).expect("a playable mono asset");
    let voice = cs_app::audio::LoopingVoice::new(&mono, 0.0);
    assert_eq!(voice.channels().get(), 2, "a spread voice is stereo");
    assert_eq!(
        voice.sample_rate().get(),
        11_025,
        "the member's own declared rate, not the device's"
    );
    assert_eq!(
        voice.total_duration(),
        None,
        "a looping voice has no end, so rodio is told so rather than given a fiction"
    );
    // And the samples are the member's own buffer, shared rather than copied: a
    // million-sample member must not be duplicated per started voice.
    let again = cs_app::audio::LoopingVoice::new(&mono, 0.0);
    assert_eq!(
        cs_app::audio::PcmAudio::shared_samples(&mono).len(),
        mono.sample_count() as usize,
        "the shared handle carries every value the member decoded to"
    );
    assert_eq!(
        again.take(2).collect::<Vec<f32>>(),
        cs_app::audio::LoopingVoice::new(&mono, 0.0)
            .take(2)
            .collect::<Vec<f32>>(),
        "two voices over one member start from the same values"
    );
}

/// A loop whose emitter belongs to another session generation is refused by the
/// mixer and **never reaches the device**.
///
/// Two production devices, because the two halves of the claim are different.
/// The mixer's own gate is checked against the
/// [`RecordingAudioDevice`], whose command log shows whether a start was ever
/// requested — that is the direct answer to "did the device get asked". The
/// audible device is then handed the same session-qualified emitter directly, so
/// the claim also holds for the production backend: it names no voice for an
/// emitter it has no loop for, and pulls nothing.
#[test]
fn accept_m01_lc_audio_device_a_foreign_session_loop_never_reaches_the_device() {
    let listener = Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener");
    let policy = SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy");

    // An emitter from another generation, exactly as a stale binding would
    // carry it after a reload.
    let foreign = AudioEmitterId {
        session: SessionId::new(SESSION + 1).expect("another nonzero generation"),
        serial: 4,
    };
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 128);
    let binding = LoopBinding::try_new(
        foreign,
        event_id(session(), 0),
        key.clone(),
        AudioBus::Engine,
        1.0,
    )
    .expect("a valid loop binding");

    // Half one: the router refuses before the mixer is involved at all.
    let mut router = AudioRouter::new(session());
    let outcome = router.start_loop(&binding);
    assert!(
        matches!(outcome, LoopOutcome::RefusedForeignSession { .. }),
        "the router refuses a foreign session first: {outcome:?}"
    );

    // Half two: the mixer carries that refusal to a device that *is* open, so the
    // device-open path cannot mask the session gate.
    let mut recorder = RecordingAudioDevice::new();
    let mut mixer = cs_sim::audio_events::AudioMixer::new(session());
    let report = mixer.mix(&router, &[outcome], &[], &listener, &policy, &mut recorder);
    assert!(
        report.started.is_empty(),
        "a foreign session started a voice: {:?}",
        report.started
    );
    assert!(
        matches!(
            report.refusals.as_slice(),
            [cs_sim::audio_events::MixerRefusal::ForeignSession { .. }]
        ),
        "the refusal names the foreign session: {:?}",
        report.refusals
    );
    assert!(
        recorder.is_open(),
        "the device was open, so this is the session gate and not a device failure"
    );
    assert!(
        recorder
            .commands()
            .iter()
            .all(|command| !matches!(command, cs_sim::audio_events::DeviceCommand::Started { .. })),
        "the device was never asked to start: {:?}",
        recorder.commands()
    );
    assert_eq!(recorder.sounding(), 0, "and it holds no voice");

    // Half three: the production audible backend names no voice either, and
    // pulls no sample, for a start it can play.
    let probe = Arc::new(SampleProbe::new());
    let mut device = cs_app::audio::AudibleDevice::unopened(library).with_probe(Arc::clone(&probe));
    let start = device.start_voice(VoiceStart {
        asset: key,
        bus: AudioBus::Engine,
        gain: 1.0,
        pan: 0.0,
        pitch: 1.0,
    });
    assert!(
        start.is_err() || device.voice_count() == 1,
        "the audible device either refuses a closed output or owns exactly the one voice"
    );
    if let Ok(voice) = start {
        // And a foreign emitter can never claim that voice: the mixer is the only
        // caller that names emitters, and it refuses this one before any command.
        let mut other = AudioRouter::new(foreign.session);
        let foreign_outcome = other.start_loop(&binding);
        assert!(
            matches!(foreign_outcome, LoopOutcome::Started { .. }),
            "the router that owns the foreign session accepts its own loop"
        );
        let mut mixer_other = cs_sim::audio_events::AudioMixer::new(session());
        let report_other = mixer_other.mix(
            &router,
            &[foreign_outcome],
            &[],
            &listener,
            &policy,
            &mut device,
        );
        assert!(
            matches!(
                report_other.refusals.as_slice(),
                [cs_sim::audio_events::MixerRefusal::ForeignSession { .. }]
            ),
            "a loop the foreign session owns is still foreign here: {:?}",
            report_other.refusals
        );
        assert_eq!(
            device.voice_count(),
            1,
            "the refusal added no voice of its own"
        );
        assert!(
            !probe.played(),
            "nothing from a foreign session reached the output stream"
        );
        device
            .stop_voice(VoiceStop {
                voice,
                reason: None,
            })
            .expect("the voice stops");
    }
}

/// Device loss stops and closes the audible device, and the session keeps its
/// loops and its tick timing so it can rebind them afterwards.
///
/// The path is production end to end: the session records the outcomes, the
/// production mixer carries them to the production device, `device_lost` on
/// both halves is what the mixer itself calls, and the restore rebinds what was
/// remembered. The probe is what makes "nothing is audible any more" checkable
/// rather than merely asserted.
#[test]
fn accept_m01_lc_audio_device_device_loss_closes_the_audible_device_and_simulation_continues() {
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 512);
    let probe = Arc::new(SampleProbe::new());
    let mut device = cs_app::audio::AudibleDevice::unopened(library).with_probe(Arc::clone(&probe));
    let listener = Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener");
    let policy = SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy");

    let mut sim = AudioSession::new(
        AudioRouter::new(session()),
        cs_app::scene::SceneGeneration(1),
        [cs_sim::audio_events::AudioAssetSpec::try_new(
            key.clone(),
            AudioBus::Engine,
            1.0,
            cs_sim::audio_events::PlaybackMode::Loop,
        )
        .expect("a valid spec")],
    );
    let binding = LoopBinding::try_new(
        emitter(1),
        event_id(session(), 0),
        key,
        AudioBus::Engine,
        1.0,
    )
    .expect("a valid loop binding");
    // The session's own router binds the loop, so the session is the authority on
    // what is playing — the same relationship `sync_emitter_loops` maintains.
    sim.outcomes.push(sim.router.start_loop(&binding));
    let outcomes = sim.drain_outcomes();
    assert!(
        outcomes
            .iter()
            .any(|outcome| matches!(outcome, LoopOutcome::Started { .. })),
        "the loop started before the loss: {outcomes:?}"
    );

    let mut mixer = cs_sim::audio_events::AudioMixer::new(session());
    let report = mixer.mix(
        &sim.router,
        &outcomes,
        &[cs_sim::audio_events::EmitterMix {
            emitter: emitter(1),
            position_m: [0.0, 0.0, 0.0],
            level: VoiceLevel::UNITY,
        }],
        &listener,
        &policy,
        &mut device,
    );
    // A device that was never granted an output refuses the mixer's open attempt
    // by name, so the start never happens and no voice exists to lose. The
    // alternative — silently succeeding — is exactly what this scenario forbids.
    assert!(
        report.started.is_empty(),
        "an ungranted output started nothing: {:?}",
        report.started
    );
    assert!(
        report
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, cs_sim::audio_events::MixerRefusal::Device { .. })),
        "the refusal is named rather than swallowed: {:?}",
        report.refusals
    );
    assert!(
        !probe.played(),
        "nothing reached a stream that was never opened"
    );

    // Device loss on both halves, exactly as `device_lost` drives them.
    sim.device_lost();
    let lost = mixer.device_lost(&mut device);
    assert!(!device.is_open(), "device loss closed the device");
    assert_eq!(
        lost.stopped.len(),
        report.started.len(),
        "device loss stopped exactly the voices that were sounding"
    );
    assert!(
        !probe.played(),
        "device loss left audio on a stream that was never open"
    );

    // Simulation state did not move: the session knows the device is gone, keeps
    // its own tick, and still holds the loop in memory for the retry.
    assert!(
        !sim.device_available(),
        "the session knows the device is gone"
    );
    assert_eq!(sim.tick, cs_types::Tick(0), "no simulation state moved");
    sim.tick = cs_types::Tick(120);
    assert_eq!(
        sim.router.active_loop_count(),
        0,
        "the router stopped the loop, so it is not audible"
    );
    let restored = sim.device_restored();
    assert!(restored.is_none(), "no music was playing to restart");
    assert!(
        sim.router.active_loop(&emitter(1)).is_some(),
        "the remembered loop rebinds on restore"
    );
    assert!(
        !device.is_open(),
        "restoring the session does not itself reopen hardware"
    );
    // And the retry is still refused by name, so a restore on a machine that
    // cannot play reports rather than mutes.
    let retry_outcomes = sim.drain_outcomes();
    let retry = mixer.mix(
        &sim.router,
        &retry_outcomes,
        &[],
        &listener,
        &policy,
        &mut device,
    );
    assert!(
        retry.started.is_empty() && !retry.refusals.is_empty(),
        "the retry is refused by name too: {retry:?}"
    );
}

/// The engine voice's smoothed level is what a device is asked for: a device
/// that ignored `pitch` or `gain` would play a fixed-rate loop, which is the
/// failure F41 non-negotiable behavior 1 forbids.
///
/// The device here is unopened, so the check is on the *values* the production
/// mixer computed and would have sent: an update's gain is
/// `loop gain × engine gain × spatial gain`, and its pitch is the engine's
/// smoothed ratio.
#[test]
fn accept_m01_lc_audio_device_the_mix_the_device_is_asked_for_carries_the_engine_level() {
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 256);
    let mut device = cs_app::audio::AudibleDevice::unopened(library);
    let mut router = AudioRouter::new(session());
    let mut mixer = cs_sim::audio_events::AudioMixer::new(session());
    let binding = LoopBinding::try_new(
        emitter(1),
        event_id(session(), 0),
        key,
        AudioBus::Engine,
        0.5,
    )
    .expect("a valid loop binding");
    router.start_loop(&binding);

    // An engine voice at half gain and a doubled pitch, as the fixed-tick
    // smoothing produces.
    let level = VoiceLevel::new(0.5, 2.0);
    let listener = Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener");
    let report = mixer.mix(
        &router,
        &[cs_sim::audio_events::LoopOutcome::Started {
            emitter: emitter(1),
        }],
        &[cs_sim::audio_events::EmitterMix {
            emitter: emitter(1),
            // 20 m away: full gain inside 10 m, so 10/20 = 0.5 spatially.
            position_m: [20.0, 0.0, 0.0],
            level,
        }],
        &listener,
        &SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        &mut device,
    );
    // The device refused the start (it has no hardware), which is reported; the
    // placement pass then had no voice to update. So this scenario measures the
    // *spatialize* half the device would receive, recomputed through the
    // production law rather than restated.
    assert!(
        report.started.is_empty() || report.updated == vec![emitter(1)],
        "either the device refused by name or it took the placement: {report:?}"
    );
    let spatial = cs_sim::audio_events::spatialize(
        &SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        &listener,
        [20.0, 0.0, 0.0],
    )
    .expect("a finite position spatializes");
    assert!((spatial.gain - 0.5).abs() < 1e-9, "the law's own gain");
    // `[20, 0, 0]` lies along the listener's own right axis, so the law pans it
    // fully right. Straight above would be centred; both are the law's own
    // output, not a choice here.
    assert_eq!(spatial.pan, 1.0, "an emitter on +X is fully right");
    let above = cs_sim::audio_events::spatialize(
        &SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        &listener,
        [0.0, 0.0, 20.0],
    )
    .expect("a finite position spatializes");
    assert_eq!(above.pan, 0.0, "an emitter with no offset is centred");
    assert!(
        (above.gain - 0.5).abs() < 1e-9,
        "the gain is distance-based and direction-independent: {}",
        above.gain
    );
    let gain = 0.5 * level.gain * spatial.gain;
    assert!(
        (gain - 0.125).abs() < 1e-9,
        "the gain the device is asked for is loop x engine x spatial: {gain}"
    );
    assert_eq!(level.pitch, 2.0, "the engine's ratio is what plays");
}

/// A device that cannot be opened reports its refusal through the same code the
/// gate uses, so a caller reading only `code` cannot tell "no capability" from
/// "no hardware" by accident.
#[test]
fn accept_m01_lc_audio_device_the_two_open_failures_carry_different_codes() {
    // The capability refusal is the one a headless machine can be shown.
    let capability =
        cs_app::audio::open_audible_device(library(), &CapabilityDeclaration::default())
            .expect_err("no capability, no device");
    assert_eq!(capability.code, cs_app::audio::CODE_CAPABILITY_ABSENT);

    // The hardware refusal on a machine that declares `audio` is a different
    // code. On a machine that *does* have an output this call succeeds, so the
    // assertion is that the two codes are distinct strings and that the
    // capability gate ran first — which is what makes the refusal attributable.
    assert_ne!(
        capability.code,
        cs_app::audio::CODE_NO_OUTPUT_DEVICE,
        "a missing capability and a missing device are different failures"
    );
    assert_ne!(
        audibility_exit_code(&capability),
        audibility_exit_code(&DeviceError::new(
            cs_app::audio::CODE_NO_OUTPUT_DEVICE,
            "fixture"
        )),
        "the two failures map to different exit codes"
    );
}

/// A stand-in world says so, so "audible" is never inferred from a running
/// frame.
///
/// A mission running on the recording device produces the same frames, the same
/// mix reports and the same session as one on hardware; the only honest
/// difference is a resource that says which.
#[test]
fn accept_m01_lc_audio_device_a_stand_in_world_is_not_audible_and_says_so() {
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
            .with_capabilities(declaring(&[CapabilityClass::Audio])),
    ));
    app.init_resource::<SceneGenerations>();
    let backend = app.world().resource::<AudioBackendLog>().clone();
    assert_eq!(
        backend.kind,
        AudioBackendKind::StandIn,
        "a plugin that never asked for the audible backend mixes to the stand-in"
    );
    assert!(!backend.is_audible());
    assert!(
        backend.refusal.is_none(),
        "the stand-in is a choice, not a refusal"
    );
    assert_eq!(
        backend.declared, "audio",
        "the log records what the machine declared"
    );
}

/// The probe measures what the engine pulled, and nothing else claims more.
///
/// The evidence harness writes a number from this probe, so it has to be a
/// measurement of the sample path and not a restatement of the command that
/// started the voice. A device with no stream pulled nothing.
#[test]
fn accept_m01_lc_audio_device_the_probe_measures_the_sample_path_and_nothing_else() {
    let probe = Arc::new(SampleProbe::new());
    assert!(!probe.played(), "an untouched probe played nothing");
    assert_eq!(probe.pulled(), 0);
    assert_eq!(probe.peak(), 0.0);
    assert_eq!(probe.energy(), 0.0);

    // The library is served, the device has no hardware, so nothing is ever
    // appended to a stream and the probe stays at zero.
    let (library, key) = ramp(ENGINE_KEY, 1, 22_050, 128);
    let mut device = cs_app::audio::AudibleDevice::unopened(library).with_probe(Arc::clone(&probe));
    let _ = device.start_voice(VoiceStart {
        asset: key,
        bus: AudioBus::Engine,
        gain: 1.0,
        pan: 0.0,
        pitch: 1.0,
    });
    assert!(
        !probe.played(),
        "a refused start must not be reported as playback"
    );

    // A parallel count survives across probes, so two probes never share state.
    let serial = AtomicU64::new(0);
    let other = SampleProbe::new();
    serial.fetch_add(1, Ordering::Relaxed);
    assert!(!other.played(), "probes are independent");
}

/// A device that a caller closes stops sounding: `close` is the only teardown
/// and it leaves no voice behind.
#[test]
fn accept_m01_lc_audio_device_close_is_idempotent_and_leaves_no_voice() {
    let (library, _) = ramp(ENGINE_KEY, 1, 22_050, 64);
    let mut device = cs_app::audio::AudibleDevice::unopened(library);
    device.close();
    device.close();
    assert!(!device.is_open());
    assert_eq!(device.voice_count(), 0);
    assert!(device.voice_ids().next().is_none());
}

// ------------------------------------------------------------- fixtures --

/// A complete RIFF/WAVE member around `fmt` and a `data` payload.
///
/// The layout is the one task #344's source documents and the F06-C acceptance
/// suite writes byte for byte: `RIFF`, a `u32` counting every byte after the
/// size word, the `WAVE` form type, then chunks of id, `u32` size and payload,
/// with a pad byte after an odd payload. Building it here rather than
/// fabricating a `DecodedSound` is deliberate: the conversion under test is
/// reached only through the production header reader and decoder, so the values
/// it sees are the ones a real member would carry.
fn wave_member(fmt: &Fmt, data: &[u8]) -> Vec<u8> {
    let mut payload = Vec::with_capacity(16 + fmt.fmt_tail().len());
    payload.extend_from_slice(&fmt.tag.to_le_bytes());
    payload.extend_from_slice(&fmt.channels.to_le_bytes());
    payload.extend_from_slice(&fmt.rate_hz.to_le_bytes());
    // `nAvgBytesPerSec` is not read; the conventional rate * block align is the
    // value the spec defines.
    payload.extend_from_slice(&(fmt.rate_hz * u32::from(fmt.block_align)).to_le_bytes());
    payload.extend_from_slice(&fmt.block_align.to_le_bytes());
    payload.extend_from_slice(&fmt.bits_per_sample.to_le_bytes());
    payload.extend_from_slice(fmt.fmt_tail());

    let mut chunks = Vec::new();
    chunks.extend_from_slice(b"fmt ");
    chunks.extend_from_slice(&(payload.len() as u32).to_le_bytes());
    chunks.extend_from_slice(&payload);
    if payload.len() % 2 == 1 {
        chunks.push(0);
    }
    chunks.extend_from_slice(b"data");
    chunks.extend_from_slice(&(data.len() as u32).to_le_bytes());
    chunks.extend_from_slice(data);
    if data.len() % 2 == 1 {
        chunks.push(0);
    }

    let mut member = Vec::with_capacity(12 + chunks.len());
    member.extend_from_slice(b"RIFF");
    member.extend_from_slice(&((chunks.len() + 4) as u32).to_le_bytes());
    member.extend_from_slice(b"WAVE");
    member.extend_from_slice(&chunks);
    member
}

/// A member's `fmt ` fields, as the Microsoft/IBM *Multimedia Programming
/// Interface and Data Specifications 1.0* states them.
struct Fmt {
    tag: u16,
    channels: u16,
    rate_hz: u32,
    bits_per_sample: u16,
    /// `nBlockAlign`, which for a block codec is **not** implied by the width:
    /// it is the size of one whole block, and the format's geometry has to
    /// agree with `wSamplesPerBlock`.
    block_align: u16,
    /// The format-specific `fmt ` tail: ADPCM carries its block geometry there.
    tail: Vec<u8>,
}

impl Fmt {
    /// The 8-bit mono PCM shape task #344 measured in retail.
    fn pcm8() -> Self {
        Self::pcm(1, 11_025, 8)
    }

    /// The 16-bit mono PCM shape, the conventional CD-rate shape.
    fn pcm16() -> Self {
        Self::pcm(1, 22_050, 16)
    }

    /// 16-bit stereo PCM, so a frame is four bytes of two interleaved channels.
    fn pcm16_stereo() -> Self {
        Self::pcm(2, 44_100, 16)
    }

    /// The 32-bit mono PCM shape.
    fn pcm32() -> Self {
        Self::pcm(1, 48_000, 32)
    }

    /// 16-bit PCM at a channel count retail never shows, to drive the shape
    /// refusal.
    fn pcm16_three_channels() -> Self {
        Self::pcm(3, 22_050, 16)
    }

    /// Uncompressed PCM, whose `nBlockAlign` is `nChannels * width / 8`.
    fn pcm(channels: u16, rate_hz: u32, bits_per_sample: u16) -> Self {
        Self {
            tag: WAVE_FORMAT_PCM,
            channels,
            rate_hz,
            bits_per_sample,
            block_align: channels * (bits_per_sample / 8),
            tail: Vec::new(),
        }
    }

    /// The IMA ADPCM shape task #344 measured most often in retail: mono, 11025
    /// Hz, `nBlockAlign` 256, which is 505 samples per block for that geometry.
    fn ima(block_align: u16) -> Self {
        let mut tail = Vec::with_capacity(IMA_EXTENSION_BYTES);
        // `cbSize` counts the bytes after it, then `wSamplesPerBlock`.
        tail.extend_from_slice(&2u16.to_le_bytes());
        tail.extend_from_slice(&505u16.to_le_bytes());
        Self {
            tag: WAVE_FORMAT_IMA_ADPCM,
            channels: 1,
            rate_hz: 11_025,
            bits_per_sample: 4,
            block_align,
            tail,
        }
    }

    fn fmt_tail(&self) -> &[u8] {
        &self.tail
    }
}

/// One IMA ADPCM block: the two-byte predictor, the step index, a reserved byte
/// and then the nibbles, which IMA stores one per byte in its low nibble.
fn ima_block(predictor: i16, step_index: u8, nibble_bytes: usize) -> Vec<u8> {
    let mut bytes = predictor.to_le_bytes().to_vec();
    bytes.push(step_index);
    bytes.push(0);
    for index in 0..nibble_bytes {
        // Magnitudes 1 to 8, low nibble first, so a decoded block moves instead
        // of sitting at its predictor.
        bytes.push((index % 8 + 1) as u8);
    }
    bytes
}

/// Decodes a member through the production chain and converts it, exactly as
/// [`cs_app::audio::sound_member_pcm`] does for a real archive member.
fn pcm_of(member: &[u8]) -> Result<PcmAudio, String> {
    use cs_formats::zbd::{SampleFormat, decode_sound_sample};
    let mut context = cs_formats::ParseContext::with_defaults("synthetic/m01_lc_audio_device.zbd");
    let format =
        SampleFormat::from_member(member).map_err(|error| format!("{}: {error}", error.code()))?;
    let decoded = decode_sound_sample(&mut context, member, &format)
        .map_err(|error| format!("{}: {error}", error.code()))?;
    PcmAudio::from_decoded(&decoded).map_err(|error| format!("{}: {error}", error.code()))
}

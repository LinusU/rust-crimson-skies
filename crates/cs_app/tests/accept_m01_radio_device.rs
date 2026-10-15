//! Acceptance: M01's queued radio lines play through the device (Rally #1461,
//! `M01-RADIO-DEVICE`).
//!
//! Follow-up of #1438 (`M01-PLAY-RADIO`). #1438 offers M01's sound-group cues to
//! [`cs_app::audio::AudioSession`]'s radio queue, but nothing *played* them: the
//! mixer consumed only loop outcomes, and the audible device started only
//! infinite loops. This suite drives the missing hop through production code
//! only — the real [`AudioPlugin`] in a real Bevy `App`, the real radio queue,
//! the real [`cs_sim::audio_events::AudioMixer`] and real devices — and shows:
//!
//! * a queued line is voiced, ends after its measured tick length, and the line
//!   behind it begins (scenario A);
//! * losing the output mid-line keeps the queue's timing exactly, the subtitle
//!   and the line's place survive, and the line that starts while the output is
//!   gone is named as unvoiced rather than left silent (scenario B);
//! * a line whose dialogue member is not in the sample library is refused **by
//!   name** (`sample_unavailable`), and the queue still completes it on
//!   schedule (scenario C).
//!
//! All content is newly authored synthetic fixture data: no scenario reads
//! `CS_GAME_DIR`, and no original audio is involved. The owner's listening test
//! is the retail half and remains their gate (F41-D); nothing here claims a
//! person heard anything.
//!
//! Sensitivity (measured on this branch by removing each hop, then restoring):
//! with `mix_session` no longer calling `mix_radio` (the pre-#1461 state) all
//! three scenarios fail; with `advance_radio` no longer reading the ledger all
//! three fail on the completion tick; and with the radio path starting a loop
//! (`start_voice`) instead of `start_one_shot`, A and B fail because the
//! recording device logs `Started` and no one-shot. Scenario C drives the
//! non-looping source at its own level: [`LoopingVoice::one_shot`] stops at the
//! member's end.

use std::sync::Arc;

use bevy::app::App;
use bevy::prelude::{MinimalPlugins, TransformPlugin};
use cs_app::audio::{
    AudibleDevice, AudioMixReport, AudioMixing, AudioPlugin, AudioSession, AudioSpatial,
    InMemorySamples, LoopingVoice, PcmAudio, SampleLibrary, device_lost, device_restored,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::scene::{SceneGeneration, SceneGenerations};
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_sim::audio_events::{
    AudioBus, AudioEventId, AudioRouter, DeviceCommand, DeviceVoiceId, Listener, RADIO_GAIN,
    RadioLine, RadioMixRefusal, RecordingAudioDevice, SpatialPolicy, VoiceStart,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 11;
const GEN: u64 = 1;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn dialogue(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Dialogue, key).expect("a valid dialogue id")
}

/// A radio line whose measured length is `duration_ticks`.
fn line(sequence: u32, asset: &str, duration_ticks: u64) -> RadioLine {
    RadioLine::try_new(
        AudioEventId {
            session: session(),
            tick: Tick(0),
            producer: 1,
            sequence,
        },
        "Jack",
        dialogue(asset),
        1,
        true,
        Some("Jack: watch your six"),
        duration_ticks,
    )
    .expect("a valid synthetic line")
}

fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// Every non-looping start a recording device logged, in order.
fn one_shots(device: &RecordingAudioDevice) -> Vec<(DeviceVoiceId, VoiceStart)> {
    device
        .commands()
        .into_iter()
        .filter_map(|command| match command {
            DeviceCommand::OneShotStarted { voice, start } => Some((voice, start)),
            _ => None,
        })
        .collect()
}

/// A world with the production audio plugin, an explicitly installed session and
/// the fixed-tick ledger the mission host stamps ids with.
///
/// The session is installed by hand rather than through the loading handoff
/// because #1438's cue producer is what drives that path and it lands
/// separately; the radio API this drives (`AudioSession::enqueue_radio`) is the
/// one `offer_cues` calls.
struct RadioWorld {
    app: App,
    device: RecordingAudioDevice,
}

impl RadioWorld {
    fn new() -> Self {
        let device = RecordingAudioDevice::new();
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
                .with_device(Box::new(device.handle())),
        ));
        app.init_resource::<SceneGenerations>();
        app.insert_resource(AudioSession::new(
            AudioRouter::new(session()),
            SceneGeneration(GEN),
            Vec::new(),
        ));
        // The loading handoff installs the mixer alongside the session it
        // installs; this world has no delivered closure (#1438 drives that
        // path), so the mixer is installed by hand over the same session.
        app.insert_resource(AudioMixing::new(session()));
        app.insert_resource(PhysicsTickLedger::default());
        Self {
            app,
            device: device.handle(),
        }
    }

    /// Commits the fixed-tick ledger to `ticks`, so the frame's audio systems
    /// read it the way the mission host does.
    fn tick(&mut self, ticks: u64) {
        self.app
            .world_mut()
            .resource_mut::<PhysicsTickLedger>()
            .ticks = ticks;
    }

    fn update(&mut self) {
        self.app.update();
    }

    fn session(&self) -> &AudioSession {
        self.app.world().resource::<AudioSession>()
    }

    fn session_mut(&mut self) -> &mut AudioSession {
        self.app
            .world_mut()
            .resource_mut::<AudioSession>()
            .into_inner()
    }

    fn report(&self) -> &AudioMixReport {
        self.app.world().resource::<AudioMixReport>()
    }

    fn enqueue(&mut self, line: RadioLine) {
        self.session_mut().enqueue_radio(line);
    }
}

/// Two lines, the first longer than the second, so the completion of one is what
/// starts the other.
fn two_line_world() -> RadioWorld {
    let mut world = RadioWorld::new();
    world.update();
    world.enqueue(line(1, "synthetic.radio.jack.1", 10));
    world.enqueue(line(2, "synthetic.radio.jack.2", 5));
    world.update();
    world
}

/// Scenario A: a queued line becomes a device voice, ends at its measured tick,
/// and the line behind it begins in the same frame.
#[test]
fn accept_m01_radio_device_a_queued_line_is_voiced_then_ends_and_the_next_begins() {
    let mut world = two_line_world();
    let first = line(1, "synthetic.radio.jack.1", 10);
    let second = line(2, "synthetic.radio.jack.2", 5);

    // Only the active line is voiced, as a one-shot on the radio bus.
    let shots = one_shots(&world.device);
    assert_eq!(shots.len(), 1, "only the active line is voiced");
    let first_voice = shots[0].0;
    assert_eq!(shots[0].1.asset, first.asset, "the member the line named");
    assert_eq!(shots[0].1.bus, AudioBus::Radio);
    assert_eq!(shots[0].1.gain, RADIO_GAIN);
    assert_eq!(
        world.report().radio.started,
        vec![first.id],
        "the line is named as started in the mix report"
    );

    // Held for its measured length: nine ticks is not ten.
    world.tick(9);
    world.update();
    assert!(
        world.device.is_sounding(first_voice),
        "the line is still sounding one tick short of its measured length"
    );
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(first.id)
    );

    // The measured length ends the first line and starts the second, and the
    // device is told in the same frame.
    world.tick(10);
    world.update();
    assert!(
        !world.device.is_sounding(first_voice),
        "the first line's voice stopped at its measured length"
    );
    let shots = one_shots(&world.device);
    assert_eq!(shots.len(), 2, "the line behind it was voiced");
    let second_voice = shots[1].0;
    assert_eq!(shots[1].1.asset, second.asset);
    assert!(world.device.is_sounding(second_voice));
    assert_eq!(world.report().radio.stopped, vec![first.id]);
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(second.id)
    );

    // And it too ends, five ticks later, with nothing left sounding.
    world.tick(15);
    world.update();
    assert!(!world.device.is_sounding(second_voice));
    assert_eq!(world.report().radio.stopped, vec![second.id]);
    assert_eq!(world.device.sounding(), 0);
    assert!(
        world.report().radio.refusals.is_empty(),
        "nothing was refused: {:?}",
        world.report().radio.refusals
    );
}

/// Scenario B: losing the output mid-line moves no clock. The line still
/// completes at its measured tick, the next starts on that same tick, the
/// subtitle and the line's place survive, and the line that starts while the
/// output is gone is named rather than silent.
#[test]
fn accept_m01_radio_device_b_a_lost_device_keeps_the_queue_timing() {
    let mut world = two_line_world();
    let first = line(1, "synthetic.radio.jack.1", 10);
    let second = line(2, "synthetic.radio.jack.2", 5);
    assert_eq!(one_shots(&world.device).len(), 1);

    world.tick(4);
    world.update();
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(first.id)
    );

    // The output goes away: the session's own device-lost path stops the voices
    // and the queue keeps the line, its place and its subtitle.
    device_lost(world.app.world_mut());
    assert!(!world.session().device_available());
    assert_eq!(world.device.sounding(), 0, "no voice survives the loss");
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(first.id)
    );
    assert_eq!(
        world.session().radio.subtitle(),
        Some("Jack: watch your six"),
        "the subtitle is shown whether or not the line is voiced"
    );

    // The loss is reported against the line by the queue, and the mixer keeps
    // the fact after it has read the event.
    world.tick(5);
    world.update();
    assert!(
        world.report().radio.lost.contains(&first.id),
        "the loss is named against the line: {:?}",
        world.report().radio
    );

    // Nine ticks after the start the line is *not* done, exactly as in the
    // voiced run: the loss moved no clock.
    world.tick(9);
    world.update();
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(first.id),
        "the line did not complete early"
    );

    // At its measured length it completes and the next begins, unvoiced.
    world.tick(10);
    world.update();
    assert_eq!(
        world.session().radio.active().map(|line| line.id),
        Some(second.id),
        "the next line started on the tick the voiced run started it"
    );
    assert!(
        world.report().radio.refusals.iter().any(|refusal| matches!(
            refusal,
            RadioMixRefusal::Unvoiced { id, .. } if *id == second.id
        )),
        "the line that started while the output was gone is named, not silent: {:?}",
        world.report().radio.refusals
    );
    assert_eq!(world.device.sounding(), 0, "nothing played while lost");
    assert!(
        !world.report().radio.started.contains(&second.id),
        "a line started while the output was gone is never voiced"
    );

    // The device returns: the running line stays unvoiced to its end, and a
    // later line is voiced again.
    device_restored(world.app.world_mut()).expect("the recording device reopens");
    world.device.clear();
    world.enqueue(line(3, "synthetic.radio.jack.3", 2));
    world.tick(15);
    world.update();
    let shots = one_shots(&world.device);
    assert_eq!(
        shots.len(),
        1,
        "only the line that started after the restore"
    );
    assert_eq!(shots[0].1.asset, line(3, "synthetic.radio.jack.3", 2).asset);
    assert!(
        !world.report().radio.started.contains(&second.id),
        "the line already running is not re-voiced mid-sentence"
    );
}

/// Scenario C: a dialogue member the device's library does not hold is refused
/// by its own stable code, a voiced completion with no voice behind it is named,
/// and the queue still completes the line on schedule.
#[test]
fn accept_m01_radio_device_c_a_missing_sample_is_refused_by_name() {
    // The real audible device with nothing in its library: `unopened` grants no
    // output, and the missing member is named before the open state matters.
    let library: Arc<dyn SampleLibrary> = Arc::new(InMemorySamples::new());
    let device = AudibleDevice::unopened(Arc::clone(&library));
    let mut app = App::new();
    app.add_plugins((
        MinimalPlugins,
        TransformPlugin,
        AudioPlugin::new(declared_synthetic_audio_catalog(), spatial())
            .with_device(Box::new(device)),
    ));
    app.init_resource::<SceneGenerations>();
    app.insert_resource(AudioSession::new(
        AudioRouter::new(session()),
        SceneGeneration(GEN),
        Vec::new(),
    ));
    app.insert_resource(AudioMixing::new(session()));
    app.insert_resource(PhysicsTickLedger::default());
    let queued = line(1, "synthetic.radio.jack.1", 3);
    app.world_mut()
        .resource_mut::<AudioSession>()
        .enqueue_radio(queued.clone());
    app.update();

    let refusals = app
        .world()
        .resource::<AudioMixReport>()
        .radio
        .refusals
        .clone();
    assert!(
        refusals.iter().any(|refusal| matches!(
            refusal,
            RadioMixRefusal::Device { code, .. }
                if *code == cs_app::audio::CODE_SAMPLE_UNAVAILABLE
        )),
        "the missing member is named with the device's own code: {refusals:?}"
    );

    // The queue is independent of the device: the line completes on its measured
    // tick, and its voiced completion with no voice behind it is named too.
    app.world_mut().resource_mut::<PhysicsTickLedger>().ticks = 3;
    app.update();
    assert!(
        app.world()
            .resource::<AudioSession>()
            .radio
            .active()
            .is_none(),
        "the queue completed the line whether or not it sounded"
    );
    let report = app.world().resource::<AudioMixReport>();
    assert!(
        report.radio.stopped.is_empty(),
        "there was no voice to stop"
    );
    assert!(
        report
            .radio
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, RadioMixRefusal::NoVoice { id } if *id == queued.id)),
        "a voiced completion that never sounded is named: {:?}",
        report.radio.refusals
    );

    // The one-shot source really ends: the same samples through `one_shot` stop
    // at the member's own end, which is what a refused voice would have played.
    let pcm = PcmAudio::try_new(1, 8_000, vec![0.5; 16]).expect("a valid mono asset");
    let mut voice = LoopingVoice::one_shot(&pcm, 0.0);
    let mut emitted = 0;
    while voice.next().is_some() {
        emitted += 1;
        assert!(emitted <= 64, "a one-shot source ends");
    }
    assert_eq!(emitted, 32, "two values per mono frame, once");
}

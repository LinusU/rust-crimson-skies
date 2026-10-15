//! Acceptance: M01's voice members reach the device's sample library under the
//! exact dialogue id a radio line names (Rally #1465,
//! `M01-RADIO-SAMPLE-IDENTITY`).
//!
//! Follow-up of #1461 (`M01-RADIO-DEVICE`). #1438 (`M01-PLAY-RADIO`) names each
//! spoken member with a [`ContentKind::Dialogue`] id over F14-D.7's
//! `install_file_key("<container>/<member>")` spelling, but nothing put that
//! member's samples in the library the device reads, so the line was refused
//! `sample_unavailable`. The owner ruled (task #1465): the id is that same
//! dialogue id, and the prepare step publishes the members it decoded into the
//! library under it. This suite drives that publish through production code
//! only — the real [`cs_app::audio::publish_voice_members`], the real
//! [`AudibleDevice`]'s own lookup, and the real mixer in a real Bevy `App`:
//!
//! * a published member is **delivered**: the device finds its samples and the
//!   mixer opens a one-shot for the line on the radio bus (scenario A);
//! * an id the prepare did not deliver is refused `sample_unavailable` by name,
//!   and the *sound* id over the same key is a different asset that does not
//!   satisfy the line (scenario B);
//! * a real M01 voice member read from `CS_GAME_DIR` reaches the library under
//!   its dialogue id (scenario C, `retail`).
//!
//! No scenario claims a person heard anything: the owner's listening test stays
//! the F41-D gate, and no original executable ran.

use std::sync::Arc;

use bevy::app::App;
use bevy::prelude::{MinimalPlugins, TransformPlugin};
use cs_app::audio::{
    AudibleDevice, AudioMixReport, AudioMixing, AudioPlugin, AudioSession, AudioSpatial,
    CODE_DEVICE_CLOSED, CODE_SAMPLE_UNAVAILABLE, DecodedVoiceMember, InMemorySamples, PcmAudio,
    SampleLibrary, publish_voice_members, voice_member_id,
};
use cs_app::physics::PhysicsTickLedger;
use cs_app::scene::{SceneGeneration, SceneGenerations};
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_content::catalog::baseline::install_file_key;
use cs_sim::audio_events::{
    AudioBus, AudioDevice, AudioEventId, AudioRouter, DeviceCommand, DeviceVoiceId, Listener,
    RADIO_GAIN, RadioLine, RadioMixRefusal, RecordingAudioDevice, SpatialPolicy, VoiceStart,
};
use cs_types::Tick;
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 21;
const GEN: u64 = 1;

/// The container M01's voice members are read from: the labelled choice #1438
/// measured (`soundsh` and `soundsl` hold the same M01 members; which the
/// original opens is unmeasured, F14-D.7).
const CONTAINER: &str = "ZBD/soundsh.zbd";

/// One real M01 voice member, measured by #1438 (`M01-PLAY-RADIO`): the first
/// line of the mission's opening crew conversation.
const M01_MEMBER: &str = "VO_c2-NW-m1_WorkersVoyage_1.wav";

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// A radio line naming `asset` that lasts `duration_ticks`.
fn line(sequence: u32, asset: ContentId, duration_ticks: u64) -> RadioLine {
    RadioLine::try_new(
        AudioEventId {
            session: session(),
            tick: Tick(0),
            producer: 1,
            sequence,
        },
        "WorkersVoyage",
        asset,
        1,
        true,
        None,
        duration_ticks,
    )
    .expect("a valid synthetic line")
}

/// The start the mixer emits for a radio line: the radio bus at unity gain.
fn start(asset: &ContentId) -> VoiceStart {
    VoiceStart {
        asset: asset.clone(),
        bus: AudioBus::Radio,
        gain: RADIO_GAIN,
        pan: 0.0,
        pitch: 1.0,
    }
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
    recording: Option<RecordingAudioDevice>,
}

impl RadioWorld {
    /// A world mixing to the recording stand-in, with its log readable.
    fn new() -> Self {
        let device = RecordingAudioDevice::new();
        let mut world = Self::with_device(Box::new(device.handle()));
        world.recording = Some(device);
        world
    }

    /// A world mixing to `device`.
    fn with_device(device: Box<dyn AudioDevice>) -> Self {
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AudioPlugin::new(declared_synthetic_audio_catalog(), spatial()).with_device(device),
        ));
        app.init_resource::<SceneGenerations>();
        app.insert_resource(AudioSession::new(
            AudioRouter::new(session()),
            SceneGeneration(GEN),
            Vec::new(),
        ));
        app.insert_resource(AudioMixing::new(session()));
        app.insert_resource(PhysicsTickLedger::default());
        Self {
            app,
            recording: None,
        }
    }

    /// Every non-looping start the recording stand-in logged.
    fn shots(&self) -> Vec<(DeviceVoiceId, VoiceStart)> {
        one_shots(
            self.recording
                .as_ref()
                .expect("a world built on the recording stand-in"),
        )
    }

    fn tick(&mut self, ticks: u64) {
        self.app
            .world_mut()
            .resource_mut::<PhysicsTickLedger>()
            .ticks = ticks;
    }

    fn update(&mut self) {
        self.app.update();
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

/// Scenario A: the prepare step publishes a decoded member and the device finds
/// it — the line is delivered, voiced on the radio bus, and refused nothing.
#[test]
fn accept_m01_radio_sample_a_a_published_member_is_delivered_and_voiced_under_its_dialogue_id() {
    let id = voice_member_id(CONTAINER, M01_MEMBER).expect("the member derives a dialogue id");
    assert_eq!(id.kind(), ContentKind::Dialogue);
    // The identity is exactly what the radio prepare mints — Dialogue over the
    // F14-D.7 key — and is a different asset from the sound id over the same key.
    let key = install_file_key(&format!("{CONTAINER}/{M01_MEMBER}"));
    assert_eq!(
        id,
        ContentId::from_source(ContentKind::Dialogue, &key).expect("the same derived id")
    );
    assert_ne!(
        id,
        ContentId::from_source(ContentKind::Sound, &key).expect("the sound id over the same key")
    );

    let library: Arc<dyn SampleLibrary> = Arc::new(InMemorySamples::new());
    assert!(
        library.pcm(&id).is_none(),
        "nothing is delivered before the prepare step publishes"
    );
    let pcm = PcmAudio::try_new(1, 11_025, vec![0.25; 32]).expect("a valid mono asset");
    let member =
        DecodedVoiceMember::new(CONTAINER, M01_MEMBER, pcm.clone()).expect("a valid voice member");
    assert_eq!(member.id(), &id, "the member carries the line's own id");
    let publication = publish_voice_members(library.as_ref(), std::slice::from_ref(&member));
    assert_eq!(publication.published, 1);
    assert_eq!(
        library.pcm(&id),
        Some(pcm),
        "the member is in the library under the dialogue id"
    );

    // The device's own lookup: the samples are found, so only the absent output
    // refuses. A member the library does not hold is refused earlier, by name.
    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let refusal = device
        .start_one_shot(start(&id))
        .expect_err("an unopened device grants no output");
    assert_eq!(
        refusal.code, CODE_DEVICE_CLOSED,
        "the device found the samples and refused only the absent output: {refusal}"
    );

    // Through the real mixer: the line reaches the device as a one-shot on the
    // radio bus carrying that exact asset.
    let mut world = RadioWorld::new();
    world.update();
    let queued = line(1, id.clone(), 4);
    world.enqueue(queued.clone());
    world.update();

    let shots = world.shots();
    assert_eq!(shots.len(), 1, "the line is voiced exactly once");
    assert_eq!(shots[0].1.asset, id);
    assert_eq!(shots[0].1.bus, AudioBus::Radio);
    assert_eq!(shots[0].1.gain, RADIO_GAIN);
    assert_eq!(world.report().radio.started, vec![queued.id]);
    assert!(
        world.report().radio.refusals.is_empty(),
        "a delivered member is refused nothing: {:?}",
        world.report().radio.refusals
    );

    // The queue is unchanged: the line ends at its own measured length and the
    // voice stops with it.
    world.tick(4);
    world.update();
    assert_eq!(world.report().radio.stopped, vec![queued.id]);
}

/// Scenario B: an id the prepare did not deliver is refused by the device's own
/// stable code, and the sound id over the same key is a different asset that
/// does not satisfy the line.
#[test]
fn accept_m01_radio_sample_b_an_id_the_prepare_did_not_deliver_is_refused_by_name() {
    let id = voice_member_id(CONTAINER, M01_MEMBER).expect("the member derives a dialogue id");
    let key = install_file_key(&format!("{CONTAINER}/{M01_MEMBER}"));
    let sound = ContentId::from_source(ContentKind::Sound, &key).expect("the sound id");

    // The sound id is present; the dialogue id is not. The line names the
    // dialogue id, and the sound samples do not stand in for it.
    let mut samples = InMemorySamples::new();
    samples.insert(
        sound.clone(),
        PcmAudio::try_new(1, 11_025, vec![0.5; 16]).expect("a valid mono asset"),
    );
    let library: Arc<dyn SampleLibrary> = Arc::new(samples);
    assert!(
        library.pcm(&sound).is_some(),
        "the sound member is delivered"
    );
    assert!(
        library.pcm(&id).is_none(),
        "a sound id is not the dialogue id a radio line names"
    );

    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let refusal = device
        .start_one_shot(start(&id))
        .expect_err("the dialogue id is not in the library");
    assert_eq!(
        refusal.code, CODE_SAMPLE_UNAVAILABLE,
        "the undelivered member is named with the device's own code: {refusal}"
    );

    // Through the real mixer: the same device, as the world's output, names the
    // refusal against the line by its own code, and the queue still completes
    // the line on schedule.
    let mut world =
        RadioWorld::with_device(Box::new(AudibleDevice::unopened(Arc::clone(&library))));
    world.update();
    let queued = line(1, id.clone(), 3);
    world.enqueue(queued.clone());
    world.update();

    assert!(
        world.report().radio.refusals.iter().any(|refusal| matches!(
            refusal,
            RadioMixRefusal::Device { id: refused, code, .. }
                if *refused == queued.id && *code == CODE_SAMPLE_UNAVAILABLE
        )),
        "the line is refused by name: {:?}",
        world.report().radio.refusals
    );
    assert!(
        !world.report().radio.started.contains(&queued.id),
        "nothing was voiced for a member the prepare did not deliver"
    );

    world.tick(3);
    world.update();
    assert!(
        world
            .app
            .world()
            .resource::<AudioSession>()
            .radio
            .active()
            .is_none(),
        "the queue completed the line whether or not it sounded"
    );
    assert!(
        world.report().radio.refusals.iter().any(|refusal| matches!(
            refusal,
            RadioMixRefusal::NoVoice { id } if *id == queued.id
        )),
        "a voiced completion that never sounded is named: {:?}",
        world.report().radio.refusals
    );
}

/// Scenario C: one real M01 voice member is read from the installation, decoded
/// with the production reader, and published under the dialogue id a radio line
/// names — the device then finds it.
#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_radio_sample_c_a_real_m01_voice_member_reaches_the_library_under_its_dialogue_id() {
    let Ok(game_dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is not set: this scenario reads a real M01 voice member")
    };

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
        &cs_types::asset_id::AssetKey::from_spelling("install", CONTAINER, "default")
            .expect("the sound family's own container key"),
    )
    .expect("ZBD/soundsh.zbd routes to the sound family");
    let mut parse = cs_formats::ParseContext::with_defaults(container.label());
    let index = container.index(&mut parse).expect("its trailer indexes");
    let table = index.member_table();
    let assets = container
        .sound_assets(&mut parse, &index, &table)
        .expect("its sound archive reads");

    let asset = assets
        .entries()
        .iter()
        .find(|asset| String::from_utf8_lossy(asset.name()).eq_ignore_ascii_case(M01_MEMBER))
        .unwrap_or_else(|| panic!("{CONTAINER} declares no member {M01_MEMBER:?}"));
    let name = String::from_utf8_lossy(asset.name()).into_owned();
    let pcm = cs_app::audio::sound_member_pcm(asset, &mut parse)
        .expect("the member decodes under its own header");
    assert!(pcm.frames() > 0, "member {name:?} decoded to no frames");

    let id = voice_member_id(CONTAINER, &name).expect("the member derives a dialogue id");
    assert_eq!(id.kind(), ContentKind::Dialogue);
    assert_eq!(
        id,
        ContentId::from_source(
            ContentKind::Dialogue,
            &install_file_key(&format!("{CONTAINER}/{name}"))
        )
        .expect("the same derived id")
    );

    let library: Arc<dyn SampleLibrary> = Arc::new(InMemorySamples::new());
    let member =
        DecodedVoiceMember::new(CONTAINER, &name, pcm.clone()).expect("a valid voice member");
    assert_eq!(
        publish_voice_members(library.as_ref(), &[member]).published,
        1
    );
    assert_eq!(
        library.pcm(&id),
        Some(pcm),
        "the real member is in the library under the dialogue id a line names"
    );

    // The device's own lookup tells the delivered member from one it was never
    // given: the real member is found (only the absent output refuses), and an
    // id the prepare did not deliver is refused by name instead.
    let mut device = AudibleDevice::unopened(Arc::clone(&library));
    let found_refusal = device
        .start_one_shot(start(&id))
        .expect_err("an unopened device grants no output");
    assert_eq!(
        found_refusal.code, CODE_DEVICE_CLOSED,
        "the device found the real member and refused only the output: {found_refusal}"
    );
    let absent = voice_member_id(CONTAINER, "VO_c2-NW-m1_nobody_99.wav")
        .expect("the absent member derives a dialogue id");
    let absent_refusal = device
        .start_one_shot(start(&absent))
        .expect_err("a member the prepare did not deliver is refused");
    assert_eq!(
        absent_refusal.code, CODE_SAMPLE_UNAVAILABLE,
        "the undelivered member is named with the device's own code: {absent_refusal}"
    );
}

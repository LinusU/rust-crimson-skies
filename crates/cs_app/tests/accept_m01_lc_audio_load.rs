//! Acceptance scenarios M01-LC-AUDIO-LOAD: a delivered F15 load closure fills
//! the sample library the audible device plays from (task #652).
//!
//! Task test prefix: `accept_m01_lc_audio_load_`. Spec:
//! `specs/F41-audio-music-radio-dialogue-and-spatial-mixing.md`, stage
//! `### F41-D`, on top of F14-D.7's sound-member identity. Finding this
//! closes the last hop of: `docs/findings/2026-10-05-m01-lc-audible-audio-device.md`
//! (recorded here: `docs/findings/2026-10-06-m01-lc-audio-load.md`).
//!
//! Every scenario drives production code only: the real [`AudioPlugin`], the
//! real F15 [`LoadTransaction`] and its controlled [`ExpectedLoad`] handoff,
//! the real VFS session over a fixture installation, the real ZBD sound
//! reader, and `sound_member_pcm` — the same consumer of the F06-C decode the
//! device uses. The fixture is authored here: no test reads `CS_GAME_DIR`, and
//! no original audio is involved.
//!
//! The sensitivity of each scenario is direct. Removing
//! [`cs_app::audio::AudioSampleSource`]'s read from the handoff leaves the
//! library empty, so the device refuses every voice by name; removing the
//! wholesale replace leaves the previous closure's samples reachable; removing
//! the per-member refusal leaves an undecodable member in the library or drops
//! it without a name.

use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::app::App;
use bevy::ecs::world::World;
use bevy::prelude::{MinimalPlugins, TransformPlugin};
use cs_app::audio::{
    AudibleDevice, AudioHandoffLog, AudioHandoffRefusal, AudioPlugin, AudioSampleSource,
    AudioSpatial, CODE_DEVICE_CLOSED, CODE_SAMPLE_ABSENT, CODE_SAMPLE_UNAVAILABLE,
    CapabilityDeclaration, InMemorySamples, LoopingVoice, SampleLibrary,
};
use cs_app::loading::{
    CompletionVerdict, Criticality, ExpectedLoad, IoOutcome, LoadItem, LoadRequest, LoadState,
    LoadTarget, LoadTransaction,
};
use cs_app::scene::SceneGenerations;
use cs_assets::install;
use cs_assets::vfs::{ContentSession, SessionBuilder};
use cs_content::audio::{
    AudioAssetRecord, AudioBus, AudioCatalog, AudioDraft, AudioLevel, AudioPlayback, DecodedPcm,
    PlaybackMode,
};
use cs_content::catalog::baseline::install_file_key;
use cs_sim::audio_events::{
    AudioBus as RuntimeBus, AudioDevice, Listener, SpatialPolicy, VoiceStart,
};
use cs_types::asset_id::{AssetKey, ResolveContext, WorldGroup};
use cs_types::content::{ContentId, ContentKind, Known, Origin, Provenance, Resolved};
use cs_types::evidence::{ClaimId, ContentHash};
use cs_types::net::SessionId;

/// The fixture sound container, spelled as an installation spells it.
const CONTAINER: &str = "ZBD/soundsl.zbd";
/// A member this container declares and the load delivers.
const ENGINE: &str = "engine.wav";
/// A second member, delivered only by the second load of a reload scenario.
const CLICK: &str = "click.wav";
/// A member whose bytes are not a RIFF/WAVE file, so its own header refuses it.
const BROKEN: &str = "broken.wav";
/// A member no fixture container declares: its id is delivered, its container
/// does not carry it.
const ABSENT: &str = "ZBD/soundsh.zbd/ghost.wav";

/// The rate every fixture member declares.
const RATE: u32 = 11_025;

/// Eight authored 8-bit mono frames for [`ENGINE`]: distinct, bounded, and
/// deliberately not silence or a plateau, so a decode that dropped, reordered
/// or mis-scaled a value changes a sample the test reads.
const ENGINE_FRAMES: [u8; 8] = [0, 64, 128, 192, 255, 1, 2, 3];

/// Four authored 8-bit mono frames for [`CLICK`].
const CLICK_FRAMES: [u8; 4] = [200, 100, 50, 250];

/// The refusal a member whose own header does not read carries, through
/// `SampleError`'s `UnreadableHeader` (see `cs_formats::zbd::wave`).
const UNREADABLE_HEADER: &str = "unreadable_header";

// ------------------------------------------------------------- the fixture --

/// A disposable installation tree, removed on drop.
struct TempInstall(PathBuf);

impl TempInstall {
    fn new(label: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "cs-m01-lc-audio-load-{label}-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).expect("the fixture directory is created");
        Self(root)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    fn write(&self, relative: &str, bytes: &[u8]) {
        let path = self.0.join(relative);
        fs::create_dir_all(path.parent().expect("a parent directory"))
            .expect("the fixture directory is created");
        fs::write(&path, bytes).expect("the fixture file is written");
    }
}

impl Drop for TempInstall {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

/// One authored RIFF chunk: id, size, payload, and the pad byte an odd payload
/// needs (IBM/Microsoft 1991, "RIFF File Format").
fn chunk(id: &[u8; 4], payload: &[u8]) -> Vec<u8> {
    let mut bytes = id.to_vec();
    bytes.extend_from_slice(&(u32::try_from(payload.len()).expect("fits")).to_le_bytes());
    bytes.extend_from_slice(payload);
    if payload.len() % 2 == 1 {
        bytes.push(0);
    }
    bytes
}

/// A `fmt ` payload: the 16 common bytes of an 8-bit mono PCM declaration.
fn mono_pcm_fmt(rate: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u16.to_le_bytes()); // wFormatTag: WAVE_FORMAT_PCM
    bytes.extend_from_slice(&1u16.to_le_bytes()); // nChannels
    bytes.extend_from_slice(&rate.to_le_bytes()); // nSamplesPerSec
    bytes.extend_from_slice(&rate.to_le_bytes()); // nAvgBytesPerSec
    bytes.extend_from_slice(&1u16.to_le_bytes()); // nBlockAlign
    bytes.extend_from_slice(&8u16.to_le_bytes()); // wBitsPerSample
    bytes
}

/// `RIFF`, its size, `WAVE` and `chunks`.
fn wave(chunks: &[Vec<u8>]) -> Vec<u8> {
    let body: Vec<u8> = chunks.concat();
    let mut bytes = b"RIFF".to_vec();
    bytes.extend_from_slice(&(u32::try_from(body.len()).expect("fits") + 4).to_le_bytes());
    bytes.extend_from_slice(b"WAVE");
    bytes.extend_from_slice(&body);
    bytes
}

/// One 8-bit mono PCM member of `samples` frames at `rate`.
fn pcm_member(rate: u32, samples: &[u8]) -> Vec<u8> {
    wave(&[chunk(b"fmt ", &mono_pcm_fmt(rate)), chunk(b"data", samples)])
}

/// A sound container laid out the way the version-one trailer reader expects
/// (task #343): member data first, then one 148-byte entry per member, then
/// the version word and the member count.
fn sound_archive(members: &[(&str, &[u8])]) -> Vec<u8> {
    let mut data = Vec::new();
    let mut entries = Vec::new();
    for (name, bytes) in members {
        let start = u32::try_from(data.len()).expect("a fixture member fits");
        let length = u32::try_from(bytes.len()).expect("a fixture member fits");
        data.extend_from_slice(bytes);
        entries.extend_from_slice(&start.to_le_bytes());
        entries.extend_from_slice(&length.to_le_bytes());
        let mut field = vec![0u8; 64];
        field[..name.len()].copy_from_slice(name.as_bytes());
        entries.extend_from_slice(&field);
        entries.extend_from_slice(&[0u8; 76]);
    }
    data.extend_from_slice(&entries);
    data.extend_from_slice(&1u32.to_le_bytes());
    data.extend_from_slice(&u32::try_from(members.len()).expect("fits").to_le_bytes());
    data
}

/// The fixture installation: one sound container holding every member the
/// scenarios below deliver, refuse or look for in vain.
fn install_tree(label: &str) -> TempInstall {
    let temp = TempInstall::new(label);
    temp.write(
        CONTAINER,
        &sound_archive(&[
            (ENGINE, &pcm_member(RATE, &ENGINE_FRAMES)),
            (CLICK, &pcm_member(RATE, &CLICK_FRAMES)),
            (BROKEN, b"this member is not a RIFF/WAVE file"),
        ]),
    );
    temp
}

/// A content session mounted over `install` with the production layout.
///
/// Every call opens a session under a **fresh** generation, which is what
/// makes a reload observable: the second load runs under a different content
/// session than the first, exactly as a real reload does.
fn content_session(tree: &TempInstall) -> ContentSession {
    let found = install::discover(tree.path()).expect("the fixture installation is discovered");
    let context = ResolveContext::new(install::fingerprint(&found.manifest));
    let mut builder = SessionBuilder::new(context);
    builder
        .mount_installation(tree.path(), &found.diagnosis)
        .expect("the fixture installation mounts");
    builder.open()
}

/// The F14-D.7 content id of the member `container` declares under `name`.
fn member_id(container: &str, name: &str) -> ContentId {
    ContentId::from_source(
        ContentKind::Sound,
        &install_file_key(&format!("{container}/{name}")),
    )
    .expect("a fixture member id is valid")
}

/// The key the closure delivers that member under: the container itself.
fn container_key() -> AssetKey {
    AssetKey::from_spelling("install", CONTAINER, "default").expect("a valid container key")
}

/// A declared audio record with known playback metadata, so lowering never
/// refuses the fixture for an unknown bus, level or mode.
fn record(id: ContentId, bus: AudioBus, mode: PlaybackMode, frames: u64) -> AudioAssetRecord {
    let designed = || {
        Provenance::designed(ClaimId::new("m01-lc-audio-load.fixture").expect("a valid claim id"))
    };
    AudioAssetRecord::try_new(AudioDraft {
        playback: AudioPlayback {
            bus: Resolved::Known(Known::new(bus, designed())),
            level: Resolved::Known(Known::new(
                AudioLevel::try_new(1.0).expect("a fixture level"),
                designed(),
            )),
            mode: Resolved::Known(Known::new(mode, designed())),
        },
        decoded: Resolved::Known(Known::new(
            DecodedPcm::try_new(frames, 1, RATE).expect("a fixture shape"),
            designed(),
        )),
        id,
        origin: Origin::SyntheticFixture,
        provenance: designed(),
    })
    .expect("a fixture record is valid")
}

/// The declared catalog these scenarios deliver into.
fn fixture_catalog() -> AudioCatalog {
    let mut catalog = AudioCatalog::new();
    for (id, bus, mode, frames) in [
        (
            member_id(CONTAINER, ENGINE),
            AudioBus::Engine,
            PlaybackMode::Loop,
            ENGINE_FRAMES.len() as u64,
        ),
        (
            member_id(CONTAINER, CLICK),
            AudioBus::Ui,
            PlaybackMode::OneShot,
            CLICK_FRAMES.len() as u64,
        ),
        (
            member_id(CONTAINER, BROKEN),
            AudioBus::Impacts,
            PlaybackMode::OneShot,
            0,
        ),
        (
            member_id(ABSENT, "ghost.wav"),
            AudioBus::Environment,
            PlaybackMode::Loop,
            4,
        ),
    ] {
        catalog
            .insert(record(id, bus, mode, frames))
            .expect("fixture ids are unique");
    }
    catalog
}

/// A load item delivering one declared audio member of the fixture container.
fn sound_item(id: &ContentId) -> LoadItem {
    LoadItem::new(
        container_key(),
        id.clone(),
        Criticality::GameplayCritical,
        64,
    )
    .expect("nonzero work units")
}

/// Runs one load over `items` and attaches its bundle through the production
/// [`ExpectedLoad`] handoff, returning the content session it ran under.
///
/// `publish` states whether that same session is published as the
/// [`AudioSampleSource`] the population pass decodes through. The scenario
/// decides: a reload whose source is stale or absent is exactly what two of
/// the tests below are about.
fn deliver(
    world: &mut World,
    install: &TempInstall,
    items: Vec<LoadItem>,
    publish: bool,
) -> SessionId {
    let session = content_session(install);
    let generation = session.generation();
    if publish {
        world.insert_resource(AudioSampleSource::of(session));
    }
    let mut transaction = LoadTransaction::issue(LoadRequest {
        session: generation,
        target: LoadTarget::world(WorldGroup::new("zbd/c1").expect("a valid world group")),
        items,
    });
    transaction.begin().expect("the load begins");
    for index in 0..transaction.items().len() {
        let ticket = transaction.issue_io(index).expect("the read is issued");
        let verdict = transaction.accept(ticket.complete(IoOutcome::Read {
            payload_sha256: ContentHash::from_bytes([index as u8 + 1; 32]),
        }));
        assert_eq!(
            verdict,
            CompletionVerdict::Accepted,
            "item {index} settles as accepted"
        );
    }
    assert_eq!(
        transaction.state(),
        LoadState::Validating,
        "a fully settled load validates itself"
    );
    transaction.validate().expect("the load validates");
    let bundle = transaction.ready_bundle().expect("a ready bundle");
    let id = SessionId::new(bundle.identity().session.get())
        .expect("a content session generation is nonzero");
    world.insert_resource(ExpectedLoad(bundle.identity()));
    bundle
        .attach(world, bundle.identity())
        .expect("the announced bundle attaches");
    world.remove_resource::<ExpectedLoad>();
    id
}

// ------------------------------------------------------------- the world ----

/// The fixture spatial configuration: the same designed values the F41-B
/// wiring scenarios use. Nothing here measures an original attenuation curve.
fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// A world with the production audio plugin, the fixture installation, and the
/// one library the audible backend holds and the load fills.
struct Fixture {
    app: App,
    install: TempInstall,
    library: Arc<InMemorySamples>,
}

impl Fixture {
    /// Builds the world and runs nothing yet: the first [`Fixture::load`]
    /// delivers the closure whose population these scenarios are about.
    fn new(label: &str) -> Self {
        let install = install_tree(label);
        let library = Arc::new(InMemorySamples::new());
        let mut app = App::new();
        app.add_plugins((
            MinimalPlugins,
            TransformPlugin,
            AudioPlugin::new(fixture_catalog(), spatial())
                // The library the load fills is the library the device reads:
                // the plugin hands the same object to both.
                .audible(library.clone())
                // No capability is declared, so the gate refuses before any
                // hardware call. These scenarios are about what reaches the
                // library; an output device is not one of their premises, and
                // CI has none.
                .with_capabilities(CapabilityDeclaration::default()),
        ));
        app.init_resource::<SceneGenerations>();
        // The scene load path consumes a generation before its load, and the
        // handoff reads the same counter.
        app.world_mut()
            .resource_mut::<SceneGenerations>()
            .take_next();
        Self {
            app,
            install,
            library,
        }
    }

    /// Delivers one closure over `ids`, and publishes (or does not publish)
    /// the sample source of the content session it ran under.
    fn load(&mut self, ids: &[ContentId], publish: bool) -> SessionId {
        let items = ids.iter().map(sound_item).collect();
        deliver(self.app.world_mut(), &self.install, items, publish)
    }

    /// Runs one frame: `insert_audio_session` — session, mixer and population
    /// pass — runs in `PreUpdate` of it.
    fn update(&mut self) {
        self.app.update();
    }

    /// The handoff's own record of what this world installed and refused.
    fn log(&self) -> AudioHandoffLog {
        self.app.world().resource::<AudioHandoffLog>().clone()
    }

    /// What the handoff installed, if anything.
    fn installed(&self) -> cs_app::audio::AudioInstall {
        self.log()
            .installed
            .clone()
            .expect("the delivered load installed an audio session")
    }

    /// The `SampleUndecodable` refusals of the last install, by content id.
    fn undecodable(&self) -> Vec<(ContentId, &'static str, String)> {
        self.log()
            .refusals
            .iter()
            .filter_map(|refusal| match refusal {
                AudioHandoffRefusal::SampleUndecodable {
                    content,
                    code,
                    detail,
                } => Some((content.clone(), *code, detail.clone())),
                _ => None,
            })
            .collect()
    }
}

// -------------------------------------------------------------- scenarios ---

/// A world whose load delivered audio content gets a populated library from
/// that same closure, and the device reads exactly what it holds.
///
/// This is the hop task #652 exists for: nothing fills the library, and every
/// `VoiceStart` for a delivered asset is refused as `sample_unavailable`
/// however real the device, plugin and mixer are.
#[test]
fn accept_m01_lc_audio_load_a_delivered_closure_populates_the_library_the_device_reads() {
    let mut fixture = Fixture::new("populate");
    let engine = member_id(CONTAINER, ENGINE);
    let click = member_id(CONTAINER, CLICK);

    fixture.load(std::slice::from_ref(&engine), true);
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(
        installed.samples, 1,
        "the delivered member's samples are in the library"
    );
    assert_eq!(installed.sample_refusals, 0, "and nothing was refused");
    assert_eq!(
        installed.specs, 1,
        "the same member lowered into a routing spec: one closure, both halves"
    );
    assert!(
        fixture.log().refusals.is_empty(),
        "a clean load refuses nothing: {:?}",
        fixture.log().refusals
    );

    // The samples are the member's own, normalized under its own declaration:
    // 8-bit RIFF/WAVE PCM is unsigned with silence at 128.
    let pcm = fixture
        .library
        .pcm(&engine)
        .expect("the delivered member is in the library");
    assert_eq!(pcm.channels(), 1, "the member's own channel count");
    assert_eq!(pcm.rate_hz(), RATE, "the member's own rate");
    assert_eq!(pcm.frames(), ENGINE_FRAMES.len() as u64);
    assert_eq!(pcm.samples()[0], -1.0, "0 decodes onto -1.0");
    assert_eq!(pcm.samples()[1], -0.5, "64 decodes onto -0.5");
    assert!(
        fixture.library.pcm(&click).is_none(),
        "a member the closure did not deliver is not reachable"
    );

    // The library is the device's, not a side table: the voice source the
    // device appends carries these samples, placed by the pan it is built
    // with. At hard left the constant-power law is exact (cos 0 = 1), so the
    // emitted values are the member's own.
    let mut voice = LoopingVoice::new(&pcm, -1.0);
    assert_eq!(voice.next(), Some(-1.0), "the member's first frame, left");
    assert_eq!(
        voice.next(),
        Some(0.0),
        "the same frame, right: hard left is silent"
    );
    assert_eq!(voice.next(), Some(-0.5), "the member's second frame, left");

    // And the device's own lookup agrees: an unopened device resolves the
    // asset before it checks its own state, so a delivered member is refused
    // as `device_closed` while anything else is refused as
    // `sample_unavailable` — the two refusals are how a reader can tell "the
    // load never delivered it" from "the device is shut".
    let mut device = AudibleDevice::unopened(fixture.library.clone());
    let delivered = device
        .start_voice(VoiceStart {
            asset: engine.clone(),
            bus: RuntimeBus::Engine,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect_err("an unopened device starts nothing");
    assert_eq!(
        delivered.code, CODE_DEVICE_CLOSED,
        "the delivered member was found: {}",
        delivered.detail
    );
    let undelivered = device
        .start_voice(VoiceStart {
            asset: click.clone(),
            bus: RuntimeBus::Ui,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect_err("a member the load never delivered cannot start");
    assert_eq!(
        undelivered.code, CODE_SAMPLE_UNAVAILABLE,
        "the refusal names what is missing: {}",
        undelivered.detail
    );
}

/// A reload replaces the library with the new load's, and the previous load's
/// samples are not reachable afterwards — not through the library and not
/// through the device that reads it.
#[test]
fn accept_m01_lc_audio_load_a_reload_replaces_the_library_and_drops_the_old_samples() {
    let mut fixture = Fixture::new("reload");
    let engine = member_id(CONTAINER, ENGINE);
    let click = member_id(CONTAINER, CLICK);

    let first = fixture.load(std::slice::from_ref(&engine), true);
    fixture.update();
    assert!(
        fixture.library.pcm(&engine).is_some(),
        "the first closure populated the library"
    );
    assert_eq!(fixture.library.len(), 1);

    let second = fixture.load(std::slice::from_ref(&click), true);
    assert_ne!(
        second, first,
        "a reload runs under a new content session generation"
    );
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(fixture.log().installs, 2, "both loads installed a session");
    assert_eq!(
        installed.session, second,
        "the library now belongs to the second load"
    );
    assert_eq!(installed.samples, 1);
    assert!(
        fixture
            .log()
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, AudioHandoffRefusal::ReplacedLoads { .. })),
        "the superseded load is named: {:?}",
        fixture.log().refusals
    );
    assert!(
        fixture.library.pcm(&engine).is_none(),
        "the previous load's samples are not reachable"
    );
    assert!(
        fixture.library.pcm(&click).is_some(),
        "and the new load's are"
    );
    assert_eq!(
        fixture.library.len(),
        1,
        "exactly this load's members, not a merge of the two"
    );

    let mut device = AudibleDevice::unopened(fixture.library.clone());
    let stale = device
        .start_voice(VoiceStart {
            asset: engine,
            bus: RuntimeBus::Engine,
            gain: 1.0,
            pan: 0.0,
            pitch: 1.0,
        })
        .expect_err("the replaced load's sample is gone");
    assert_eq!(
        stale.code, CODE_SAMPLE_UNAVAILABLE,
        "the device cannot reach the previous closure either: {}",
        stale.detail
    );
}

/// A member whose decode is refused is named by its own refusal code and is
/// absent from the library, while its siblings are unaffected.
///
/// Two refusals, two different kinds: a member whose own header does not read
/// carries the decoder's code, and a member the container does not declare
/// carries [`CODE_SAMPLE_ABSENT`]. Neither is dropped silently.
#[test]
fn accept_m01_lc_audio_load_a_refused_member_is_named_by_its_own_code_and_absent() {
    let mut fixture = Fixture::new("refused");
    let engine = member_id(CONTAINER, ENGINE);
    let broken = member_id(CONTAINER, BROKEN);
    let ghost = member_id(ABSENT, "ghost.wav");

    fixture.load(&[engine.clone(), broken.clone(), ghost.clone()], true);
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(
        installed.samples, 1,
        "only the member that decodes reached the library"
    );
    assert_eq!(
        installed.sample_refusals, 2,
        "every member that did not is accounted for"
    );
    assert_eq!(
        installed.specs, 3,
        "all three are declared, so all three lowered: the refusal is about the
         samples, not the routing"
    );

    let undecodable = fixture.undecodable();
    assert_eq!(
        undecodable.len(),
        2,
        "both refusals are recorded by name: {:?}",
        undecodable
    );
    let header = undecodable
        .iter()
        .find(|(content, ..)| *content == broken)
        .expect("the member that could not decode is named");
    assert_eq!(
        header.1, UNREADABLE_HEADER,
        "the refusal carries the decoder's own code: {}",
        header.2
    );
    assert!(
        header.2.contains(BROKEN),
        "and the detail names the member: {}",
        header.2
    );
    let absent = undecodable
        .iter()
        .find(|(content, ..)| *content == ghost)
        .expect("the member the container does not hold is named");
    assert_eq!(absent.1, CODE_SAMPLE_ABSENT);
    assert!(
        absent.2.contains(CONTAINER),
        "the detail names the container it looked in: {}",
        absent.2
    );

    assert!(
        fixture.library.pcm(&broken).is_none(),
        "the refused member is absent from the library"
    );
    assert!(
        fixture.library.pcm(&ghost).is_none(),
        "and so is the one nothing declares"
    );
    assert!(
        fixture.library.pcm(&engine).is_some(),
        "its sibling is unaffected"
    );
}

/// A source that reads through another content session is refused, and the
/// library is emptied rather than left holding the replaced load's samples.
///
/// This is the session-generation discipline of the rest of the handoff, on
/// the half that produces bytes: decoding one session's content into another
/// session's library would make a stale sample indistinguishable from a live
/// one.
#[test]
fn accept_m01_lc_audio_load_a_source_from_another_session_is_refused_and_empties_the_library() {
    let mut fixture = Fixture::new("foreign");
    let engine = member_id(CONTAINER, ENGINE);
    let click = member_id(CONTAINER, CLICK);

    fixture.load(std::slice::from_ref(&engine), true);
    fixture.update();
    assert!(
        fixture.library.pcm(&engine).is_some(),
        "the first load populated the library"
    );

    // The reload publishes no source, so the world still holds the previous
    // load's — one content session behind the closure now being attached.
    let second = fixture.load(std::slice::from_ref(&click), false);
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(
        installed.session, second,
        "the session still installs: the simulation is not held hostage to the samples"
    );
    assert_eq!(installed.samples, 0);
    assert_eq!(installed.sample_refusals, 1);
    assert!(
        fixture.log().refusals.iter().any(|refusal| matches!(
            refusal,
            AudioHandoffRefusal::ForeignSampleSource { load, generation }
                if *load == installed.load && generation.get() != installed.load.session.get()
        )),
        "the stale source is named: {:?}",
        fixture.log().refusals
    );
    assert!(
        fixture.library.pcm(&engine).is_none(),
        "the replaced load's samples are gone"
    );
    assert!(
        fixture.library.pcm(&click).is_none(),
        "and nothing was decoded through the stale source"
    );
    assert_eq!(fixture.library.len(), 0);
}

/// A world whose load delivered audio but publishes no sample source says so
/// by name, and the library is empty rather than left holding the previous
/// closure's samples.
#[test]
fn accept_m01_lc_audio_load_a_world_without_a_sample_source_names_it_and_fills_nothing() {
    let mut fixture = Fixture::new("no-source");
    let engine = member_id(CONTAINER, ENGINE);
    let click = member_id(CONTAINER, CLICK);

    fixture.load(std::slice::from_ref(&engine), true);
    fixture.update();
    assert!(fixture.library.pcm(&engine).is_some());

    let removed = fixture
        .app
        .world_mut()
        .remove_resource::<AudioSampleSource>();
    assert!(removed.is_some(), "the first load published a source");
    fixture.load(&[click], false);
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(installed.samples, 0);
    assert_eq!(installed.sample_refusals, 1);
    assert!(
        fixture
            .log()
            .refusals
            .iter()
            .any(|refusal| matches!(refusal, AudioHandoffRefusal::NoSampleSource)),
        "the missing source is named: {:?}",
        fixture.log().refusals
    );
    assert_eq!(
        fixture.library.len(),
        0,
        "no library holds the replaced load's samples: {:?}",
        fixture.log().refusals
    );
}

/// A delivered audio content the declared catalog holds no record for is
/// refused by name and never decoded: the library holds what the load
/// delivered **and** the catalog declares, and nothing else.
#[test]
fn accept_m01_lc_audio_load_an_undeclared_delivered_audio_is_named_and_not_decoded() {
    let mut fixture = Fixture::new("undeclared");
    let undeclared = ContentId::from_source(ContentKind::Sound, "synthetic.undeclared.cue")
        .expect("a fixture id is valid");
    let engine = member_id(CONTAINER, ENGINE);

    // Delivered *next to* a declared member, so the assertion is about which
    // one is decoded rather than about an empty library.
    fixture.load(&[undeclared.clone(), engine.clone()], true);
    fixture.update();

    let installed = fixture.installed();
    assert_eq!(installed.specs, 1, "the declared member lowered");
    assert_eq!(
        installed.refused, 1,
        "and the handoff refused the other by name"
    );
    assert_eq!(
        installed.samples, 1,
        "the declared member's samples are in the library"
    );
    assert_eq!(
        installed.sample_refusals, 0,
        "the undeclared one is not a sample refusal: it never became a candidate"
    );
    assert!(
        fixture.log().refusals.iter().any(|refusal| matches!(
            refusal,
            AudioHandoffRefusal::UndeclaredAsset { content } if *content == undeclared
        )),
        "the undeclared id is named: {:?}",
        fixture.log().refusals
    );
    assert!(
        fixture.library.pcm(&undeclared).is_none(),
        "nothing was decoded for the undeclared id"
    );
    assert!(
        fixture.library.pcm(&engine).is_some(),
        "its declared neighbour is unaffected"
    );
    assert_eq!(fixture.library.len(), 1, "exactly one member was decoded");
}

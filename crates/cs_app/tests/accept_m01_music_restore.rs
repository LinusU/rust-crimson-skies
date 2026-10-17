//! Acceptance: the current music cue is re-issued to the mixer after a device
//! restore (Rally #1552, `M01-MUSIC-RESTORE`).
//!
//! #1547 played M01's music state requests through the device and kept the
//! director's cue current through a device loss, but nothing re-issued it: a
//! restore brought the output and the loops back and the music stayed silent
//! until the mission requested another state
//! (`docs/findings/2026-10-17-m01-music-device.md`, "Designed, not measured").
//! This suite pins that missing hop through production code — the real
//! [`AudioPlugin`] in a real Bevy `App`, a hand-installed [`AudioSession`] and
//! [`AudioMixing`], the real mixer and a [`RecordingAudioDevice`]:
//!
//! * a request sounds on the music bus; the output is lost (the music voice
//!   stops); the output is restored — and the next mix pass sounds the **same**
//!   cue again, from the same identity the director kept;
//! * a world with no current cue re-issues nothing, and a second restore is a
//!   no-op, so the replay can never double up;
//! * the replay is labelled **designed**, not measured: restarting the current
//!   cue from its beginning rather than resuming or fading it back is this
//!   build's choice (the original's behavior is unmeasured).
//!
//! All content is newly authored synthetic fixture data: no scenario reads
//! `CS_GAME_DIR`, and no original audio is involved. Whether a person hears the
//! restarted cue is the owner's gate (F41-D); nothing here claims that.

use bevy::app::App;
use bevy::prelude::{MinimalPlugins, TransformPlugin};
use cs_app::audio::{
    AudioMixReport, AudioMixing, AudioPlugin, AudioSession, AudioSpatial, device_lost,
    device_restored,
};
use cs_app::scene::{SceneGeneration, SceneGenerations};
use cs_content::audio::declared_synthetic_audio_catalog;
use cs_sim::audio_events::{
    AudioBus, AudioRouter, DeviceCommand, Listener, MUSIC_GAIN, MusicCue, MusicOutcome,
    RecordingAudioDevice, SpatialPolicy, VoiceStart,
};
use cs_types::content::{ContentId, ContentKind};
use cs_types::net::SessionId;

const SESSION: u64 = 9;

fn session() -> SessionId {
    SessionId::new(SESSION).expect("a nonzero session generation")
}

fn music(key: &str) -> ContentId {
    ContentId::from_source(ContentKind::Music, key).expect("a valid music id")
}

fn spatial() -> AudioSpatial {
    AudioSpatial::new(
        SpatialPolicy::try_new(10.0, 100.0).expect("a valid policy"),
        Listener::try_new([0.0, 0.0, 0.0], [1.0, 0.0, 0.0]).expect("a valid listener"),
    )
}

/// A world with the production audio plugin, an explicitly installed session
/// and a recording output device whose log the test reads.
///
/// The session and mixer are installed by hand for the same reason
/// `accept_m01_radio_device` does: this suite drives the music API directly, and
/// the loading handoff that would own them needs a delivered closure.
struct MusicWorld {
    app: App,
    device: RecordingAudioDevice,
}

impl MusicWorld {
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
            SceneGeneration(1),
            Vec::new(),
        ));
        app.insert_resource(AudioMixing::new(session()));
        Self {
            app,
            device: device.handle(),
        }
    }

    fn update(&mut self) {
        self.app.update();
    }

    fn session(&self) -> &AudioSession {
        self.app.world().resource::<AudioSession>()
    }

    fn session_mut(&mut self) -> bevy::ecs::change_detection::Mut<'_, AudioSession> {
        self.app.world_mut().resource_mut::<AudioSession>()
    }

    /// Requests `asset` at the session's next music id, as a mission cue does.
    fn request(&mut self, asset: &ContentId) -> MusicOutcome {
        let id = self.session_mut().mint_music_id();
        self.session_mut().request_music(MusicCue {
            id,
            asset: asset.clone(),
        })
    }

    /// The track the mixer's one music voice is sounding, if any.
    fn playing(&self) -> Option<ContentId> {
        self.app
            .world()
            .resource::<AudioMixing>()
            .mixer()
            .music_playing()
            .cloned()
    }

    fn report(&self) -> &AudioMixReport {
        self.app.world().resource::<AudioMixReport>()
    }

    fn lose_device(&mut self) {
        device_lost(self.app.world_mut());
    }

    fn restore_device(&mut self) -> Option<MusicCue> {
        device_restored(self.app.world_mut()).expect("the recording device reopens")
    }

    /// Every music-bus voice start the recording device logged since the last
    /// [`RecordingAudioDevice::clear`].
    fn music_starts(&self) -> Vec<VoiceStart> {
        self.device
            .commands()
            .into_iter()
            .filter_map(|command| match command {
                DeviceCommand::Started { start, .. } if start.bus == AudioBus::Music => Some(start),
                _ => None,
            })
            .collect()
    }
}

#[test]
fn accept_m01_music_restore_reissues_the_current_cue_after_the_device_returns() {
    let mut world = MusicWorld::new();
    world.update();

    let track = music("synthetic.music.restore.primary");
    assert_eq!(
        world.request(&track),
        MusicOutcome::Started {
            to: track.clone(),
            audible: true,
        }
    );
    world.update();
    assert_eq!(world.playing(), Some(track.clone()), "the request sounds");
    assert_eq!(world.music_starts().len(), 1, "one music voice started");

    // The output is lost: the mixer stops the music voice, and the director
    // keeps the cue current through the loss.
    world.device.clear();
    world.lose_device();
    assert_eq!(world.playing(), None, "the loss silenced the music");
    assert_eq!(
        world.session().music.current().map(|cue| &cue.asset),
        Some(&track),
        "the director keeps the cue current through the loss"
    );
    world.update();
    assert_eq!(world.playing(), None, "a lost output stays silent");
    assert!(
        world.music_starts().is_empty(),
        "nothing sounded a lost output: {:?}",
        world.device.commands()
    );

    // The output returns: the restore returns the current cue (unchanged) and
    // re-issues it, so the next mix pass sounds it again on the music bus.
    let restart = world.restore_device();
    assert_eq!(
        restart.map(|cue| cue.asset),
        Some(track.clone()),
        "the restore returns the cue the director kept"
    );
    world.update();
    assert_eq!(
        world.playing(),
        Some(track.clone()),
        "the current cue sounds again after the restore"
    );
    let starts = world.music_starts();
    assert_eq!(
        starts.len(),
        1,
        "exactly one music voice restarted: {:?}",
        world.device.commands()
    );
    assert_eq!(
        starts[0].asset, track,
        "the restarted voice is the same cue"
    );
    assert!((starts[0].gain - MUSIC_GAIN).abs() < f64::EPSILON);
    assert_eq!(
        world.report().music.started,
        vec![starts[0].asset.clone()],
        "the mix pass that restarted it names it"
    );
    assert!(
        world.report().music.refusals.is_empty(),
        "{:?}",
        world.report().music.refusals
    );
}

#[test]
fn accept_m01_music_restore_reissues_nothing_without_a_current_cue() {
    let mut world = MusicWorld::new();
    world.update();

    // No music was ever requested: a restore has no cue to re-issue, and the
    // music bus stays silent rather than starting a guessed track.
    world.lose_device();
    world.update();
    let restart = world.restore_device();
    assert!(restart.is_none(), "no cue was current to re-issue");
    world.update();
    assert_eq!(world.playing(), None, "nothing was invented to play");
    assert!(
        world.music_starts().is_empty(),
        "nothing started on the music bus: {:?}",
        world.device.commands()
    );
    assert!(world.report().music.started.is_empty());
}

#[test]
fn accept_m01_music_restore_reissues_the_cue_once_per_loss() {
    let mut world = MusicWorld::new();
    world.update();

    let track = music("synthetic.music.restore.idempotent");
    world.request(&track);
    world.update();
    assert_eq!(world.playing(), Some(track.clone()));

    world.device.clear();
    world.lose_device();
    // First restore re-issues the cue; a second restore (the device is already
    // available) is a no-op, so a re-detection cannot double up the voice.
    assert!(
        world.restore_device().is_some(),
        "the current cue is returned"
    );
    assert!(
        world.restore_device().is_none(),
        "a second restore re-issues nothing"
    );
    world.update();
    assert_eq!(world.playing(), Some(track.clone()));
    assert_eq!(
        world.music_starts().len(),
        1,
        "exactly one music voice restarted: {:?}",
        world.device.commands()
    );
}

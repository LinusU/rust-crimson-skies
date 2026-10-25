//! Acceptance: M01's music tracks are decoded when a music state request
//! first starts them, not when the content is prepared (Rally #1551,
//! `M01-MUSIC-LAZY-DECODE`).
//!
//! The track ids and the `accept_m01_music_device_` behaviour are unchanged:
//! `MissionRadio::read` still names every selectable track under its owner-ruled
//! music id, and `ensure_music_published` (what the `publish_requested_music`
//! system runs before the mixer's music pass) hands the device exactly the
//! track a verdict starts.

use std::path::PathBuf;
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Instant;

use cs_app::audio::{
    DecodedMusicMember, InMemorySamples, LazyMusicTracks, MusicTrackLoader, PcmAudio,
    SampleLibrary, ensure_music_published, music_member_id,
};
use cs_app::mission_launch::plan_mission_launch;
use cs_app::mission_session::{MissionRadio, declared_engine_objectives, declared_sound_groups};
use cs_app::objectives::recover_retail_objectives;
use cs_content::campaign_bindings::MissionLabel;
use cs_sim::audio_events::MusicOutcome;
use cs_types::content::ContentId;

fn id(name: &str) -> ContentId {
    music_member_id("ZBD/soundsh.zbd", name).expect("the track derives a music id")
}

fn pcm() -> PcmAudio {
    PcmAudio::try_new(2, 22_050, vec![0.25; 8]).expect("a playable shape")
}

#[derive(Debug, Default)]
struct CountingLoader {
    loads: AtomicUsize,
    asked: Mutex<Vec<ContentId>>,
}

impl MusicTrackLoader for CountingLoader {
    fn load(&self, wanted: &ContentId) -> Result<DecodedMusicMember, String> {
        self.loads.fetch_add(1, Ordering::SeqCst);
        self.asked.lock().expect("lock").push(wanted.clone());
        for name in ["a.wav", "b.wav"] {
            if &id(name) == wanted {
                return DecodedMusicMember::new("ZBD/soundsh.zbd", name, pcm())
                    .map_err(|error| error.to_string());
            }
        }
        Err(format!("{wanted} is no track"))
    }
}

#[test]
fn accept_m01_music_lazy_decodes_only_the_started_track_once() {
    let loader = std::sync::Arc::new(CountingLoader::default());
    let tracks = LazyMusicTracks::new(loader.clone());
    let library = InMemorySamples::new();

    // Nothing is decoded until a verdict starts a track.
    assert_eq!(loader.loads.load(Ordering::SeqCst), 0);
    let none: Vec<_> = ensure_music_published(&library, &tracks, &[MusicOutcome::Unchanged]);
    assert!(none.is_empty());
    assert_eq!(loader.loads.load(Ordering::SeqCst), 0);

    // An inaudible start does not decode either.
    ensure_music_published(
        &library,
        &tracks,
        &[MusicOutcome::Started {
            to: id("a.wav"),
            audible: false,
        }],
    );
    assert_eq!(loader.loads.load(Ordering::SeqCst), 0);

    // An audible start decodes exactly that track, under its music id.
    let failed = ensure_music_published(
        &library,
        &tracks,
        &[MusicOutcome::Started {
            to: id("a.wav"),
            audible: true,
        }],
    );
    assert!(failed.is_empty());
    assert!(library.pcm(&id("a.wav")).is_some());
    assert!(library.pcm(&id("b.wav")).is_none(), "b was never selected");

    // Selecting it again, then transitioning to b, decodes only b.
    ensure_music_published(
        &library,
        &tracks,
        &[
            MusicOutcome::Transition {
                from: id("a.wav"),
                to: id("a.wav"),
                audible: true,
            },
            MusicOutcome::Transition {
                from: id("a.wav"),
                to: id("b.wav"),
                audible: true,
            },
        ],
    );
    assert_eq!(
        *loader.asked.lock().expect("lock"),
        [id("a.wav"), id("b.wav")]
    );
    assert!(library.pcm(&id("b.wav")).is_some());
}

#[test]
fn accept_m01_music_lazy_names_a_track_that_will_not_load() {
    let tracks = LazyMusicTracks::new(std::sync::Arc::new(CountingLoader::default()));
    let library = InMemorySamples::new();
    let failed = ensure_music_published(
        &library,
        &tracks,
        &[MusicOutcome::Started {
            to: id("missing.wav"),
            audible: true,
        }],
    );
    assert_eq!(failed.len(), 1);
    assert_eq!(failed[0].0, id("missing.wav"));
    assert!(failed[0].1.contains("no track"));
    assert!(library.pcm(&id("missing.wav")).is_none());
}

fn game_dir() -> PathBuf {
    let Ok(dir) = std::env::var("CS_GAME_DIR") else {
        panic!("CS_GAME_DIR is required for the retail music tests");
    };
    PathBuf::from(dir)
}

#[test]
#[ignore = "requires CS_GAME_DIR"]
fn accept_m01_music_lazy_prepare_names_tracks_and_decodes_on_request() {
    let root = game_dir();
    let plan = plan_mission_launch(
        &root,
        MissionLabel::new("M01").expect("label"),
        "The Lost Treasure",
    )
    .expect("M01's launch closure plans");
    let mission_dir = plan.mission_dir.clone();
    drop(plan);
    let recovery = recover_retail_objectives(&root, &mission_dir).expect("objectives recover");
    let engine = declared_engine_objectives(&recovery).expect("engine objectives build");

    let started = Instant::now();
    let radio = MissionRadio::read(&root, declared_sound_groups(&engine)).expect("radio reads");
    eprintln!("MissionRadio::read took {:?}", started.elapsed());

    // M01 "The Lost Treasure" is `ZBD/C3/M01` (#1565, `dd19ce9dd`): its
    // objectives cue four music states (`music_battlesuccess_sg` is declared
    // only by `ZBD/C1C/M01`, the M06 archive this pin was first measured on),
    // and those four select 13 tracks: 6 missionsuccess, 3 distinct prebattle
    // names, 2 primaryobj, 2 secondaryobj.
    let music_states: Vec<String> = declared_sound_groups(&engine)
        .into_iter()
        .filter(|group| group.starts_with("music_"))
        .collect();
    assert_eq!(
        music_states,
        [
            "music_missionsuccess_sg",
            "music_prebattle_sg",
            "music_primaryobj_sg",
            "music_secondaryobj_sg"
        ]
    );
    // Every selectable track is named by its owner-ruled id.
    let sources = radio.music_tracks();
    assert_eq!(
        sources.len(),
        13,
        "M01's four music states select 13 tracks"
    );
    assert_eq!(radio.failures().count(), 0);
    for source in sources {
        assert_eq!(
            source.id,
            music_member_id(&source.container, &source.member).expect("derives")
        );
    }

    // The first request decodes that track and only that track.
    let tracks = radio.music_loader();
    let library = InMemorySamples::new();
    let wanted = sources[0].id.clone();
    let decode = Instant::now();
    let failed = ensure_music_published(
        &library,
        &tracks,
        &[MusicOutcome::Started {
            to: wanted.clone(),
            audible: true,
        }],
    );
    eprintln!(
        "first request decoded {} in {:?}",
        sources[0].member,
        decode.elapsed()
    );
    assert!(failed.is_empty(), "{failed:?}");
    let pcm = library
        .pcm(&wanted)
        .expect("the requested track is published");
    assert!(pcm.frames() > 0);
    assert!(pcm.samples().iter().any(|sample| sample.abs() > 1.0e-4));
    for other in &sources[1..] {
        assert!(
            library.pcm(&other.id).is_none(),
            "{} stays undecoded",
            other.member
        );
    }
}

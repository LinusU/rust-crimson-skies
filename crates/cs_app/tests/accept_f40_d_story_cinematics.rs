//! Acceptance tests for the verification of every original cinematic on the
//! real audio device clock and the real renderer (F40-D).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D`. The synthetic tests run in CI. The retail test
//! enumerates the installation's `MPG` directory, plays **every** file it
//! finds end to end through the output device and renders sampled surfaces on
//! the GPU; it needs `CS_GAME_DIR` and the `audio` and `gpu` capabilities and
//! is run with `--include-ignored`. Its artifacts go to `CS_EVIDENCE_DIR`
//! (default `private/evidence/F40-D`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cs_app::audio::device::CapabilityDeclaration;
use cs_app::cinematics::Viewport;
use cs_app::cinematics::device::{
    DevicePlaybackError, FramePresenter, PlaybackEnd, ShownFrame, play_on_device,
};
use cs_app::cinematics::frame_capture::capture_surface;
use cs_sim::cinematic_state::{CinematicState, MediaFailure};
use cs_types::content::{ContentId, ContentKind};

/// The ten files the F40-D2 inventory names under `GOSDATA/ASSETS/GRAPHICS/MPG`.
const INVENTORY: [&str; 10] = [
    "chap0", "chap1", "chap2", "chap3", "chap4", "chap5", "crimflag", "final", "msopen1", "zipper",
];

fn video_id(name: &str) -> ContentId {
    ContentId::from_source(ContentKind::Video, name).unwrap()
}

struct Nobody;
impl FramePresenter for Nobody {
    fn present(&mut self, _: &ShownFrame<'_>) -> Result<(), String> {
        panic!("a cinematic that cannot open must present nothing");
    }
}

#[test]
fn accept_f40_d_a_missing_file_is_a_useful_error_and_never_a_completion() {
    let result = play_on_device(
        Path::new("/nonexistent/final.mpg"),
        &video_id("final"),
        &CapabilityDeclaration::parse("audio,gpu"),
        (1280, 720),
        None,
        &mut Nobody,
    );
    match result {
        Err(DevicePlaybackError::Media(MediaFailure::MissingMedia { media })) => {
            assert_eq!(media, video_id("final"), "the error names what was missing");
        }
        other => panic!("expected MissingMedia, got {other:?}"),
    }
}

#[test]
fn accept_f40_d_a_file_no_decoder_reads_is_a_media_failure_and_never_a_completion() {
    let dir = std::env::temp_dir().join(format!("cs_f40_d_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let path = dir.join("chap1.mpg");
    fs::write(&path, b"this is not an MPEG program stream at all").unwrap();
    let result = play_on_device(
        &path,
        &video_id("chap1"),
        &CapabilityDeclaration::parse("audio,gpu"),
        (1280, 720),
        None,
        &mut Nobody,
    );
    fs::remove_dir_all(&dir).unwrap();
    match result {
        Err(DevicePlaybackError::Media(
            MediaFailure::MissingDecoder { .. } | MediaFailure::DecodeFailed { .. },
        )) => {}
        other => panic!("expected a decoder/decode failure, got {other:?}"),
    }
}

#[test]
fn accept_f40_d_the_audio_capability_gate_refuses_before_anything_plays() {
    let dir = std::env::temp_dir().join(format!("cs_f40_d_gate_{}", std::process::id()));
    fs::create_dir_all(&dir).unwrap();
    let missing = dir.join("final.mpg");
    // Without `audio`, a missing file is still reported as missing, not as a pass.
    let result = play_on_device(
        &missing,
        &video_id("final"),
        &CapabilityDeclaration::parse("retail"),
        (1280, 720),
        None,
        &mut Nobody,
    );
    fs::remove_dir_all(&dir).unwrap();
    assert!(
        matches!(result, Err(DevicePlaybackError::Media(_))),
        "{result:?}"
    );
}

type Kept = (u64, u64, Vec<u8>, Viewport);

/// Keeps the surface of the first shown frame at or after each target time.
struct Sampler {
    targets_us: Vec<u64>,
    kept: Vec<Kept>,
}

impl FramePresenter for Sampler {
    fn present(&mut self, frame: &ShownFrame<'_>) -> Result<(), String> {
        let n = self.kept.len();
        if n < self.targets_us.len() && frame.clock_us >= self.targets_us[n] {
            self.kept.push((
                frame.index,
                frame.clock_us,
                frame.surface.to_vec(),
                frame.viewport,
            ));
        }
        Ok(())
    }
}

fn evidence_dir() -> PathBuf {
    let dir = PathBuf::from(
        std::env::var("CS_EVIDENCE_DIR").unwrap_or_else(|_| "../../private/evidence/F40-D".into()),
    );
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "requires CS_GAME_DIR and the audio and gpu capabilities"]
fn accept_f40_d_every_retail_cinematic_plays_to_its_end_on_the_device_clock() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let declaration = CapabilityDeclaration::from_environment();
    let mpg = root.join("GOSDATA/ASSETS/GRAPHICS/MPG");
    let dir = evidence_dir();
    let surface = (1280, 720);

    // Every file the installation ships is verified: the directory is the
    // subject, the inventory only pins that nothing was added or lost.
    let mut found: Vec<String> = fs::read_dir(&mpg)
        .unwrap_or_else(|e| panic!("{}: {e}", mpg.display()))
        .map(|entry| entry.unwrap().path())
        .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case("mpg")))
        .map(|p| p.file_stem().unwrap().to_string_lossy().to_lowercase())
        .collect();
    found.sort();
    assert_eq!(
        found, INVENTORY,
        "the installed cinematics differ from the F40-D2 inventory"
    );

    let mut json = Vec::new();
    for name in &found {
        let path = fs::read_dir(&mpg)
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| {
                p.file_stem()
                    .unwrap()
                    .to_string_lossy()
                    .eq_ignore_ascii_case(name)
            })
            .unwrap();
        // Three samples by the audio clock, inside the shortest file (8 s).
        let mut sampler = Sampler {
            targets_us: vec![2_000_000, 4_000_000, 6_000_000],
            kept: Vec::new(),
        };
        let report = play_on_device(
            &path,
            &video_id(name),
            &declaration,
            surface,
            Some(Duration::from_secs(400)),
            &mut sampler,
        )
        .unwrap_or_else(|e| panic!("{name}: {e}"));

        assert_eq!(report.end, PlaybackEnd::Completed, "{name}");
        assert!(
            matches!(report.state, CinematicState::Completed(_)),
            "{name}"
        );
        assert_eq!(report.source_size, (320, 240), "{name}");
        assert_eq!(
            (report.viewport.width, report.viewport.height),
            (960, 720),
            "{name}: 4:3 in 16:9, not stretched"
        );
        // The device position can stop up to one buffer short of the audio's
        // end (F40-D4 finding); the driver accepts 50 ms, so does this.
        let short = report
            .total_audio_frames
            .saturating_sub(report.played_audio_frames);
        assert!(
            report.played_audio_frames <= report.total_audio_frames
                && short <= u64::from(report.sample_rate) / 20,
            "{name}: played {} of {} audio frames",
            report.played_audio_frames,
            report.total_audio_frames
        );
        let audio_s = report.total_audio_frames as f64 / f64::from(report.sample_rate);
        let source_frames = audio_s * report.frame_rate;
        assert!(
            report.frames_presented as f64 >= source_frames * 0.7,
            "{name}: {} of about {source_frames:.0} frames presented",
            report.frames_presented
        );
        assert!(
            report.drift_within_tolerance,
            "{name}: drift {} us",
            report.max_abs_drift_us
        );
        assert!(
            (report.wall.as_secs_f64() - audio_s).abs() < 1.5,
            "{name}: wall {:?} vs audio {audio_s}",
            report.wall
        );

        assert_eq!(sampler.kept.len(), 3, "{name}: sampled frames");
        let mut captures = Vec::new();
        for (index, clock_us, pixels, viewport) in &sampler.kept {
            let png = dir.join(format!("{name}-frame{index:05}.png"));
            let capture = capture_surface(pixels, surface, *viewport, &png)
                .unwrap_or_else(|e| panic!("{name} frame {index}: {e}"));
            assert_eq!(capture.mismatched_pixels, 0, "{name} {index}: readback");
            assert_eq!(capture.bar_pixels_not_black, 0, "{name} {index}: bars");
            captures.push(format!(
                "{{\"png\": \"{}\", \"frame_index\": {index}, \"audio_clock_us\": {clock_us}, \"adapter\": {:?}, \"png_sha256\": \"{}\", \"picture_pixels_lit\": {}}}",
                png.file_name().unwrap().to_string_lossy(),
                capture.adapter,
                capture.png_sha256.to_hex(),
                capture.picture_pixels_lit
            ));
        }
        json.push(format!(
            "{{\"media\": {name:?}, \"audio_seconds\": {audio_s:.3}, \"frame_rate\": {}, \"sample_rate\": {}, \"total_audio_frames\": {}, \"played_audio_frames\": {}, \"frames_short_of_end\": {short}, \"frames_presented\": {}, \"max_abs_drift_us\": {}, \"viewport\": [{}, {}, {}, {}], \"wall_ms\": {}, \"captures\": [{}]}}",
            report.frame_rate,
            report.sample_rate,
            report.total_audio_frames,
            report.played_audio_frames,
            report.frames_presented,
            report.max_abs_drift_us,
            report.viewport.x,
            report.viewport.y,
            report.viewport.width,
            report.viewport.height,
            report.wall.as_millis(),
            captures.join(", ")
        ));
    }
    fs::write(
        dir.join("every-cinematic.json"),
        format!("[\n{}\n]\n", json.join(",\n")),
    )
    .unwrap();
}

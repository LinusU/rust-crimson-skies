//! Acceptance tests of original cinematic playback on the real audio device
//! clock and the real renderer (F40-D4).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-D`. The synthetic tests run in CI. The retail test plays
//! three original cinematics through the real output device, composes every
//! shown frame into a 16:9 surface and renders sampled surfaces on the real
//! GPU; it needs `CS_GAME_DIR` and the `audio` and `gpu` capabilities and is
//! run with `--include-ignored`. Its artifacts go to `CS_EVIDENCE_DIR`
//! (default `private/evidence/F40-D4`).

use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

use cs_app::audio::device::CapabilityDeclaration;
use cs_app::cinematics::device::{
    ComposeError, DevicePlaybackError, FramePresenter, PlaybackEnd, ShownFrame,
    compose_letterboxed, play_on_device,
};
use cs_app::cinematics::frame_capture::capture_surface;
use cs_sim::cinematic_state::{CinematicState, MediaFailure};
use cs_types::content::{ContentId, ContentKind};

fn video_id(name: &str) -> ContentId {
    ContentId::from_source(ContentKind::Video, name).unwrap()
}

#[test]
fn accept_f40_d4_frames_are_letterboxed_never_stretched() {
    // A 4×2 frame whose left half is red and right half is green.
    let mut frame = Vec::new();
    for _row in 0..2 {
        for col in 0..4 {
            frame.extend_from_slice(if col < 2 {
                &[255, 0, 0, 255]
            } else {
                &[0, 255, 0, 255]
            });
        }
    }
    let (surface, viewport) = compose_letterboxed(&frame, (4, 2), (16, 12)).unwrap();
    // 2:1 into 16×12 -> 16×8 centred: two bars of 2 rows.
    assert_eq!(
        (viewport.x, viewport.y, viewport.width, viewport.height),
        (0, 2, 16, 8)
    );
    let px = |x: usize, y: usize| &surface[(y * 16 + x) * 4..(y * 16 + x) * 4 + 4];
    assert_eq!(px(3, 0), [0, 0, 0, 255], "top bar is opaque black");
    assert_eq!(px(3, 11), [0, 0, 0, 255], "bottom bar is opaque black");
    assert_eq!(px(3, 5), [255, 0, 0, 255], "left half of the picture");
    assert_eq!(px(12, 5), [0, 255, 0, 255], "right half of the picture");

    // Pillarbox: the same frame into a surface taller than its aspect.
    let (_, tall) = compose_letterboxed(&frame, (4, 2), (8, 16)).unwrap();
    assert_eq!((tall.width, tall.height), (8, 4));

    assert_eq!(
        compose_letterboxed(&frame[..8], (4, 2), (16, 12)).unwrap_err(),
        ComposeError::WrongLength {
            expected: 32,
            got: 8
        }
    );
    assert_eq!(
        compose_letterboxed(&frame, (4, 2), (0, 12)).unwrap_err(),
        ComposeError::Empty
    );
}

struct Nobody;
impl FramePresenter for Nobody {
    fn present(&mut self, _: &ShownFrame<'_>) -> Result<(), String> {
        panic!("a missing file must present nothing");
    }
}

#[test]
fn accept_f40_d4_a_missing_cinematic_is_a_media_failure_and_never_a_completion() {
    let declaration = CapabilityDeclaration::parse("audio,gpu");
    let result = play_on_device(
        Path::new("/nonexistent/chap9.mpg"),
        &video_id("chap9"),
        &declaration,
        (1280, 720),
        None,
        &mut Nobody,
    );
    match result {
        Err(DevicePlaybackError::Media(MediaFailure::MissingMedia { .. })) => {}
        other => panic!("expected a MissingMedia failure, got {other:?}"),
    }
}

/// Keeps the surface of the first shown frame at or after each target index.
struct Sampler {
    targets: Vec<u64>,
    kept: Vec<(u64, u64, Vec<u8>, cs_app::cinematics::Viewport)>,
}

impl FramePresenter for Sampler {
    fn present(&mut self, frame: &ShownFrame<'_>) -> Result<(), String> {
        let wanted = self
            .targets
            .iter()
            .any(|t| *t <= frame.index && !self.kept.iter().any(|(i, ..)| *i >= *t));
        if wanted {
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
        std::env::var("CS_EVIDENCE_DIR").unwrap_or_else(|_| "../../private/evidence/F40-D4".into()),
    );
    fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
#[ignore = "requires CS_GAME_DIR and the audio and gpu capabilities"]
fn accept_f40_d4_retail_cinematics_play_on_the_device_clock_and_render_on_the_gpu() {
    let root = PathBuf::from(
        std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must point at the original installation"),
    );
    let declaration = CapabilityDeclaration::from_environment();
    let dir = evidence_dir();
    let surface = (1280, 720);
    let mut json = Vec::new();

    // Without the audio capability the device is refused by name, before any
    // hardware call, and nothing is played.
    let refused = play_on_device(
        &root.join("GOSDATA/ASSETS/GRAPHICS/MPG/crimflag.mpg"),
        &video_id("crimflag"),
        &CapabilityDeclaration::parse("retail"),
        surface,
        None,
        &mut Nobody,
    );
    assert!(
        matches!(refused, Err(DevicePlaybackError::Device(_))),
        "{refused:?}"
    );

    for (name, frames) in [("crimflag", 241_u64), ("msopen1", 405), ("zipper", 609)] {
        let path = root.join(format!("GOSDATA/ASSETS/GRAPHICS/MPG/{name}.mpg"));
        let mut sampler = Sampler {
            targets: vec![frames / 4, frames / 2, frames * 3 / 4],
            kept: Vec::new(),
        };
        let report = play_on_device(
            &path,
            &video_id(name),
            &declaration,
            surface,
            Some(Duration::from_secs(60)),
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
        assert_eq!(
            report.played_audio_frames, report.total_audio_frames,
            "{name}"
        );
        assert!(
            report.frames_presented * 4 >= frames * 3,
            "{name}: {} of {frames} frames presented",
            report.frames_presented
        );
        assert!(
            report.drift_within_tolerance,
            "{name}: drift {} us",
            report.max_abs_drift_us
        );
        // Real-time: the run lasted about as long as the audio.
        let audio_s = report.total_audio_frames as f64 / f64::from(report.sample_rate);
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
            assert!(capture.picture_pixels_lit > 1000, "{name} {index}: picture");
            captures.push(format!(
                "{{\"png\": \"{}\", \"frame_index\": {index}, \"audio_clock_us\": {clock_us}, \"adapter\": {:?}, \"png_sha256\": \"{}\", \"png_bytes\": {}, \"picture_pixels_lit\": {}}}",
                png.file_name().unwrap().to_string_lossy(),
                capture.adapter,
                capture.png_sha256.to_hex(),
                capture.png_bytes,
                capture.picture_pixels_lit
            ));
        }
        json.push(format!(
            "{{\"media\": {name:?}, \"source_size\": [{}, {}], \"frame_rate\": {}, \"sample_rate\": {}, \"total_audio_frames\": {}, \"played_audio_frames\": {}, \"frames_presented\": {}, \"max_abs_drift_us\": {}, \"surface\": [{}, {}], \"viewport\": [{}, {}, {}, {}], \"wall_ms\": {}, \"captures\": [{}]}}",
            report.source_size.0,
            report.source_size.1,
            report.frame_rate,
            report.sample_rate,
            report.total_audio_frames,
            report.played_audio_frames,
            report.frames_presented,
            report.max_abs_drift_us,
            surface.0,
            surface.1,
            report.viewport.x,
            report.viewport.y,
            report.viewport.width,
            report.viewport.height,
            report.wall.as_millis(),
            captures.join(", ")
        ));
    }
    fs::write(
        dir.join("device-playback.json"),
        format!("[\n{}\n]\n", json.join(",\n")),
    )
    .unwrap();
}

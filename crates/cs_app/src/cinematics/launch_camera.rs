//! #1665 (`PARITY-M01-LC-CAMERA`): the launch cinematic's camera script, read
//! from the camera carrier and played at a cinematic time.
//!
//! The finding is `docs/findings/2026-10-19-m01-launch-camera.md`. It joins
//! three measured sources and invents nothing:
//!
//! * **the pose tracks**, the `.zan` tracks in the tail of `cam_anim.zbd`
//!   (`docs/findings/2026-10-18-parity-m01-lc-cinematic-camera-pose-source.md`,
//!   read by [`cs_formats::zbd::anim_tracks`]);
//! * **the scene timeline**, which scene runs when
//!   ([`super::intro_timeline::measure_intro_timeline`]); each scene's camera
//!   clock starts at the scene's call time;
//! * **the variant chains** ([`super::intro_timeline::variant_chains`]): the
//!   original draws which `gi_cam*` variant plays from a `rand()` table, so
//!   the choice is the caller's draw, not a datum.
//!
//! The poses are in the frame of `piratezep`, the Pandora
//! (`docs/findings/2026-10-18-parity-m01-lc-camera-frame-and-rotation.md`);
//! this module returns them in that frame and composes nothing with the world.

use std::fmt;

use cs_formats::io::ParseContext;
use cs_formats::zbd::anim_tracks::{TrackBlock, TrackPart, TrackSegment, read_animation_tracks};
use cs_formats::zbd::{ZbdFamily, ZbdProbe, dispatch, family_record, read_animation_index};
use cs_types::install::RelativePath;

use super::intro_timeline::{IntroTimeline, RecordView, measure_intro_timeline, variant_chains};
use super::launch_end_pose::LAUNCH_INTRO_ANIMATION;
use crate::mission_session::LAUNCH_CAMERA_CARRIER;

/// The node the camera tracks move.
pub const CAMERA_NODE: &[u8] = b"camera1";

/// Why the camera script could not be read.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LaunchCameraError(pub String);

impl fmt::Display for LaunchCameraError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "the launch cinematic's camera is not measured: {}",
            self.0
        )
    }
}

impl std::error::Error for LaunchCameraError {}

fn refuse<T>(what: impl Into<String>) -> Result<T, LaunchCameraError> {
    Err(LaunchCameraError(what.into()))
}

/// One camera track: the segments of one `camera1` block.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraTrack {
    /// The block number in the carrier's tail.
    pub block: usize,
    cubic: bool,
    segments: Vec<TrackSegment>,
}

/// One scene's camera: which track plays between `start` and `stop`.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraScene {
    /// The scene animation (`gi_scene1`).
    pub animation: String,
    /// Cinematic time the scene's `CALL_ANIMATION` ran, in seconds.
    pub start: f64,
    /// Cinematic time its `STOP_ANIMATION` ran, in seconds.
    pub stop: f64,
    /// The variant that was drawn for it.
    pub track: CameraTrack,
}

/// The camera's pose in the Pandora's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    /// Local position, stored units (metres).
    pub position: [f32; 3],
    /// The composed unit quaternion `(w, x, y, z)` of the camera's rotation.
    pub quaternion: [f64; 4],
    /// Index into [`LaunchCameraScript::scenes`] of the scene that is playing.
    pub scene: usize,
}

/// The cinematic's camera: one drawn track per camera scene.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchCameraScript {
    scenes: Vec<CameraScene>,
    /// The cinematic's length in seconds (the last event of `generic_intro`).
    pub length: f64,
}

/// The variant the original picks from one chain, given its draws: the first
/// `k` with `draw_k <= thresholds[k]`, else the last (unconditional) one.
/// Draws are taken from `draw` one per threshold tried.
#[must_use]
pub fn pick_variant(
    thresholds: &[f32],
    blocks: &[u32],
    draw: &mut dyn FnMut() -> f32,
) -> Option<u32> {
    for (threshold, block) in thresholds.iter().zip(blocks) {
        if draw() <= *threshold {
            return Some(*block);
        }
    }
    blocks.get(thresholds.len()).copied()
}

fn open_part(part: &TrackPart, cubic: bool, u: f32) -> [f32; 3] {
    if cubic {
        [
            part.cubic_at(0, u),
            part.cubic_at(1, u),
            part.cubic_at(2, u),
        ]
    } else {
        let start = part.start_vector();
        let rate = part.linear_rate();
        [
            start[0] + u * rate[0],
            start[1] + u * rate[1],
            start[2] + u * rate[2],
        ]
    }
}

impl CameraTrack {
    fn from_block(block: &TrackBlock<'_>) -> Self {
        Self {
            block: block.index(),
            cubic: block.mode() != 0,
            segments: block.segments().to_vec(),
        }
    }

    /// The track's length in seconds.
    #[must_use]
    pub fn duration(&self) -> f32 {
        self.segments.last().map_or(0.0, TrackSegment::end)
    }

    /// The pose at track time `u`. A part that a segment does not carry keeps
    /// what an earlier segment set, as the engine's setters leave the node
    /// alone; a track time past a segment's end evaluates it at its end.
    #[must_use]
    pub fn pose_at(&self, u: f32) -> Option<([f32; 3], [f64; 4])> {
        let mut position = None;
        let mut rotation = None;
        for segment in &self.segments {
            if segment.start() > u {
                break;
            }
            let local = u.min(segment.end()) - segment.start();
            if let Some(part) = segment.position() {
                position = Some(open_part(part, self.cubic, local));
            }
            if let Some(orientation) = segment.orientation_at(self.cubic, local) {
                rotation = Some(orientation.quaternion);
            }
        }
        Some((position?, rotation?))
    }
}

impl LaunchCameraScript {
    /// The scenes that carry a camera, in play order.
    #[must_use]
    pub fn scenes(&self) -> &[CameraScene] {
        &self.scenes
    }

    /// The camera's pose at cinematic time `seconds`, clamped to the script's
    /// ends. `None` when the drawn track carries no pose at that time.
    #[must_use]
    pub fn pose_at(&self, seconds: f64) -> Option<CameraPose> {
        let seconds = seconds.clamp(0.0, self.length);
        let scene = self
            .scenes
            .iter()
            .rposition(|scene| scene.start <= seconds)
            .unwrap_or(0);
        let played = &self.scenes[scene];
        let local = (seconds.min(played.stop) - played.start).max(0.0);
        let (position, quaternion) = played.track.pose_at(local as f32)?;
        Some(CameraPose {
            position,
            quaternion,
            scene,
        })
    }
}

/// Builds the script from the camera carrier `bytes` for a player whose scene
/// root is `scene_root`, taking each variant chain's choice from `draw`.
///
/// # Errors
///
/// [`LaunchCameraError`] when the carrier, the timeline, a chain or a track is
/// not the measured shape.
pub fn measure_launch_camera(
    path: &RelativePath,
    bytes: &[u8],
    scene_root: &str,
    draw: &mut dyn FnMut() -> f32,
) -> Result<LaunchCameraScript, LaunchCameraError> {
    let key = path.logical_key();
    let mut context = ParseContext::with_defaults(&key);
    let needed = family_record(ZbdFamily::Animation)
        .header_rule()
        .signature()
        .map_or(0, |rule| rule.required_bytes())
        .min(bytes.len());
    let decision = dispatch(ZbdProbe::new(&key, path, &bytes[..needed]))
        .map_err(|error| LaunchCameraError(format!("{key}: dispatch ({})", error.code())))?;
    let index = read_animation_index(&mut context, decision, bytes)
        .map_err(|error| LaunchCameraError(format!("{key}: index ({})", error.code())))?;
    let payload = index
        .payload()
        .map_err(|error| LaunchCameraError(format!("{key}: payload ({})", error.code())))?;
    let records = payload
        .records()
        .map_err(|error| LaunchCameraError(format!("{key}: records ({})", error.code())))?;
    let rows = index
        .members()
        .iter()
        .filter(|row| row.path().to_ascii_lowercase().ends_with(b".zan"))
        .count();
    let tracks = read_animation_tracks(records.trailing(), rows)
        .map_err(|error| LaunchCameraError(format!("{key}: tracks ({})", error.code())))?;

    let view_of = |name: &str| -> Result<RecordView<'_>, LaunchCameraError> {
        let mut found = records.by_anim_name(name.as_bytes());
        let (Some(record), None) = (found.next(), found.next()) else {
            return refuse(format!("`{name}` must name exactly one record"));
        };
        RecordView::from_record(record).map_err(|error| LaunchCameraError(error.to_string()))
    };
    let intro = view_of(LAUNCH_INTRO_ANIMATION)?;
    let timeline: IntroTimeline = measure_intro_timeline(&intro, &[scene_root])
        .map_err(|error| LaunchCameraError(error.to_string()))?;

    let mut scenes = Vec::new();
    for span in &timeline.scenes {
        let Ok(view) = view_of(&span.animation) else {
            continue;
        };
        let camera_blocks: Vec<u32> = view
            .index_words
            .iter()
            .copied()
            .filter(|block| {
                tracks
                    .block(*block as usize)
                    .is_some_and(|b| b.object_name() == CAMERA_NODE)
            })
            .collect();
        if camera_blocks.is_empty() {
            continue;
        }
        let Some(stop) = span.stopped_at else {
            return refuse(format!("`{}` is never stopped", span.animation));
        };
        let chains = variant_chains(&span.animation, &view)
            .map_err(|error| LaunchCameraError(error.to_string()))?;
        let block = match chains.as_slice() {
            [] => match camera_blocks.as_slice() {
                [only] => *only,
                _ => {
                    return refuse(format!(
                        "`{}` has {} camera tracks and no variant chain",
                        span.animation,
                        camera_blocks.len()
                    ));
                }
            },
            [chain] => pick_variant(&chain.thresholds, &chain.track_blocks, draw)
                .ok_or_else(|| LaunchCameraError(format!("`{}`: empty chain", span.animation)))?,
            _ => {
                return refuse(format!(
                    "`{}` has more than one variant chain",
                    span.animation
                ));
            }
        };
        let Some(chosen) = tracks.block(block as usize) else {
            return refuse(format!("`{}`: no track block {block}", span.animation));
        };
        if chosen.object_name() != CAMERA_NODE {
            return refuse(format!(
                "`{}`: block {block} moves another node",
                span.animation
            ));
        }
        scenes.push(CameraScene {
            animation: span.animation.clone(),
            start: span.started_at,
            stop,
            track: CameraTrack::from_block(chosen),
        });
    }
    if scenes.is_empty() {
        return refuse("the intro starts no scene with a camera track");
    }
    Ok(LaunchCameraScript {
        scenes,
        length: timeline.length,
    })
}

/// A seeded uniform draw in `[0, 1)` standing in for the original's `rand()`
/// table (`SplitMix64`; the original's own sequence is not reproducible from
/// the data, see the timeline finding).
#[derive(Clone, Debug)]
pub struct VariantDraw {
    state: u64,
}

impl VariantDraw {
    /// A generator from `seed`.
    #[must_use]
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    /// The next draw.
    pub fn next_draw(&mut self) -> f32 {
        self.state = self.state.wrapping_add(0x9e37_79b9_7f4a_7c15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
        z ^= z >> 31;
        // 24 bits fill an f32 mantissa exactly: the value is in [0, 1).
        (z >> 40) as f32 / 16_777_216.0
    }
}

/// Reads [`LAUNCH_CAMERA_CARRIER`] from the installation and builds the
/// script, drawing the variants from `seed`.
///
/// # Errors
///
/// [`LaunchCameraError`] when the file cannot be read or measured.
pub fn read_launch_camera(
    install_root: &std::path::Path,
    scene_root: &str,
    seed: u64,
) -> Result<LaunchCameraScript, LaunchCameraError> {
    let bytes = std::fs::read(install_root.join(LAUNCH_CAMERA_CARRIER)).map_err(|error| {
        LaunchCameraError(format!("cannot read {LAUNCH_CAMERA_CARRIER}: {error}"))
    })?;
    let path = RelativePath::new(&LAUNCH_CAMERA_CARRIER.to_ascii_lowercase())
        .map_err(|error| LaunchCameraError(format!("path: {error}")))?;
    let mut draws = VariantDraw::new(seed);
    measure_launch_camera(&path, &bytes, scene_root, &mut || draws.next_draw())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_takes_the_first_draw_under_its_threshold() {
        let mut draws = [0.9_f32, 0.1].into_iter();
        let mut draw = || draws.next().expect("a draw");
        assert_eq!(
            pick_variant(&[0.2, 0.2, 0.2], &[0, 1, 2, 3], &mut draw),
            Some(1)
        );
    }

    #[test]
    fn draws_are_in_the_unit_interval_and_follow_the_seed() {
        let mut a = VariantDraw::new(7);
        let mut b = VariantDraw::new(7);
        for _ in 0..1000 {
            let x = a.next_draw();
            assert!((0.0..1.0).contains(&x));
            assert_eq!(x, b.next_draw());
        }
    }

    #[test]
    fn pick_falls_through_to_the_unconditional_variant() {
        let mut draw = || 0.99_f32;
        assert_eq!(
            pick_variant(&[0.2, 0.2, 0.2], &[0, 1, 2, 3], &mut draw),
            Some(3)
        );
    }
}

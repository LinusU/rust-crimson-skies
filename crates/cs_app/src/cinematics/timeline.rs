//! Authored camera timelines sampled per cinematic tick (F40-B).
//!
//! Spec: `specs/F40-cutscenes-video-scripted-cameras-and-transitions.md`,
//! stage `### F40-B`. A [`CameraTimeline`] is a list of keyframes at integer
//! cinematic ticks; [`CameraTimeline::sample`] is a pure function of the tick,
//! so a skip or a replay samples identically. Interpolation is linear and a
//! designed choice: the original track format and easing are unrecovered
//! (`docs/findings/2026-10-05-f40-b-playback-and-camera-timelines.md`).

use std::fmt;

/// A camera pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CameraPose {
    /// World position.
    pub position: [f32; 3],
    /// Vertical field of view in degrees.
    pub fov_deg: f32,
}

/// A pose at a tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    /// The cinematic tick.
    pub tick: u64,
    /// The pose.
    pub pose: CameraPose,
}

/// Why a timeline was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TimelineError {
    /// No keyframes.
    Empty,
    /// Ticks were not strictly increasing at this index.
    NotIncreasing {
        /// The offending keyframe index.
        index: usize,
    },
    /// A pose component was not finite.
    NotFinite {
        /// The offending keyframe index.
        index: usize,
    },
}

impl fmt::Display for TimelineError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => write!(f, "a camera timeline needs at least one keyframe"),
            Self::NotIncreasing { index } => {
                write!(f, "keyframe {index} is not after the previous keyframe")
            }
            Self::NotFinite { index } => write!(f, "keyframe {index} has a non-finite value"),
        }
    }
}

impl std::error::Error for TimelineError {}

/// A validated keyframe track.
#[derive(Clone, Debug, PartialEq)]
pub struct CameraTimeline {
    keys: Vec<Keyframe>,
}

impl CameraTimeline {
    /// Builds a timeline.
    ///
    /// # Errors
    ///
    /// [`TimelineError`] for no keyframes, non-increasing ticks or a
    /// non-finite value.
    pub fn try_new(keys: Vec<Keyframe>) -> Result<Self, TimelineError> {
        if keys.is_empty() {
            return Err(TimelineError::Empty);
        }
        for (index, key) in keys.iter().enumerate() {
            let finite =
                key.pose.fov_deg.is_finite() && key.pose.position.iter().all(|c| c.is_finite());
            if !finite {
                return Err(TimelineError::NotFinite { index });
            }
            if index > 0 && key.tick <= keys[index - 1].tick {
                return Err(TimelineError::NotIncreasing { index });
            }
        }
        Ok(Self { keys })
    }

    /// The tick of the last keyframe.
    #[must_use]
    pub fn end_tick(&self) -> u64 {
        self.keys.last().map_or(0, |key| key.tick)
    }

    /// The pose at `tick`: clamped before the first and after the last
    /// keyframe, linear between.
    #[must_use]
    pub fn sample(&self, tick: u64) -> CameraPose {
        let after = self.keys.partition_point(|key| key.tick <= tick);
        if after == 0 {
            return self.keys[0].pose;
        }
        let prev = &self.keys[after - 1];
        let Some(next) = self.keys.get(after) else {
            return prev.pose;
        };
        let span = (next.tick - prev.tick) as f32;
        let t = (tick - prev.tick) as f32 / span;
        let lerp = |a: f32, b: f32| (b - a).mul_add(t, a);
        CameraPose {
            position: [
                lerp(prev.pose.position[0], next.pose.position[0]),
                lerp(prev.pose.position[1], next.pose.position[1]),
                lerp(prev.pose.position[2], next.pose.position[2]),
            ],
            fov_deg: lerp(prev.pose.fov_deg, next.pose.fov_deg),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn key(tick: u64, x: f32, fov: f32) -> Keyframe {
        Keyframe {
            tick,
            pose: CameraPose {
                position: [x, 0.0, 0.0],
                fov_deg: fov,
            },
        }
    }

    fn timeline() -> CameraTimeline {
        CameraTimeline::try_new(vec![key(10, 0.0, 60.0), key(20, 100.0, 40.0)]).unwrap()
    }

    #[test]
    fn accept_f40_b_timeline_interpolates_between_keys_and_clamps_outside() {
        let t = timeline();
        assert_eq!(t.sample(0).position[0], 0.0);
        assert_eq!(t.sample(10).position[0], 0.0);
        assert_eq!(t.sample(15).position[0], 50.0);
        assert_eq!(t.sample(15).fov_deg, 50.0);
        assert_eq!(t.sample(20).position[0], 100.0);
        assert_eq!(t.sample(999).fov_deg, 40.0);
        assert_eq!(t.end_tick(), 20);
    }

    #[test]
    fn accept_f40_b_timeline_sampling_is_a_pure_function_of_the_tick() {
        let t = timeline();
        // A skip straight to the end and a tick-by-tick playout agree.
        let stepped = (0..=20).map(|tick| t.sample(tick)).next_back().unwrap();
        assert_eq!(stepped, t.sample(20));
        assert_eq!(t.sample(13), t.sample(13));
    }

    #[test]
    fn accept_f40_b_timeline_refuses_bad_tracks() {
        assert_eq!(CameraTimeline::try_new(vec![]), Err(TimelineError::Empty));
        assert_eq!(
            CameraTimeline::try_new(vec![key(5, 0.0, 60.0), key(5, 1.0, 60.0)]),
            Err(TimelineError::NotIncreasing { index: 1 })
        );
        assert_eq!(
            CameraTimeline::try_new(vec![key(1, f32::NAN, 60.0)]),
            Err(TimelineError::NotFinite { index: 0 })
        );
    }
}

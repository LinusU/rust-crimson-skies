//! Tick-indexed trajectories: position, orientation and velocity from one
//! function.

use cs_types::Tick;

use super::math::{Quat, norm};

/// One authored key: the pose at an integer tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Keyframe {
    pub tick: Tick,
    pub position_m: [f64; 3],
    pub orientation: Quat,
}

/// A refused trajectory.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrajectoryError {
    /// Fewer than one key.
    Empty,
    /// Keys are not strictly increasing in tick.
    NotAscending { index: usize },
    /// A position was non-finite or an orientation was not a unit quaternion.
    Invalid { index: usize },
    /// The tick rate was zero.
    ZeroTickRate,
}

/// The kinematic state of an actor at one tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Pose {
    pub position_m: [f64; 3],
    pub orientation: Quat,
    /// The derivative of `position_m` with respect to time.
    pub velocity_m_s: [f64; 3],
    /// World-frame angular velocity.
    pub angular_velocity_rad_s: [f64; 3],
}

/// A piecewise-linear path (slerped orientation) over simulation ticks.
///
/// Before the first key and from the last the actor holds the end pose with
/// zero velocity: a train that reached the end of its authored path is
/// stopped, it does not extrapolate.
#[derive(Clone, Debug, PartialEq)]
pub struct Trajectory {
    keys: Vec<Keyframe>,
    ticks_per_second: u32,
}

impl Trajectory {
    /// # Errors
    ///
    /// [`TrajectoryError`] for empty, unordered or non-finite keys, or a zero
    /// tick rate.
    pub fn new(keys: Vec<Keyframe>, ticks_per_second: u32) -> Result<Self, TrajectoryError> {
        if ticks_per_second == 0 {
            return Err(TrajectoryError::ZeroTickRate);
        }
        if keys.is_empty() {
            return Err(TrajectoryError::Empty);
        }
        for (index, k) in keys.iter().enumerate() {
            if !k.position_m.iter().all(|v| v.is_finite()) || !k.orientation.is_unit() {
                return Err(TrajectoryError::Invalid { index });
            }
            if index > 0 && k.tick <= keys[index - 1].tick {
                return Err(TrajectoryError::NotAscending { index });
            }
        }
        Ok(Self {
            keys,
            ticks_per_second,
        })
    }

    /// The pose at `tick`. Depends on the tick only: whether anything is
    /// visible, loaded or culled cannot change it.
    #[must_use]
    pub fn sample(&self, tick: Tick) -> Pose {
        let first = self.keys[0];
        let last = self.keys[self.keys.len() - 1];
        let held = |k: Keyframe| Pose {
            position_m: k.position_m,
            orientation: k.orientation,
            velocity_m_s: [0.0; 3],
            angular_velocity_rad_s: [0.0; 3],
        };
        if tick < first.tick {
            return held(first);
        }
        if tick >= last.tick {
            return held(last);
        }
        // At a key tick the outgoing segment applies, so a path that starts
        // moving at its first key has its velocity there.
        let hi = self.keys.partition_point(|k| k.tick <= tick);
        let (a, b) = (self.keys[hi - 1], self.keys[hi]);
        let span_ticks = (b.tick.0 - a.tick.0) as f64;
        let t = (tick.0 - a.tick.0) as f64 / span_ticks;
        let span_s = span_ticks / f64::from(self.ticks_per_second);
        let mut position_m = [0.0; 3];
        let mut velocity_m_s = [0.0; 3];
        for i in 0..3 {
            let d = b.position_m[i] - a.position_m[i];
            position_m[i] = a.position_m[i] + d * t;
            velocity_m_s[i] = d / span_s;
        }
        let w = a.orientation.slerp_angular_velocity(b.orientation);
        Pose {
            position_m,
            orientation: a.orientation.slerp(b.orientation, t),
            velocity_m_s,
            angular_velocity_rad_s: w.map(|v| v / span_s),
        }
    }

    /// The tick of the final key.
    #[must_use]
    pub fn end_tick(&self) -> Tick {
        self.keys[self.keys.len() - 1].tick
    }

    /// Speed at `tick`, in m/s.
    #[must_use]
    pub fn speed_m_s(&self, tick: Tick) -> f64 {
        norm(self.sample(tick).velocity_m_s)
    }
}

/// The synthetic train: 100 m along +X in 10 s at 10 ticks/s, no rotation.
#[must_use]
pub fn synthetic_train_trajectory() -> Trajectory {
    Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(100),
                position_m: [100.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        ],
        10,
    )
    .expect("synthetic train trajectory is valid")
}

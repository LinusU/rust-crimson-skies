//! Force-feedback effects and the commands that play them (SIDEWINDER-FFB2, #1968).
//!
//! The vocabulary is the standard one of USB PID and DirectInput: constant, ramp,
//! periodic and condition (spring, damper, inertia, friction) effects with an
//! envelope, a gain, a direction and a duration. Units follow DirectInput:
//! magnitudes, gains and saturations in -10000..=10000, times in microseconds,
//! angles in hundredths of a degree. The [`pid`](super::pid) driver scales them into
//! each device's own report ranges.
//!
//! Ported from the owner's drydock86 project (`drydock-joystick/src/ff.rs`, same
//! author).

/// Duration or iteration count that never ends.
pub const INFINITE: u32 = u32::MAX;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Waveform {
    Square,
    Sine,
    Triangle,
    SawtoothUp,
    SawtoothDown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConditionKind {
    Spring,
    Damper,
    Inertia,
    Friction,
}

/// One axis of a condition effect (DICONDITION).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Condition {
    pub offset: i32,
    pub positive_coefficient: i32,
    pub negative_coefficient: i32,
    pub positive_saturation: u32,
    pub negative_saturation: u32,
    pub dead_band: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EffectKind {
    Constant {
        magnitude: i32,
    },
    Ramp {
        start: i32,
        end: i32,
    },
    Periodic {
        waveform: Waveform,
        magnitude: u32,
        offset: i32,
        phase: u32,
        period_us: u32,
    },
    /// One entry per axis, in the order of [`Effect::axes`].
    Condition {
        kind: ConditionKind,
        axes: Vec<Condition>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Envelope {
    pub attack_level: u32,
    pub attack_us: u32,
    pub fade_level: u32,
    pub fade_us: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Axis {
    X,
    Y,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Effect {
    pub kind: EffectKind,
    /// Microseconds, or [`INFINITE`].
    pub duration_us: u32,
    pub sample_period_us: u32,
    /// 0..=10000.
    pub gain: u32,
    /// Zero-based button that plays the effect.
    pub trigger_button: Option<u8>,
    pub trigger_repeat_us: u32,
    /// Actuators the effect drives.
    pub axes: Vec<Axis>,
    /// Polar direction: hundredths of a degree clockwise from north (the -y direction),
    /// as DirectInput reports `DIEFF_POLAR` directions.
    pub direction: u32,
    pub envelope: Option<Envelope>,
    pub start_delay_us: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Create or update the effect with this id (the caller's own effect name).
    Download {
        id: u32,
        effect: Effect,
    },
    /// Play the effect `iterations` times ([`INFINITE`] loops); `solo` stops the others.
    Start {
        id: u32,
        iterations: u32,
        solo: bool,
    },
    Stop {
        id: u32,
    },
    /// Remove the effect from the device.
    Unload {
        id: u32,
    },
    /// Stop and unload every effect.
    Reset,
    StopAll,
    Pause,
    Continue,
    Actuators(bool),
    /// Device gain 0..=10000.
    Gain(u32),
    Autocenter(bool),
}

impl Command {
    fn effect_id(&self) -> Option<u32> {
        match self {
            Command::Download { id, .. }
            | Command::Start { id, .. }
            | Command::Stop { id }
            | Command::Unload { id } => Some(*id),
            _ => None,
        }
    }
}

/// Drop parameter updates that a later update of the same effect replaces, so a caller
/// that changes an effect every frame does not queue up work for a slow device.
pub fn coalesce(cmds: Vec<Command>) -> Vec<Command> {
    let mut keep = vec![true; cmds.len()];
    for (i, c) in cmds.iter().enumerate() {
        let Command::Download { id, .. } = c else {
            continue;
        };
        for later in &cmds[i + 1..] {
            match later {
                Command::Download { id: l, .. } if l == id => {
                    keep[i] = false;
                    break;
                }
                // Anything that orders against this effect (or all effects) must see it.
                c if c.effect_id().is_none_or(|l| l == *id) => break,
                _ => {}
            }
        }
    }
    cmds.into_iter()
        .zip(keep)
        .filter(|(_, k)| *k)
        .map(|(c, _)| c)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn constant(m: i32) -> Effect {
        Effect {
            kind: EffectKind::Constant { magnitude: m },
            duration_us: INFINITE,
            sample_period_us: 0,
            gain: 10000,
            trigger_button: None,
            trigger_repeat_us: 0,
            axes: vec![Axis::X, Axis::Y],
            direction: 0,
            envelope: None,
            start_delay_us: 0,
        }
    }

    #[test]
    fn coalesce_keeps_the_last_update_before_each_ordering_command() {
        let cmds = vec![
            Command::Download {
                id: 1,
                effect: constant(1),
            },
            Command::Download {
                id: 2,
                effect: constant(2),
            },
            Command::Download {
                id: 1,
                effect: constant(3),
            },
            Command::Start {
                id: 1,
                iterations: 1,
                solo: false,
            },
            Command::Download {
                id: 1,
                effect: constant(4),
            },
            Command::StopAll,
            Command::Download {
                id: 1,
                effect: constant(5),
            },
        ];
        let out = coalesce(cmds);
        let mags: Vec<_> = out
            .iter()
            .map(|c| match c {
                Command::Download { id, effect } => match effect.kind {
                    EffectKind::Constant { magnitude } => format!("d{id}:{magnitude}"),
                    _ => unreachable!(),
                },
                Command::Start { id, .. } => format!("s{id}"),
                Command::StopAll => "stopall".into(),
                _ => "?".into(),
            })
            .collect();
        assert_eq!(mags, ["d2:2", "d1:3", "s1", "d1:4", "stopall", "d1:5"]);
    }
}

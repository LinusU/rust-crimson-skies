//! The designed 60-minute soak and its memory-trend bound (F60-A, AC01).
//!
//! A soak is a repeating cycle of mission play, an AI engagement and a return
//! to the menu. One [`SoakSample`] is taken at the end of every cycle's menu
//! phase, where a correct game is back in a known state, so a counter that does
//! not return to its baseline there is a leak (sheet rule 4), and the resident
//! memory of those samples must not trend upward past the bound. The first
//! `warmup_cycles` cycles are excluded from the trend because caches fill
//! there; the leak baseline is the last warm-up sample.
//!
//! Time is integer simulation ticks at [`SIM_HZ`](super::scenario::SIM_HZ).
//! The trend is an exact integer least-squares slope: no floats.

use std::fmt;

use super::scenario::SIM_HZ;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SoakPhase {
    MissionPlay,
    AiEngagement,
    MenuReturn,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoakPlan {
    /// Phase lengths of one cycle, in ticks, in order.
    pub cycle: Vec<(SoakPhase, u64)>,
    pub cycles: u32,
    pub warmup_cycles: u32,
}

impl SoakPlan {
    /// Minimum soak length from the sheet, in ticks.
    pub const MIN_TICKS: u64 = 60 * 60 * SIM_HZ;

    /// Ten 6-minute cycles (3 mission, 2 AI, 1 menu): exactly 60 minutes.
    pub fn designed() -> Self {
        let minute = 60 * SIM_HZ;
        Self {
            cycle: vec![
                (SoakPhase::MissionPlay, 3 * minute),
                (SoakPhase::AiEngagement, 2 * minute),
                (SoakPhase::MenuReturn, minute),
            ],
            cycles: 10,
            warmup_cycles: 1,
        }
    }

    pub fn ticks_per_cycle(&self) -> u64 {
        self.cycle.iter().map(|(_, t)| t).sum()
    }

    pub fn total_ticks(&self) -> u64 {
        self.ticks_per_cycle() * u64::from(self.cycles)
    }

    /// Tick at which the sample of `cycle` (0-based) is due.
    pub fn sample_tick(&self, cycle: u32) -> u64 {
        self.ticks_per_cycle() * (u64::from(cycle) + 1)
    }

    pub fn validate(&self) -> Result<(), SoakError> {
        let has = |p| self.cycle.iter().any(|(q, t)| *q == p && *t > 0);
        if !(has(SoakPhase::MissionPlay)
            && has(SoakPhase::AiEngagement)
            && has(SoakPhase::MenuReturn))
        {
            return Err(SoakError::PlanMissingPhase);
        }
        if self.total_ticks() < Self::MIN_TICKS {
            return Err(SoakError::PlanTooShort {
                ticks: self.total_ticks(),
                required: Self::MIN_TICKS,
            });
        }
        // One warm-up sample for the baseline, three for a trend.
        if self.warmup_cycles == 0 || self.cycles < self.warmup_cycles + 3 {
            return Err(SoakError::PlanTooFewCycles);
        }
        Ok(())
    }
}

/// Counters sampled at a cycle's menu boundary.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SoakSample {
    pub tick: u64,
    pub resident_bytes: u64,
    pub entities: u64,
    pub asset_handles: u64,
    pub audio_loops: u64,
    pub tasks: u64,
}

/// Memory trend limit: resident bytes may grow by at most this per hour.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MemoryTrendBound {
    pub max_growth_bytes_per_hour: u64,
}

impl MemoryTrendBound {
    /// Authored development value (16 MiB/h); to be replaced from a measured
    /// baseline.
    pub const DESIGNED: Self = Self {
        max_growth_bytes_per_hour: 16 * 1024 * 1024,
    };
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeakCounter {
    Entities,
    AssetHandles,
    AudioLoops,
    Tasks,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CycleLeak {
    pub cycle: u32,
    pub counter: LeakCounter,
    pub baseline: u64,
    pub got: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SoakReport {
    pub leaks: Vec<CycleLeak>,
    /// Least-squares resident growth, bytes per hour, truncated toward zero.
    pub growth_bytes_per_hour: i128,
    pub trend_within_bound: bool,
}

impl SoakReport {
    pub fn passed(&self) -> bool {
        self.leaks.is_empty() && self.trend_within_bound
    }
}

/// The soak did not produce evidence that can be judged at all.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SoakError {
    PlanMissingPhase,
    PlanTooShort { ticks: u64, required: u64 },
    PlanTooFewCycles,
    SampleCount { expected: usize, got: usize },
    SampleTick { cycle: u32, expected: u64, got: u64 },
}

impl fmt::Display for SoakError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::PlanMissingPhase => write!(f, "soak cycle lacks a mission, AI or menu phase"),
            Self::PlanTooShort { ticks, required } => {
                write!(f, "soak is {ticks} ticks, needs {required}")
            }
            Self::PlanTooFewCycles => write!(f, "soak has too few cycles for a baseline and trend"),
            Self::SampleCount { expected, got } => {
                write!(f, "expected {expected} samples, got {got}")
            }
            Self::SampleTick {
                cycle,
                expected,
                got,
            } => write!(f, "cycle {cycle} sampled at tick {got}, due at {expected}"),
        }
    }
}

impl std::error::Error for SoakError {}

/// Judge a soak: refuse an invalid plan or misplaced/missing samples, then
/// report leaks against the last warm-up sample and the exact memory trend.
pub fn evaluate_soak(
    plan: &SoakPlan,
    bound: MemoryTrendBound,
    samples: &[SoakSample],
) -> Result<SoakReport, SoakError> {
    plan.validate()?;
    if samples.len() != plan.cycles as usize {
        return Err(SoakError::SampleCount {
            expected: plan.cycles as usize,
            got: samples.len(),
        });
    }
    for (i, s) in samples.iter().enumerate() {
        let cycle = i as u32;
        let expected = plan.sample_tick(cycle);
        if s.tick != expected {
            return Err(SoakError::SampleTick {
                cycle,
                expected,
                got: s.tick,
            });
        }
    }

    let warm = plan.warmup_cycles as usize;
    let baseline = samples[warm - 1];
    let mut leaks = Vec::new();
    for (i, s) in samples.iter().enumerate().skip(warm) {
        let counters = [
            (LeakCounter::Entities, baseline.entities, s.entities),
            (
                LeakCounter::AssetHandles,
                baseline.asset_handles,
                s.asset_handles,
            ),
            (LeakCounter::AudioLoops, baseline.audio_loops, s.audio_loops),
            (LeakCounter::Tasks, baseline.tasks, s.tasks),
        ];
        for (counter, base, got) in counters {
            if got != base {
                leaks.push(CycleLeak {
                    cycle: i as u32,
                    counter,
                    baseline: base,
                    got,
                });
            }
        }
    }

    let pts: Vec<(i128, i128)> = samples[warm..]
        .iter()
        .map(|s| (i128::from(s.tick), i128::from(s.resident_bytes)))
        .collect();
    let n = pts.len() as i128;
    let sx: i128 = pts.iter().map(|p| p.0).sum();
    let sy: i128 = pts.iter().map(|p| p.1).sum();
    let sxx: i128 = pts.iter().map(|p| p.0 * p.0).sum();
    let sxy: i128 = pts.iter().map(|p| p.0 * p.1).sum();
    let num = n * sxy - sx * sy;
    let den = n * sxx - sx * sx; // > 0: the sample ticks are distinct
    let ticks_per_hour = i128::from(3600 * SIM_HZ);
    Ok(SoakReport {
        leaks,
        growth_bytes_per_hour: num * ticks_per_hour / den,
        trend_within_bound: num * ticks_per_hour
            <= i128::from(bound.max_growth_bytes_per_hour) * den,
    })
}

/// Synthetic fixture: 512 MiB resident, a one-off 64 MiB cache fill in the
/// first cycle, then `growth_per_cycle` bytes per cycle, and
/// `leaked_entities_per_cycle` entities left behind each cycle. Not a
/// measurement of anything.
pub fn synthetic_soak_samples(
    plan: &SoakPlan,
    growth_per_cycle: u64,
    leaked_entities_per_cycle: u64,
) -> Vec<SoakSample> {
    const MIB: u64 = 1024 * 1024;
    (0..plan.cycles)
        .map(|c| SoakSample {
            tick: plan.sample_tick(c),
            resident_bytes: 512 * MIB + 64 * MIB + growth_per_cycle * u64::from(c),
            entities: 1_000 + leaked_entities_per_cycle * u64::from(c),
            asset_handles: 300,
            audio_loops: 2,
            tasks: 4,
        })
        .collect()
}

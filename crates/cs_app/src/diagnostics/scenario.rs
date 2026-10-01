//! Scenarios, hardware profiles and budget evaluation (F60-A, AC02 inputs).

use std::fmt;

/// Simulation rate the budgets are stated against (F23).
pub const SIM_HZ: u64 = 120;
/// Designed presentation target: 60 FPS, in microseconds per frame.
pub const FRAME_BUDGET_US_60FPS: u64 = 16_667;
/// Designed simulation budget per 120 Hz tick, in microseconds.
pub const SIM_TICK_BUDGET_US_120HZ: u64 = 8_333;

/// A system a scenario must keep running. Removing one to pass a budget is
/// refused ([`ScenarioError::SystemDisabled`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum SystemKind {
    Collision,
    Ai,
    Audio,
    MissionContent,
    Rendering,
    Networking,
}

impl SystemKind {
    /// Systems every scenario keeps on.
    pub const ALWAYS: [SystemKind; 5] = [
        SystemKind::Collision,
        SystemKind::Ai,
        SystemKind::Audio,
        SystemKind::MissionContent,
        SystemKind::Rendering,
    ];
}

/// The worst cases the sheet names, plus the AC01/AC02 scenarios.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScenarioKind {
    WorstCaseCampaignBattle,
    DenseWorld,
    SmokeTransparentEffects,
    ManyProjectiles,
    FullMultiplayerLoad,
    ColdLoad,
    WarmLoad,
    Soak,
}

impl ScenarioKind {
    pub const ALL: [ScenarioKind; 8] = [
        ScenarioKind::WorstCaseCampaignBattle,
        ScenarioKind::DenseWorld,
        ScenarioKind::SmokeTransparentEffects,
        ScenarioKind::ManyProjectiles,
        ScenarioKind::FullMultiplayerLoad,
        ScenarioKind::ColdLoad,
        ScenarioKind::WarmLoad,
        ScenarioKind::Soak,
    ];
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Platform {
    MacosAppleSilicon,
    WindowsX86_64,
    LinuxX86_64,
}

/// Quality is an explicit setting; the correctness invariants do not change
/// with it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quality {
    Reference,
    Reduced,
}

/// The hardware, resolution and settings a budget is stated for. No budget
/// exists without one.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HardwareProfile {
    pub id: String,
    pub platform: Platform,
    pub resolution: (u32, u32),
    pub quality: Quality,
    /// `true` once a fixed baseline was measured on this machine. Until then
    /// only the designed frame and simulation targets exist.
    pub baseline_measured: bool,
}

impl HardwareProfile {
    /// Designed target profile for a platform at 1080p, reference quality. It
    /// is not a machine: the owner's reference machine is unrecorded.
    pub fn designed_1080p(platform: Platform) -> Self {
        let id = match platform {
            Platform::MacosAppleSilicon => "macos-apple-silicon-1080p",
            Platform::WindowsX86_64 => "windows-x86_64-1080p",
            Platform::LinuxX86_64 => "linux-x86_64-1080p",
        };
        Self {
            id: id.to_owned(),
            platform,
            resolution: (1920, 1080),
            quality: Quality::Reference,
            baseline_measured: false,
        }
    }
}

/// One limit of a budget.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BudgetLimit {
    /// Authored target, not yet backed by a measured baseline.
    Designed(u64),
    /// Set from a measured baseline on the profile's hardware.
    Measured(u64),
    /// Nothing may be claimed until a baseline is measured.
    Unset,
}

impl BudgetLimit {
    fn limit(self) -> Option<u64> {
        match self {
            BudgetLimit::Designed(v) | BudgetLimit::Measured(v) => Some(v),
            BudgetLimit::Unset => None,
        }
    }
}

/// Frame-time style distribution, microseconds.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Percentiles {
    pub p50: u64,
    pub p95: u64,
    pub p99: u64,
}

/// Nearest-rank percentiles of `samples`; `None` when there are none.
pub fn percentiles(samples: &[u64]) -> Option<Percentiles> {
    if samples.is_empty() {
        return None;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_unstable();
    let rank = |pct: usize| sorted[(sorted.len() * pct).div_ceil(100).max(1) - 1];
    Some(Percentiles {
        p50: rank(50),
        p95: rank(95),
        p99: rank(99),
    })
}

/// Budgets for one scenario on one hardware profile.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BudgetTable {
    pub frame_p95_us: BudgetLimit,
    pub frame_p99_us: BudgetLimit,
    pub sim_tick_p99_us: BudgetLimit,
    pub load_ms: BudgetLimit,
    pub peak_memory_bytes: BudgetLimit,
    pub cache_bytes: BudgetLimit,
}

/// A scenario bound to a profile and its budget.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BenchmarkScenario {
    pub kind: ScenarioKind,
    pub profile: HardwareProfile,
    pub required_systems: Vec<SystemKind>,
    pub budget: BudgetTable,
}

/// Build the scenario for `kind` on `profile`. Presentation scenarios get the
/// designed 60 FPS / 120 Hz limits; loads and memory stay `Unset` until the
/// profile's baseline is measured (the sheet: "set budgets after measuring").
pub fn scenario_for(kind: ScenarioKind, profile: HardwareProfile) -> BenchmarkScenario {
    let mut required = SystemKind::ALWAYS.to_vec();
    if kind == ScenarioKind::FullMultiplayerLoad {
        required.push(SystemKind::Networking);
    }
    let timed = !matches!(kind, ScenarioKind::ColdLoad | ScenarioKind::WarmLoad);
    let designed = |on: bool, v: u64| {
        if on {
            BudgetLimit::Designed(v)
        } else {
            BudgetLimit::Unset
        }
    };
    let budget = BudgetTable {
        frame_p95_us: designed(timed, FRAME_BUDGET_US_60FPS),
        frame_p99_us: BudgetLimit::Unset,
        sim_tick_p99_us: designed(timed, SIM_TICK_BUDGET_US_120HZ),
        load_ms: BudgetLimit::Unset,
        peak_memory_bytes: BudgetLimit::Unset,
        cache_bytes: BudgetLimit::Unset,
    };
    BenchmarkScenario {
        kind,
        profile,
        required_systems: required,
        budget,
    }
}

/// What one run measured. Absent measurements stay `None`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RunMeasurements {
    pub profile_id: String,
    pub resolution: (u32, u32),
    pub quality: Quality,
    pub enabled_systems: Vec<SystemKind>,
    pub frame_us: Option<Percentiles>,
    pub sim_tick_us: Option<Percentiles>,
    pub load_ms: Option<u64>,
    pub peak_memory_bytes: Option<u64>,
    pub cache_bytes: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ScenarioError {
    /// Measured on other hardware, resolution or settings than the budget.
    ProfileMismatch { expected: String, got: String },
    /// A required system was off, so the numbers say nothing.
    SystemDisabled(SystemKind),
}

impl fmt::Display for ScenarioError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProfileMismatch { expected, got } => {
                write!(f, "measured on {got}, budget is for {expected}")
            }
            Self::SystemDisabled(s) => write!(f, "required system {s:?} was disabled"),
        }
    }
}

impl std::error::Error for ScenarioError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Verdict {
    Pass,
    Fail {
        limit: u64,
        measured: u64,
    },
    /// No limit, or no measurement: never a pass.
    Unevaluated,
}

/// Per-limit verdicts for one run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioReport {
    pub frame_p95: Verdict,
    pub frame_p99: Verdict,
    pub sim_tick_p99: Verdict,
    pub load: Verdict,
    pub peak_memory: Verdict,
    pub cache: Verdict,
}

impl ScenarioReport {
    fn all(&self) -> [Verdict; 6] {
        [
            self.frame_p95,
            self.frame_p99,
            self.sim_tick_p99,
            self.load,
            self.peak_memory,
            self.cache,
        ]
    }

    /// Every limit was set, measured and met.
    pub fn is_complete_pass(&self) -> bool {
        self.all().iter().all(|v| *v == Verdict::Pass)
    }

    /// No limit failed (unevaluated limits do not count as failures here, but
    /// [`Self::is_complete_pass`] is the only statement of success).
    pub fn has_failure(&self) -> bool {
        self.all().iter().any(|v| matches!(v, Verdict::Fail { .. }))
    }
}

fn judge(limit: BudgetLimit, measured: Option<u64>) -> Verdict {
    match (limit.limit(), measured) {
        (Some(limit), Some(measured)) if measured <= limit => Verdict::Pass,
        (Some(limit), Some(measured)) => Verdict::Fail { limit, measured },
        _ => Verdict::Unevaluated,
    }
}

impl BenchmarkScenario {
    /// Judge a run. A run on other hardware/resolution/quality, or with a
    /// required system off, is an error, not a verdict.
    pub fn evaluate(&self, run: &RunMeasurements) -> Result<ScenarioReport, ScenarioError> {
        if run.profile_id != self.profile.id
            || run.resolution != self.profile.resolution
            || run.quality != self.profile.quality
        {
            return Err(ScenarioError::ProfileMismatch {
                expected: self.profile.id.clone(),
                got: run.profile_id.clone(),
            });
        }
        if let Some(missing) = self
            .required_systems
            .iter()
            .find(|s| !run.enabled_systems.contains(s))
        {
            return Err(ScenarioError::SystemDisabled(*missing));
        }
        let b = &self.budget;
        Ok(ScenarioReport {
            frame_p95: judge(b.frame_p95_us, run.frame_us.map(|p| p.p95)),
            frame_p99: judge(b.frame_p99_us, run.frame_us.map(|p| p.p99)),
            sim_tick_p99: judge(b.sim_tick_p99_us, run.sim_tick_us.map(|p| p.p99)),
            load: judge(b.load_ms, run.load_ms),
            peak_memory: judge(b.peak_memory_bytes, run.peak_memory_bytes),
            cache: judge(b.cache_bytes, run.cache_bytes),
        })
    }
}

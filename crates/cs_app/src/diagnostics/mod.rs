//! Benchmark scenarios, hardware-specific budgets and the soak contract
//! (F60-A).
//!
//! Spec: `specs/F60-performance-memory-stability-and-platforms.md`, stage
//! `### F60-A`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! This stage defines *what is measured and what counts as a pass*; it does
//! not instrument the running game (F60-B) and measures nothing itself.
//!
//! * [`scenario`] names the worst-case scenarios, the systems each one must
//!   keep enabled (collision, AI, audio and mission content are never traded
//!   for speed), the hardware profile a budget is stated for and the budget
//!   evaluation. A budget without a measured baseline is `Unset` and reports
//!   [`scenario::Verdict::Unevaluated`], never a pass.
//! * [`soak`] is AC01: the designed 60-minute mission / Instant Action / menu
//!   soak, the samples taken at each cycle's menu boundary and the evaluation
//!   that bounds the memory trend and refuses any leaked entity, asset handle,
//!   audio loop or task.
//!
//! All numbers are authored development targets, not measurements of the
//! original game or of any machine.

pub mod scenario;
pub mod soak;

pub use scenario::{
    BenchmarkScenario, BudgetLimit, BudgetTable, HardwareProfile, Percentiles, Platform, Quality,
    RunMeasurements, ScenarioError, ScenarioKind, ScenarioReport, SystemKind, Verdict, percentiles,
    scenario_for,
};
pub use soak::{
    CycleLeak, LeakCounter, MemoryTrendBound, SoakError, SoakPhase, SoakPlan, SoakReport,
    SoakSample, evaluate_soak, synthetic_soak_samples,
};

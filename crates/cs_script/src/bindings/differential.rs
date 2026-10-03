//! Normalized differential traces of one measured scenario (F38-B).
//!
//! `specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
//! AC02: "Differential traces compare normalized original and recreated event
//! ordering for a measured scenario." Shared contract
//! `docs/contracts/SCRIPT-MISSION.md`.
//!
//! The two traces are **not** a byte comparison and not a simulation replay:
//!
//! * the **original** trace is what a measured original program *declares* —
//!   the dispatch values it spells, in the order the corpus measured them;
//! * the **recreated** trace is what the recreated engine *emits* — the
//!   [`crate::runtime::MissionEvent`]s its runtime produced for a
//!   [`crate::ir::MissionProgram`] the caller built from that same measurement.
//!
//! No family of the measured corpus lowers to an engine operation yet, so no
//! *real* lowering of a measured program exists to compare against: a caller
//! supplies the lowering and this module reduces both sides to the same
//! [`NormalizedStep`] vocabulary. It compares what it is given and claims
//! nothing beyond that.
//!
//! Both are normalized to [`NormalizedStep`]s first: a step names a dispatch
//! value and where it sat, with every unstable identifier (symbols, entity ids,
//! addresses) dropped. Only the ordering and the call identities are compared,
//! which is exactly what AC02 asks for; nothing here compares values that the
//! original was never measured to produce.
//!
//! The comparison is one-directional and fails closed: an original step with no
//! recreated counterpart is a divergence at its own index, never a dropped step
//! on the recreated side, and the whole comparison is refused when the two
//! traces were not built from the same measured scenario.

use std::fmt;

use crate::bindings::observed::MeasuredForm;
use crate::runtime::MissionEvent;

/// One declared host call of a measured original program, in source order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DeclaredStep {
    /// The measured dispatch form.
    pub form: MeasuredForm,
    /// The measured dispatch value.
    pub native_id: i64,
    /// The site's ordinal in the program, from 0.
    pub ordinal: u32,
    /// The site's byte offset in the original program.
    pub offset: u64,
}

/// A normalized trace: the comparable form of a declared or emitted sequence.
///
/// Normalization drops everything that cannot be compared across the two sides
/// — program counters, byte offsets, session and tick numbers — and keeps the
/// dispatch value, the form and the ordering. Two normalized traces that are
/// equal describe the same host-call ordering.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NormalizedTrace {
    steps: Vec<NormalizedStep>,
}

impl NormalizedTrace {
    /// The normalized steps, in order.
    pub fn steps(&self) -> &[NormalizedStep] {
        &self.steps
    }

    /// How many steps the trace holds.
    pub fn len(&self) -> usize {
        self.steps.len()
    }

    /// Whether the trace holds no step.
    pub fn is_empty(&self) -> bool {
        self.steps.is_empty()
    }

    /// The step kinds, in order — the sequence a report renders.
    pub fn kinds(&self) -> Vec<TraceStepKind> {
        self.steps.iter().map(NormalizedStep::kind).collect()
    }
}

/// One normalized step.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NormalizedStep {
    /// What this step carries.
    pub kind: TraceStepKind,
    /// The site's position in the original program.
    pub ordinal: u32,
    /// The emitting source symbol, for a recreated step; `None` for a declared
    /// step, which has no symbol.
    pub source: Option<u32>,
    /// The emitting program sequence, for a recreated step.
    pub sequence: Option<u32>,
}

impl NormalizedStep {
    /// What this step is, for a comparison and for a report.
    pub const fn kind(&self) -> TraceStepKind {
        self.kind
    }
}

/// The comparable identity of one step: the measured dispatch value it carries.
///
/// [`TraceStepKind::Unattributed`] is a distinct variant rather than a sentinel
/// id, so an engine event that could not be traced back to a measured call can
/// never compare equal to a declared step — not even one whose dispatch value
/// was itself never measured.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum TraceStepKind {
    /// A measured dispatch value.
    Measured { form: MeasuredForm, native_id: i64 },
    /// An emitted event no declared step accounts for.
    Unattributed,
}

impl fmt::Display for TraceStepKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Measured { form, native_id } => write!(f, "{}#{}", form.label(), native_id),
            Self::Unattributed => f.write_str("unattributed"),
        }
    }
}

/// Why a differential comparison was refused.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceError {
    /// The two traces were normalized from different measured scenarios.
    ScenarioMismatch {
        /// The original scenario's digest.
        original: String,
        /// The recreated scenario's digest.
        recreated: String,
    },
    /// The scenario digest is empty, so a trace carries no provenance.
    NoScenario {
        /// Which side: `original` or `recreated`.
        side: &'static str,
    },
}

impl TraceError {
    /// Stable lowercase identifier for logs and structured diagnostics.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::ScenarioMismatch { .. } => "scenario_mismatch",
            Self::NoScenario { .. } => "no_scenario",
        }
    }
}

impl fmt::Display for TraceError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ScenarioMismatch {
                original,
                recreated,
            } => write!(
                f,
                "the two traces come from different measured scenarios ({original} and {recreated})"
            ),
            Self::NoScenario { side } => write!(f, "the {side} trace carries no scenario"),
        }
    }
}

impl std::error::Error for TraceError {}

/// One difference between the two normalized traces.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TraceDivergence {
    /// The declared step at this index has no counterpart in the recreated
    /// trace at the same index: the engine did not emit it there.
    Missing {
        /// The index in the original trace.
        index: usize,
        /// What the original declares.
        declared: TraceStepKind,
    },
    /// The recreated trace has a step where the original declares none.
    Unexpected {
        /// The index in the recreated trace.
        index: usize,
        /// What the engine emitted.
        emitted: TraceStepKind,
    },
    /// Both traces have a step at this index but they are not the same call.
    Mismatched {
        /// The index in both traces.
        index: usize,
        /// What the original declares.
        declared: TraceStepKind,
        /// What the engine emitted.
        emitted: TraceStepKind,
    },
}

impl fmt::Display for TraceDivergence {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Missing { index, declared } => {
                write!(
                    f,
                    "step {index}: original declares {declared}, engine emitted nothing"
                )
            }
            Self::Unexpected { index, emitted } => {
                write!(
                    f,
                    "step {index}: engine emitted {emitted}, original declares nothing"
                )
            }
            Self::Mismatched {
                index,
                declared,
                emitted,
            } => write!(
                f,
                "step {index}: original declares {declared}, engine emitted {emitted}"
            ),
        }
    }
}

/// The result of one differential comparison.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TraceComparison {
    /// Steps both traces agreed on, in order.
    pub matched: usize,
    /// The first difference, or `None` when the two orderings agree.
    pub first_divergence: Option<TraceDivergence>,
    /// Steps in the original trace.
    pub original_len: usize,
    /// Steps in the recreated trace.
    pub recreated_len: usize,
}

impl TraceComparison {
    /// Whether the two traces describe the same host-call ordering.
    pub fn agrees(&self) -> bool {
        self.first_divergence.is_none()
    }

    /// Every difference, not only the first. A comparison that reports one
    /// difference is a report, not a count.
    ///
    /// Takes the two traces again because a [`TraceComparison`] deliberately
    /// holds only the first divergence and the two lengths: the full report is
    /// derived, never stored, so it cannot disagree with the traces it came
    /// from.
    pub fn divergences(
        &self,
        original: &NormalizedTrace,
        recreated: &NormalizedTrace,
    ) -> Vec<TraceDivergence> {
        let mut out = Vec::new();
        for index in 0..original.len().max(recreated.len()) {
            match (original.steps.get(index), recreated.steps.get(index)) {
                (Some(d), Some(e)) if d.kind() == e.kind() => {}
                (Some(d), Some(e)) => out.push(TraceDivergence::Mismatched {
                    index,
                    declared: d.kind(),
                    emitted: e.kind(),
                }),
                (Some(d), None) => out.push(TraceDivergence::Missing {
                    index,
                    declared: d.kind(),
                }),
                (None, Some(e)) => out.push(TraceDivergence::Unexpected {
                    index,
                    emitted: e.kind(),
                }),
                (None, None) => break,
            }
        }
        out
    }
}

/// A measured scenario: the program both traces were built from, named by a
/// digest of the measurement that produced it.
///
/// Two traces may only be compared when they name the same scenario. Without
/// this, a "differential" could compare two unrelated programs and report
/// agreement.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScenarioRef {
    digest: String,
}

impl ScenarioRef {
    /// Names a scenario by its measurement digest.
    ///
    /// # Errors
    ///
    /// A `TraceError` is not used here; an empty digest is refused by the
    /// comparison, so this constructor takes any non-empty string and
    /// [`ScenarioRef::from_digest`] is the checked form.
    pub fn new(digest: impl Into<String>) -> Self {
        Self {
            digest: digest.into(),
        }
    }

    /// Names a scenario by a measurement digest, refusing an empty one.
    ///
    /// # Errors
    ///
    /// `TraceError::NoScenario` for `original`/`recreated` naming an empty
    /// digest, so a trace cannot claim a provenance it does not have.
    pub fn from_digest(side: &'static str, digest: impl Into<String>) -> Result<Self, TraceError> {
        let digest = digest.into();
        if digest.trim().is_empty() {
            return Err(TraceError::NoScenario { side });
        }
        Ok(Self { digest })
    }

    /// The digest.
    pub fn digest(&self) -> &str {
        &self.digest
    }
}

/// Normalizes the declared host calls of a measured original program.
///
/// `scenario` names the measurement; the ordinals and offsets are preserved in
/// the steps but never compared, so a different build's numbering cannot create
/// or hide a difference.
pub fn normalize_declared(
    scenario: &ScenarioRef,
    declared: &[DeclaredStep],
) -> Result<NormalizedTrace, TraceError> {
    if scenario.digest().trim().is_empty() {
        return Err(TraceError::NoScenario { side: "original" });
    }
    Ok(NormalizedTrace {
        steps: declared
            .iter()
            .map(|d| NormalizedStep {
                kind: TraceStepKind::Measured {
                    form: d.form,
                    native_id: d.native_id,
                },
                ordinal: d.ordinal,
                source: None,
                sequence: None,
            })
            .collect(),
    })
}

/// Where an emitted event's measured origin is: the dispatch form, the dispatch
/// value, and the declared step's ordinal in the original program.
///
/// A consumer that lowered the program out of the measurement already knows
/// this mapping; it is passed in rather than recomputed here, because this
/// module does not hold the lowering.
pub type EventOrigin<'a> = dyn Fn(&MissionEvent) -> Option<(MeasuredForm, i64, u32)> + 'a;

/// Normalizes the mission events the recreated engine emitted for that same
/// scenario.
///
/// Each event is attributed to the measured dispatch value its action came
/// from through `origin`: a `(source, sequence)` pair naming the declared
/// step. An event whose pair names no declared step is **not** dropped — it is
/// reported as an unexpected step by the comparison, so an engine that emits
/// something the original never declared cannot hide.
pub fn normalize_emitted(
    scenario: &ScenarioRef,
    events: &[MissionEvent],
    origin: &EventOrigin<'_>,
) -> Result<NormalizedTrace, TraceError> {
    if scenario.digest().trim().is_empty() {
        return Err(TraceError::NoScenario { side: "recreated" });
    }
    // The runtime emits events sorted by `EventKey`, so the normalized trace is
    // in observation order — the order AC02 compares.
    let mut steps = Vec::with_capacity(events.len());
    for event in events {
        match origin(event) {
            Some((form, native_id, ordinal)) => steps.push(NormalizedStep {
                kind: TraceStepKind::Measured { form, native_id },
                ordinal,
                source: Some(event.key.source.0),
                sequence: Some(event.key.sequence),
            }),
            None => steps.push(NormalizedStep {
                kind: TraceStepKind::Unattributed,
                ordinal: u32::MAX,
                source: Some(event.key.source.0),
                sequence: Some(event.key.sequence),
            }),
        }
    }
    Ok(NormalizedTrace { steps })
}

/// Compares two normalized traces of the same measured scenario.
///
/// # Errors
///
/// `TraceError` when the two scenarios differ or either is unnamed. A trace is
/// never compared against a trace of a different program: the check is
/// structural, not a note in the report.
pub fn compare_traces(
    original: &NormalizedTrace,
    recreated: &NormalizedTrace,
    original_scenario: &ScenarioRef,
    recreated_scenario: &ScenarioRef,
) -> Result<TraceComparison, TraceError> {
    if original_scenario.digest().trim().is_empty() {
        return Err(TraceError::NoScenario { side: "original" });
    }
    if recreated_scenario.digest().trim().is_empty() {
        return Err(TraceError::NoScenario { side: "recreated" });
    }
    if original_scenario != recreated_scenario {
        return Err(TraceError::ScenarioMismatch {
            original: original_scenario.digest().to_owned(),
            recreated: recreated_scenario.digest().to_owned(),
        });
    }
    let shared = original.len().min(recreated.len());
    let mut matched = 0usize;
    let mut first_divergence = None;
    for index in 0..shared {
        if original.steps[index].kind() == recreated.steps[index].kind() {
            matched += 1;
        } else {
            first_divergence = Some(TraceDivergence::Mismatched {
                index,
                declared: original.steps[index].kind(),
                emitted: recreated.steps[index].kind(),
            });
            break;
        }
    }
    if first_divergence.is_none() {
        match (original.len(), recreated.len()) {
            (a, b) if a > b => {
                first_divergence = Some(TraceDivergence::Missing {
                    index: b,
                    declared: original.steps[b].kind(),
                });
            }
            (a, b) if b > a => {
                first_divergence = Some(TraceDivergence::Unexpected {
                    index: a,
                    emitted: recreated.steps[a].kind(),
                });
            }
            _ => {}
        }
    }
    Ok(TraceComparison {
        matched,
        first_divergence,
        original_len: original.len(),
        recreated_len: recreated.len(),
    })
}

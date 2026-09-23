//! The fixed-tick headless synthetic smoke run (F00-C, acceptance case AC03).
//!
//! [`run_synthetic`] is the consumer of the asset-free
//! [`SyntheticScene`](crate::synthetic::SyntheticScene): it builds a fresh
//! world from the typed fixture, advances it exactly `request.ticks` times
//! and optionally records the sample of every tick — 0 through the requested
//! final one — to a JSON Lines trace. Running it twice is part of the
//! contract — each call constructs a new world, so a second
//! run cannot inherit the first run's tick counter or body state. Teardown is
//! explicit: the trace is flushed and synced before success is reported, and
//! every IO or scene error propagates with the exit code the CLI evidence
//! contract assigns to it (never a logged failure with exit 0).

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};

use cs_types::{BodyKind, BodySample, SceneProvenance, SyntheticBodySpec, Tick};

use crate::cli::{EXIT_INVALID_INPUT, EXIT_RUNTIME_FAILURE};
use crate::synthetic::{SyntheticScene, SyntheticSceneError, TICK_HZ};

/// Typed input of one headless synthetic run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntheticRequest {
    /// Simulation ticks the run must reach; 0 builds the world and stops.
    pub ticks: u64,
    /// JSON Lines trace to write; the run fails if it cannot be opened,
    /// written or flushed.
    pub trace: Option<PathBuf>,
}

/// Typed output of a completed run, read back from the world itself.
#[derive(Clone, Debug, PartialEq)]
pub struct SyntheticRunReport {
    /// The tick count the request asked for.
    pub requested_ticks: u64,
    /// Ticks actually advanced, counted while stepping the world.
    pub ticks_completed: u64,
    /// Provenance read back out of the world's marker.
    pub provenance: SceneProvenance,
    /// Last sample read from the synthetic body, whose [`BodySample::tick`]
    /// is the world's own counter.
    pub final_sample: BodySample,
    /// The trace file written, when one was requested.
    pub trace: Option<PathBuf>,
}

/// Why a run could not complete.
#[derive(Debug)]
pub enum RunError {
    /// The typed fixture was rejected before a world existed.
    Scene(SyntheticSceneError),
    /// The trace file could not be created, written, flushed or synced.
    Trace { path: PathBuf, source: io::Error },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "cannot build the synthetic scene: {error}"),
            Self::Trace { path, source } => {
                write!(f, "cannot write trace {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            Self::Trace { source, .. } => Some(source),
        }
    }
}

impl RunError {
    /// Exit code for this failure per `docs/contracts/CLI-EVIDENCE.md`: a
    /// rejected request is invalid input (2), an IO failure while running is
    /// a runtime failure (1).
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Scene(_) => EXIT_INVALID_INPUT,
            Self::Trace { .. } => EXIT_RUNTIME_FAILURE,
        }
    }
}

/// Runs the asset-free `SYNTHETIC` scene for a fixed number of ticks.
///
/// The world is built first (so a scene that cannot exist writes no trace),
/// then the trace is opened and the loop advances one tick at a time so every
/// tick can be recorded. Errors are never swallowed: a trace that cannot be
/// flushed turns a completed simulation into a failed run.
pub fn run_synthetic(request: &SyntheticRequest) -> Result<SyntheticRunReport, RunError> {
    let mut scene = SyntheticScene::new(SyntheticBodySpec::falling_box(BodyKind::Dynamic))
        .map_err(RunError::Scene)?;

    let mut trace = match &request.trace {
        Some(path) => Some(TraceWriter::open(path)?),
        None => None,
    };
    if let Some(writer) = trace.as_mut() {
        writer.header(request.ticks)?;
        // The initial state is part of the record: the trace then holds one
        // sample for every tick from 0 through the requested final tick.
        writer.record(&scene.sample())?;
    }

    let mut ticks_completed = 0u64;
    while ticks_completed < request.ticks {
        scene.step(1);
        ticks_completed += 1;
        if let Some(writer) = trace.as_mut() {
            writer.record(&scene.sample())?;
        }
    }

    // Teardown: evidence must be on disk before the run may be called a
    // success, so a flush or sync failure propagates instead of exiting 0.
    if let Some(writer) = trace {
        writer.finish()?;
    }

    let final_sample = scene.sample();
    debug_assert_eq!(final_sample.tick, Tick(ticks_completed));
    Ok(SyntheticRunReport {
        requested_ticks: request.ticks,
        ticks_completed,
        provenance: scene.provenance(),
        final_sample,
        trace: request.trace.clone(),
    })
}

/// JSON Lines writer for one run: a header line plus one sample for every
/// tick from 0 through the requested final tick.
struct TraceWriter {
    path: PathBuf,
    inner: BufWriter<File>,
}

impl TraceWriter {
    fn open(path: &Path) -> Result<Self, RunError> {
        let file = File::create(path).map_err(|source| RunError::Trace {
            path: path.to_path_buf(),
            source,
        })?;
        Ok(Self {
            path: path.to_path_buf(),
            inner: BufWriter::new(file),
        })
    }

    fn write_line(&mut self, line: &str) -> Result<(), RunError> {
        self.inner
            .write_all(line.as_bytes())
            .and_then(|()| self.inner.write_all(b"\n"))
            .map_err(|source| RunError::Trace {
                path: self.path.clone(),
                source,
            })
    }

    /// Declares what the file records, including the `SYNTHETIC` provenance,
    /// so a trace can never be mistaken for retail evidence.
    fn header(&mut self, requested_ticks: u64) -> Result<(), RunError> {
        let line = format!(
            "{{\"kind\":\"synthetic-headless\",\"provenance\":\"{}\",\"tick_hz\":{},\"requested_ticks\":{}}}",
            SceneProvenance::Synthetic.label(),
            TICK_HZ,
            requested_ticks
        );
        self.write_line(&line)
    }

    fn record(&mut self, sample: &BodySample) -> Result<(), RunError> {
        let line = format!(
            "{{\"tick\":{},\"position_m\":{},\"linear_velocity_m_s\":{}}}",
            sample.tick.0,
            json_numbers(sample.position_m),
            json_numbers(sample.linear_velocity_m_s)
        );
        self.write_line(&line)
    }

    /// Flushes the buffer and syncs the file, so a completed run really has
    /// its trace on disk before success is reported.
    fn finish(self) -> Result<(), RunError> {
        let path = self.path.clone();
        let mut inner = self.inner;
        inner.flush().map_err(|source| RunError::Trace {
            path: path.clone(),
            source,
        })?;
        let file = inner.into_inner().map_err(|error| RunError::Trace {
            path: path.clone(),
            source: error.into_error(),
        })?;
        file.sync_all()
            .map_err(|source| RunError::Trace { path, source })
    }
}

/// JSON array of three components. A non-finite value becomes `null` rather
/// than `NaN`/`inf`, which are not JSON numbers.
fn json_numbers(values: [f32; 3]) -> String {
    let mut out = String::from("[");
    for (index, value) in values.iter().enumerate() {
        if index > 0 {
            out.push(',');
        }
        if value.is_finite() {
            out.push_str(&value.to_string());
        } else {
            out.push_str("null");
        }
    }
    out.push(']');
    out
}

//! The fixed-tick headless synthetic runs (F00-C and F21-E).
//!
//! [`run_synthetic`] is the consumer of the asset-free
//! [`SyntheticScene`]: it builds a fresh
//! world from the typed fixture, advances it exactly `request.ticks` times
//! and optionally records the sample of every tick — 0 through the requested
//! final one — to a JSON Lines trace. Running it twice is part of the
//! contract — each call constructs a new world, so a second
//! run cannot inherit the first run's tick counter or body state. Teardown is
//! explicit: the trace is flushed and synced before success is reported, and
//! every IO or scene error propagates with the exit code the CLI evidence
//! contract assigns to it (never a logged failure with exit 0).
//!
//! [`run_capture`] is the same scene driven by F21-E's deterministic capture
//! flags: the world advances to the capture's tick, the installed
//! [`CaptureRequest`](crate::camera::CaptureRequest) applies to the one
//! [`CameraSession`](crate::camera::CameraSession) frame produced there, and
//! the typed output is the [`CaptureReport`](crate::camera::CaptureReport)
//! listing every override the request made — the report the CLI prints.

use std::fmt;
use std::fs::File;
use std::io::{self, BufWriter, Write};
use std::path::{Path, PathBuf};
use std::time::Duration;

use cs_content::cameras::AspectRatio;
use cs_sim::damage::ActorId;
use cs_types::net::SessionId;
use cs_types::space::{Quaternion, WorldPosition};
use cs_types::{BodyKind, BodySample, SceneProvenance, SyntheticBodySpec, Tick};

use crate::camera::{
    BodyPose, CameraLowerError, CameraPose, CameraRig, CameraSession, CaptureError, CaptureReport,
    CaptureRequest, RigError, SessionError, SessionFrameInputs, SmoothingError, lower_camera_modes,
};
use crate::cli::{EXIT_INVALID_INPUT, EXIT_RUNTIME_FAILURE};
use crate::origin::OriginChange;
use crate::synthetic::{SyntheticScene, SyntheticSceneError, TICK_HZ};

/// Typed input of one headless synthetic run.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SyntheticRequest {
    /// Simulation ticks the run must reach; 0 builds the world and stops.
    pub ticks: u64,
    /// JSON Lines trace to write; the run fails if it cannot be opened,
    /// written or flushed.
    pub trace: Option<PathBuf>,
    /// Root seed of the run (`--seed`), per
    /// `docs/contracts/CLI-EVIDENCE.md`. `None` runs the canonical
    /// `SyntheticBodySpec::falling_box` fixture unchanged; `Some(root)` hands
    /// the root seed to the fixture, which varies only its lateral position
    /// and lateral velocity.
    pub seed: Option<u64>,
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

/// Typed input of one headless deterministic capture run (F21-E).
///
/// `--synthetic --headless --mission <id> --tick <n> --pose <x,y,z>`: the
/// scene is the same `SyntheticBodySpec` fixture as the plain run, stepped
/// to the tick [`CaptureRequest::tick`] names; the capture applies to the
/// one session frame produced there.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRunRequest {
    /// The capture the flags built: mission, tick and every pinned override.
    pub capture: CaptureRequest,
    /// Root seed of the run (`--seed`), the same rule as the plain
    /// synthetic run: it varies only the body's lateral pose, never the
    /// request's pinned pose.
    pub seed: Option<u64>,
    /// JSON Lines trace of the run up to the captured tick (`--trace`).
    pub trace: Option<PathBuf>,
}

/// Typed output of a completed capture run: the applied report plus the run
/// bookkeeping of the scene it was taken in.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureRunReport {
    /// What the session reports for the applied request: the mission, the
    /// tick, the frame's view, pose and aspect, every override and the
    /// pinned projection with its `f64 -> f32` narrowings.
    pub capture: CaptureReport,
    /// Ticks actually advanced to reach the capture tick, counted while
    /// stepping the world.
    pub ticks_completed: u64,
    /// Provenance read back out of the world's marker.
    pub provenance: SceneProvenance,
    /// The synthetic body's sample at the capture tick.
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
    /// The declared synthetic camera modes could not be lowered. The
    /// fixture is designed-valid, so this is a broken-invariant failure
    /// rather than bad input.
    CameraModes(CameraLowerError),
    /// The camera rig over those modes could not be built.
    CameraRig(RigError),
    /// The camera session over that rig could not be built.
    CameraSmoothing(SmoothingError),
    /// The capture request was refused when it was installed into the
    /// session: invalid input, naming the request's problem.
    Capture(CaptureError),
    /// The frame at the capture's tick could not be produced.
    Frame(SessionError),
    /// The installed capture produced no report at its own tick — the
    /// session's own invariant, so a runtime failure and never bad input.
    CaptureMissed {
        /// The tick the request asked for.
        requested: Tick,
    },
}

impl fmt::Display for RunError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Scene(error) => write!(f, "cannot build the synthetic scene: {error}"),
            Self::Trace { path, source } => {
                write!(f, "cannot write trace {}: {source}", path.display())
            }
            Self::CameraModes(error) => {
                write!(f, "cannot lower the synthetic camera modes: {error}")
            }
            Self::CameraRig(error) => {
                write!(f, "cannot build the capture camera rig: {error}")
            }
            Self::CameraSmoothing(error) => {
                write!(f, "cannot build the capture camera session: {error}")
            }
            Self::Capture(error) => {
                write!(f, "the capture request was refused: {error}")
            }
            Self::Frame(error) => {
                write!(f, "the frame at the capture's tick was refused: {error}")
            }
            Self::CaptureMissed { requested } => write!(
                f,
                "the capture installed for tick {} produced no report at that tick",
                requested.0
            ),
        }
    }
}

impl std::error::Error for RunError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Scene(error) => Some(error),
            Self::Trace { source, .. } => Some(source),
            Self::CameraModes(error) => Some(error),
            Self::CameraRig(error) => Some(error),
            Self::CameraSmoothing(error) => Some(error),
            Self::Capture(error) => Some(error),
            Self::Frame(error) => Some(error),
            Self::CaptureMissed { .. } => None,
        }
    }
}

impl RunError {
    /// Exit code for this failure per `docs/contracts/CLI-EVIDENCE.md`: a
    /// rejected request is invalid input (2), an IO failure while running is
    /// a runtime failure (1).
    pub const fn exit_code(&self) -> u8 {
        match self {
            Self::Scene(_) | Self::Capture(_) => EXIT_INVALID_INPUT,
            Self::Trace { .. }
            | Self::CameraModes(_)
            | Self::CameraRig(_)
            | Self::CameraSmoothing(_)
            | Self::Frame(_)
            | Self::CaptureMissed { .. } => EXIT_RUNTIME_FAILURE,
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
    // `None` is the canonical fixture and must stay byte-identical to the
    // unseeded run; `Some(root)` only ever moves the body sideways.
    let spec = match request.seed {
        Some(root_seed) => SyntheticBodySpec::falling_box(BodyKind::Dynamic).seeded(root_seed),
        None => SyntheticBodySpec::falling_box(BodyKind::Dynamic),
    };
    let mut scene = SyntheticScene::new(spec).map_err(RunError::Scene)?;

    let mut trace = match &request.trace {
        Some(path) => Some(TraceWriter::open(path)?),
        None => None,
    };
    if let Some(writer) = trace.as_mut() {
        writer.header(request.ticks, request.seed)?;
        // The initial state is part of the record: the trace then holds one
        // sample for every tick from 0 through the requested final tick.
        writer.record(&scene.sample())?;
    }

    let ticks_completed = step_and_trace(&mut scene, request.ticks, &mut trace)?;

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

/// Runs the asset-free `SYNTHETIC` scene to a capture's tick and applies the
/// request to the frame produced there (F21-E, non-negotiable behavior 5).
///
/// The scene, the seed and the trace are the plain synthetic run's own; what
/// the flags add is the camera half: a [`CameraSession`] over the same
/// declared synthetic camera modes the playtest's view runs, the request
/// installed before the first frame, and exactly one frame produced at the
/// capture's tick — a capture is one frame and then it is gone
/// ([`CameraSession::apply_capture`]). The body's rotation is the identity:
/// the fixture integrates a falling box with no angular velocity, and the
/// pinned pose overrides whatever the rig derives anyway.
///
/// Nothing is drawn and no image is produced: the typed report is the whole
/// output of the run.
pub fn run_capture(request: &CaptureRunRequest) -> Result<CaptureRunReport, RunError> {
    let spec = match request.seed {
        Some(root_seed) => SyntheticBodySpec::falling_box(BodyKind::Dynamic).seeded(root_seed),
        None => SyntheticBodySpec::falling_box(BodyKind::Dynamic),
    };
    let mut scene = SyntheticScene::new(spec).map_err(RunError::Scene)?;

    // The camera session over the declared synthetic fixture — the mode set
    // every --view value the parser accepts resolves against, so an
    // install-time refusal is a defect in the fixture, not in the flags.
    let modes = lower_camera_modes(&cs_content::cameras::declared_synthetic_camera_modes())
        .map_err(RunError::CameraModes)?;
    let rig = CameraRig::new(modes).map_err(RunError::CameraRig)?;
    let mut session = CameraSession::new(rig).map_err(RunError::CameraSmoothing)?;
    session
        .apply_capture(request.capture.clone())
        .map_err(RunError::Capture)?;

    let mut trace = match &request.trace {
        Some(path) => Some(TraceWriter::open(path)?),
        None => None,
    };
    if let Some(writer) = trace.as_mut() {
        writer.header(request.capture.tick().0, request.seed)?;
        writer.record(&scene.sample())?;
    }

    let ticks_completed = step_and_trace(&mut scene, request.capture.tick().0, &mut trace)?;

    // The one frame the capture is of. The headless viewport frames at the
    // modes' declared reference aspect unless the request pinned one, the
    // body appears with no continuous prior pose (Teleport), and the elapsed
    // slice is one tick — a first frame reseats whatever it is given.
    let player = ActorId {
        session: SessionId::new(1).expect("session generation 1 is valid"),
        serial: 1,
    };
    let sample = scene.sample();
    let body = CameraPose::new(
        WorldPosition::try_new(sample.position_m.map(f64::from))
            .expect("an Avian position is finite"),
        Quaternion::IDENTITY,
    );
    let bodies = [BodyPose::new(player, body)];
    let frame = session
        .frame(&SessionFrameInputs {
            at: sample.tick,
            player,
            bodies: &bodies,
            aspect: AspectRatio::FOUR_THREE,
            elapsed: Duration::from_secs_f64(1.0 / TICK_HZ),
            look: None,
            spyglass: None,
            origin_change: OriginChange::Teleport,
        })
        .map_err(RunError::Frame)?;

    if let Some(writer) = trace {
        writer.finish()?;
    }

    let Some(capture) = frame.capture else {
        return Err(RunError::CaptureMissed {
            requested: request.capture.tick(),
        });
    };
    Ok(CaptureRunReport {
        capture,
        ticks_completed,
        provenance: scene.provenance(),
        final_sample: sample,
        trace: request.trace.clone(),
    })
}

/// Advances the scene to `ticks`, recording each tick's sample like the
/// plain run's loop, and returns how many ticks it completed.
fn step_and_trace(
    scene: &mut SyntheticScene,
    ticks: u64,
    trace: &mut Option<TraceWriter>,
) -> Result<u64, RunError> {
    let mut ticks_completed = 0u64;
    while ticks_completed < ticks {
        scene.step(1);
        ticks_completed += 1;
        if let Some(writer) = trace.as_mut() {
            writer.record(&scene.sample())?;
        }
    }
    Ok(ticks_completed)
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

    /// Declares what the file records, including the `SYNTHETIC` provenance
    /// and the root seed that produced it, so a trace can never be mistaken
    /// for retail evidence and always states which seed — if any — it came
    /// from.
    fn header(&mut self, requested_ticks: u64, seed: Option<u64>) -> Result<(), RunError> {
        let seed = match seed {
            Some(root_seed) => root_seed.to_string(),
            None => "null".to_string(),
        };
        let line = format!(
            "{{\"kind\":\"synthetic-headless\",\"provenance\":\"{}\",\"tick_hz\":{},\"requested_ticks\":{},\"seed\":{}}}",
            SceneProvenance::Synthetic.label(),
            TICK_HZ,
            requested_ticks,
            seed
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

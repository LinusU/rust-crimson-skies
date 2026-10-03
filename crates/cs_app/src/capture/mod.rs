//! Deterministic input and state capture (F59-B).
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`. Shared contract: `docs/contracts/CLI-EVIDENCE.md`.
//!
//! This is the **runtime half** of F59. F59-A built the engine-free records in
//! `cs_content::replay` and said, in its own module documentation, that the
//! per-tick state hashes are produced by a running session and that the
//! commands that drive them are F59-C's. This module is the session-side code in
//! between:
//!
//! * [`identity`] computes the three digests a replay record's build
//!   fingerprint is built from — engine, **content** and rules — from what a run
//!   really loaded. It is the reason a content-asset edit moves the content
//!   digest and therefore the compatibility signature, and it is where F59
//!   non-negotiable 1 gets something to measure.
//! * [`state`] is the only producer of per-tick state hashes in the runtime.
//!   A reading is a pose read back out of Avian plus the forces the tick's own
//!   law computed; nothing here derives a hash from the input frame, so a state
//!   digest that agrees with a promise is evidence rather than a tautology.
//! * [`replay`] drives the production flight world from a recorded
//!   [`CommandStream`](cs_types::input::CommandStream), measures every tick and
//!   either records the run ([`replay::record`]) or replays a record and reports
//!   AC01's envelope comparison beside AC02's compatibility verdict
//!   ([`replay::replay`]).
//! * [`render`] is the single boundary between a capture record's exact
//!   `u32`-thousandths [`RenderConfig`](cs_content::replay::RenderConfig) and the
//!   renderer's `f32` [`ComparisonSettings`], so a record's pinned settings and
//!   the settings a frame was actually rendered under cannot drift apart.
//!
//! # What this stage does not do
//!
//! Nothing here is wired to a command line, and nothing writes a file. The
//! `--input-replay`, `--cam` and `--screenshot` invocations of
//! `docs/contracts/CLI-EVIDENCE.md`, the `cs-inspect`/`cs_xtask` evidence
//! commands and the freshness/capability validation against a real build are
//! F59-C and F59-D. No GPU render, no screenshot writer and no audio capture
//! exists here either: a capture's *bytes* are produced by the render path,
//! which is a different capability from measuring a state hash.
//!
//! # What is measured and what is designed
//!
//! Every state hash, every envelope comparison and every compatibility verdict
//! in this module is measured from a running production world. The *digests of
//! declared records* — the airframe coefficients, the handling profile, the
//! loaded content rows — are project design: the airframe they digest is the
//! synthetic fixture (`cs_sim::flight::synthetic_fixed_wing`), carries
//! `Origin::SyntheticFixture`, and no value in this module is derived from the
//! original game or can certify anything about it. The engine's tick rate, the
//! original's own replay format (if it had one) and its throttle axis mapping
//! are unmeasured and are not claimed; see `docs/findings/` for what a later
//! stage still has to resolve.

pub mod identity;
pub mod render;
pub mod replay;
pub mod state;

pub use identity::{
    AIRFRAME_CONTENT_KEY, LoadedContent, RunIdentity, STATE_FORMAT_VERSION,
    airframe_content_digest, engine_digest, host_platform, rules_digest,
};
pub use render::{render_for, settings_for, tonemap_for, tonemap_label};
pub use replay::{
    BuildContext, CaptureRunError, INITIAL_STATE_LABEL_SUFFIX, RecordedRun, ReplayOutcome,
    ReplaySubject, RunRequest, record, record_run, replay,
};
pub use state::{STATE_DIGEST_DOMAIN, StateProbe, StateProbeError, StateReading};

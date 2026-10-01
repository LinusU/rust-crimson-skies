//! Capital-ship subsystems, propulsion, bays, launch sockets and capture
//! contracts (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Stage **F35-A** defines the typed contract and a minimal synthetic
//! fixture — not the movement/weakpoint/turret runtime (F35-B), the
//! launch/capture/cargo/staged-destruction wiring (F35-C) or the original
//! mission validation (F35-D). The module is split so each part is one
//! owner:
//!
//! * [`subsystem`] is the shared identity and state vocabulary:
//!   [`subsystem::SubsystemKey`], [`subsystem::SubsystemKind`],
//!   [`subsystem::SubsystemEffect`] and the [`subsystem::SubsystemGraph`]
//!   whose [`subsystem::SubsystemGraph::disable`] is the one transition
//!   that applies a destroyed part's behavior.
//! * [`motion`] is the engine record and the thrust arithmetic a destroyed
//!   engine removes (the F35-A minimum scenario).
//! * [`bay`] is the explicit time-varying weakpoint state:
//!   [`bay::ExposureWindow`] and [`bay::BayState`], where a closed bay is
//!   never a hittable weakpoint.
//! * [`launch`] is [`launch::LaunchSocket`] and the once-only
//!   [`launch::LaunchLedger`] that cancels a destroyed bay's pending
//!   launches instead of spawning duplicates.
//! * [`capture`] is the staged [`capture::CaptureTransaction`] ownership
//!   transaction and its [`capture::Ownership`] result.
//! * [`ship`] is [`ship::CapitalShip`], the aggregate the acceptance tests
//!   drive, and [`synthetic`] is the designed fixture.
//!
//! The declared, provenance-carrying schema that records where each ship
//! and value came from is `cs_content::capital`; the lowering boundary is
//! `cs_app::capital`. The original capital-ship model — subsystem set,
//! engine coefficients, bay timings, capture rules — is unrecovered, so
//! everything here is designed behavior and nothing is an original-fidelity
//! claim: see
//! `docs/findings/2026-10-01-f35-a-capital-subsystems-and-bays.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.

pub mod bay;
pub mod capture;
pub mod launch;
pub mod motion;
pub mod parts;
pub mod ship;
pub mod subsystem;
pub mod synthetic;

pub use bay::{Bay, BayKind, BayState, ExposureError, ExposureWindow};
pub use capture::{CaptureRefusal, CaptureStage, CaptureTransaction, Ownership};
pub use launch::{
    LaunchId, LaunchLedger, LaunchRefusal, LaunchSocket, LaunchTick, PendingLaunch,
    ReleasedAircraft, release_aircraft,
};
pub use motion::{EngineSpec, PropulsionError, acceleration_m_s2, validated_axis};
pub use parts::{DockingAnchor, TurretMount};
pub use ship::{CapitalError, CapitalParts, CapitalShip, synthetic_capital_trajectory};
pub use subsystem::{
    DisableOutcome, MAX_SUBSYSTEM_KEY_LEN, Subsystem, SubsystemEffect, SubsystemGraph,
    SubsystemGraphError, SubsystemKey, SubsystemKeyError, SubsystemKind, SubsystemState,
};
pub use synthetic::{
    SYNTHETIC_CAPITAL_KEY, SYNTHETIC_DOCKING_ANCHOR, SYNTHETIC_ENGINE_1, SYNTHETIC_ENGINE_2,
    SYNTHETIC_ENGINE_THRUST_N, SYNTHETIC_GAS_CELL, SYNTHETIC_KEEL, SYNTHETIC_LAUNCH_BAY,
    SYNTHETIC_MASS_KG, SYNTHETIC_TURRET, SYNTHETIC_WEAPON_BAY, synthetic_capital_bays,
    synthetic_capital_docking_anchors, synthetic_capital_engines, synthetic_capital_graph,
    synthetic_capital_ownership, synthetic_capital_ship, synthetic_capital_turrets,
    synthetic_launch_socket,
};

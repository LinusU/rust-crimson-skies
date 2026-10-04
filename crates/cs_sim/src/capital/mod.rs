//! Capital-ship subsystems, propulsion, bays, launch sockets and capture
//! contracts (F35-A), the F35-B movement, weakpoint and turret runtime, and
//! the F35-C launch, capture, cargo and staged-destruction wiring.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stages `### F35-A`, `### F35-B` and `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Stage **F35-A** defines the typed contract and a minimal synthetic
//! fixture; stage **F35-B** adds the session runtime
//! ([`runtime::CapitalShipSet`]) that steps ships along their authored
//! courses at a rate set by surviving engine thrust, resolves subsystem
//! hits through the bay weakpoint windows and section integrity pools, and
//! gates turret aim and fire; stage **F35-C** makes that runtime *drive* the
//! transactions the first stage declared — releases each scheduled launch
//! exactly once, cancels what a destroyed bay or a dying carrier can no longer
//! run, commits a capture as one ownership switch, bounds cargo by the ship's
//! declared capacity and separates destruction from despawn. Still open is the
//! original mission validation (F35-D). The module is split so each part is one
//! owner:
//!
//! * [`subsystem`] is the shared identity and state vocabulary:
//!   [`subsystem::SubsystemKey`], [`subsystem::SubsystemKind`],
//!   [`subsystem::SubsystemEffect`] and the [`subsystem::SubsystemGraph`]
//!   whose [`subsystem::SubsystemGraph::disable`] is the one transition
//!   that applies a destroyed part's behavior.
//! * [`motion`] is the engine record and the thrust arithmetic a destroyed
//!   engine removes.
//! * [`bay`] is the explicit time-varying weakpoint state:
//!   [`bay::ExposureWindow`] and [`bay::BayState`], where a closed bay is
//!   never a hittable weakpoint, plus the [`parts::LaunchBayRig`] a launch
//!   bay carries (F35-C).
//! * [`launch`] is [`launch::LaunchSocket`], the once-only
//!   [`launch::LaunchLedger`] that cancels a destroyed bay's pending
//!   launches instead of spawning duplicates, and the targeted
//!   [`launch::LaunchLedger::release_one`] the F35-C retry door uses.
//! * [`capture`] is the staged [`capture::CaptureTransaction`] ownership
//!   transaction, its [`capture::Ownership`] result, the F35-C
//!   [`capture::ShipControl`] record every consumer of an owner reads, and
//!   the [`capture::CaptureTicket`] that qualifies an attempt.
//! * [`ship`] is [`ship::CapitalShip`], the aggregate the acceptance tests
//!   drive: F35-B adds [`ship::CapitalShip::apply_hit`] (the weakpoint and
//!   section-pool resolver), [`ship::CapitalShip::propulsion_fraction`] and
//!   the [`ship::CapitalShip::aim_turret`] / [`ship::CapitalShip::may_fire`]
//!   turret gating; F35-C adds [`ship::CapitalShip::adopt_ownership`],
//!   [`ship::CapitalShip::launch_rig`] and [`ship::CapitalShip::docking_open`].
//! * [`runtime`] is [`runtime::CapitalShipSet`], the per-session registry
//!   whose integer ticks move ships under their own power, refuse foreign-tick
//!   hits, freeze destroyed ships as zero-velocity wrecks and run the F35-C
//!   launch/despawn pass.
//! * [`wiring`] is the state that pass drives: [`wiring::ShipWiring`]'s
//!   launch ledger, [`wiring::CargoLedger`], the in-flight capture and the
//!   [`wiring::DestructionState`] staged-destruction phase.
//! * [`synthetic`] is the designed fixture.
//!
//! The declared, provenance-carrying schema that records where each ship
//! and value came from is `cs_content::capital`; the lowering boundary is
//! `cs_app::capital`. The original capital-ship model — subsystem set,
//! engine coefficients, bay timings, capture rules — is unrecovered, so
//! everything here is designed behavior and nothing is an original-fidelity
//! claim: see
//! `docs/findings/2026-10-01-f35-a-capital-subsystems-and-bays.md`,
//! `docs/findings/2026-10-04-f35-b-movement-weakpoints-and-turrets.md` and
//! `docs/findings/2026-10-04-f35-c-launch-capture-cargo-destruction.md`.
//!
//! `cs_sim` may depend only on [`cs_types`] and [`cs_script`]
//! (`docs/01-ARCHITECTURE.md`): no Bevy, no renderer, no file access.

pub mod bay;
pub mod capture;
pub mod launch;
pub mod motion;
pub mod parts;
pub mod runtime;
pub mod ship;
pub mod subsystem;
pub mod synthetic;
pub mod wiring;

pub use bay::{Bay, BayKind, BayState, ExposureError, ExposureWindow};
pub use capture::{
    CaptureRefusal, CaptureStage, CaptureTicket, CaptureTransaction, Ownership, ShipControl,
};
pub use launch::{
    LaunchId, LaunchLedger, LaunchRefusal, LaunchSocket, LaunchTick, PendingLaunch,
    ReleasedAircraft, release_aircraft,
};
pub use motion::{EngineSpec, PropulsionError, acceleration_m_s2, validated_axis};
pub use parts::{DockingAnchor, IntegrityPool, LaunchBayRig, TurretMount};
pub use runtime::{CapitalHit, CapitalRuntimeError, CapitalShipEvent, CapitalShipSet};
pub use ship::{
    CapitalError, CapitalParts, CapitalShip, HitError, HitOutcome, TurretAim, TurretRefusal,
    synthetic_capital_trajectory,
};
pub use subsystem::{
    DisableOutcome, MAX_SUBSYSTEM_KEY_LEN, Subsystem, SubsystemEffect, SubsystemGraph,
    SubsystemGraphError, SubsystemKey, SubsystemKeyError, SubsystemKind, SubsystemState,
};
pub use synthetic::{
    SYNTHETIC_CAPITAL_KEY, SYNTHETIC_DOCKING_ANCHOR, SYNTHETIC_ENGINE_1, SYNTHETIC_ENGINE_2,
    SYNTHETIC_ENGINE_THRUST_N, SYNTHETIC_GAS_CELL, SYNTHETIC_GAS_CELL_INTEGRITY, SYNTHETIC_KEEL,
    SYNTHETIC_KEEL_INTEGRITY, SYNTHETIC_LAUNCH_BAY, SYNTHETIC_LAUNCH_BAY_CAPACITY,
    SYNTHETIC_LAUNCH_BAY_SOCKET_M, SYNTHETIC_LAUNCH_EJECT_M_S, SYNTHETIC_MASS_KG, SYNTHETIC_TURRET,
    SYNTHETIC_TURRET_BORESIGHT, SYNTHETIC_TURRET_TRAVERSE_DEG, SYNTHETIC_WEAPON_BAY,
    synthetic_capital_bays, synthetic_capital_docking_anchors, synthetic_capital_engines,
    synthetic_capital_graph, synthetic_capital_ownership, synthetic_capital_sections,
    synthetic_capital_ship, synthetic_capital_turrets, synthetic_launch_socket,
};
pub use wiring::{
    CaptureProgress, CargoLedger, CargoRefusal, DespawnPolicy, DestructionState,
    LaunchCancelReason, LaunchCancellation, LaunchStatus, ShipWiring,
};

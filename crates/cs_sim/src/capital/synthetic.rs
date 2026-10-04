//! The minimal synthetic capital-ship fixture (F35-A).
//!
//! One designed capital ship the acceptance tests drive: two engines, a
//! weapon bay, a launch bay, a docking anchor, a lifting-gas cell and a
//! keel. Every value is newly authored fixture content with designed
//! provenance — `Origin::SyntheticFixture` on the declared record — and can
//! never stand in for missing retail data.

use cs_script::ir::ActorId;
use cs_types::content::{ContentId, ContentKind, Known, Provenance, Resolved};
use cs_types::evidence::ClaimId;

use super::bay::{Bay, BayKind, ExposureWindow};
use super::capture::Ownership;
use super::launch::LaunchSocket;
use super::motion::EngineSpec;
use super::parts::{DockingAnchor, IntegrityPool, LaunchBayRig, TurretMount};
use super::ship::{CapitalParts, CapitalShip, synthetic_capital_trajectory};
use super::subsystem::{Subsystem, SubsystemEffect, SubsystemGraph, SubsystemKey, SubsystemKind};

/// The catalog id of the synthetic capital ship.
pub const SYNTHETIC_CAPITAL_KEY: &str = "synthetic.leviathan";

/// The first engine subsystem.
pub const SYNTHETIC_ENGINE_1: &str = "engine_1";
/// The second engine subsystem.
pub const SYNTHETIC_ENGINE_2: &str = "engine_2";
/// The weapon bay subsystem.
pub const SYNTHETIC_WEAPON_BAY: &str = "weapon_bay_1";
/// The launch bay subsystem.
pub const SYNTHETIC_LAUNCH_BAY: &str = "launch_bay_1";
/// The docking anchor subsystem.
pub const SYNTHETIC_DOCKING_ANCHOR: &str = "docking_anchor_1";
/// The turret subsystem.
pub const SYNTHETIC_TURRET: &str = "turret_1";
/// The lifting-gas cell subsystem.
pub const SYNTHETIC_GAS_CELL: &str = "gas_cell_1";
/// The lethal keel subsystem.
pub const SYNTHETIC_KEEL: &str = "keel";

/// The rated thrust of one synthetic engine, in newtons.
pub const SYNTHETIC_ENGINE_THRUST_N: f64 = 400_000.0;
/// The synthetic ship's mass, in kilograms.
pub const SYNTHETIC_MASS_KG: f64 = 200_000.0;
/// The gas cell's declared integrity pool.
pub const SYNTHETIC_GAS_CELL_INTEGRITY: f64 = 120.0;
/// The keel's declared integrity pool.
pub const SYNTHETIC_KEEL_INTEGRITY: f64 = 200.0;
/// The turret mount's traverse arc, in degrees.
pub const SYNTHETIC_TURRET_TRAVERSE_DEG: f64 = 180.0;
/// The dorsal turret's boresight: up (+Y) in the ship body frame.
pub const SYNTHETIC_TURRET_BORESIGHT: [f64; 3] = [0.0, 1.0, 0.0];

/// The launch bay's declared release socket offset, in the ship body frame.
pub const SYNTHETIC_LAUNCH_BAY_SOCKET_M: [f64; 3] = [0.0, -5.0, 0.0];
/// The launch bay's declared aircraft capacity: four may wait aboard it.
pub const SYNTHETIC_LAUNCH_BAY_CAPACITY: u32 = 4;
/// The designed ejection a scheduled aircraft leaves the launch socket with.
pub const SYNTHETIC_LAUNCH_EJECT_M_S: [f64; 3] = [0.0, 0.0, 2.0];

fn key(name: &str) -> SubsystemKey {
    SubsystemKey::new(name).expect("fixture subsystem keys are valid")
}

fn designed(value: f64) -> Resolved<f64> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(
            ClaimId::new("f35a.synthetic-leviathan").expect("fixture claim id is valid"),
        ),
    ))
}

/// The synthetic ship's subsystem graph.
#[must_use]
pub fn synthetic_capital_graph() -> SubsystemGraph {
    SubsystemGraph::try_new(vec![
        Subsystem::new(key(SYNTHETIC_ENGINE_1), SubsystemKind::Engine)
            .with_effect(SubsystemEffect::Propulsion),
        Subsystem::new(key(SYNTHETIC_ENGINE_2), SubsystemKind::Engine)
            .with_effect(SubsystemEffect::Propulsion),
        Subsystem::new(key(SYNTHETIC_WEAPON_BAY), SubsystemKind::WeaponBay)
            .with_effect(SubsystemEffect::WeaponAccess),
        Subsystem::new(key(SYNTHETIC_LAUNCH_BAY), SubsystemKind::LaunchBay)
            .with_effect(SubsystemEffect::Launching),
        Subsystem::new(key(SYNTHETIC_DOCKING_ANCHOR), SubsystemKind::DockingAnchor)
            .with_effect(SubsystemEffect::Docking),
        Subsystem::new(key(SYNTHETIC_TURRET), SubsystemKind::Turret)
            .with_effect(SubsystemEffect::WeaponAccess),
        Subsystem::new(key(SYNTHETIC_GAS_CELL), SubsystemKind::GasCell)
            .with_effect(SubsystemEffect::Vulnerability)
            .with_lethal(true),
        Subsystem::new(key(SYNTHETIC_KEEL), SubsystemKind::StructuralSection)
            .with_effect(SubsystemEffect::MissionCondition)
            .with_lethal(true),
    ])
    .expect("the synthetic capital subsystem graph is valid")
}

/// The synthetic ship's engines, each thrusting along +X.
#[must_use]
pub fn synthetic_capital_engines() -> Vec<EngineSpec> {
    [SYNTHETIC_ENGINE_1, SYNTHETIC_ENGINE_2]
        .into_iter()
        .map(|name| {
            EngineSpec::try_new(
                key(name),
                designed(SYNTHETIC_ENGINE_THRUST_N),
                [1.0, 0.0, 0.0],
            )
            .expect("the synthetic engines are valid")
        })
        .collect()
}

/// The synthetic ship's bays, each with its authored exposure cycle. The
/// launch bay carries its F35-C release wiring: the designed socket it
/// releases at and a capacity of four waiting aircraft.
#[must_use]
pub fn synthetic_capital_bays() -> Vec<Bay> {
    vec![
        Bay::new(
            key(SYNTHETIC_WEAPON_BAY),
            BayKind::Weapon,
            ExposureWindow::try_new(40, 10, 60, 10).expect("the weapon bay window is valid"),
        ),
        Bay::new(
            key(SYNTHETIC_LAUNCH_BAY),
            BayKind::Launch,
            ExposureWindow::try_new(30, 5, 45, 5).expect("the launch bay window is valid"),
        )
        .with_launch_rig(LaunchBayRig {
            offset_m: designed_vec(SYNTHETIC_LAUNCH_BAY_SOCKET_M),
            capacity: Resolved::Known(Known::new(
                SYNTHETIC_LAUNCH_BAY_CAPACITY,
                Provenance::designed(
                    ClaimId::new("f35a.synthetic-leviathan").expect("fixture claim id is valid"),
                ),
            )),
        }),
    ]
}

/// The synthetic ship's initial ownership.
#[must_use]
pub fn synthetic_capital_ownership() -> Ownership {
    Ownership {
        owner: ContentId::from_source(ContentKind::Faction, "synthetic.raiders")
            .expect("fixture owner id is valid"),
        captured: false,
    }
}

fn designed_vec(value: [f64; 3]) -> Resolved<[f64; 3]> {
    Resolved::Known(Known::new(
        value,
        Provenance::designed(
            ClaimId::new("f35a.synthetic-leviathan").expect("fixture claim id is valid"),
        ),
    ))
}

/// The synthetic ship's turret mounts. The weapon binding is an explicit
/// unknown: no original turret record is measured. The dorsal mount bears
/// up in the body frame and sweeps a 180-degree cone — designed values.
#[must_use]
pub fn synthetic_capital_turrets() -> Vec<TurretMount> {
    vec![TurretMount {
        key: key(SYNTHETIC_TURRET),
        weapon: Resolved::Unknown {
            claim_id: ClaimId::new("f35a.turret-weapon-unmeasured")
                .expect("fixture claim id is valid"),
            reason: "turret weapon binding unmeasured".to_owned(),
        },
        traverse_deg: designed(SYNTHETIC_TURRET_TRAVERSE_DEG),
        boresight: SYNTHETIC_TURRET_BORESIGHT,
    }]
}

/// The synthetic ship's gas-cell and structural-section damage pools.
#[must_use]
pub fn synthetic_capital_sections() -> Vec<IntegrityPool> {
    vec![
        IntegrityPool {
            key: key(SYNTHETIC_GAS_CELL),
            integrity: designed(SYNTHETIC_GAS_CELL_INTEGRITY),
        },
        IntegrityPool {
            key: key(SYNTHETIC_KEEL),
            integrity: designed(SYNTHETIC_KEEL_INTEGRITY),
        },
    ]
}

/// The synthetic ship's docking anchors.
#[must_use]
pub fn synthetic_capital_docking_anchors() -> Vec<DockingAnchor> {
    vec![DockingAnchor {
        key: key(SYNTHETIC_DOCKING_ANCHOR),
        offset_m: designed_vec([0.0, 0.0, 20.0]),
    }]
}

/// The launch socket of the synthetic ship's launch bay.
#[must_use]
pub fn synthetic_launch_socket() -> LaunchSocket {
    LaunchSocket {
        actor: ActorId(1),
        socket: 0,
        offset_m: SYNTHETIC_LAUNCH_BAY_SOCKET_M,
    }
}

/// The whole synthetic capital ship the acceptance tests drive.
#[must_use]
pub fn synthetic_capital_ship() -> CapitalShip {
    CapitalShip::try_new(
        ContentId::from_source(ContentKind::Airframe, SYNTHETIC_CAPITAL_KEY)
            .expect("fixture subject id is valid"),
        ActorId(1),
        CapitalParts {
            graph: synthetic_capital_graph(),
            engines: synthetic_capital_engines(),
            bays: synthetic_capital_bays(),
            turrets: synthetic_capital_turrets(),
            docking_anchors: synthetic_capital_docking_anchors(),
            sections: synthetic_capital_sections(),
            cargo: designed(5_000.0),
            ownership: synthetic_capital_ownership(),
            trajectory: Some(synthetic_capital_trajectory()),
        },
    )
    .expect("the synthetic capital ship is valid")
}

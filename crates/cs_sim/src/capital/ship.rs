//! The capital-ship runtime aggregate: subsystems, propulsion, bays,
//! turrets, anchors, cargo, ownership and trajectory (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! [`CapitalShip`] ties the F35-A contracts to one actor and is the only
//! place the F35-A minimum scenario runs: a [`CapitalShip::disable`] of an
//! engine removes that engine's thrust (so the measured motion response
//! changes) while the hull stays intact, because only a lethal subsystem
//! destroys the actor. The full movement integration, weakpoint resolver,
//! turret behaviour and script wiring are F35-B/F35-C; this aggregate does
//! no tick and owns no Avian body.
//!
//! Runtime behavior is *designed*; no original coefficient is measured.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};

use super::bay::{Bay, BayState};
use super::capture::Ownership;
use super::motion::{EngineSpec, PropulsionError, acceleration_m_s2};
use super::parts::{DockingAnchor, TurretMount};
use super::subsystem::{
    DisableOutcome, SubsystemGraph, SubsystemGraphError, SubsystemKey, SubsystemKind,
    SubsystemState,
};
use crate::world_actors::trajectory::Trajectory;

/// Why a [`CapitalShip`] was rejected.
#[derive(Clone, Debug, PartialEq)]
pub enum CapitalError {
    /// Two parts of one kind share a key.
    DuplicatePart {
        /// The kind.
        kind: SubsystemKind,
        /// The duplicated key.
        key: SubsystemKey,
    },
    /// A part key names no subsystem of the graph.
    UnknownPart {
        /// The kind.
        kind: SubsystemKind,
        /// The dangling key.
        key: SubsystemKey,
    },
    /// A part key names a subsystem of the wrong kind.
    PartKindMismatch {
        /// The offending key.
        key: SubsystemKey,
        /// The kind the part record requires.
        expected: SubsystemKind,
        /// The kind the subsystem declares.
        actual: SubsystemKind,
    },
    /// The cargo capacity was not finite.
    NonFiniteCargo,
    /// The cargo capacity was negative.
    NegativeCargo {
        /// The refused value.
        value: f64,
    },
}

impl std::fmt::Display for CapitalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::DuplicatePart { kind, key } => {
                write!(f, "{kind} key {key:?} is used twice")
            }
            Self::UnknownPart { kind, key } => {
                write!(f, "{kind} {key} is not a subsystem of the ship")
            }
            Self::PartKindMismatch {
                key,
                expected,
                actual,
            } => write!(f, "part {key} is {actual}, expected {expected}"),
            Self::NonFiniteCargo => write!(f, "the cargo capacity is not finite"),
            Self::NegativeCargo { value } => {
                write!(f, "the cargo capacity {value} is negative")
            }
        }
    }
}

impl std::error::Error for CapitalError {}

/// Everything [`CapitalShip::try_new`] assembles into one ship.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalParts {
    /// The subsystem graph.
    pub graph: SubsystemGraph,
    /// The engines.
    pub engines: Vec<EngineSpec>,
    /// The weapon and launch bays.
    pub bays: Vec<Bay>,
    /// The turret mounts.
    pub turrets: Vec<TurretMount>,
    /// The docking anchors.
    pub docking_anchors: Vec<DockingAnchor>,
    /// The cargo capacity, or an explicit unknown.
    pub cargo: Resolved<f64>,
    /// The initial ownership.
    pub ownership: Ownership,
    /// The authored trajectory, when the ship has one.
    pub trajectory: Option<Trajectory>,
}

/// One capital ship's contract state.
#[derive(Clone, Debug, PartialEq)]
pub struct CapitalShip {
    subject: ContentId,
    actor: ActorId,
    graph: SubsystemGraph,
    engines: BTreeMap<SubsystemKey, EngineSpec>,
    bays: BTreeMap<SubsystemKey, Bay>,
    turrets: BTreeMap<SubsystemKey, TurretMount>,
    docking_anchors: BTreeMap<SubsystemKey, DockingAnchor>,
    cargo: Resolved<f64>,
    ownership: Ownership,
    trajectory: Option<Trajectory>,
}

impl CapitalShip {
    /// Assembles a ship from its validated parts.
    ///
    /// Every typed part key must be a subsystem of `graph` of the matching
    /// kind, and no key may repeat within a kind.
    ///
    /// # Errors
    ///
    /// [`CapitalError`] naming the duplicate, unknown or mistyped key, or a
    /// corrupt cargo capacity.
    pub fn try_new(
        subject: ContentId,
        actor: ActorId,
        parts: CapitalParts,
    ) -> Result<Self, CapitalError> {
        if let Resolved::Known(known) = &parts.cargo {
            if !known.value.is_finite() {
                return Err(CapitalError::NonFiniteCargo);
            }
            if known.value < 0.0 {
                return Err(CapitalError::NegativeCargo { value: known.value });
            }
        }

        let engines = collect(
            SubsystemKind::Engine,
            parts.engines,
            &parts.graph,
            |engine| engine.key.clone(),
        )?;
        let mut bays: BTreeMap<SubsystemKey, Bay> = BTreeMap::new();
        for bay in parts.bays {
            let kind = bay.kind.subsystem_kind();
            check_part(kind, bay.key.clone(), &parts.graph)?;
            let key = bay.key.clone();
            if bays.insert(key.clone(), bay).is_some() {
                return Err(CapitalError::DuplicatePart { kind, key });
            }
        }
        let turrets = collect(
            SubsystemKind::Turret,
            parts.turrets,
            &parts.graph,
            |turret| turret.key.clone(),
        )?;
        let docking_anchors = collect(
            SubsystemKind::DockingAnchor,
            parts.docking_anchors,
            &parts.graph,
            |anchor| anchor.key.clone(),
        )?;

        Ok(Self {
            subject,
            actor,
            graph: parts.graph,
            engines,
            bays,
            turrets,
            docking_anchors,
            cargo: parts.cargo,
            ownership: parts.ownership,
            trajectory: parts.trajectory,
        })
    }

    /// The catalog subject the ship was defined under.
    #[must_use]
    pub fn subject(&self) -> &ContentId {
        &self.subject
    }

    /// The actor this ship is.
    #[must_use]
    pub fn actor(&self) -> ActorId {
        self.actor
    }

    /// The subsystem graph.
    #[must_use]
    pub fn graph(&self) -> &SubsystemGraph {
        &self.graph
    }

    /// The current ownership.
    #[must_use]
    pub fn ownership(&self) -> &Ownership {
        &self.ownership
    }

    /// The cargo capacity, or an explicit unknown.
    #[must_use]
    pub const fn cargo(&self) -> &Resolved<f64> {
        &self.cargo
    }

    /// The authored trajectory, when the ship has one.
    #[must_use]
    pub fn trajectory(&self) -> Option<&Trajectory> {
        self.trajectory.as_ref()
    }

    /// A subsystem's state.
    #[must_use]
    pub fn subsystem_state(&self, key: &SubsystemKey) -> Option<SubsystemState> {
        self.graph.state(key)
    }

    /// Whether a lethal subsystem has destroyed the ship.
    #[must_use]
    pub fn is_destroyed(&self) -> bool {
        self.graph.is_destroyed()
    }

    /// Disables one subsystem and reports the behavior its destruction
    /// applies. Only a lethal subsystem destroys the ship.
    ///
    /// # Errors
    ///
    /// [`SubsystemGraphError::UnknownSubsystem`] when the key names no part.
    pub fn disable(&mut self, key: &SubsystemKey) -> Result<DisableOutcome, SubsystemGraphError> {
        self.graph.disable(key)
    }

    /// The engines, in key order.
    pub fn engines(&self) -> impl Iterator<Item = &EngineSpec> {
        self.engines.values()
    }

    /// The turret mounts, in key order.
    pub fn turrets(&self) -> impl Iterator<Item = &TurretMount> {
        self.turrets.values()
    }

    /// The docking anchors, in key order.
    pub fn docking_anchors(&self) -> impl Iterator<Item = &DockingAnchor> {
        self.docking_anchors.values()
    }

    /// A bay's state at `tick`: [`BayState::Destroyed`] once its subsystem
    /// is disabled, otherwise its authored exposure cycle.
    #[must_use]
    pub fn bay_state(&self, key: &SubsystemKey, tick: Tick) -> Option<BayState> {
        let bay = self.bays.get(key)?;
        if self.graph.state(key) == Some(SubsystemState::Disabled) {
            return Some(BayState::Destroyed);
        }
        Some(bay.exposure.state_at(tick))
    }

    /// The sum of the thrust of every engine that is still intact.
    ///
    /// # Errors
    ///
    /// [`PropulsionError::UnknownThrust`] when an intact engine's thrust is
    /// unresolved — the total is refused, never approximated.
    pub fn engine_thrust_n(&self) -> Result<f64, PropulsionError> {
        let mut total = 0.0_f64;
        for engine in self.engines.values() {
            if self.graph.state(&engine.key) == Some(SubsystemState::Disabled) {
                continue;
            }
            total += engine.known_thrust_n()?;
        }
        Ok(total)
    }

    /// The summed body-frame force of every intact engine at `throttle`.
    ///
    /// # Errors
    ///
    /// [`PropulsionError`] for an unresolved thrust or a non-finite
    /// throttle.
    pub fn propulsive_force_n(&self, throttle: f64) -> Result<[f64; 3], PropulsionError> {
        let mut force = [0.0_f64; 3];
        for engine in self.engines.values() {
            if self.graph.state(&engine.key) == Some(SubsystemState::Disabled) {
                continue;
            }
            let engine_force = engine.force_n(throttle)?;
            force[0] += engine_force[0];
            force[1] += engine_force[1];
            force[2] += engine_force[2];
        }
        Ok(force)
    }

    /// The body-frame acceleration of every intact engine at `throttle` for
    /// a mass of `mass_kg`. Disabling engines lowers this; disabling all of
    /// them yields zero, and none of it touches the hull's subsystem state.
    ///
    /// # Errors
    ///
    /// [`PropulsionError`] for an unresolved thrust, a non-finite throttle
    /// or a non-positive mass.
    pub fn propulsive_acceleration_m_s2(
        &self,
        throttle: f64,
        mass_kg: f64,
    ) -> Result<[f64; 3], PropulsionError> {
        acceleration_m_s2(self.propulsive_force_n(throttle)?, mass_kg)
    }
}

/// Checks that `key` is a subsystem of the expected kind.
fn check_part(
    expected: SubsystemKind,
    key: SubsystemKey,
    graph: &SubsystemGraph,
) -> Result<(), CapitalError> {
    match graph.subsystem(&key) {
        None => Err(CapitalError::UnknownPart {
            kind: expected,
            key,
        }),
        Some(subsystem) if subsystem.kind != expected => Err(CapitalError::PartKindMismatch {
            key: subsystem.key.clone(),
            expected,
            actual: subsystem.kind,
        }),
        Some(_) => Ok(()),
    }
}

/// Builds a keyed map from a typed part list, checking each key's kind and
/// refusing duplicates.
fn collect<T>(
    expected: SubsystemKind,
    items: Vec<T>,
    graph: &SubsystemGraph,
    key_of: impl Fn(&T) -> SubsystemKey,
) -> Result<BTreeMap<SubsystemKey, T>, CapitalError> {
    let mut map = BTreeMap::new();
    for item in items {
        let key = key_of(&item);
        check_part(expected, key.clone(), graph)?;
        if map.insert(key.clone(), item).is_some() {
            return Err(CapitalError::DuplicatePart {
                kind: expected,
                key,
            });
        }
    }
    Ok(map)
}

/// The synthetic ship's straight-line trajectory: 20 m/s along +X at
/// 10 ticks/s for 1000 m, then held. Designed, not measured.
#[must_use]
pub fn synthetic_capital_trajectory() -> Trajectory {
    use crate::world_actors::Quat;
    use crate::world_actors::trajectory::Keyframe;
    Trajectory::new(
        vec![
            Keyframe {
                tick: Tick(0),
                position_m: [0.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
            Keyframe {
                tick: Tick(500),
                position_m: [1000.0, 0.0, 0.0],
                orientation: Quat::IDENTITY,
            },
        ],
        10,
    )
    .expect("the synthetic capital trajectory is valid")
}

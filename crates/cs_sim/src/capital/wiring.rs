//! The F35-C wiring state each capital ship carries in its session: the
//! launch ledger the set drives, cargo accounting, the staged-destruction
//! phase and the control a capture switches.
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-C`. Shared contract:
//! `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! F35-A defined the identity and ordering of these transactions and F35-B
//! built the movement, weakpoint and turret runtime, but nothing *drove*
//! them: the ledger had no caller, the capture had no commit, the cargo
//! capacity had no accounting and a wreck was simply a pose. This module is
//! the state those wirings need, and
//! [`crate::capital::CapitalShipSet`] is the single place that advances it.
//! Everything here is designed behavior; no original capital-ship cargo,
//! launch cadence, capture timing or destruction phase is measured — see
//! `docs/findings/2026-10-04-f35-c-launch-capture-cargo-destruction.md`.
//!
//! # Launch: release once, cancel on destruction
//!
//! [`ShipWiring`] holds the F35-A [`LaunchLedger`] plus the *outcome* of every
//! launch id: [`LaunchStatus::Released`] carries the one
//! [`ReleasedAircraft`] the id produced, [`LaunchStatus::Cancelled`] carries
//! the launch that will never spawn and why. The set's tick pass is the only
//! writer, so an id resolves exactly once — a destroyed launch bay moves its
//! waiting launches to `Cancelled` in the same pass that refuses to release
//! them, which is AC03's "no duplicate aircraft".
//!
//! # Cargo: capacity-bounded, unknown-capable
//!
//! [`CargoLedger`] bounds what a ship may hold by its declared capacity. A
//! known capacity enforces the bound; an unresolved one refuses every load by
//! claim, because an unbounded hold is never assumed. Units are the declared
//! capacity's own scale — the declared record carries no unit, so none is
//! invented here.
//!
//! # Destruction and despawn are separate
//!
//! [`DestructionState`] holds the distinction non-negotiable behavior 5 asks
//! for. A lethal subsystem starts the staged destruction: the wreck's pose is
//! frozen (F35-B) but the ship is still in the world and still takes hits
//! until [`DespawnPolicy`] says the phase ended, at which point the record is
//! [`DestructionState::Despawned`] and refuses every new order. The phase
//! models *how long the wreck stays*, not how it moves: a sinking or crashing
//! trajectory would need original evidence this project does not have.

use std::collections::BTreeMap;

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::{ContentId, Resolved};
use cs_types::evidence::ClaimId;

use super::capture::{CaptureTicket, CaptureTransaction, ShipControl};
use super::launch::{LaunchId, LaunchLedger, PendingLaunch, ReleasedAircraft};
use super::subsystem::SubsystemKey;

/// Why a pending launch will never spawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchCancelReason {
    /// The launch bay subsystem itself was destroyed while the ship lived: its
    /// later spawns are cancelled, not deferred.
    BayDestroyed,
    /// The carrier was destroyed. A dying ship launches nothing, so every
    /// launch still waiting aboard is cancelled rather than released from a
    /// wreck.
    ShipDestroyed,
    /// The carrier despawned before the launch could run.
    ShipDespawned,
}

impl LaunchCancelReason {
    /// The stable label used in reports.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::BayDestroyed => "bay_destroyed",
            Self::ShipDestroyed => "ship_destroyed",
            Self::ShipDespawned => "ship_despawned",
        }
    }
}

impl std::fmt::Display for LaunchCancelReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.label())
    }
}

/// A launch that was cancelled before it could spawn.
#[derive(Clone, Debug, PartialEq)]
pub struct LaunchCancellation {
    /// The launch that will never run.
    pub launch: PendingLaunch,
    /// Why it was cancelled.
    pub reason: LaunchCancelReason,
    /// The tick it was cancelled on.
    pub at: Tick,
}

/// How one launch id resolved — the answer to "did this aircraft spawn?".
#[derive(Clone, Debug, PartialEq)]
pub enum LaunchStatus {
    /// Still waiting for its tick or its bay to open.
    Pending {
        /// The launch that is waiting.
        launch: PendingLaunch,
    },
    /// Released exactly once. The [`ReleasedAircraft`] is the authoritative
    /// spawn state; its id is fresh for the session.
    Released {
        /// The aircraft this id produced.
        aircraft: ReleasedAircraft,
    },
    /// Cancelled before release. No aircraft was ever created for it.
    Cancelled {
        /// What was cancelled.
        cancellation: LaunchCancellation,
    },
}

impl LaunchStatus {
    /// The released aircraft, when the id produced one.
    #[must_use]
    pub const fn released(&self) -> Option<&ReleasedAircraft> {
        match self {
            Self::Released { aircraft } => Some(aircraft),
            Self::Pending { .. } | Self::Cancelled { .. } => None,
        }
    }

    /// Whether the id has been resolved and can never resolve again.
    #[must_use]
    pub const fn is_settled(&self) -> bool {
        !matches!(self, Self::Pending { .. })
    }
}

/// Why a cargo operation was refused.
#[derive(Clone, Debug, PartialEq)]
pub enum CargoRefusal {
    /// The load was NaN or infinite.
    NonFiniteLoad,
    /// The load was negative: cargo cannot be negative.
    NegativeLoad {
        /// The refused amount.
        value: f64,
    },
    /// The load would exceed the declared capacity.
    InsufficientCapacity {
        /// The declared capacity.
        capacity: f64,
        /// What the ship already holds.
        loaded: f64,
        /// What the load asked for.
        requested: f64,
    },
    /// The capacity is unresolved: nothing is loaded into an unbounded hold.
    CapacityUnknown {
        /// The claim the unknown capacity is recorded under.
        claim_id: ClaimId,
        /// Why the capacity is unknown.
        reason: String,
    },
    /// The unload asked for more than the ship holds.
    InsufficientLoad {
        /// What the ship holds.
        loaded: f64,
        /// What the unload asked for.
        requested: f64,
    },
    /// The ship is destroyed: nothing new is loaded into a wreck.
    ShipDestroyed {
        /// The wreck.
        actor: ActorId,
    },
    /// The ship despawned, so its record is closed.
    ShipDespawned {
        /// The despawned ship.
        actor: ActorId,
    },
}

impl std::fmt::Display for CargoRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFiniteLoad => write!(f, "the cargo load is not finite"),
            Self::NegativeLoad { value } => write!(f, "the cargo load {value} is negative"),
            Self::InsufficientCapacity {
                capacity,
                loaded,
                requested,
            } => write!(
                f,
                "loading {requested} would exceed the capacity {capacity} (holding {loaded})"
            ),
            Self::CapacityUnknown { claim_id, reason } => write!(
                f,
                "the cargo capacity is unknown ({}: {reason})",
                claim_id.as_str()
            ),
            Self::InsufficientLoad { loaded, requested } => {
                write!(f, "unloading {requested} exceeds the {loaded} held")
            }
            Self::ShipDestroyed { actor } => write!(f, "ship {actor:?} is destroyed"),
            Self::ShipDespawned { actor } => write!(f, "ship {actor:?} has despawned"),
        }
    }
}

impl std::error::Error for CargoRefusal {}

/// What one ship holds and how much of it it may hold.
#[derive(Clone, Debug, PartialEq)]
pub struct CargoLedger {
    capacity: Resolved<f64>,
    loaded: f64,
}

impl CargoLedger {
    /// An empty hold against `capacity`.
    #[must_use]
    pub fn new(capacity: Resolved<f64>) -> Self {
        Self {
            capacity,
            loaded: 0.0,
        }
    }

    /// The declared capacity, verbatim: an unknown stays unknown.
    #[must_use]
    pub const fn capacity(&self) -> &Resolved<f64> {
        &self.capacity
    }

    /// What the ship currently holds.
    #[must_use]
    pub fn loaded(&self) -> f64 {
        self.loaded
    }

    /// How much more the ship may take, or `None` when its capacity is
    /// unresolved.
    #[must_use]
    pub fn remaining(&self) -> Option<f64> {
        match &self.capacity {
            Resolved::Known(known) => Some((known.value - self.loaded).max(0.0)),
            Resolved::Unknown { .. } => None,
        }
    }

    /// Adds `units` and returns the new load.
    ///
    /// # Errors
    ///
    /// [`CargoRefusal::NonFiniteLoad`], [`CargoRefusal::NegativeLoad`],
    /// [`CargoRefusal::InsufficientCapacity`] or
    /// [`CargoRefusal::CapacityUnknown`]. A refused load changes nothing.
    pub fn load(&mut self, units: f64) -> Result<f64, CargoRefusal> {
        if !units.is_finite() {
            return Err(CargoRefusal::NonFiniteLoad);
        }
        if units < 0.0 {
            return Err(CargoRefusal::NegativeLoad { value: units });
        }
        let capacity = match &self.capacity {
            Resolved::Known(known) => known.value,
            Resolved::Unknown { claim_id, reason } => {
                return Err(CargoRefusal::CapacityUnknown {
                    claim_id: claim_id.clone(),
                    reason: reason.clone(),
                });
            }
        };
        let requested = self.loaded + units;
        if requested > capacity {
            return Err(CargoRefusal::InsufficientCapacity {
                capacity,
                loaded: self.loaded,
                requested: units,
            });
        }
        self.loaded = requested;
        Ok(self.loaded)
    }

    /// Removes `units` and returns the new load. Unloading is allowed while a
    /// ship is dying but not after it despawned: it reads state that exists,
    /// where a load would add state to a wreck.
    ///
    /// # Errors
    ///
    /// [`CargoRefusal::NonFiniteLoad`], [`CargoRefusal::NegativeLoad`] or
    /// [`CargoRefusal::InsufficientLoad`]. A refused unload changes nothing.
    pub fn unload(&mut self, units: f64) -> Result<f64, CargoRefusal> {
        if !units.is_finite() {
            return Err(CargoRefusal::NonFiniteLoad);
        }
        if units < 0.0 {
            return Err(CargoRefusal::NegativeLoad { value: units });
        }
        let requested = self.loaded - units;
        if requested < 0.0 {
            return Err(CargoRefusal::InsufficientLoad {
                loaded: self.loaded,
                requested: units,
            });
        }
        self.loaded = requested;
        Ok(self.loaded)
    }

    /// Empties the hold: what the cargo did when the ship despawned.
    pub fn clear(&mut self) {
        self.loaded = 0.0;
    }
}

/// How long a destroyed ship stays in the world before it despawns.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct DespawnPolicy {
    /// Ticks after destruction at which the ship despawns, or `None` to hold
    /// the wreck until the mission despawns it explicitly. Holding is the
    /// default because no original destruction timing is measured.
    pub after_ticks: Option<u64>,
}

impl DespawnPolicy {
    /// Keep the wreck until something despawns it explicitly.
    pub const HOLD: Self = Self { after_ticks: None };

    /// Despawn the wreck `ticks` after it was destroyed. Zero despawns it on
    /// the tick it died.
    #[must_use]
    pub const fn after_ticks(ticks: u64) -> Self {
        Self {
            after_ticks: Some(ticks),
        }
    }
}

impl Default for DespawnPolicy {
    fn default() -> Self {
        Self::HOLD
    }
}

/// Where a ship is in the destruction/despawn lifecycle (F35-C).
///
/// Destruction and despawn are separate transitions: destruction starts the
/// staged phase, despawn closes the record. Non-negotiable behavior 5.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DestructionState {
    /// The ship is intact.
    Live,
    /// A lethal subsystem destroyed the ship at `since`. The wreck stays in
    /// the world, frozen and still a valid hit target, until the set reaches
    /// `despawn_at` — or forever, when that is `None`.
    Sinking {
        /// The tick destruction happened on.
        since: Tick,
        /// The tick the wreck despawns, or `None` when it is held.
        despawn_at: Option<Tick>,
    },
    /// The ship left the world at `at`. Its record is closed: every new order
    /// refuses. Its last pose is still readable for diagnostics.
    Despawned {
        /// The tick it despawned on.
        at: Tick,
    },
}

impl DestructionState {
    /// Whether the ship is intact and acting.
    #[must_use]
    pub const fn is_live(&self) -> bool {
        matches!(self, Self::Live)
    }

    /// Whether a lethal subsystem has destroyed the ship — true through the
    /// whole staged phase and after the despawn.
    #[must_use]
    pub const fn is_destroyed(&self) -> bool {
        !self.is_live()
    }

    /// Whether the ship is still in the world and may be hit.
    #[must_use]
    pub const fn is_present(&self) -> bool {
        !matches!(self, Self::Despawned { .. })
    }

    /// Whether the ship has left the world: its record is closed.
    #[must_use]
    pub const fn is_despawned(&self) -> bool {
        matches!(self, Self::Despawned { .. })
    }

    /// The tick the ship despawned on, when it has.
    #[must_use]
    pub const fn despawned_at(&self) -> Option<Tick> {
        match self {
            Self::Despawned { at } => Some(*at),
            Self::Live | Self::Sinking { .. } => None,
        }
    }

    /// Starts the staged destruction at `at` under `policy`.
    ///
    /// A zero-tick policy despawns immediately; a saturating schedule (a
    /// policy so long its deadline would wrap) holds the wreck instead of
    /// despawning it at an unreachable tick. Destruction is monotonic: a
    /// second call leaves the phase that is already running.
    pub fn destroy(&mut self, at: Tick, policy: DespawnPolicy) {
        if self.is_destroyed() {
            return;
        }
        *self = match policy.after_ticks {
            Some(0) => Self::Despawned { at },
            Some(ticks) => match at.0.checked_add(ticks) {
                Some(deadline) => Self::Sinking {
                    since: at,
                    despawn_at: Some(Tick(deadline)),
                },
                None => Self::Sinking {
                    since: at,
                    despawn_at: None,
                },
            },
            None => Self::Sinking {
                since: at,
                despawn_at: None,
            },
        };
    }

    /// Closes the record at `at`, whatever phase it was in.
    ///
    /// Only a *destroyed* ship may be despawned — a live ship leaving the
    /// world is a mission removal, a different transition — so the caller is
    /// responsible for that precondition; the session set checks it. Returns
    /// `false` when the record was already closed.
    pub fn despawn_now(&mut self, at: Tick) -> bool {
        if self.is_despawned() {
            return false;
        }
        *self = Self::Despawned { at };
        true
    }

    /// Advances the staged phase to `to`, returning `true` when this call
    /// despawned the ship.
    pub fn advance(&mut self, to: Tick) -> bool {
        let Self::Sinking {
            despawn_at: Some(deadline),
            ..
        } = *self
        else {
            return false;
        };
        if to >= deadline {
            *self = Self::Despawned { at: to };
            return true;
        }
        false
    }
}

/// One capture attempt in flight, with the ticket that qualifies it.
#[derive(Clone, Debug, PartialEq)]
pub struct CaptureProgress {
    /// The handle the caller must present to advance or abort.
    pub ticket: CaptureTicket,
    /// The staged transaction itself.
    pub transaction: CaptureTransaction,
}

/// Everything the F35-C wiring tracks for one registered ship.
#[derive(Clone, Debug, PartialEq)]
pub struct ShipWiring {
    /// The once-only launch ledger.
    pub ledger: LaunchLedger,
    /// The ejection velocity each pending launch leaves the socket with, keyed
    /// by its launch id. It lives beside the ledger because the release needs
    /// both, and it is dropped when the id resolves.
    pub ejections: BTreeMap<LaunchId, [f64; 3]>,
    /// The aircraft each released launch id produced.
    pub released: BTreeMap<LaunchId, ReleasedAircraft>,
    /// The launches that were cancelled before they could spawn.
    pub cancelled: BTreeMap<LaunchId, LaunchCancellation>,
    /// What the ship holds and how much of it it may hold.
    pub cargo: CargoLedger,
    /// The capture attempt in flight, if any.
    pub capture: Option<CaptureProgress>,
    /// The attempt ordinal handed to the next capture.
    pub attempts: u64,
    /// Where the ship is in the destruction/despawn lifecycle.
    pub destruction: DestructionState,
    /// Who the ship's guns, targeting, docking and AI act for.
    pub control: ShipControl,
}

impl ShipWiring {
    /// Fresh wiring for a ship owned by `owner`, with `docking_open` reading
    /// the ship's declared anchors and `capacity` its declared cargo capacity.
    #[must_use]
    pub fn new(owner: &ContentId, docking_open: bool, capacity: Resolved<f64>) -> Self {
        Self {
            ledger: LaunchLedger::new(),
            ejections: BTreeMap::new(),
            released: BTreeMap::new(),
            cancelled: BTreeMap::new(),
            cargo: CargoLedger::new(capacity),
            capture: None,
            attempts: 0,
            destruction: DestructionState::Live,
            control: ShipControl::initial(owner, docking_open),
        }
    }

    /// How many launches of `bay` are still waiting aboard it.
    #[must_use]
    pub fn pending_in(&self, bay: &SubsystemKey) -> u32 {
        u32::try_from(
            self.ledger
                .pending()
                .filter(|launch| &launch.id.bay == bay)
                .count(),
        )
        .unwrap_or(u32::MAX)
    }

    /// How one launch id resolved, or `None` when the ledger never issued it.
    #[must_use]
    pub fn status(&self, id: &LaunchId) -> Option<LaunchStatus> {
        if let Some(aircraft) = self.released.get(id) {
            return Some(LaunchStatus::Released {
                aircraft: *aircraft,
            });
        }
        if let Some(cancellation) = self.cancelled.get(id) {
            return Some(LaunchStatus::Cancelled {
                cancellation: cancellation.clone(),
            });
        }
        self.ledger
            .pending()
            .find(|launch| &launch.id == id)
            .map(|launch| LaunchStatus::Pending {
                launch: launch.clone(),
            })
    }
}

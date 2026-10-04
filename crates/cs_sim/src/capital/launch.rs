//! Capital-ship launch sockets and the once-only release ledger (F35-A).
//!
//! Spec: `specs/F35-zeppelins-capital-ships-subsystems-and-launch-bays.md`,
//! stage `### F35-A`. Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
//!
//! Non-negotiable behavior 3: aircraft release from verified bay transforms,
//! inherit the carrier's motion and gain dynamic authority exactly once, and
//! destroying a launch bay affects later spawns. The F35-A contract is the
//! identity and once-only shape of that handoff:
//!
//! * [`LaunchSocket`] names the carrier, socket index and body-frame offset;
//!   [`release_aircraft`] turns a sampled carrier pose into a released
//!   aircraft at the socket with the carrier's velocity — the same
//!   [`anchor_sample`] the F34 anchor renderer uses, so the spawn pose and
//!   the visual pose cannot disagree.
//! * [`LaunchLedger`] issues every scheduled launch at most once: a released
//!   id is never released again, and destroying a bay cancels its pending
//!   launches instead of leaving them to spawn later. This is the data half
//!   of AC03; F35-C drives it from the session set, which is what actually
//!   releases the aircraft and what makes AC03 observable end to end.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::ContentId;
use cs_types::evidence::ClaimId;

use super::bay::BayState;
use super::subsystem::SubsystemKey;
use crate::world_actors::anchor::{AnchorSocket, anchor_sample};
use crate::world_actors::trajectory::Pose;

/// A named release point fixed in a carrier's frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LaunchSocket {
    /// The carrier actor the socket belongs to.
    pub actor: ActorId,
    /// The socket index inside the carrier's launch bay.
    pub socket: u16,
    /// Offset from the carrier origin, in the carrier's frame.
    pub offset_m: [f64; 3],
}

/// A released aircraft's initial kinematic state.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReleasedAircraft {
    /// A fresh id, never the carrier's.
    pub actor: ActorId,
    /// The tick the release happened on.
    pub tick: Tick,
    /// The socket's world position at release.
    pub position_m: [f64; 3],
    /// The carrier's socket velocity plus the ejection.
    pub velocity_m_s: [f64; 3],
    /// Whether the aircraft has dynamic authority. It gains it once, here.
    pub dynamic_authority: bool,
}

/// Releases `aircraft` from `socket` using the carrier `pose` sampled at
/// `tick`. The aircraft starts at the socket with the carrier's velocity
/// (rotational part included) plus the ejection, and gains dynamic authority
/// exactly once.
#[must_use]
pub fn release_aircraft(
    tick: Tick,
    pose: &Pose,
    socket: &LaunchSocket,
    aircraft: ActorId,
    eject_m_s: [f64; 3],
) -> ReleasedAircraft {
    let anchor = anchor_sample(
        tick,
        pose,
        &AnchorSocket {
            actor: socket.actor,
            socket: socket.socket,
            offset_m: socket.offset_m,
        },
    );
    let eject = anchor.orientation.rotate(eject_m_s);
    ReleasedAircraft {
        actor: aircraft,
        tick,
        position_m: anchor.position_m,
        velocity_m_s: [
            anchor.velocity_m_s[0] + eject[0],
            anchor.velocity_m_s[1] + eject[1],
            anchor.velocity_m_s[2] + eject[2],
        ],
        dynamic_authority: true,
    }
}

/// The stable identity of one scheduled launch: its bay and ordinal.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LaunchId {
    /// The launch bay the aircraft belongs to.
    pub bay: SubsystemKey,
    /// The monotonically assigned sequence within that bay.
    pub sequence: u64,
}

/// One launch waiting for its tick and its bay to open.
#[derive(Clone, Debug, PartialEq)]
pub struct PendingLaunch {
    /// The launch's identity.
    pub id: LaunchId,
    /// The aircraft catalog id to spawn.
    pub aircraft: ContentId,
    /// The tick at or after which the launch is ready.
    pub ready_tick: Tick,
}

/// What one release pass did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchTick {
    /// Launches whose bay was open and whose tick arrived, released once.
    pub released: Vec<PendingLaunch>,
    /// Launches cancelled because their bay was destroyed.
    pub cancelled: Vec<PendingLaunch>,
}

/// Why an aircraft could not be released.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LaunchRefusal {
    /// The bay is not a launch bay of this ship.
    UnknownBay,
    /// The bay was destroyed; later spawns are cancelled, not deferred.
    BayDestroyed,
    /// The launch has already been released once.
    AlreadyReleased,
    /// The launch was cancelled and can never be released.
    Cancelled,
    /// The launch's ready tick has not arrived.
    NotReady {
        /// The tick the launch becomes ready.
        ready_tick: Tick,
    },
    /// The bay is concealed or moving; it is not open.
    NotOpen,
    /// The bay already holds as many waiting aircraft as its declared
    /// capacity. Only launches still aboard count; a released aircraft has
    /// left the bay and freed its slot.
    BayFull {
        /// The bay's declared capacity.
        capacity: u32,
        /// How many launches are waiting aboard it.
        pending: u32,
    },
    /// The bay's capacity is unresolved: an unbounded hangar is never
    /// assumed, so nothing is scheduled into it.
    CapacityUnknown {
        /// The claim the unknown capacity is recorded under.
        claim_id: ClaimId,
        /// Why the capacity is unknown.
        reason: String,
    },
    /// The bay's release socket is unresolved: an aircraft is never spawned at
    /// an invented transform.
    SocketUnknown {
        /// The claim the unknown socket is recorded under.
        claim_id: ClaimId,
        /// Why the socket is unknown.
        reason: String,
    },
    /// The ejection velocity was NaN or infinite.
    NonFiniteEjection,
    /// The session's actor ids are exhausted; no id is reused.
    ActorIdExhausted,
    /// The carrier is destroyed: a dying ship launches nothing, and its
    /// waiting launches are cancelled instead.
    ShipDestroyed {
        /// The wreck.
        carrier: ActorId,
    },
    /// The carrier despawned and its record is closed.
    ShipDespawned {
        /// The despawned carrier.
        carrier: ActorId,
    },
}

impl std::fmt::Display for LaunchRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownBay => write!(f, "the bay is not a launch bay of this ship"),
            Self::BayDestroyed => write!(f, "the launch bay is destroyed"),
            Self::AlreadyReleased => write!(f, "the launch was already released once"),
            Self::Cancelled => write!(f, "the launch was cancelled"),
            Self::NotReady { ready_tick } => {
                write!(f, "the launch is not ready until tick {ready_tick:?}")
            }
            Self::NotOpen => write!(f, "the launch bay is not open"),
            Self::BayFull { capacity, pending } => {
                write!(
                    f,
                    "the launch bay holds {pending} of {capacity} waiting aircraft"
                )
            }
            Self::CapacityUnknown { claim_id, reason } => write!(
                f,
                "the launch bay capacity is unknown ({}: {reason})",
                claim_id.as_str()
            ),
            Self::SocketUnknown { claim_id, reason } => write!(
                f,
                "the launch bay socket is unknown ({}: {reason})",
                claim_id.as_str()
            ),
            Self::NonFiniteEjection => write!(f, "the ejection velocity is not finite"),
            Self::ActorIdExhausted => write!(f, "the session's actor ids are exhausted"),
            Self::ShipDestroyed { carrier } => {
                write!(f, "carrier {carrier:?} is destroyed and launches nothing")
            }
            Self::ShipDespawned { carrier } => {
                write!(f, "carrier {carrier:?} despawned and launches nothing")
            }
        }
    }
}

impl std::error::Error for LaunchRefusal {}

/// Tracks scheduled launches and guarantees each is released at most once.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LaunchLedger {
    next_sequence: BTreeMap<SubsystemKey, u64>,
    pending: BTreeMap<LaunchId, PendingLaunch>,
    released: BTreeSet<LaunchId>,
}

impl LaunchLedger {
    /// An empty ledger.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Schedules a launch on `bay`, assigning the next stable id for that
    /// bay. Ids are never reused within a ledger.
    pub fn schedule(
        &mut self,
        bay: SubsystemKey,
        aircraft: ContentId,
        ready_tick: Tick,
    ) -> LaunchId {
        let sequence = self.next_sequence.entry(bay.clone()).or_insert(0);
        let id = LaunchId {
            bay,
            sequence: *sequence,
        };
        *sequence += 1;
        self.pending.insert(
            id.clone(),
            PendingLaunch {
                id: id.clone(),
                aircraft,
                ready_tick,
            },
        );
        id
    }

    /// The launches still waiting.
    pub fn pending(&self) -> impl Iterator<Item = &PendingLaunch> {
        self.pending.values()
    }

    /// Whether `id` has already been released.
    #[must_use]
    pub fn was_released(&self, id: &LaunchId) -> bool {
        self.released.contains(id)
    }

    /// Releases every pending launch whose bay is open and whose ready tick
    /// has arrived, and cancels the pending launches of destroyed bays.
    ///
    /// Each launch is released at most once: a release removes it from
    /// `pending` and records its id in `released`, so a second pass over the
    /// same tick produces nothing. A destroyed bay's launches are removed
    /// and reported in [`LaunchTick::cancelled`] — they never spawn later.
    pub fn release_ready(
        &mut self,
        tick: Tick,
        bay_state: impl Fn(&SubsystemKey) -> BayState,
    ) -> LaunchTick {
        let ids: Vec<LaunchId> = self.pending.keys().cloned().collect();
        let mut out = LaunchTick::default();
        for id in ids {
            let Some(launch) = self.pending.get(&id) else {
                continue;
            };
            match bay_state(&id.bay) {
                BayState::Destroyed => {
                    if let Some(cancelled) = self.pending.remove(&id) {
                        out.cancelled.push(cancelled);
                    }
                }
                BayState::Exposed => {
                    if launch.ready_tick <= tick
                        && self.released.insert(id.clone())
                        && let Some(released) = self.pending.remove(&id)
                    {
                        out.released.push(released);
                    }
                }
                BayState::Concealed | BayState::Opening | BayState::Closing => {}
            }
        }
        out
    }

    /// Releases exactly `id` now, leaving every other launch alone.
    ///
    /// The gate is [`Self::check`]'s — the bay must be open, the ready tick
    /// must have arrived and the id must still be pending — and the once-only
    /// guarantee is the same: a successful call records the id as released and
    /// removes it from `pending`, so no later pass can produce a second
    /// aircraft for it. This is the F35-C retry door a caller uses instead of
    /// waiting for the tick pass.
    ///
    /// # Errors
    ///
    /// [`LaunchRefusal`] naming why not. A refused call resolves nothing: the
    /// launch stays pending and the tick pass still owns its teardown.
    pub fn release_one(
        &mut self,
        id: &LaunchId,
        tick: Tick,
        bay_state: impl FnOnce(&SubsystemKey) -> BayState,
    ) -> Result<PendingLaunch, LaunchRefusal> {
        self.check(id, tick, bay_state)?;
        let Some(launch) = self.pending.remove(id) else {
            // Unreachable while `check` and `release` share one map, kept so a
            // future divergence surfaces as a refusal instead of a silent
            // spawn.
            return Err(LaunchRefusal::UnknownBay);
        };
        self.released.insert(id.clone());
        Ok(launch)
    }

    /// Checks whether `id` could be released right now, without releasing
    /// it. Used by callers that want the refusal reason.
    ///
    /// # Errors
    ///
    /// [`LaunchRefusal`] naming why not.
    pub fn check(
        &self,
        id: &LaunchId,
        tick: Tick,
        bay_state: impl FnOnce(&SubsystemKey) -> BayState,
    ) -> Result<(), LaunchRefusal> {
        if self.released.contains(id) {
            return Err(LaunchRefusal::AlreadyReleased);
        }
        let Some(launch) = self.pending.get(id) else {
            return Err(LaunchRefusal::UnknownBay);
        };
        match bay_state(&id.bay) {
            BayState::Destroyed => Err(LaunchRefusal::BayDestroyed),
            BayState::Exposed => {
                if launch.ready_tick <= tick {
                    Ok(())
                } else {
                    Err(LaunchRefusal::NotReady {
                        ready_tick: launch.ready_tick,
                    })
                }
            }
            _ => Err(LaunchRefusal::NotOpen),
        }
    }
}

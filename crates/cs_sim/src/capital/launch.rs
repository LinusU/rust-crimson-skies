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
//!   of AC03; the script-driven runtime is F35-C.

use std::collections::{BTreeMap, BTreeSet};

use cs_script::ir::ActorId;
use cs_types::Tick;
use cs_types::content::ContentId;

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
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchRefusal {
    /// The bay is not a launch bay of this ship.
    UnknownBay,
    /// The bay was destroyed; later spawns are cancelled, not deferred.
    BayDestroyed,
    /// The launch has already been released once.
    AlreadyReleased,
    /// The launch's ready tick has not arrived.
    NotReady {
        /// The tick the launch becomes ready.
        ready_tick: Tick,
    },
    /// The bay is concealed or moving; it is not open.
    NotOpen,
}

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

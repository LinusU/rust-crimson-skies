//! Session-scoped wire identity (F54-A).
//!
//! Spec: `specs/F54-modern-multiplayer-transport-and-authority-protocol.md`,
//! stage `### F54-A`. Contracts: `docs/contracts/IDENTITY-CONTENT.md`
//! ("`SessionId` is a monotonically allocated opaque runtime generation",
//! "`ActorId` includes session plus a non-recycled generation-qualified local
//! id", "`EventId(session, tick, producer, sequence)`") and
//! `docs/contracts/UI-NETWORK.md` ("Protocol messages carry session epoch and
//! sequence/tick. Epoch mismatch rejects stale packets.").
//!
//! These are the identity types the wire protocol (`cs_net`) and its future
//! consumers share. They live in `cs_types` because simulation and content
//! crates must name sessions, actors and events without depending on the
//! protocol crate (`docs/01-ARCHITECTURE.md` dependency table).
//!
//! The session/peer identity types are integer newtypes that are **never
//! zero**: zero is reserved as the "no session / no peer" sentinel, so a
//! default-constructed or truncated field can never alias a live id. The
//! compound ids ([`ActorId`], [`EventId`]) pair such a newtype with a
//! component that legitimately starts at 0 (`producer` is the first counter of
//! the first producing system); what zero never names there is a *live* actor
//! or event, because [`ActorAllocator`] hands out serials from 1.
//!
//! Nothing here is derived from original game data; the shapes are the
//! new-engine contract.

use std::fmt;

use crate::Tick;

/// One session generation: the contract `SessionId` and the wire's "session
/// epoch".
///
/// The host allocates a fresh id for every session (a lobby launch, a mission
/// retry); it is monotonic and never reused, so a message stamped with an old
/// id names a dead session and is rejected rather than applied
/// (`docs/contracts/UI-NETWORK.md`, "Epoch mismatch rejects stale packets").
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SessionId(u64);

impl SessionId {
    /// Wraps an allocated id number; zero is refused.
    pub const fn new(value: u64) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The allocated number.
    pub const fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for SessionId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "session {}", self.0)
    }
}

/// One connected participant of a session, allocated by the host.
///
/// A peer id names the transport-level participant, not a profile or a
/// callsign (those are lobby vocabulary, F55). It is valid only together with
/// the [`SessionId`] it was allocated under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PeerId(u16);

impl PeerId {
    /// Wraps an allocated peer number; zero is refused so a missing or
    /// truncated field cannot alias a live peer.
    pub const fn new(value: u16) -> Option<Self> {
        if value == 0 { None } else { Some(Self(value)) }
    }

    /// The allocated number.
    pub const fn get(self) -> u16 {
        self.0
    }
}

impl fmt::Display for PeerId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "peer {}", self.0)
    }
}

/// One actor inside one session: the contract's
/// `ActorId { session, serial }` shape.
///
/// `serial` is allocated by the server and never recycled inside the session
/// (`docs/01-ARCHITECTURE.md`: "a non-recycled generation-qualified local
/// id"), so an id always names exactly one actor of one session and a stale
/// snapshot or event can never alias a respawned actor
/// (`docs/contracts/UI-NETWORK.md`: "Interpolation buffers separate actor
/// generations").
///
/// `cs_sim::damage` uses this type directly for damage actors:
/// `cs_sim::damage::ActorId` *is* this type (task #442). `cs_script::ir`
/// still carries a script-scoped realization of the shape; migrating it is
/// separate follow-up work recorded in the F29-A findings.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ActorId {
    /// The session generation the actor belongs to.
    pub session: SessionId,
    /// The actor's serial within that session. Server-allocated and never
    /// recycled; [`ActorAllocator`] issues from 1, so serial 0 is never a live
    /// actor.
    pub serial: u64,
}

impl fmt::Display for ActorId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "actor {}:{}", self.session.get(), self.serial)
    }
}

/// The identity of one semantic event: the contract's
/// `EventId(session, tick, producer, sequence)` shape.
///
/// Reliable semantic events deduplicate by this id, not by transport delivery
/// (`docs/contracts/UI-NETWORK.md`: "application-level ids deduplicate
/// events"), because reconnect and retry can replay a request. `producer` is
/// the serial of the system that emitted the event and `sequence` orders that
/// producer's own events, so ordering by the whole id is total and
/// deterministic.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EventId {
    /// The session generation the event was produced in.
    pub session: SessionId,
    /// The simulation tick the event belongs to.
    pub tick: Tick,
    /// The producing system's serial, counted from 0.
    pub producer: u32,
    /// The event's sequence within its producer, counted from 0.
    pub sequence: u32,
}

impl fmt::Display for EventId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "event {}:{}:{}:{}",
            self.session.get(),
            self.tick.0,
            self.producer,
            self.sequence
        )
    }
}

/// Why an [`ActorAllocator`] refused to allocate.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AllocError {
    /// The serial space is exhausted: the next serial could never be issued,
    /// so no further actor can ever be spawned in this session.
    Exhausted,
}

impl fmt::Display for AllocError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Exhausted => write!(f, "actor serials exhausted for this session"),
        }
    }
}

impl std::error::Error for AllocError {}

/// Server-side allocation of [`ActorId`] serials for one session.
///
/// The server owns actor id allocation (`docs/contracts/UI-NETWORK.md`
/// ownership table); clients never mint actor ids. Serials are handed out
/// monotonically and never recycled, so a despawned actor's id can never name
/// a later spawn.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ActorAllocator {
    session: SessionId,
    next_serial: u64,
}

impl ActorAllocator {
    /// Starts allocation for `session`; the first serial is 1 so serial 0 is
    /// never a live actor.
    pub const fn new(session: SessionId) -> Self {
        Self {
            session,
            next_serial: 1,
        }
    }

    /// The session this allocator mints ids for.
    pub const fn session(&self) -> SessionId {
        self.session
    }

    /// The serial the next allocation would issue.
    pub const fn next_serial(&self) -> u64 {
        self.next_serial
    }

    /// Allocates the next actor id.
    ///
    /// # Errors
    ///
    /// [`AllocError::Exhausted`] when the serial space is at the top of its
    /// range; the allocator then stays exhausted rather than wrapping onto a
    /// serial that was already issued.
    pub const fn allocate(&mut self) -> Result<ActorId, AllocError> {
        let serial = self.next_serial;
        let Some(next) = serial.checked_add(1) else {
            return Err(AllocError::Exhausted);
        };
        self.next_serial = next;
        Ok(ActorId {
            session: self.session,
            serial,
        })
    }
}

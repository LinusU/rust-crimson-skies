//! The load transaction and its bounded, cache-backed reads (F15-A, F15-B).
//!
//! Spec `specs/F15-asynchronous-asset-loading-and-private-cache.md`,
//! "Deliverable and interfaces": "A content load is a cancellable
//! transaction from Requested through Loading, Validating, Ready or
//! Failed. No world becomes interactive until its gameplay-critical
//! closure is ready."
//!
//! * [`LoadRequest`] is the typed input: the [`SessionGeneration`] the load
//!   belongs to, the [`LoadTarget`] being built (world and/or mission) and
//!   the [`LoadItem`]s of its closure, each declared critical or deferred
//!   with the measured work units progress is reported in.
//! * [`LoadTransaction`] carries the transaction through its states. Every
//!   in-flight read is an [`IoTicket`] stamped with a [`LoadIdentity`] —
//!   `(session generation, transaction serial)` — and every finished read
//!   is an [`IoCompletion`] carrying the same stamp. [`LoadTransaction::
//!   accept`] takes a completion only while the transaction that issued
//!   it is still loading; a completion of a cancelled, finished or foreign
//!   transaction is discarded with a verdict saying why (non-negotiable
//!   behavior 2: "discards stale session results. Never attach an old
//!   mission texture to a new mission because async completion arrived
//!   late").
//! * [`LoadTransaction::cancel`] is the switch-world path: it moves the
//!   transaction to `Cancelled` and flags every outstanding ticket's
//!   [`cs_assets::vfs::ReadCancel`], so an in-flight read stops at its
//!   next chunk boundary instead of being orphaned.
//! * [`ReadyBundle`] is the versioned handoff (non-negotiable behavior 4):
//!   it exists only for a `Ready` transaction, carries the closure hash of
//!   the loaded items, and [`ReadyBundle::attach`] spawns its
//!   [`LoadedItemBinding`] entities into a Bevy [`World`] only when the
//!   world's expected identity matches the bundle's — a stale bundle is
//!   refused and spawns nothing.
//!
//! F15-B adds the production path those contracts describe: [`LoadDriver`]
//! takes a transaction and a [`CacheStore`] and runs its items' bounded
//! reads — a verified cache hit when the store has one, otherwise a
//! bounded source read, the conversion and one atomic cache write — then
//! re-verifies every cache-delivered payload through
//! [`LoadTransaction::reject_validation`] before the transaction may go
//! `Ready`. What it deliberately does not do: run a schedule, spawn scene
//! entities or draw UI. F15-C wires the UI, the cancellation surface and
//! the simulation handoff; F15-D runs the cold/warm comparison on real
//! content.
//!
//! What the original engine did is not asserted anywhere here: the load
//! pipeline, the cache and its format are new-engine design.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;

use cs_assets::cache::bound::BudgetExceeded;
use cs_assets::cache::key::CacheKey;
use cs_assets::cache::store::{
    CACHE_IO_CHUNK, CacheLookup, CacheReadError, CacheStore, StoreError,
};
use cs_assets::install::{Sha256, sha256};
use cs_assets::vfs::{ReadCancel, ReadError, ReadProgress, SessionGeneration};
use cs_types::asset_id::{AssetKey, MissionScope, WorldGroup};
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

use crate::assets::{CanonicalPayload, ConversionError};

/// The domain separator of the [`ReadyBundle`] closure hash.
const CLOSURE_HASH_DOMAIN: &[u8] = b"cs-app/loading/closure/v1\0";

/// The next transaction serial handed out; serials start at 1.
static NEXT_SERIAL: AtomicU64 = AtomicU64::new(1);

/// The identity of one load transaction inside this process.
///
/// Serials are assigned by [`LoadTransaction::issue`] from a process-wide
/// counter, never supplied by the caller, so a retry or a successor world
/// can never share the serial of the transaction it replaced.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct LoadSerial(u64);

impl LoadSerial {
    /// The raw counter value, for reports.
    pub fn get(self) -> u64 {
        self.0
    }
}

impl fmt::Display for LoadSerial {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "load#{}", self.0)
    }
}

/// The full stamp an [`IoTicket`], [`IoCompletion`] or [`ReadyBundle`]
/// carries: which content session's generation the work belongs to, and
/// which transaction inside that session.
///
/// Both halves are needed. The session generation is what
/// `cs_assets::vfs` already stamps its reads with; the serial
/// distinguishes two transactions of the *same* session (a retry, a
/// sub-load) so one cannot accept the other's completions.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct LoadIdentity {
    /// The content session generation the load runs under.
    pub session: SessionGeneration,
    /// The transaction serial within the process.
    pub serial: LoadSerial,
}

impl fmt::Display for LoadIdentity {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} of {}", self.serial, self.session)
    }
}

/// What a load builds: the world and/or mission it is for.
///
/// Both fields may be empty (a shared-menu load) or both set (a mission
/// inside its world); the record exists so a report always names what the
/// load was for instead of leaving the consumer to guess.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadTarget {
    /// The world group being loaded, when the load is world-scoped.
    pub world: Option<WorldGroup>,
    /// The mission being loaded, when the load is mission-scoped.
    pub mission: Option<MissionScope>,
}

impl LoadTarget {
    /// A target with no world or mission scope (a shared load).
    pub fn shared() -> Self {
        Self {
            world: None,
            mission: None,
        }
    }

    /// A world-scoped target.
    pub fn world(world: WorldGroup) -> Self {
        Self {
            world: Some(world),
            mission: None,
        }
    }

    /// Adds the mission scope.
    pub fn with_mission(mut self, mission: MissionScope) -> Self {
        self.mission = Some(mission);
        self
    }
}

impl fmt::Display for LoadTarget {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match (&self.world, &self.mission) {
            (Some(world), Some(mission)) => write!(f, "world {world}, mission {mission}"),
            (Some(world), None) => write!(f, "world {world}"),
            (None, Some(mission)) => write!(f, "mission {mission}"),
            (None, None) => f.write_str("shared"),
        }
    }
}

/// Whether an item gates the world becoming interactive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Criticality {
    /// The world must not become interactive without it ("no world
    /// becomes interactive until its gameplay-critical closure is
    /// ready"). Its failure fails the whole load.
    GameplayCritical,
    /// The load may finish without it; its failure is recorded and
    /// reported, never hidden.
    Deferred,
}

impl Criticality {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::GameplayCritical => "gameplay_critical",
            Self::Deferred => "deferred",
        }
    }
}

/// Why a [`LoadItem`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum LoadItemError {
    /// Progress is measured in work units (non-negotiable behavior 5); an
    /// item declaring zero would make a total of zero and a meaningless
    /// ratio.
    ZeroWorkUnits,
}

impl fmt::Display for LoadItemError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::ZeroWorkUnits => write!(
                f,
                "a load item must declare at least one measured work unit"
            ),
        }
    }
}

impl std::error::Error for LoadItemError {}

/// One asset a load transaction must deliver.
///
/// `key` is what the VFS resolves and reads; `content` is the stable
/// catalog id the entity binding records; `derived` is the
/// [`CacheKey`] the converted form is stored under when the item goes
/// through the private cache — `None` for an item used without a derived
/// entry.
#[derive(Clone, Debug)]
pub struct LoadItem {
    /// The asset to resolve and read.
    pub key: AssetKey,
    /// The stable content id it stands for.
    pub content: ContentId,
    /// Whether the world may become interactive without it.
    pub criticality: Criticality,
    /// The measured work this item accounts for (for example its member's
    /// byte length). Progress sums these units, never a guessed percent.
    pub work_units: u64,
    /// The cache key of its derived form, when it is a cached derivation.
    pub derived: Option<CacheKey>,
}

impl LoadItem {
    /// Declares a load item.
    ///
    /// # Errors
    ///
    /// [`LoadItemError::ZeroWorkUnits`].
    pub fn new(
        key: AssetKey,
        content: ContentId,
        criticality: Criticality,
        work_units: u64,
    ) -> Result<Self, LoadItemError> {
        if work_units == 0 {
            return Err(LoadItemError::ZeroWorkUnits);
        }
        Ok(Self {
            key,
            content,
            criticality,
            work_units,
            derived: None,
        })
    }

    /// Records the cache key the item's derived form is stored under.
    pub fn with_derived(mut self, derived: CacheKey) -> Self {
        self.derived = Some(derived);
        self
    }
}

/// What a transaction is asked to deliver.
#[derive(Clone, Debug)]
pub struct LoadRequest {
    /// The content session generation the load runs under. A completion
    /// stamped with any other generation belongs to a session that has
    /// been replaced.
    pub session: SessionGeneration,
    /// What the load builds.
    pub target: LoadTarget,
    /// The closure of assets to deliver.
    pub items: Vec<LoadItem>,
}

/// The phase of a load transaction.
///
/// The lifecycle of spec F15: `Requested` through `Loading` and
/// `Validating` to `Ready` or `Failed`, with `Cancelled` reachable from
/// every non-terminal state. Terminal states accept no further
/// transitions and no further completions.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LoadState {
    /// Declared, not started.
    Requested,
    /// Reading: IO tickets may be issued and completions accepted.
    Loading,
    /// Every item is settled; the closure is being validated.
    Validating,
    /// Validated; the [`ReadyBundle`] exists.
    Ready,
    /// A gameplay-critical item failed or validation refused the closure.
    Failed,
    /// The load was cancelled; all outstanding work was detached.
    Cancelled,
}

impl LoadState {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Requested => "requested",
            Self::Loading => "loading",
            Self::Validating => "validating",
            Self::Ready => "ready",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    /// Whether this state ends the transaction.
    pub const fn is_terminal(self) -> bool {
        matches!(self, Self::Ready | Self::Failed | Self::Cancelled)
    }

    /// Whether the transition `self -> to` is legal.
    const fn permits(self, to: LoadState) -> bool {
        match self {
            Self::Requested => matches!(to, Self::Loading | Self::Cancelled),
            Self::Loading => matches!(to, Self::Validating | Self::Failed | Self::Cancelled),
            Self::Validating => matches!(to, Self::Ready | Self::Failed | Self::Cancelled),
            Self::Ready | Self::Failed | Self::Cancelled => false,
        }
    }
}

/// Why a state transition was refused.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TransitionError {
    /// The state the transaction was in.
    pub from: LoadState,
    /// The state that was requested.
    pub to: LoadState,
}

impl fmt::Display for TransitionError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "a load transaction cannot go from {} to {}",
            self.from.label(),
            self.to.label()
        )
    }
}

impl std::error::Error for TransitionError {}

/// What a transaction can do with a failure, surfaced to the user
/// instead of a stalled progress bar (non-negotiable behavior 5).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RecoveryPath {
    /// The read may be retried.
    Retry,
    /// The derived cache entry is invalid and is rebuilt.
    RebuildDerived,
    /// A dependency of the load is missing; the dependency is named in
    /// the failure's detail.
    MissingDependency,
    /// The load cannot be recovered; the session unwinds to its caller.
    Abort,
}

impl RecoveryPath {
    /// The stable label used in reports.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Retry => "retry",
            Self::RebuildDerived => "rebuild_derived",
            Self::MissingDependency => "missing_dependency",
            Self::Abort => "abort",
        }
    }
}

/// One failed item of a load: the missing or failed dependency and the
/// recovery path, as non-negotiable behavior 5 requires.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct LoadFailure {
    /// The item that failed.
    pub key: AssetKey,
    /// A stable lowercase code for the failure class (`io_fault`,
    /// `cancelled`, ...).
    pub code: &'static str,
    /// What happened, in words.
    pub detail: String,
    /// What the transaction can do about it.
    pub recovery: RecoveryPath,
}

impl fmt::Display for LoadFailure {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} failed ({code}): {detail} — recovery: {recovery}",
            self.key,
            code = self.code,
            detail = self.detail,
            recovery = self.recovery.label(),
        )
    }
}

/// What an issued read produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IoOutcome {
    /// The read delivered bytes, recorded by digest. The bytes themselves
    /// stay with the IO layer; the transaction tracks identity.
    Read {
        /// SHA-256 of the delivered payload.
        payload_sha256: ContentHash,
    },
    /// The read failed; the failure names the recovery path.
    Fault {
        /// A stable lowercase code for the failure class.
        code: &'static str,
        /// What happened, in words.
        detail: String,
        /// What the transaction can do about it.
        recovery: RecoveryPath,
    },
}

/// A handle on one in-flight read of a [`LoadTransaction`].
///
/// Issued by [`LoadTransaction::issue_io`], stamped with the transaction's
/// [`LoadIdentity`]. Completing it consumes the handle, so one read can
/// only ever report once. Its [`ReadCancel`] is shared with the
/// transaction: [`LoadTransaction::cancel`] flags it, so a read in flight
/// stops at its next chunk boundary (the same mechanism
/// `cs_assets::vfs::PendingRead` checks mid-read).
#[derive(Debug)]
pub struct IoTicket {
    identity: LoadIdentity,
    item: usize,
    cancel: ReadCancel,
}

impl IoTicket {
    /// The load this read belongs to.
    pub fn identity(&self) -> LoadIdentity {
        self.identity
    }

    /// The index of the item it reads.
    pub fn item(&self) -> usize {
        self.item
    }

    /// The shared cancel switch the IO worker observes.
    pub fn cancel_handle(&self) -> ReadCancel {
        self.cancel.clone()
    }

    /// Completes the read. The completion keeps the issuing identity, so a
    /// late completion is provably of the transaction that issued it —
    /// including a cancelled one.
    pub fn complete(self, outcome: IoOutcome) -> IoCompletion {
        IoCompletion {
            identity: self.identity,
            item: self.item,
            outcome,
        }
    }
}

/// A finished read, still stamped with the load that issued it.
///
/// Only [`LoadTransaction::accept`] turns a completion into an item of the
/// transaction; until then it is inert evidence of work.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IoCompletion {
    identity: LoadIdentity,
    item: usize,
    outcome: IoOutcome,
}

impl IoCompletion {
    /// The load that issued the read.
    pub fn identity(&self) -> LoadIdentity {
        self.identity
    }

    /// The index of the item it read.
    pub fn item(&self) -> usize {
        self.item
    }

    /// What the read produced.
    pub fn outcome(&self) -> &IoOutcome {
        &self.outcome
    }
}

/// What a transaction did with a completion.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompletionVerdict {
    /// The completion belonged to this transaction and its item is now
    /// loaded.
    Accepted,
    /// The completion belonged to this transaction and its item failed.
    ItemFailed,
    /// The completion was stamped by a different transaction — a stale
    /// session result. Discarded.
    Foreign {
        /// The load that actually issued the read.
        issued_by: LoadIdentity,
    },
    /// This transaction is not accepting completions in its current state
    /// (not yet loading, already validating, or terminal — including
    /// cancelled). The stale result is discarded.
    Discarded {
        /// The state the transaction is in.
        state: LoadState,
    },
}

impl CompletionVerdict {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Accepted => "accepted",
            Self::ItemFailed => "item_failed",
            Self::Foreign { .. } => "foreign",
            Self::Discarded { .. } => "discarded",
        }
    }
}

/// Why an [`IoTicket`] could not be issued.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum IssueError {
    /// Reads are issued only while the transaction is `Loading`.
    WrongState {
        /// The state the transaction is in.
        state: LoadState,
    },
    /// No item exists at the requested index.
    UnknownItem {
        /// The index asked for.
        index: usize,
    },
    /// The item already has a read issued or is already settled; issuing
    /// another would make two completions race for one slot.
    ItemBusy {
        /// The index asked for.
        index: usize,
    },
}

impl fmt::Display for IssueError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::WrongState { state } => write!(
                f,
                "IO can only be issued while the load is loading, not while it is {}",
                state.label()
            ),
            Self::UnknownItem { index } => {
                write!(f, "the load has no item {index}")
            }
            Self::ItemBusy { index } => {
                write!(
                    f,
                    "load item {index} already has a read in flight or is settled"
                )
            }
        }
    }
}

impl std::error::Error for IssueError {}

/// Measured progress of a load (non-negotiable behavior 5): work units,
/// not a guessed percentage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct LoadProgress {
    /// Work units of settled (loaded or failed) items.
    pub completed_units: u64,
    /// Work units of all items.
    pub total_units: u64,
    /// Items loaded.
    pub items_ready: usize,
    /// Items failed.
    pub items_failed: usize,
    /// Items in the request.
    pub items_total: usize,
}

impl LoadProgress {
    /// Whether every item is settled (loaded or failed).
    pub fn is_settled(&self) -> bool {
        self.items_ready + self.items_failed == self.items_total
    }
}

/// What [`LoadTransaction::cancel`] detached.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CancelReport {
    /// The load that ended.
    pub identity: LoadIdentity,
    /// How many in-flight reads were flagged cancelled.
    pub detached: usize,
}

/// One item's place inside the transaction.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ItemStatus {
    /// Declared, no read issued.
    Pending,
    /// A read is in flight.
    InFlight,
    /// The read delivered bytes, recorded by digest.
    Loaded { payload_sha256: ContentHash },
    /// The read failed.
    Failed,
}

/// A cancellable content-load transaction.
///
/// Constructed by [`LoadTransaction::issue`], which assigns the
/// process-unique [`LoadSerial`]. States advance only through the methods
/// below, each checking [`LoadState::permits`]; there is no path from a
/// terminal state back into the load, and no path for a foreign or stale
/// completion into the item records.
#[derive(Debug)]
pub struct LoadTransaction {
    identity: LoadIdentity,
    target: LoadTarget,
    items: Vec<LoadItem>,
    states: Vec<ItemStatus>,
    tickets: Vec<Option<ReadCancel>>,
    failures: Vec<LoadFailure>,
    state: LoadState,
    closure_hash: Option<ContentHash>,
}

impl LoadTransaction {
    /// Declares a load: state `Requested`, serial freshly assigned.
    pub fn issue(request: LoadRequest) -> Self {
        let count = request.items.len();
        Self {
            identity: LoadIdentity {
                session: request.session,
                serial: LoadSerial(NEXT_SERIAL.fetch_add(1, Ordering::Relaxed)),
            },
            target: request.target,
            items: request.items,
            states: vec![ItemStatus::Pending; count],
            tickets: vec![None; count],
            failures: Vec::new(),
            state: LoadState::Requested,
            closure_hash: None,
        }
    }

    /// This transaction's identity: `(session generation, serial)`.
    pub fn identity(&self) -> LoadIdentity {
        self.identity
    }

    /// What this load builds.
    pub fn target(&self) -> &LoadTarget {
        &self.target
    }

    /// The transaction's current state.
    pub fn state(&self) -> LoadState {
        self.state
    }

    /// The items of the load's closure.
    pub fn items(&self) -> &[LoadItem] {
        &self.items
    }

    /// The failures recorded so far, in arrival order.
    pub fn failures(&self) -> &[LoadFailure] {
        &self.failures
    }

    /// `Requested -> Loading`.
    ///
    /// An empty closure has nothing to read, so `begin` advances it
    /// `Loading -> Validating` in the same step — the same rule
    /// [`LoadTransaction::accept`] applies when the last item settles.
    /// Without it an empty load could never leave `Loading` and could
    /// only be cancelled.
    ///
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is not `Requested`.
    pub fn begin(&mut self) -> Result<(), TransitionError> {
        self.transition(LoadState::Loading)?;
        if self.all_settled() {
            self.transition(LoadState::Validating)?;
        }
        Ok(())
    }

    /// Issues a read for item `index`, moving it `Pending -> InFlight`.
    ///
    /// # Errors
    ///
    /// [`IssueError::WrongState`] unless the transaction is `Loading`,
    /// [`IssueError::UnknownItem`] for an out-of-range index and
    /// [`IssueError::ItemBusy`] when the item already has a read or is
    /// settled.
    pub fn issue_io(&mut self, index: usize) -> Result<IoTicket, IssueError> {
        if self.state != LoadState::Loading {
            return Err(IssueError::WrongState { state: self.state });
        }
        let Some(status) = self.states.get_mut(index) else {
            return Err(IssueError::UnknownItem { index });
        };
        if *status != ItemStatus::Pending {
            return Err(IssueError::ItemBusy { index });
        }
        let cancel = ReadCancel::default();
        *status = ItemStatus::InFlight;
        self.tickets[index] = Some(cancel.clone());
        Ok(IoTicket {
            identity: self.identity,
            item: index,
            cancel,
        })
    }

    /// Takes a finished read.
    ///
    /// The verdict records exactly what happened:
    ///
    /// * the identity is not this transaction's — `Foreign`, discarded;
    /// * the transaction is not `Loading` (not started, already
    ///   validating, failed, ready or **cancelled**) — `Discarded`;
    /// * the item's read already settled — `Discarded`;
    /// * a `Read` outcome — the item is `Loaded`, `Accepted`;
    /// * a `Fault` outcome — the item is `Failed` with its [`LoadFailure`]
    ///   recorded, `ItemFailed`. A failed `GameplayCritical` item ends the
    ///   transaction in `Failed`: a world without its critical closure is
    ///   not a world that may go interactive.
    ///
    /// When the last pending item settles the transaction advances
    /// `Loading -> Validating` itself.
    pub fn accept(&mut self, completion: IoCompletion) -> CompletionVerdict {
        if completion.identity != self.identity {
            return CompletionVerdict::Foreign {
                issued_by: completion.identity,
            };
        }
        if self.state != LoadState::Loading {
            return CompletionVerdict::Discarded { state: self.state };
        }
        let index = completion.item;
        let Some(status) = self.states.get_mut(index) else {
            return CompletionVerdict::Discarded { state: self.state };
        };
        if *status != ItemStatus::InFlight {
            return CompletionVerdict::Discarded { state: self.state };
        }
        self.tickets[index] = None;
        let verdict = match &completion.outcome {
            IoOutcome::Read { payload_sha256 } => {
                *status = ItemStatus::Loaded {
                    payload_sha256: *payload_sha256,
                };
                CompletionVerdict::Accepted
            }
            IoOutcome::Fault {
                code,
                detail,
                recovery,
            } => {
                *status = ItemStatus::Failed;
                let failure = LoadFailure {
                    key: self.items[index].key.clone(),
                    code,
                    detail: detail.clone(),
                    recovery: *recovery,
                };
                self.failures.push(failure);
                if self.items[index].criticality == Criticality::GameplayCritical {
                    // A legal transition: Loading -> Failed. A world
                    // missing critical content may never become
                    // interactive, so the whole load ends here — and its
                    // remaining in-flight reads are detached exactly as a
                    // cancel would flag them; `cancel` cannot be called on
                    // a terminal transaction to do it later.
                    self.state = LoadState::Failed;
                    self.detach_tickets();
                    return CompletionVerdict::ItemFailed;
                }
                CompletionVerdict::ItemFailed
            }
        };
        if self.all_settled() {
            // Every item is settled; the load leaves IO behind. Deferred
            // failures stay on the record; the world decides readiness at
            // validation.
            self.state = LoadState::Validating;
        }
        verdict
    }

    /// Cancels the load: every outstanding read is flagged cancelled and
    /// the transaction moves to `Cancelled`.
    ///
    /// The detached count reports how many in-flight reads were asked to
    /// stop; their completions, whenever they arrive, are `Discarded`.
    /// Cancelling is sticky — a second cancel, like every other
    /// transition out of a terminal state, is refused.
    ///
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is already terminal.
    pub fn cancel(&mut self) -> Result<CancelReport, TransitionError> {
        self.transition(LoadState::Cancelled)?;
        let detached = self.detach_tickets();
        Ok(CancelReport {
            identity: self.identity,
            detached,
        })
    }

    /// `Validating -> Ready`: computes the closure hash over the loaded
    /// items' `(content id, payload digest)` pairs, so the bundle's
    /// identity covers exactly what was delivered — equal loads hash
    /// equal, whatever order the reads finished in (spec F15 AC04).
    ///
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is not `Validating`.
    pub fn validate(&mut self) -> Result<(), TransitionError> {
        self.transition(LoadState::Ready)?;
        let mut pairs: Vec<(String, ContentHash)> = self
            .items
            .iter()
            .zip(&self.states)
            .filter_map(|(item, status)| match status {
                ItemStatus::Loaded { payload_sha256 } => {
                    Some((item.content.as_str().to_owned(), *payload_sha256))
                }
                _ => None,
            })
            .collect();
        // `ContentHash` carries no ordering; sort by the id string first
        // and the digest bytes second — either way the sequence is canonical.
        pairs.sort_by(|(a_id, a_hash), (b_id, b_hash)| {
            a_id.cmp(b_id)
                .then_with(|| a_hash.as_bytes().cmp(b_hash.as_bytes()))
        });
        let mut hasher = Sha256::new();
        hasher.update(CLOSURE_HASH_DOMAIN);
        hasher.update(&(pairs.len() as u64).to_le_bytes());
        for (content, digest) in &pairs {
            hasher.update(&(content.len() as u64).to_le_bytes());
            hasher.update(content.as_bytes());
            hasher.update(digest.as_bytes());
        }
        self.closure_hash = Some(hasher.finalize());
        Ok(())
    }

    /// `Validating -> Failed`: the checks that run during validation
    /// refuse the closure — for example an integrity re-verification of a
    /// delivered payload failing after the reads settled (F15-B's share
    /// of non-negotiable behavior 3). `failure` names the offending item
    /// and the recovery path, as non-negotiable behavior 5 requires, and
    /// joins the transaction's failure record.
    ///
    /// Without this method the transaction table's `Validating -> Failed`
    /// arc would be unreachable and validation could only approve. The
    /// refusal is strictly that arc: a `Loading` transaction fails
    /// through a gameplay-critical `Fault`, not through validation.
    ///
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is not `Validating`.
    pub fn reject_validation(&mut self, failure: LoadFailure) -> Result<(), TransitionError> {
        if self.state != LoadState::Validating {
            return Err(TransitionError {
                from: self.state,
                to: LoadState::Failed,
            });
        }
        self.transition(LoadState::Failed)?;
        self.failures.push(failure);
        Ok(())
    }

    /// Measured progress: settled work units over total work units.
    pub fn progress(&self) -> LoadProgress {
        let mut progress = LoadProgress {
            completed_units: 0,
            total_units: self.items.iter().map(|item| item.work_units).sum(),
            items_ready: 0,
            items_failed: 0,
            items_total: self.items.len(),
        };
        for (item, status) in self.items.iter().zip(&self.states) {
            match status {
                ItemStatus::Loaded { .. } => {
                    progress.items_ready += 1;
                    progress.completed_units += item.work_units;
                }
                ItemStatus::Failed => {
                    progress.items_failed += 1;
                    progress.completed_units += item.work_units;
                }
                ItemStatus::Pending | ItemStatus::InFlight => {}
            }
        }
        progress
    }

    /// Whether every gameplay-critical item is loaded — the gate of "no
    /// world becomes interactive until its gameplay-critical closure is
    /// ready".
    pub fn critical_closure_ready(&self) -> bool {
        self.items
            .iter()
            .zip(&self.states)
            .filter(|(item, _)| item.criticality == Criticality::GameplayCritical)
            .all(|(_, status)| matches!(status, ItemStatus::Loaded { .. }))
    }

    /// Whether the world this load builds may become interactive: the
    /// transaction is `Ready` *and* the gameplay-critical closure is
    /// loaded. Both halves are required — a `Ready` state with a missing
    /// critical item cannot exist, because such a failure ends the
    /// transaction in `Failed` first.
    pub fn is_world_interactive(&self) -> bool {
        self.state == LoadState::Ready && self.critical_closure_ready()
    }

    /// The versioned ready bundle, only for a `Ready` transaction.
    ///
    /// # Errors
    ///
    /// [`HandoffError::NotReady`] naming the actual state — including
    /// `Cancelled`, which is how a switched-away load answers for its
    /// bundle.
    pub fn ready_bundle(&self) -> Result<ReadyBundle, HandoffError> {
        if self.state != LoadState::Ready {
            return Err(HandoffError::NotReady { state: self.state });
        }
        let items = self
            .items
            .iter()
            .zip(&self.states)
            .filter_map(|(item, status)| match status {
                ItemStatus::Loaded { payload_sha256 } => Some(ReadyItem {
                    content: item.content.clone(),
                    key: item.key.clone(),
                    criticality: item.criticality,
                    payload_sha256: *payload_sha256,
                    derived: item.derived.clone(),
                }),
                _ => None,
            })
            .collect();
        let omitted = self
            .items
            .iter()
            .zip(&self.states)
            .filter(|(_, status)| matches!(status, ItemStatus::Failed))
            .map(|(item, _)| item.key.clone())
            .collect();
        Ok(ReadyBundle {
            identity: self.identity,
            closure_hash: self
                .closure_hash
                .expect("a Ready transaction computed its closure hash"),
            items,
            omitted,
        })
    }

    /// Whether every item is settled (loaded or failed): the auto-advance
    /// rule `begin` applies to an empty closure and `accept` applies when
    /// the last completion arrives.
    fn all_settled(&self) -> bool {
        self.states
            .iter()
            .all(|status| matches!(status, ItemStatus::Loaded { .. } | ItemStatus::Failed))
    }

    /// Flags every outstanding ticket's cancel switch and reports how
    /// many were in flight — the detach half of `cancel` and of a
    /// critical failure's transition to `Failed`.
    fn detach_tickets(&mut self) -> usize {
        let mut detached = 0;
        for ticket in self.tickets.iter_mut().flatten() {
            ticket.cancel();
            detached += 1;
        }
        detached
    }

    /// Applies one transition, refusing what [`LoadState::permits`] does
    /// not allow.
    fn transition(&mut self, to: LoadState) -> Result<(), TransitionError> {
        if !self.state.permits(to) {
            return Err(TransitionError {
                from: self.state,
                to,
            });
        }
        self.state = to;
        Ok(())
    }
}

/// One delivered item of a [`ReadyBundle`].
#[derive(Clone, Debug)]
pub struct ReadyItem {
    /// The stable content id the binding records.
    pub content: ContentId,
    /// The asset key the item was read under.
    pub key: AssetKey,
    /// Whether the item gated interactivity.
    pub criticality: Criticality,
    /// The digest of the delivered payload.
    pub payload_sha256: ContentHash,
    /// The cache key of the item's derived form, when it has one.
    pub derived: Option<CacheKey>,
}

/// Why a [`ReadyBundle`] could not be produced or attached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HandoffError {
    /// The transaction has not reached `Ready`, so no bundle exists. The
    /// state is named — a cancelled load says `cancelled`, not a generic
    /// failure.
    NotReady {
        /// The state the transaction is in.
        state: LoadState,
    },
    /// The bundle's identity is not the identity the world expects: it
    /// belongs to a load that has been replaced. Nothing is spawned.
    Foreign {
        /// The load the bundle belongs to.
        bundle: LoadIdentity,
        /// The load the world expects.
        expected: LoadIdentity,
    },
}

impl fmt::Display for HandoffError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotReady { state } => write!(
                f,
                "no ready bundle exists while the load is {}",
                state.label()
            ),
            Self::Foreign { bundle, expected } => write!(
                f,
                "bundle {bundle} cannot attach to a world expecting {expected}: \
                 the load that produced it has been replaced"
            ),
        }
    }
}

impl std::error::Error for HandoffError {}

/// Component: marks an entity as delivered by one load transaction.
///
/// The binding records the full [`LoadIdentity`] — session generation and
/// serial — so a sweep after a world switch identifies old-session
/// entities by the mismatch rather than by surviving pointers (the same
/// pattern as `crate::scene::SceneNodeBinding`, and F15 non-negotiable
/// behavior 2's guarantee that stale session results never attach).
#[derive(Component, Clone, Debug, PartialEq)]
pub struct LoadedItemBinding {
    /// The load that delivered this entity's content.
    pub load: LoadIdentity,
    /// The stable content id the entity presents.
    pub content: ContentId,
    /// The asset key the content was loaded under.
    pub key: AssetKey,
}

/// The versioned output of a finished load: the simulation receives this
/// at a controlled boundary, never mid-tick callbacks (non-negotiable
/// behavior 4).
///
/// A bundle exists only for a `Ready` transaction and only through
/// [`LoadTransaction::ready_bundle`]. Its version is the full
/// [`LoadIdentity`] plus the closure hash of the delivered items, so a
/// consumer can always say *which* load and *which* closure it is looking
/// at. [`ReadyBundle::attach`] is the controlled boundary: it checks the
/// world's expected identity before spawning and refuses a bundle of a
/// replaced load, spawning nothing.
#[derive(Clone, Debug)]
pub struct ReadyBundle {
    identity: LoadIdentity,
    closure_hash: ContentHash,
    items: Vec<ReadyItem>,
    /// Keys of deferred items that failed; reported, never hidden.
    omitted: Vec<AssetKey>,
}

impl ReadyBundle {
    /// The load that produced this bundle.
    pub fn identity(&self) -> LoadIdentity {
        self.identity
    }

    /// The hash of the delivered closure: equal delivered content hashes
    /// equal, independent of completion order or cache warmth.
    pub fn closure_hash(&self) -> ContentHash {
        self.closure_hash
    }

    /// The delivered items.
    pub fn items(&self) -> &[ReadyItem] {
        &self.items
    }

    /// The keys of deferred items that failed and are not in the bundle.
    pub fn omitted(&self) -> &[AssetKey] {
        &self.omitted
    }

    /// Spawns this bundle's entities into `world`, each carrying a
    /// [`LoadedItemBinding`] of this bundle's identity.
    ///
    /// This is the controlled boundary of non-negotiable behavior 4: the
    /// caller picks when it runs, and the identity check runs before the
    /// first spawn. A bundle whose [`LoadIdentity`] is not `expected`
    /// belongs to a replaced load; [`HandoffError::Foreign`] is returned
    /// and the world is left untouched — no old entities appear.
    ///
    /// # Errors
    ///
    /// [`HandoffError::Foreign`].
    pub fn attach(
        &self,
        world: &mut World,
        expected: LoadIdentity,
    ) -> Result<Vec<Entity>, HandoffError> {
        if self.identity != expected {
            return Err(HandoffError::Foreign {
                bundle: self.identity,
                expected,
            });
        }
        let entities = self
            .items
            .iter()
            .map(|item| {
                world
                    .spawn(LoadedItemBinding {
                        load: self.identity,
                        content: item.content.clone(),
                        key: item.key.clone(),
                    })
                    .id()
            })
            .collect();
        Ok(entities)
    }
}

// --- F15-B: the bounded read path and the atomic store ------------------

/// How much work one bounded step has done, in measured units.
///
/// Two measurements, never a guessed percentage (non-negotiable behavior
/// 5): `load` is the transaction's own progress over the whole closure,
/// and `io` is the bounded chunk step in flight — a cache read, a source
/// read, or the staged cache write.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StepProgress {
    /// The load's progress over all its items.
    pub load: LoadProgress,
    /// The bounded step in flight: bytes done of bytes total.
    pub io: ReadProgress,
}

impl StepProgress {
    /// A step that has not started: the item's work is still outstanding.
    pub fn idle(load: LoadProgress) -> Self {
        Self {
            load,
            io: ReadProgress { read: 0, total: 0 },
        }
    }
}

/// Why a stored entry was not served and the item was rebuilt instead.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RebuildCause {
    /// The store holds no entry for the key: a cold cache, or an entry that
    /// has not been written yet.
    NoEntry,
    /// An entry was stored but refused: it was a partially written entry,
    /// it belonged to another derivation, or its bytes disagreed with its
    /// own header. `code` is the store's stable code and `detail` says what
    /// was wrong. Spec F15 behavior 3 — rebuilt, never served.
    Refused {
        /// The stable code of the refusal.
        code: &'static str,
        /// What was wrong, in words.
        detail: String,
    },
    /// The store could not be read at all. The item is rebuilt from its
    /// sources and nothing is cached: a cache that cannot be read is a
    /// performance problem, not a reason to fail a load that can read its
    /// sources (behavior 1).
    StoreUnavailable {
        /// The store's stable error code.
        code: &'static str,
        /// What happened, in words.
        detail: String,
    },
    /// The item declares no derived cache key: it is used without a stored
    /// entry at all.
    NotCacheable,
}

impl RebuildCause {
    /// The stable label used in reports.
    pub fn label(&self) -> &'static str {
        match self {
            Self::NoEntry => "no_entry",
            Self::Refused { .. } => "refused",
            Self::StoreUnavailable { .. } => "store_unavailable",
            Self::NotCacheable => "not_cacheable",
        }
    }
}

/// Why a rebuilt derived asset was not written to the cache.
///
/// None of these fails the load: the derived bytes are already in hand and
/// the cache is a performance optimization, never the authoritative data
/// source (spec F15 non-negotiable behavior 1).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum UncachedReason {
    /// The store's budget would not admit the entry. The bound is
    /// reported, not silently widened.
    Budget(BudgetExceeded),
    /// The store itself failed while the entry was being written. The
    /// payload is still delivered; the derived form is simply not cached.
    Store {
        /// The store's stable error code.
        code: &'static str,
        /// What happened, in words.
        detail: String,
    },
}

/// What one item's bounded read produced.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemRead {
    /// The item's derived bytes were read from a verified cache entry.
    /// The source was not read and nothing was written.
    CacheHit {
        /// SHA-256 of the verified derived payload.
        payload_sha256: ContentHash,
    },
    /// The store had no usable entry, so the item was rebuilt from its
    /// sources and its derived form was committed to the store.
    Rebuilt {
        /// SHA-256 of the freshly derived payload.
        payload_sha256: ContentHash,
        /// Why the cache could not serve it.
        cause: RebuildCause,
    },
    /// The item was rebuilt and delivered, but the derived form was not
    /// cached. The load is unaffected.
    Uncached {
        /// SHA-256 of the freshly derived payload.
        payload_sha256: ContentHash,
        /// Why the cache could not serve it.
        cause: RebuildCause,
        /// Why it was not written either.
        reason: UncachedReason,
    },
    /// The item failed. A gameplay-critical failure ends the transaction;
    /// a deferred one is recorded and the load continues. Either way the
    /// [`LoadFailure`] names the missing dependency and the recovery path.
    Failed {
        /// What failed and how to recover from it.
        failure: LoadFailure,
    },
}

impl ItemRead {
    /// The digest of the payload this item delivered, when it delivered
    /// one. A failed or refused item delivered nothing.
    pub fn payload_sha256(&self) -> Option<ContentHash> {
        match self {
            Self::CacheHit { payload_sha256 }
            | Self::Rebuilt { payload_sha256, .. }
            | Self::Uncached { payload_sha256, .. } => Some(*payload_sha256),
            Self::Failed { .. } => None,
        }
    }

    /// Whether the item's bytes came out of the cache rather than its
    /// sources.
    pub fn is_cache_hit(&self) -> bool {
        matches!(self, Self::CacheHit { .. })
    }
}

/// Why the driver could not run an item or finish the load.
///
/// Every variant is reported rather than logged away: a load that cannot
/// proceed names the item and its recovery path (behavior 5).
#[derive(Debug)]
pub enum DriverError {
    /// The item index is not one of the transaction's items, or it is
    /// already settled. The transaction is untouched.
    NotAccepted {
        /// The item that was asked for.
        index: usize,
        /// The state the transaction is in.
        state: LoadState,
        /// Why the read could not be issued.
        error: IssueError,
    },
    /// The store could not be read while a delivered entry was being
    /// re-verified. The load keeps whatever state it had — the entry is
    /// not proved good, and it is not proved bad either — and the item is
    /// named so the caller can decide whether to retry.
    Store {
        /// The item the store was working for.
        index: usize,
        /// The store's refusal.
        error: StoreError,
    },
    /// Validation was refused: a cache entry the load served no longer
    /// hashes to what was delivered. The transaction has been failed with
    /// a `RebuildDerived` recovery path; campaign state cannot be changed
    /// by cache corruption.
    Integrity {
        /// The offending item.
        index: usize,
        /// What was delivered.
        delivered: ContentHash,
        /// What the store holds now, when it is readable at all.
        found: Option<ContentHash>,
        /// Why it no longer matches.
        detail: String,
        /// The transaction's state: `Failed` when the refusal ended the
        /// load, whatever else it had already reached.
        state: LoadState,
    },
    /// The transaction refused the transition itself.
    Transition(TransitionError),
}

impl fmt::Display for DriverError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotAccepted {
                index,
                state,
                error,
            } => write!(
                f,
                "load item {index} could not be read: {error} (the load is {})",
                state.label()
            ),
            Self::Store { index, error } => {
                write!(f, "load item {index} could not use the cache: {error}")
            }
            Self::Integrity {
                index,
                delivered,
                found,
                detail,
                state,
            } => match found {
                Some(found) => write!(
                    f,
                    "load item {index} was delivered from a cache entry that now holds \
                     {found}, not the delivered {delivered}: {detail} (the load is {})",
                    state.label()
                ),
                None => write!(
                    f,
                    "load item {index} was delivered from a cache entry that is no longer \
                     readable: {detail} (the load is {})",
                    state.label()
                ),
            },
            Self::Transition(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for DriverError {}

/// The bounded, cache-backed read path of one load (F15-B).
///
/// The transaction of F15-A says *what* a load must deliver and when a
/// result may be accepted. This is the machinery that produces the
/// results: for each item it asks the private store first, reads a
/// verified entry in cancellable chunks when one exists, and otherwise
/// reads the source, converts it and commits the derived form with one
/// atomic write. The transaction is the only thing that settles an item,
/// so its identity and state rules are unchanged by anything here.
///
/// Three properties the sheet names are structural in this type:
///
/// * a *partial* write never becomes an entry — the driver only ever hands
///   the store a write it sealed, and the store publishes it with one
///   rename ([`CacheStore::commit`]);
/// * a *stale* read is discarded, not served — a stored entry that fails
///   verification is dropped and rebuilt, and an entry the load delivered
///   is re-verified before the transaction may go `Ready`
///   ([`LoadDriver::validate_delivered`]);
/// * a *cancelled* load detaches rather than orphans — [`LoadDriver::cancel`]
///   flags the driver's own switch, which the bounded steps observe at
///   their next chunk boundary, and the transaction's tickets besides.
#[derive(Debug)]
pub struct LoadDriver {
    transaction: LoadTransaction,
    store: CacheStore,
    cancel: ReadCancel,
    /// Per item: the digest it delivered and whether it came from the
    /// store, which is what the validation re-verification walks.
    delivered: Vec<DeliveredItem>,
    /// Per item: whether this driver has settled it. A driver refuses to
    /// read an item twice, so a retry is a new transaction rather than a
    /// second read racing for one slot.
    settled: Vec<bool>,
}

/// What the driver delivered for one item.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct DeliveredItem {
    payload_sha256: ContentHash,
    from_cache: bool,
}

impl LoadDriver {
    /// Takes a transaction and the store its items' derived forms live in.
    ///
    /// The transaction is not started; the caller decides when, so a load
    /// can be declared and then scheduled (F15-C owns the schedule).
    pub fn new(transaction: LoadTransaction, store: CacheStore) -> Self {
        let items = transaction.items().len();
        Self {
            transaction,
            store,
            cancel: ReadCancel::default(),
            delivered: vec![
                DeliveredItem {
                    payload_sha256: ContentHash::from_bytes([0; 32]),
                    from_cache: false,
                };
                items
            ],
            settled: vec![false; items],
        }
    }

    /// The transaction being driven.
    pub const fn transaction(&self) -> &LoadTransaction {
        &self.transaction
    }

    /// The transaction being driven, mutably, for the caller that starts
    /// and inspects it. Settling an item still goes through
    /// [`LoadDriver::load_item`], which is the only path that records what
    /// was delivered.
    pub const fn transaction_mut(&mut self) -> &mut LoadTransaction {
        &mut self.transaction
    }

    /// The store the items' derived forms live in.
    pub const fn store(&self) -> &CacheStore {
        &self.store
    }

    /// Splits the driver back into the transaction it drives and the store
    /// it was reading, so a caller can hand the ready transaction to the
    /// simulation boundary and keep the store for the next load.
    pub fn into_parts(self) -> (LoadTransaction, CacheStore) {
        (self.transaction, self.store)
    }

    /// The driver's cancel switch, cloneable onto any thread.
    ///
    /// It is observed at every bounded step's chunk boundary, so throwing
    /// it stops the current cache read or staged write promptly and makes
    /// the next [`LoadDriver::load_item`] refuse to start.
    pub fn cancel_handle(&self) -> ReadCancel {
        self.cancel.clone()
    }

    /// Whether the driver has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.is_cancelled()
    }

    /// Cancels the load: the driver's switch is thrown, so the bounded step
    /// in flight stops at its next chunk boundary, and the transaction is
    /// cancelled, so every outstanding ticket is flagged and any late
    /// completion is discarded (non-negotiable behavior 2).
    ///
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is already terminal.
    pub fn cancel(&mut self) -> Result<CancelReport, TransitionError> {
        self.cancel.cancel();
        self.transaction.cancel()
    }

    /// Runs one item end to end: store lookup, bounded read, or source
    /// read plus conversion plus one atomic cache write.
    ///
    /// `read_source` performs the bounded source read (a
    /// `vfs::PendingRead` completes exactly that way) and `convert` turns
    /// the canonical bytes into the derived form. Both are only called on
    /// the path that needs them, so a warm load touches neither.
    ///
    /// The item is settled through the transaction in every case, so the
    /// load's own state machine — not this method — decides whether a
    /// critical failure ends it.
    ///
    /// # Errors
    ///
    /// [`DriverError::NotAccepted`] when the index is not the
    /// transaction's, is already settled, or the load is not `Loading`;
    /// [`DriverError::Store`] when the store cannot be written and the
    /// transaction has already moved on.
    pub fn load_item(
        &mut self,
        index: usize,
        progress: &mut dyn FnMut(StepProgress),
        read_source: impl FnOnce() -> Result<Vec<u8>, ReadError>,
        convert: impl FnOnce(&CanonicalPayload) -> Result<Vec<u8>, ConversionError>,
    ) -> Result<ItemRead, DriverError> {
        if index >= self.transaction.items().len() {
            return Err(DriverError::NotAccepted {
                index,
                state: self.transaction.state(),
                error: IssueError::UnknownItem { index },
            });
        }
        if self.settled[index] {
            return Err(DriverError::NotAccepted {
                index,
                state: self.transaction.state(),
                error: IssueError::ItemBusy { index },
            });
        }
        let ticket =
            self.transaction
                .issue_io(index)
                .map_err(|error| DriverError::NotAccepted {
                    index,
                    state: self.transaction.state(),
                    error,
                })?;
        let snapshot = self.transaction.progress();
        let mut report = |step: StepProgress| progress(step);
        if self.cancel.is_cancelled() {
            return Ok(self.stop_cancelled(
                index,
                ticket,
                "the load was cancelled before the read began",
            ));
        }
        let item = self.transaction.items()[index].clone();

        // The store is asked first: a verified entry means the source is
        // never read. An entry that is refused is dropped and rebuilt —
        // spec F15 behavior 3, never served. A store that cannot be read
        // is a performance problem, not a load failure, so it is recorded
        // and the rebuild continues.
        let mut store_fault = None;
        let cause = match item.derived.as_ref() {
            None => Some(RebuildCause::NotCacheable),
            Some(key) => match self.store.begin_read(key) {
                Ok(CacheLookup::Miss) => None,
                Ok(CacheLookup::Rebuild { reason }) => {
                    let _ = self.store.discard(key);
                    Some(RebuildCause::Refused {
                        code: reason.code(),
                        detail: reason.to_string(),
                    })
                }
                Ok(CacheLookup::Hit(pending)) => {
                    let switch = pending.cancel_handle();
                    let outcome = pending.complete_with(|io| {
                        // The load's own switch reaches the bounded read.
                        if self.cancel.is_cancelled() {
                            switch.cancel();
                        }
                        report(StepProgress { load: snapshot, io });
                    });
                    match outcome {
                        Ok(entry) => {
                            let payload_sha256 = sha256(entry.payload());
                            return Ok(self.settle_read(
                                ticket,
                                index,
                                payload_sha256,
                                true,
                                ItemRead::CacheHit { payload_sha256 },
                            ));
                        }
                        Err(CacheReadError::Cancelled { .. }) => {
                            return Ok(self.stop_cancelled(
                                index,
                                ticket,
                                "the bounded cache read was cancelled",
                            ));
                        }
                        Err(error) => {
                            let _ = self.store.discard(key);
                            Some(RebuildCause::Refused {
                                code: error.code(),
                                detail: error.to_string(),
                            })
                        }
                    }
                }
                Err(error) => {
                    store_fault = Some(UncachedReason::Store {
                        code: error.code(),
                        detail: error.to_string(),
                    });
                    Some(RebuildCause::StoreUnavailable {
                        code: error.code(),
                        detail: error.to_string(),
                    })
                }
            },
        };

        // Rebuild from the sources. The conversion is what produces the
        // derived form; the source bytes are its canonical input.
        let cause = cause.unwrap_or(RebuildCause::NoEntry);
        report(StepProgress::idle(snapshot));
        let canonical = match read_source() {
            Ok(bytes) => CanonicalPayload::new(item.content.kind(), bytes),
            Err(error) => {
                return Ok(self.settle_failure(
                    index,
                    ticket,
                    self.failure(index, "source_read", error.to_string(), RecoveryPath::Retry),
                ));
            }
        };
        let derived = match convert(&canonical) {
            Ok(bytes) => bytes,
            Err(error) => {
                return Ok(self.settle_failure(
                    index,
                    ticket,
                    self.failure(index, "conversion", error.to_string(), RecoveryPath::Abort),
                ));
            }
        };
        let payload_sha256 = sha256(&derived);
        // Publish the derived form, if the item has one. Neither a budget
        // refusal nor a store failure reaches the load's verdict: the
        // bytes are already in hand and a cache is an optimization.
        let published = match item.derived.as_ref() {
            None => Ok(()),
            Some(key) => {
                let mut report_io = |io: ReadProgress| report(StepProgress { load: snapshot, io });
                self.commit_derived(key, &derived, &mut report_io)
            }
        };
        let outcome = match (published, store_fault) {
            (Ok(()), None) => ItemRead::Rebuilt {
                payload_sha256,
                cause,
            },
            (published, fault) => ItemRead::Uncached {
                payload_sha256,
                cause,
                reason: fault.unwrap_or_else(|| match published {
                    Ok(()) => UncachedReason::Store {
                        code: "cache_write",
                        detail: "the derived form was not published".to_owned(),
                    },
                    Err(reason) => reason,
                }),
            },
        };
        Ok(self.settle_read(ticket, index, payload_sha256, false, outcome))
    }

    /// Re-verifies everything the load delivered from the store, then
    /// `Validating -> Ready`.
    ///
    /// This is the `Validating -> Failed` arc F15-A left open for this
    /// stage: a cache entry that no longer hashes to what the load
    /// delivered — because the store was tampered with, truncated, or
    /// replaced between the read and validation — is a
    /// [`LoadDriverError::Integrity`], the transaction is failed with a
    /// `RebuildDerived` recovery path, and the world does not go
    /// interactive. Cache corruption cannot change campaign state, and it
    /// cannot make a world interactive either.
    ///
    /// # Errors
    ///
    /// [`DriverError::Integrity`] for an entry that no longer matches,
    /// [`DriverError::Store`] for a store that cannot be read, and
    /// [`DriverError::Transition`] when the transaction is not
    /// `Validating`.
    pub fn validate_delivered(&mut self) -> Result<(), DriverError> {
        for index in 0..self.transaction.items().len() {
            let delivered = self.delivered[index];
            if !delivered.from_cache || !self.settled[index] {
                continue;
            }
            let Some(key) = self.transaction.items()[index].derived.clone() else {
                continue;
            };
            let found = match self.store.begin_read(&key) {
                Ok(CacheLookup::Hit(pending)) => match pending.complete() {
                    Ok(entry) => Some(sha256(entry.payload())),
                    Err(error) => {
                        return Err(self.refuse(
                            index,
                            delivered.payload_sha256,
                            None,
                            error.to_string(),
                        ));
                    }
                },
                Ok(CacheLookup::Miss) => {
                    return Err(self.refuse(
                        index,
                        delivered.payload_sha256,
                        None,
                        "the entry is no longer stored".to_owned(),
                    ));
                }
                Ok(CacheLookup::Rebuild { reason }) => {
                    return Err(self.refuse(
                        index,
                        delivered.payload_sha256,
                        None,
                        reason.to_string(),
                    ));
                }
                Err(error) => {
                    return Err(DriverError::Store { index, error });
                }
            };
            if let Some(found) = found
                && found != delivered.payload_sha256
            {
                return Err(self.refuse(
                    index,
                    delivered.payload_sha256,
                    Some(found),
                    "the stored entry no longer hashes to the delivered payload".to_owned(),
                ));
            }
        }
        self.transaction.validate().map_err(DriverError::Transition)
    }

    /// The one place an item is settled with a read, so the delivery
    /// record and the transaction can never disagree.
    fn settle_read(
        &mut self,
        ticket: IoTicket,
        index: usize,
        payload_sha256: ContentHash,
        from_cache: bool,
        outcome: ItemRead,
    ) -> ItemRead {
        let verdict = self
            .transaction
            .accept(ticket.complete(IoOutcome::Read { payload_sha256 }));
        // `Discarded` means the load moved on while the read was in
        // flight; `ItemFailed` cannot happen for a `Read` outcome.
        self.settled[index] = matches!(verdict, CompletionVerdict::Accepted);
        if self.settled[index] {
            self.delivered[index] = DeliveredItem {
                payload_sha256,
                from_cache,
            };
        }
        outcome
    }

    /// Settles one item with a fault, letting the transaction decide
    /// whether a critical failure ends the load.
    fn settle_failure(&mut self, index: usize, ticket: IoTicket, failure: LoadFailure) -> ItemRead {
        self.transaction.accept(ticket.complete(IoOutcome::Fault {
            code: failure.code,
            detail: failure.detail.clone(),
            recovery: failure.recovery,
        }));
        // The item's slot is settled either way: a fault that ended the
        // whole load is no more in flight than one that did not, and a
        // caller must not be able to read it a second time.
        self.settled[index] = true;
        ItemRead::Failed { failure }
    }

    /// A cancelled bounded step ends the *load*, not just the item.
    ///
    /// Only the load-wide switch cancels a read, so a cancellation means
    /// the caller asked for the load to stop: the transaction is cancelled
    /// (so its state says `Cancelled`, not `Failed`, and every other
    /// outstanding ticket is flagged) and the cancellation is reported to
    /// the caller as a retryable failure. Cancelling is what a world switch
    /// does, and a switched-away load must not be reported as broken
    /// content.
    fn stop_cancelled(&mut self, index: usize, ticket: IoTicket, detail: &str) -> ItemRead {
        let failure = self.failure(
            index,
            "read_cancelled",
            detail.to_owned(),
            RecoveryPath::Retry,
        );
        // The ticket is consumed by the cancellation itself: `cancel`
        // flagged it and the transaction no longer accepts completions, so
        // settling it would only produce a discarded record.
        drop(ticket);
        let _ = self.transaction.cancel();
        self.settled[index] = true;
        ItemRead::Failed { failure }
    }

    /// Builds the failure record for `index`, naming the item's key and
    /// the recovery path behavior 5 requires.
    fn failure(
        &self,
        index: usize,
        code: &'static str,
        detail: String,
        recovery: RecoveryPath,
    ) -> LoadFailure {
        LoadFailure {
            key: self.transaction.items()[index].key.clone(),
            code,
            detail,
            recovery,
        }
    }

    /// Fails the validating load because a delivered entry no longer
    /// matches, and reports why.
    fn refuse(
        &mut self,
        index: usize,
        delivered: ContentHash,
        found: Option<ContentHash>,
        detail: String,
    ) -> DriverError {
        let failure = self.failure(
            index,
            "cache_integrity",
            format!("{detail} (delivered {delivered})"),
            RecoveryPath::RebuildDerived,
        );
        // The refusal is what fails the load. A transaction that has moved
        // on cannot be failed here, and the reported state says which.
        let _ = self.transaction.reject_validation(failure);
        DriverError::Integrity {
            index,
            delivered,
            found,
            detail,
            state: self.transaction.state(),
        }
    }

    /// Publishes one derived payload atomically, in bounded chunks.
    ///
    /// # Errors
    ///
    /// [`UncachedReason`] when the derived form could not be stored. A
    /// budget refusal is not an error: it is the bound doing its job, and
    /// a cache that cannot hold an entry is still a working cache.
    fn commit_derived(
        &mut self,
        key: &CacheKey,
        derived: &[u8],
        report: &mut dyn FnMut(ReadProgress),
    ) -> Result<(), UncachedReason> {
        let mut write = self
            .store
            .begin_write(key, derived.len() as u64)
            .map_err(|error| uncached_from_store(&error))?;
        let chunk = usize::try_from(CACHE_IO_CHUNK).expect("a 1 MiB chunk fits in memory");
        for start in (0..derived.len()).step_by(chunk) {
            let end = (start + chunk).min(derived.len());
            let step = write
                .append(&derived[start..end])
                .map_err(|error| uncached_from_store(&error))?;
            report(step);
        }
        write.seal().map_err(|error| uncached_from_store(&error))?;
        self.store
            .commit(write)
            .map(|_| ())
            .map_err(|error| match error {
                StoreError::Budget(exceeded) => UncachedReason::Budget(exceeded),
                other => uncached_from_store(&other),
            })
    }
}

/// Turns a store refusal into "delivered, not cached" — with the store's
/// own code and message, never a swallowed error.
fn uncached_from_store(error: &StoreError) -> UncachedReason {
    UncachedReason::Store {
        code: error.code(),
        detail: error.to_string(),
    }
}

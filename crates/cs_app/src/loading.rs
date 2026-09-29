//! The load-transaction contract (F15-A).
//!
//! Spec `specs/F15-asynchronous-asset-loading-and-private-cache.md`,
//! "Deliverable and interfaces": "A content load is a cancellable
//! transaction from Requested through Loading, Validating, Ready or
//! Failed. No world becomes interactive until its gameplay-critical
//! closure is ready." This module is that transaction as typed records
//! and transition rules — not the async machinery, which is F15-B.
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
//! What this stage deliberately does not do: perform real IO, write the
//! cache, or run a schedule. F15-B implements bounded reads and the atomic
//! store; F15-C wires the UI and the simulation boundary.

use std::fmt;
use std::sync::atomic::{AtomicU64, Ordering};

use bevy::ecs::component::Component;
use bevy::ecs::entity::Entity;
use bevy::ecs::world::World;

use cs_assets::cache::CacheKey;
use cs_assets::install::Sha256;
use cs_assets::vfs::{ReadCancel, SessionGeneration};
use cs_types::asset_id::{AssetKey, MissionScope, WorldGroup};
use cs_types::content::ContentId;
use cs_types::evidence::ContentHash;

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
    /// # Errors
    ///
    /// [`TransitionError`] when the transaction is not `Requested`.
    pub fn begin(&mut self) -> Result<(), TransitionError> {
        self.transition(LoadState::Loading)
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
                    // interactive, so the whole load ends here.
                    self.state = LoadState::Failed;
                    return CompletionVerdict::ItemFailed;
                }
                CompletionVerdict::ItemFailed
            }
        };
        if self
            .states
            .iter()
            .all(|status| matches!(status, ItemStatus::Loaded { .. } | ItemStatus::Failed))
        {
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
        let mut detached = 0;
        for ticket in self.tickets.iter_mut().flatten() {
            ticket.cancel();
            detached += 1;
        }
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

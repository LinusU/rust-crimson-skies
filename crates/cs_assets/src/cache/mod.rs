//! The private derived-asset cache (F15-A contracts, F15-B store).
//!
//! Spec `specs/F15-asynchronous-asset-loading-and-private-cache.md`: a
//! derived cache is a performance optimization, never the authoritative
//! data source (see also `docs/01-ARCHITECTURE.md`, "Asset pipeline"). The
//! first three modules are the typed contracts; [`store`] is the mechanism
//! that implements them on disk.
//!
//! * [`key`] is the cache-key contract: a [`CacheKey`] identifies the
//!   installation hash, every source span hash, the decoder/IR version and
//!   the conversion options an entry was derived under, and its digest is
//!   the store-facing identity. Per-input granularity is what limits an
//!   invalidation to the derived assets that actually used a changed
//!   source (spec F15 AC03).
//! * [`entry`] is the integrity contract: a stored entry records a
//!   [`EntryState::Writing`]/`Committed` publish state plus declared
//!   payload length and digest, and [`verify_entry`] is the only path by
//!   which stored bytes become usable — a partially written or corrupted
//!   entry fails validation and is rebuilt (non-negotiable behavior 3).
//! * [`bound`] is the location and size contract: a [`CacheDirectory`]
//!   cannot lie inside the source installation (non-negotiable behavior 1:
//!   private, no writes to the source) and a [`CacheBudget`] bounds every
//!   store by entries and bytes.
//! * [`store`] is the production store over those three: a
//!   [`CacheStore`] stages every write in scratch and publishes it with
//!   one atomic directory rename, sweeps interrupted writes when it opens
//!   (spec F15 AC02), reads entries in cancellable chunks and evicts within
//!   its budget before it grows.
//!
//! The load transaction that drives the store is `cs_app::loading`. Nothing
//! here is derived from original game data.

pub mod bound;
pub mod entry;
pub mod key;
pub mod store;

pub use bound::{BudgetError, BudgetExceeded, CacheBudget, CacheDirectory, CacheLocationError};
pub use entry::{EntryHeader, EntryState, IntegrityError, VerifiedEntry, verify_entry};
pub use key::{
    CacheKey, CacheKeyError, CacheLabelError, ConversionOption, ConversionOptions,
    ConverterVersion, DecoderId, IrVersion, OptionsError, SourceSpanHash,
};
pub use store::{
    CACHE_IO_CHUNK, CacheLookup, CacheReadError, CacheStore, HEADER_FILE, HEADER_FORMAT,
    InvalidationReport, PAYLOAD_FILE, PendingCacheRead, PendingStoreWrite, RecoveryReport,
    StoreError, StoreUsage, StoredEntry,
};

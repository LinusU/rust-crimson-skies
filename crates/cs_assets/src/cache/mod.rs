//! The private derived-asset cache contract (F15-A).
//!
//! Spec `specs/F15-asynchronous-asset-loading-and-private-cache.md`: a
//! derived cache is a performance optimization, never the authoritative
//! data source (see also `docs/01-ARCHITECTURE.md`, "Asset pipeline"). This
//! module defines the typed contracts the F15-B store implements; it
//! stores nothing itself.
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
//!
//! The async read pipeline and the atomic store mechanics are F15-B; the
//! load transaction that drives them is `cs_app::loading`. Nothing here is
//! derived from original game data.

pub mod bound;
pub mod entry;
pub mod key;

pub use bound::{BudgetError, BudgetExceeded, CacheBudget, CacheDirectory, CacheLocationError};
pub use entry::{EntryHeader, EntryState, IntegrityError, VerifiedEntry, verify_entry};
pub use key::{
    CacheKey, CacheKeyError, CacheLabelError, ConversionOption, ConversionOptions,
    ConverterVersion, DecoderId, IrVersion, OptionsError, SourceSpanHash,
};

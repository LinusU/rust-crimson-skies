//! The private derived-asset store: bounded, atomic, recoverable (F15-B).
//!
//! `key`, `entry` and `bound` are the F15-A contracts: they say *what* an
//! entry is, when it may be served and where it may live. This module is
//! the mechanism F15-A deliberately left open — the on-disk layout, the
//! commit boundary and the eviction policy — implementing spec F15
//! non-negotiable behaviors 1 and 3 without weakening either contract.
//!
//! # Layout
//!
//! ```text
//! <CacheDirectory root>/entries/<key-digest-hex>/header   the declaration
//! <CacheDirectory root>/entries/<key-digest-hex>/payload  the derived bytes
//! <CacheDirectory root>/staging/<pid>-<serial>/header    a write in progress
//! <CacheDirectory root>/staging/<pid>-<serial>/payload
//! ```
//!
//! An entry directory is named by the canonical lowercase hex SHA-256 of
//! its [`CacheKey`] digest (`docs/contracts/IDENTITY-CONTENT.md`: hash
//! strings are canonical lowercase hexadecimal of the declared algorithm).
//!
//! The persisted `header` is a strict line-based record. It records the
//! **whole** key, not just its digest, so a reader can rebuild the key
//! from the record and check that the record really is that key:
//!
//! ```text
//! cs-cache-entry/2
//! state writing|committed
//! key <64 hex>            the digest the facets below must hash to
//! install <64 hex>
//! input <64 hex>          one line per source span hash
//! decoder <label>
//! decoder_version <u32>
//! ir <u32>
//! option <name> <value>   zero or more, already normalized
//! length <u64>            committed only
//! payload <64 hex>        committed only
//! sequence <u64>          committed only: the store's write order
//! ```
//!
//! An unknown field, a repeated field, a missing field, a label the
//! contract refuses, a `writing` record carrying committed-only fields, or
//! facets that do not hash to the recorded key are all
//! [`StoreError::CorruptHeader`]. The store therefore trusts its own
//! bookkeeping exactly as far as [`verify_entry`](super::verify_entry)
//! trusts it: not at all. The input list is also what makes spec F15 AC03
//! answerable from the store's own records — see
//! [`CacheStore::invalidate_source`].
//!
//! # The commit boundary
//!
//! [`CacheStore::begin_write`] stages a [`PendingStoreWrite`] in
//! `staging/<pid>-<serial>`, starting with a `writing` header — the same
//! [`EntryState::Writing`] an interrupted write is defined to have. The
//! write is sealed (payload flushed and synced, committed header written
//! and synced, scratch directory synced) and only then published by
//! [`CacheStore::commit`], which renames the finished scratch directory
//! onto `entries/<digest>`. A rename within one directory is atomic, so an
//! entry is either absent or complete: **the commit is the rename, not the
//! header write**. A process killed at any point before it leaves only
//! scratch, which startup recovery ([`CacheStore::open`]) sweeps, so the
//! next run rebuilds the entry from its sources (spec F15 AC02). Nothing
//! ever writes inside the source installation: the root is a
//! [`CacheDirectory`], which refuses a root inside the installation.
//!
//! # Bounded reads
//!
//! [`CacheStore::begin_read`] returns a [`PendingCacheRead`] that reads the
//! payload in [`CACHE_IO_CHUNK`]-sized chunks, checks a [`ReadCancel`]
//! before each one and reports measured progress per chunk — the same
//! shape as `vfs::PendingRead`, so a load's progress is measured work, not
//! a guessed percent (behavior 5). The bytes only become a
//! [`VerifiedEntry`] by passing [`verify_entry`](super::verify_entry):
//! the header is decoded again at read time, the payload is re-hashed, and
//! both are checked against the key that was asked for.
//!
//! # Budget
//!
//! The store is always opened *with* a [`CacheBudget`]. Before publishing
//! an entry it evicts committed entries with the lowest write sequence
//! until the new one fits; if the entry alone exceeds the byte bound it is
//! refused ([`StoreError::Budget`]) rather than evicting everything for a
//! store that still could not hold it. The write sequence is part of the
//! published record, so that order survives a restart instead of degrading
//! into the digest tie-break. A read never refreshes the order, so the
//! policy is "least recently written first", which is deterministic across
//! runs — no timestamps, no clock, no host dependence. Refusing to cache
//! is never a load failure: the caller serves the bytes it just built
//! (spec F15 behavior 1 — the cache is an optimization, never the
//! authoritative data source).
//!
//! One store at a time owns a cache directory: startup recovery sweeps
//! scratch directories, so a second live store on the same root would
//! discard the first one's in-flight write. The root is a private
//! per-user path, which is what makes that acceptable (recorded as a
//! limitation in `docs/findings/`).

use std::collections::BTreeMap;
use std::fmt;
use std::fs;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use cs_types::evidence::ContentHash;

use crate::cache::bound::{BudgetExceeded, CacheBudget, CacheDirectory};
use crate::cache::entry::{EntryHeader, EntryState, IntegrityError, VerifiedEntry, verify_entry};
use crate::cache::key::{
    CacheKey, ConversionOption, ConversionOptions, ConverterVersion, DecoderId, IrVersion,
    SourceSpanHash,
};
use crate::install::Sha256;
use crate::vfs::session::{ReadCancel, ReadProgress};

/// The name of the published-entry directory below the cache root.
pub const ENTRIES_DIR: &str = "entries";

/// The name of the scratch directory below the cache root.
pub const STAGING_DIR: &str = "staging";

/// The file name of a persisted entry declaration.
pub const HEADER_FILE: &str = "header";

/// The file name of a persisted entry payload.
pub const PAYLOAD_FILE: &str = "payload";

/// The version line of the persisted header format.
pub const HEADER_FORMAT: &str = "cs-cache-entry/2";

/// How many bytes one append or one read step moves, and how often a
/// cancellation is observed.
///
/// The same granularity `vfs::PENDING_READ_CHUNK` uses for source reads,
/// so a cancelled load stops within one chunk of the switch being thrown
/// whatever it happens to be reading.
pub const CACHE_IO_CHUNK: u64 = 1 << 20;

/// Serial counter for scratch directory names, so two staged writes in one
/// process never share one.
static NEXT_STAGING: AtomicU64 = AtomicU64::new(0);

/// Why a store operation failed.
#[derive(Debug)]
pub enum StoreError {
    /// The cache directory could not be read, created, written or removed.
    Io {
        /// The path being worked on.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
    /// A stored declaration could not be decoded: an unknown or repeated
    /// field, a missing field, a wrong format line, facets that do not
    /// hash to the recorded key, or a `writing` record carrying
    /// committed-only fields.
    CorruptHeader {
        /// The header that could not be decoded.
        path: PathBuf,
        /// What was wrong with it.
        reason: String,
    },
    /// The publish would not fit the store's budget even after evicting
    /// every entry it could.
    Budget(BudgetExceeded),
    /// More bytes were appended than the entry declares. A write never
    /// grows past its declared length: the committed header would then
    /// describe bytes that are not there.
    EntryTooLong {
        /// The declared length.
        declared: u64,
        /// How many bytes were offered in total.
        offered: u64,
    },
    /// The staged payload no longer holds the bytes the sealed record
    /// declares — it was truncated or replaced behind the write's back.
    /// Nothing is published: the entry is rebuilt from its sources, and
    /// the scratch directory goes when the write is dropped.
    PayloadChanged {
        /// The length the record declares.
        declared: u64,
        /// What the staged payload actually holds.
        found: u64,
    },
    /// The write was cancelled. Nothing was published; the scratch
    /// directory is removed when the write is dropped.
    WriteCancelled {
        /// Bytes written before the cancellation was observed.
        written: u64,
        /// The declared length.
        total: u64,
    },
    /// The write was already sealed, so its payload can no longer change.
    Sealed,
}

impl StoreError {
    /// The stable lowercase code used in reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Io { .. } => "cache_io",
            Self::CorruptHeader { .. } => "corrupt_header",
            Self::Budget(_) => "budget",
            Self::EntryTooLong { .. } => "entry_too_long",
            Self::PayloadChanged { .. } => "payload_changed",
            Self::WriteCancelled { .. } => "write_cancelled",
            Self::Sealed => "sealed",
        }
    }
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io { path, source } => write!(f, "cache store {}: {source}", path.display()),
            Self::CorruptHeader { path, reason } => {
                write!(
                    f,
                    "cache store header {} is unreadable: {reason}",
                    path.display()
                )
            }
            Self::Budget(exceeded) => write!(f, "cache store: {exceeded}"),
            Self::EntryTooLong { declared, offered } => write!(
                f,
                "cache store: the entry declares {declared} bytes but {offered} were offered"
            ),
            Self::PayloadChanged { declared, found } => write!(
                f,
                "cache store: the staged payload holds {found} bytes where the record \
                 declares {declared}; nothing was published"
            ),
            Self::WriteCancelled { written, total } => write!(
                f,
                "cache store: the write was cancelled after {written} of {total} bytes; \
                 nothing was published"
            ),
            Self::Sealed => write!(
                f,
                "cache store: the write is sealed; its payload can no longer change"
            ),
        }
    }
}

impl std::error::Error for StoreError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Budget(exceeded) => Some(exceeded),
            _ => None,
        }
    }
}

/// Why a bounded cache read did not deliver a payload.
#[derive(Debug)]
pub enum CacheReadError {
    /// The read was cancelled; the bytes read so far were discarded and
    /// nothing is delivered.
    Cancelled {
        /// Bytes read before the cancellation was observed.
        read: u64,
        /// The entry's declared length.
        total: u64,
    },
    /// The stored entry failed integrity validation. Nothing is delivered
    /// and the entry is rebuilt from its sources — never served, never
    /// repaired in place (spec F15 behavior 3).
    Corrupt(IntegrityError),
    /// The stored entry's declaration could not be decoded, or it is not
    /// accompanied by the payload it declares.
    CorruptHeader {
        /// The path that could not be decoded.
        path: PathBuf,
        /// What was wrong with it.
        reason: String,
    },
    /// The cache directory could not be read.
    Io {
        /// The path being read.
        path: PathBuf,
        /// The failure.
        source: io::Error,
    },
}

impl CacheReadError {
    /// The stable lowercase code used in reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Cancelled { .. } => "read_cancelled",
            Self::Corrupt(error) => error.code(),
            Self::CorruptHeader { .. } => "corrupt_header",
            Self::Io { .. } => "cache_io",
        }
    }

    /// Whether this failure means the stored entry has to be rebuilt.
    ///
    /// Every failure but a cancellation does: the entry is rebuilt from
    /// its sources, never served and never repaired in place (spec F15
    /// behavior 3). A cancellation is not a verdict on the entry — nothing
    /// was delivered, and the entry is left exactly as it was for the next
    /// read. Which load-level recovery path either of those is remains the
    /// consumer's decision, not the store's.
    pub const fn rebuilds(&self) -> bool {
        !matches!(self, Self::Cancelled { .. })
    }
}

impl fmt::Display for CacheReadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Cancelled { read, total } => write!(
                f,
                "the cache read was cancelled after {read} of {total} bytes; nothing \
                 was delivered"
            ),
            Self::Corrupt(error) => write!(f, "the cached entry is unusable: {error}"),
            Self::CorruptHeader { path, reason } => {
                write!(f, "cache entry {} is unreadable: {reason}", path.display())
            }
            Self::Io { path, source } => write!(f, "cannot read {}: {source}", path.display()),
        }
    }
}

impl std::error::Error for CacheReadError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io { source, .. } => Some(source),
            Self::Corrupt(error) => Some(error),
            _ => None,
        }
    }
}

/// How much the store currently holds, measured — never estimated.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct StoreUsage {
    /// Committed entries.
    pub entries: u64,
    /// Total payload bytes of those entries.
    pub bytes: u64,
}

/// What startup recovery did, so a run can say what it cleaned up instead
/// of silently repairing behind the caller's back.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct RecoveryReport {
    /// Scratch directories of interrupted writes that were removed.
    pub swept_staging: usize,
    /// Entry directories whose header never reached `committed`.
    pub dropped_uncommitted: usize,
    /// Entry directories whose header, key or payload length disagreed
    /// with what they should hold.
    pub dropped_corrupt: usize,
    /// Entries below `entries/` that are not this store's own bookkeeping
    /// and were therefore left untouched.
    pub kept_unknown: usize,
    /// What the store holds after recovery.
    pub usage: StoreUsage,
}

impl fmt::Display for RecoveryReport {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "swept {} interrupted write(s), dropped {} uncommitted and {} corrupt \
             entries, kept {} unrecognized entries; {} entries totalling {} bytes \
             remain",
            self.swept_staging,
            self.dropped_uncommitted,
            self.dropped_corrupt,
            self.kept_unknown,
            self.usage.entries,
            self.usage.bytes
        )
    }
}

/// What one source-input invalidation removed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct InvalidationReport {
    /// Digests of the entries that were removed, in removal order.
    pub dropped: Vec<ContentHash>,
    /// Entry count after the invalidation.
    pub entries: u64,
    /// Payload bytes after the invalidation.
    pub bytes: u64,
}

/// What a store lookup found.
#[derive(Debug)]
pub enum CacheLookup {
    /// No entry is stored for this key: build it.
    Miss,
    /// A stored entry exists but cannot be served. The reason is named so
    /// a run can say *why* it rebuilt; the entry stays on disk until the
    /// rebuild's [`CacheStore::commit`] replaces it or
    /// [`CacheStore::discard`] removes it.
    Rebuild {
        /// The integrity refusal: `Uncommitted` for a partially written
        /// entry, `WrongKey` for one stored under another derivation,
        /// `LengthMismatch` for one whose payload is not what its header
        /// declares.
        reason: IntegrityError,
    },
    /// A committed entry exists: read it in bounded chunks.
    Hit(PendingCacheRead),
}

/// A committed entry's bounded read.
///
/// Issued by [`CacheStore::begin_read`], completed on a worker thread
/// while the load keeps running. Its bytes become a [`VerifiedEntry`] only
/// by passing [`verify_entry`](super::verify_entry), so a store that lies
/// about its own entry cannot make a consumer serve it.
#[derive(Debug)]
pub struct PendingCacheRead {
    key: CacheKey,
    directory: PathBuf,
    total: u64,
    cancel: ReadCancel,
}

impl PendingCacheRead {
    /// The key this read answers.
    pub fn key(&self) -> &CacheKey {
        &self.key
    }

    /// The entry's declared payload length — the work this read measures.
    pub fn total(&self) -> u64 {
        self.total
    }

    /// A switch that stops this read at its next chunk boundary, from any
    /// thread.
    pub fn cancel_handle(&self) -> ReadCancel {
        self.cancel.clone()
    }

    /// Reads and verifies the whole payload.
    pub fn complete(self) -> Result<VerifiedEntry, CacheReadError> {
        self.complete_with(|_| {})
    }

    /// Reads the whole payload in [`CACHE_IO_CHUNK`]-sized chunks,
    /// reporting measured progress after each one and checking the
    /// cancellation before each one.
    ///
    /// The header is decoded *again* here rather than taken from the
    /// lookup, so what is validated is what is on disk at the moment the
    /// bytes are read. A cancelled read delivers nothing.
    pub fn complete_with(
        self,
        mut progress: impl FnMut(ReadProgress),
    ) -> Result<VerifiedEntry, CacheReadError> {
        let header_path = self.directory.join(HEADER_FILE);
        let stored = match read_header(&header_path) {
            Ok(stored) => stored,
            Err(StoreError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Err(CacheReadError::CorruptHeader {
                    path: header_path,
                    reason: "the entry has no header".to_owned(),
                });
            }
            Err(error) => {
                return Err(CacheReadError::CorruptHeader {
                    path: header_path,
                    reason: error.to_string(),
                });
            }
        };
        // `verify_entry` compares keys by digest; the stored record is
        // rebuilt from its own facets and must be the key that was asked
        // for, or this entry answers a different derivation.
        if stored.key != self.key {
            return Err(CacheReadError::Corrupt(IntegrityError::WrongKey {
                requested: self.key.digest(),
                stored: stored.key.digest(),
            }));
        }
        let EntryState::Committed {
            payload_len,
            payload_sha256,
        } = stored.state
        else {
            return Err(CacheReadError::Corrupt(IntegrityError::Uncommitted));
        };
        let bytes = self.read_payload(&self.payload_path(), payload_len, &mut progress)?;
        let header =
            EntryHeader::committed_streaming(self.key.clone(), payload_len, payload_sha256);
        verify_entry(&self.key, &header, &bytes).map_err(CacheReadError::Corrupt)
    }

    /// The bounded read itself: chunks, a cancellation check before each,
    /// and the declared length checked against the file before anything is
    /// allocated or hashed.
    fn read_payload(
        &self,
        payload_path: &Path,
        declared: u64,
        progress: &mut impl FnMut(ReadProgress),
    ) -> Result<Vec<u8>, CacheReadError> {
        let io_error = |source| CacheReadError::Io {
            path: payload_path.to_path_buf(),
            source,
        };
        let mut file = match fs::File::open(payload_path) {
            Ok(file) => file,
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Err(CacheReadError::CorruptHeader {
                    path: payload_path.to_path_buf(),
                    reason: "the committed entry has no payload".to_owned(),
                });
            }
            Err(source) => return Err(io_error(source)),
        };
        let found = file.metadata().map_err(io_error)?.len();
        if found != declared {
            return Err(CacheReadError::Corrupt(IntegrityError::LengthMismatch {
                declared,
                actual: found,
            }));
        }
        let length = usize::try_from(declared).map_err(|_| {
            CacheReadError::Corrupt(IntegrityError::LengthMismatch {
                declared,
                actual: found,
            })
        })?;
        let mut bytes = vec![0u8; length];
        let mut read: u64 = 0;
        while read < declared {
            if self.cancel.is_cancelled() {
                return Err(CacheReadError::Cancelled {
                    read,
                    total: declared,
                });
            }
            let step = CACHE_IO_CHUNK.min(declared - read);
            let start = usize::try_from(read).expect("a length that fit in memory");
            let end = start + usize::try_from(step).expect("a chunk below the buffer");
            file.read_exact(&mut bytes[start..end]).map_err(io_error)?;
            read += step;
            progress(ReadProgress {
                read,
                total: declared,
            });
        }
        Ok(bytes)
    }

    /// The payload path of the entry this read answers.
    fn payload_path(&self) -> PathBuf {
        self.directory.join(PAYLOAD_FILE)
    }
}

/// A write that has been staged but not published.
///
/// Created by [`CacheStore::begin_write`], filled by
/// [`PendingStoreWrite::append`] or [`PendingStoreWrite::write_all`],
/// sealed by [`PendingStoreWrite::seal`] and published by
/// [`CacheStore::commit`]. Dropping an unpublished write removes its
/// scratch directory, so an abandoned or cancelled write leaves nothing
/// behind; a *killed* process leaves the scratch directory, which startup
/// recovery sweeps.
pub struct PendingStoreWrite {
    key: CacheKey,
    staging: PathBuf,
    /// `None` only once the write is sealed: a sealed payload can no
    /// longer change, and closing the handle is what lets the scratch
    /// directory be removed on every platform. An unsealed write always
    /// holds it.
    file: Option<fs::File>,
    /// Kept, never taken: a seal that fails must leave the write exactly
    /// as it was, so the digest is computed from a copy of it.
    hasher: Sha256,
    declared: u64,
    written: u64,
    sealed: bool,
    published: bool,
    cancel: ReadCancel,
}

impl PendingStoreWrite {
    /// The key the entry is being written under.
    pub fn key(&self) -> &CacheKey {
        &self.key
    }

    /// The declared payload length.
    pub fn total(&self) -> u64 {
        self.declared
    }

    /// How many bytes have been appended so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Whether the write is sealed: every declared byte is on disk and the
    /// committed header is staged, but the entry is still not published.
    pub const fn is_sealed(&self) -> bool {
        self.sealed
    }

    /// The scratch directory this write is staged in.
    pub fn staging(&self) -> &Path {
        &self.staging
    }

    /// A switch that stops this write at its next chunk boundary, from any
    /// thread.
    pub fn cancel_handle(&self) -> ReadCancel {
        self.cancel.clone()
    }

    /// Appends at most [`CACHE_IO_CHUNK`] bytes: the bounded write step.
    ///
    /// A caller with a large buffer calls this repeatedly, so no single
    /// step moves an unbounded amount of data and a cancellation is
    /// observed between steps. More bytes than the entry still declares
    /// are refused ([`StoreError::EntryTooLong`]): a committed header that
    /// describes bytes which are not there is exactly the partial entry
    /// the commit boundary exists to prevent.
    ///
    /// # Errors
    ///
    /// [`StoreError::Sealed`] if the write is already sealed,
    /// [`StoreError::EntryTooLong`] if the bytes exceed what the entry
    /// still declares, [`StoreError::WriteCancelled`] if the switch was
    /// thrown, and [`StoreError::Io`] if the append failed.
    pub fn append(&mut self, bytes: &[u8]) -> Result<ReadProgress, StoreError> {
        if self.sealed {
            return Err(StoreError::Sealed);
        }
        let remaining = self.declared - self.written;
        if bytes.len() as u64 > remaining {
            return Err(StoreError::EntryTooLong {
                declared: self.declared,
                offered: self.written + bytes.len() as u64,
            });
        }
        if self.cancel.is_cancelled() {
            return Err(StoreError::WriteCancelled {
                written: self.written,
                total: self.declared,
            });
        }
        let chunk = usize::try_from(CACHE_IO_CHUNK).expect("a 1 MiB chunk fits in memory");
        let step = bytes.len().min(chunk);
        let path = self.staging.join(PAYLOAD_FILE);
        // Unreachable while the write is unsealed, and reported rather
        // than panicked on if it ever were: an open handle is a state, not
        // an invariant a caller may not survive.
        let Some(file) = self.file.as_mut() else {
            return Err(StoreError::Sealed);
        };
        file.write_all(&bytes[..step])
            .map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
        self.hasher.update(&bytes[..step]);
        self.written += step as u64;
        Ok(ReadProgress {
            read: self.written,
            total: self.declared,
        })
    }

    /// Appends `bytes` in [`CACHE_IO_CHUNK`]-sized steps, checking the
    /// cancellation between them.
    ///
    /// # Errors
    ///
    /// Whatever [`PendingStoreWrite::append`] reports, plus
    /// [`StoreError::EntryTooLong`] if `bytes` is longer than the declared
    /// length.
    pub fn write_all(&mut self, bytes: &[u8]) -> Result<(), StoreError> {
        if bytes.len() as u64 > self.declared {
            return Err(StoreError::EntryTooLong {
                declared: self.declared,
                offered: bytes.len() as u64,
            });
        }
        let chunk = usize::try_from(CACHE_IO_CHUNK).expect("a 1 MiB chunk fits in memory");
        for start in (0..bytes.len()).step_by(chunk) {
            let end = (start + chunk).min(bytes.len());
            self.append(&bytes[start..end])?;
        }
        Ok(())
    }

    /// Prepares the publish: the payload is flushed and synced, the
    /// committed header is written and synced, and the scratch directory
    /// is synced.
    ///
    /// Everything the entry needs is on disk when this returns, but the
    /// entry is **not** published: only [`CacheStore::commit`] does that,
    /// with one rename. A write sealed but never committed — because the
    /// process died, or the caller dropped it — is scratch, not an entry.
    ///
    /// A seal that fails changes nothing: the payload handle and the
    /// digest stay where they are, so the write can be appended to and
    /// sealed again. A half-sealed write is not a state a caller has to
    /// survive by dropping the write.
    ///
    /// # Errors
    ///
    /// [`StoreError::EntryTooLong`] if fewer than the declared bytes were
    /// appended, and [`StoreError::Io`] if a flush, sync or write failed.
    pub fn seal(&mut self) -> Result<(), StoreError> {
        if self.sealed {
            return Ok(());
        }
        if self.written != self.declared {
            return Err(StoreError::EntryTooLong {
                declared: self.declared,
                offered: self.written,
            });
        }
        let payload_path = self.staging.join(PAYLOAD_FILE);
        let header_path = self.staging.join(HEADER_FILE);
        // Sync through the open handle and digest a *copy* of the hasher:
        // the hasher is what a retry needs, so it is never given up here.
        let Some(file) = self.file.as_ref() else {
            return Err(StoreError::Sealed);
        };
        file.sync_all().map_err(|source| StoreError::Io {
            path: payload_path,
            source,
        })?;
        let record = StoredHeader {
            state: EntryState::Committed {
                payload_len: self.declared,
                payload_sha256: self.hasher.clone().finalize(),
            },
            key: self.key.clone(),
            // The store assigns the write order and records it in the
            // published entry; see `CacheStore::commit`.
            sequence: 0,
        };
        write_header(&header_path, &record).map_err(|source| StoreError::Io {
            path: header_path,
            source,
        })?;
        sync_dir(&self.staging).map_err(|source| StoreError::Io {
            path: self.staging.clone(),
            source,
        })?;
        // Sealed and committed to disk: the payload can no longer change,
        // so the handle is closed here and every later append is refused.
        self.file = None;
        self.sealed = true;
        Ok(())
    }
}

impl fmt::Debug for PendingStoreWrite {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("PendingStoreWrite")
            .field("key", &self.key.digest())
            .field("staging", &self.staging)
            .field("declared", &self.declared)
            .field("written", &self.written)
            .field("sealed", &self.sealed)
            .field("published", &self.published)
            .finish()
    }
}

impl Drop for PendingStoreWrite {
    fn drop(&mut self) {
        if self.published {
            return;
        }
        // Close the payload first: an open handle would keep the scratch
        // directory from being removed on Windows.
        self.file = None;
        let _ = fs::remove_dir_all(&self.staging);
    }
}

/// What one published entry is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct StoredEntry {
    /// The key it is stored under.
    pub key: CacheKey,
    /// The published payload length.
    pub payload_len: u64,
    /// The payload's SHA-256, as the committed header declares it.
    pub payload_sha256: ContentHash,
    /// The write sequence the store assigned it.
    pub sequence: u64,
    /// Entries evicted to make room, oldest write sequence first.
    pub evicted: Vec<ContentHash>,
}

/// One committed entry as the store indexed it.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct IndexedEntry {
    payload_len: u64,
    sequence: u64,
}

/// A decoded persisted header: the whole key, plus the publish state.
///
/// The key is rebuilt from the recorded facets and only kept when it
/// hashes to the digest the record itself declares, so a stored record can
/// never claim an identity its bytes do not support.
#[derive(Clone, Debug)]
struct StoredHeader {
    state: EntryState,
    key: CacheKey,
    sequence: u64,
}

/// The private, bounded, atomic derived-asset store.
#[derive(Debug)]
pub struct CacheStore {
    directory: CacheDirectory,
    budget: CacheBudget,
    /// Key digest hex -> what the store holds for it.
    entries: BTreeMap<String, IndexedEntry>,
    bytes: u64,
    next_sequence: u64,
    recovery: RecoveryReport,
}

impl CacheStore {
    /// Opens the store in `directory` and recovers it.
    ///
    /// Recovery is the AC02 half: every scratch directory of an
    /// interrupted write is swept, and an entry that never reached a
    /// committed header — or whose record, key or payload length does not
    /// match what it holds — is dropped so the entry is rebuilt from its
    /// sources. Entries below `entries/` that are not this store's own
    /// bookkeeping (not a 64-hex directory, or a file where a directory
    /// belongs) are counted in [`RecoveryReport::kept_unknown`] and left
    /// untouched: a private cache never deletes what it did not write.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] if the two store directories cannot be created
    /// or listed.
    pub fn open(directory: CacheDirectory, budget: CacheBudget) -> Result<Self, StoreError> {
        let entries_root = directory.root().join(ENTRIES_DIR);
        let staging_root = directory.root().join(STAGING_DIR);
        create_dir(&entries_root)?;
        create_dir(&staging_root)?;

        let mut store = Self {
            directory,
            budget,
            entries: BTreeMap::new(),
            bytes: 0,
            next_sequence: 1,
            recovery: RecoveryReport::default(),
        };
        store.sweep_staging(&staging_root)?;
        store.scan_entries(&entries_root)?;
        store.recovery.usage = store.usage();
        Ok(store)
    }

    /// The private cache root.
    pub fn directory(&self) -> &CacheDirectory {
        &self.directory
    }

    /// The bound this store is held to.
    pub const fn budget(&self) -> &CacheBudget {
        &self.budget
    }

    /// The published-entry directory.
    pub fn entries_root(&self) -> PathBuf {
        self.directory.root().join(ENTRIES_DIR)
    }

    /// The scratch directory an interrupted write is left in.
    pub fn staging_root(&self) -> PathBuf {
        self.directory.root().join(STAGING_DIR)
    }

    /// What the store holds, measured from what it published.
    pub fn usage(&self) -> StoreUsage {
        StoreUsage {
            entries: self.entries.len() as u64,
            bytes: self.bytes,
        }
    }

    /// What the last [`CacheStore::open`] recovered.
    pub const fn recovery(&self) -> &RecoveryReport {
        &self.recovery
    }

    /// The path an entry is published at.
    fn entry_path(&self, key: &CacheKey) -> PathBuf {
        self.entries_root().join(key.digest().to_hex())
    }

    /// Looks `key` up, refusing anything that is not a committed, correct
    /// entry.
    ///
    /// Only the record is read here; the payload is read in bounded chunks
    /// by the [`PendingCacheRead`] this returns and verified there. A
    /// record that is uncommitted, stores another key, or declares a
    /// length the payload does not have is a [`CacheLookup::Rebuild`] —
    /// named, never silently served.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] or [`StoreError::CorruptHeader`] if the entry
    /// directory or its record cannot be read or decoded.
    pub fn begin_read(&self, key: &CacheKey) -> Result<CacheLookup, StoreError> {
        let directory = self.entry_path(key);
        let header_path = directory.join(HEADER_FILE);
        let stored = match read_header(&header_path) {
            Ok(stored) => stored,
            Err(StoreError::Io { source, .. }) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(CacheLookup::Miss);
            }
            Err(error) => return Err(error),
        };
        if stored.key != *key {
            return Ok(CacheLookup::Rebuild {
                reason: IntegrityError::WrongKey {
                    requested: key.digest(),
                    stored: stored.key.digest(),
                },
            });
        }
        let EntryState::Committed { payload_len, .. } = stored.state else {
            return Ok(CacheLookup::Rebuild {
                reason: IntegrityError::Uncommitted,
            });
        };
        let payload = directory.join(PAYLOAD_FILE);
        let found = match fs::metadata(&payload) {
            Ok(metadata) => metadata.len(),
            Err(source) if source.kind() == io::ErrorKind::NotFound => {
                return Ok(CacheLookup::Rebuild {
                    reason: IntegrityError::LengthMismatch {
                        declared: payload_len,
                        actual: 0,
                    },
                });
            }
            Err(source) => {
                return Err(StoreError::Io {
                    path: payload,
                    source,
                });
            }
        };
        if found != payload_len {
            return Ok(CacheLookup::Rebuild {
                reason: IntegrityError::LengthMismatch {
                    declared: payload_len,
                    actual: found,
                },
            });
        }
        Ok(CacheLookup::Hit(PendingCacheRead {
            key: key.clone(),
            directory,
            total: payload_len,
            cancel: ReadCancel::default(),
        }))
    }

    /// Stages a write of `payload_len` bytes under `key`.
    ///
    /// The scratch directory and its `writing` record exist before this
    /// returns, so a process that dies here leaves a store that startup
    /// recovery can name and sweep.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] if the scratch directory, its `writing` record or
    /// its payload file cannot be created.
    pub fn begin_write(
        &self,
        key: &CacheKey,
        payload_len: u64,
    ) -> Result<PendingStoreWrite, StoreError> {
        let serial = NEXT_STAGING.fetch_add(1, Ordering::Relaxed);
        let staging = self
            .staging_root()
            .join(format!("{}-{serial}", std::process::id()));
        let io_error = |source| StoreError::Io {
            path: staging.clone(),
            source,
        };
        fs::create_dir(&staging).map_err(io_error)?;
        let header_path = staging.join(HEADER_FILE);
        write_header(
            &header_path,
            &StoredHeader {
                state: EntryState::Writing,
                key: key.clone(),
                sequence: 0,
            },
        )
        .map_err(|source| StoreError::Io {
            path: header_path,
            source,
        })?;
        let file = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(staging.join(PAYLOAD_FILE))
            .map_err(io_error)?;
        Ok(PendingStoreWrite {
            key: key.clone(),
            staging,
            file: Some(file),
            hasher: Sha256::new(),
            declared: payload_len,
            written: 0,
            sealed: false,
            published: false,
            cancel: ReadCancel::default(),
        })
    }

    /// Publishes a sealed write: records the write order, evicts what the
    /// budget requires, then renames the finished scratch directory into
    /// place.
    ///
    /// The rename is the commit boundary. Everything before it is scratch
    /// (removable, never served); everything after it is a complete entry
    /// that a reader can verify. Replacing an existing entry of the same
    /// key removes the old directory first, so a crash in that window
    /// leaves the entry absent — rebuilt next time — never half of each.
    ///
    /// The write order is written into the record *before* the rename, so
    /// it is in the published entry and a later run evicts in the order
    /// entries were really written, not in whatever order their digests
    /// happen to sort. Rewriting a scratch record changes nothing a reader
    /// can see: the commit is still the rename.
    ///
    /// # Errors
    ///
    /// [`StoreError::EntryTooLong`] if the write is not sealed or does not
    /// hold its declared length, [`StoreError::PayloadChanged`] if the
    /// staged payload no longer holds what the sealed record declares,
    /// [`StoreError::Budget`] if the entry does not fit even an empty
    /// store, and [`StoreError::Io`] if the eviction, rename or sync
    /// failed.
    pub fn commit(&mut self, write: PendingStoreWrite) -> Result<StoredEntry, StoreError> {
        let mut write = write;
        if !write.sealed || write.written != write.declared {
            return Err(StoreError::EntryTooLong {
                declared: write.declared,
                offered: write.written,
            });
        }
        let stored = read_header(&write.staging.join(HEADER_FILE))?;
        let EntryState::Committed {
            payload_len,
            payload_sha256,
        } = stored.state
        else {
            return Err(StoreError::EntryTooLong {
                declared: write.declared,
                offered: write.written,
            });
        };
        // The write's own accounting is not the file: a payload that was
        // truncated or replaced after the seal is refused here, so the
        // store never publishes a record it already knows is wrong.
        let staged = write.staging.join(PAYLOAD_FILE);
        let found = fs::metadata(&staged)
            .map_err(|source| StoreError::Io {
                path: staged.clone(),
                source,
            })?
            .len();
        if found != payload_len {
            return Err(StoreError::PayloadChanged {
                declared: payload_len,
                found,
            });
        }

        let name = write.key.digest().to_hex();
        // The write order goes into the record *before* anything is
        // evicted, so a run that fails here costs the store no entries: the
        // sequence may end up unused, which is cheaper than a needless
        // eviction.
        let sequence = self.next_sequence;
        self.next_sequence += 1;
        let header_path = write.staging.join(HEADER_FILE);
        write_header(
            &header_path,
            &StoredHeader {
                state: EntryState::Committed {
                    payload_len,
                    payload_sha256,
                },
                key: write.key.clone(),
                sequence,
            },
        )
        .map_err(|source| StoreError::Io {
            path: header_path,
            source,
        })?;

        let evicted = self.make_room(payload_len, &name)?;
        let target = self.entry_path(&write.key);
        remove_entry(&target)?;
        // The replaced entry is gone from the disk, so it leaves the index
        // now: a rename that fails must not leave the index claiming an
        // entry that is not there, which would make `usage` over-report
        // and could let the store grow past its budget.
        if let Some(replaced) = self.entries.remove(&name) {
            self.bytes = self.bytes.saturating_sub(replaced.payload_len);
        }
        fs::rename(&write.staging, &target).map_err(|source| StoreError::Io {
            path: target.clone(),
            source,
        })?;
        // Published: the scratch directory is the entry now, so `Drop` must
        // not remove it.
        write.published = true;
        // Indexed before the sync, so the index describes the disk even if
        // the sync fails: `usage` is measured, never intended.
        self.entries.insert(
            name,
            IndexedEntry {
                payload_len,
                sequence,
            },
        );
        self.bytes += payload_len;
        sync_dir(&self.entries_root()).map_err(|source| StoreError::Io {
            path: self.entries_root(),
            source,
        })?;

        Ok(StoredEntry {
            key: write.key.clone(),
            payload_len,
            payload_sha256,
            sequence,
            evicted,
        })
    }

    /// Removes the entry stored under `key`, reporting whether there was
    /// one. This is how an entry that failed its read is dropped: the
    /// bytes are already refused, and the rebuild's commit replaces it.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] if a present entry cannot be removed.
    pub fn discard(&mut self, key: &CacheKey) -> Result<bool, StoreError> {
        let name = key.digest().to_hex();
        let path = self.entry_path(key);
        if !path.exists() {
            return Ok(false);
        }
        remove_entry(&path)?;
        if let Some(removed) = self.entries.remove(&name) {
            self.bytes = self.bytes.saturating_sub(removed.payload_len);
        }
        Ok(true)
    }

    /// Removes every entry whose key lists `input`, and nothing else.
    ///
    /// This is the store half of spec F15 AC03: the granularity comes from
    /// the key's own input list, which every persisted record carries, so
    /// an edited livery source drops the derived assets built from it and
    /// leaves every unrelated entry in place.
    ///
    /// # Errors
    ///
    /// [`StoreError::Io`] or [`StoreError::CorruptHeader`] if a record
    /// cannot be read, or an entry cannot be removed.
    pub fn invalidate_source(
        &mut self,
        input: SourceSpanHash,
    ) -> Result<InvalidationReport, StoreError> {
        let mut victims = Vec::new();
        for name in self.entries.keys() {
            let path = self.entries_root().join(name);
            let stored = read_header(&path.join(HEADER_FILE))?;
            if stored.key.depends_on(input) {
                victims.push(name.clone());
            }
        }
        let mut dropped = Vec::new();
        for name in victims {
            let path = self.entries_root().join(&name);
            remove_entry(&path)?;
            if let Some(removed) = self.entries.remove(&name) {
                self.bytes = self.bytes.saturating_sub(removed.payload_len);
            }
            dropped.push(ContentHash::from_hex(&name).map_err(|error| {
                StoreError::CorruptHeader {
                    path: path.clone(),
                    reason: error.to_string(),
                }
            })?);
        }
        let usage = self.usage();
        Ok(InvalidationReport {
            dropped,
            entries: usage.entries,
            bytes: usage.bytes,
        })
    }

    /// Evicts committed entries, lowest write sequence first, until an
    /// entry of `payload_len` bytes fits without displacing itself.
    fn make_room(
        &mut self,
        payload_len: u64,
        replacing: &str,
    ) -> Result<Vec<ContentHash>, StoreError> {
        // An entry that could not fit an *empty* store is refused before
        // anything is evicted: a refusal must not cost the store the
        // entries it is already holding.
        if payload_len > self.budget.max_bytes() {
            return Err(StoreError::Budget(BudgetExceeded::Bytes {
                limit: self.budget.max_bytes(),
                requested: payload_len,
            }));
        }
        let mut evicted = Vec::new();
        loop {
            let (entries, bytes) = match self.entries.get(replacing) {
                Some(held) => (
                    self.entries.len() as u64,
                    self.bytes
                        .saturating_sub(held.payload_len)
                        .saturating_add(payload_len),
                ),
                None => (
                    self.entries.len() as u64 + 1,
                    self.bytes.saturating_add(payload_len),
                ),
            };
            let exceeded = match self.budget.check(entries, bytes) {
                Ok(()) => return Ok(evicted),
                Err(exceeded) => exceeded,
            };
            let Some(victim) = self.oldest_entry(Some(replacing)) else {
                return Err(StoreError::Budget(exceeded));
            };
            let path = self.entries_root().join(&victim);
            remove_entry(&path)?;
            if let Some(removed) = self.entries.remove(&victim) {
                self.bytes = self.bytes.saturating_sub(removed.payload_len);
            }
            evicted.push(ContentHash::from_hex(&victim).map_err(|error| {
                StoreError::CorruptHeader {
                    path: path.clone(),
                    reason: error.to_string(),
                }
            })?);
        }
    }

    /// The entry to evict next: the lowest write sequence, ties broken by
    /// the digest spelling so the order never depends on iteration luck.
    /// `keep` is never chosen — a rebuild must not evict its own entry.
    fn oldest_entry(&self, keep: Option<&str>) -> Option<String> {
        self.entries
            .iter()
            .filter(|(name, _)| Some(name.as_str()) != keep)
            .min_by(|left, right| {
                left.1
                    .sequence
                    .cmp(&right.1.sequence)
                    .then_with(|| left.0.cmp(right.0))
            })
            .map(|(name, _)| name.clone())
    }

    /// Removes every scratch directory: the debris of writes that were
    /// interrupted, whether by a killed process or by a crash after a seal
    /// but before the commit rename.
    fn sweep_staging(&mut self, staging_root: &Path) -> Result<(), StoreError> {
        let entries = fs::read_dir(staging_root).map_err(|source| StoreError::Io {
            path: staging_root.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: staging_root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path).map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            if metadata.file_type().is_symlink() {
                // Not this store's own scratch; a cache never follows or
                // removes a link it did not write.
                self.recovery.kept_unknown += 1;
                continue;
            }
            let removed = if metadata.is_dir() {
                fs::remove_dir_all(&path)
            } else {
                fs::remove_file(&path)
            };
            match removed {
                Ok(()) => self.recovery.swept_staging += 1,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(source) => {
                    return Err(StoreError::Io {
                        path: path.clone(),
                        source,
                    });
                }
            }
        }
        Ok(())
    }

    /// Indexes the published entries, dropping the ones that cannot be
    /// served.
    fn scan_entries(&mut self, entries_root: &Path) -> Result<(), StoreError> {
        let entries = fs::read_dir(entries_root).map_err(|source| StoreError::Io {
            path: entries_root.to_path_buf(),
            source,
        })?;
        for entry in entries {
            let entry = entry.map_err(|source| StoreError::Io {
                path: entries_root.to_path_buf(),
                source,
            })?;
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = fs::symlink_metadata(&path).map_err(|source| StoreError::Io {
                path: path.clone(),
                source,
            })?;
            let digest =
                if metadata.file_type().is_symlink() || !metadata.is_dir() || !is_entry_name(&name)
                {
                    // Not an entry this store published. Leave it alone.
                    self.recovery.kept_unknown += 1;
                    continue;
                } else {
                    match ContentHash::from_hex(&name) {
                        Ok(digest) => digest,
                        Err(_) => {
                            self.recovery.kept_unknown += 1;
                            continue;
                        }
                    }
                };
            match self.index_entry(&path, &name, digest) {
                Ok(()) => {}
                Err(Unusable::Uncommitted) => {
                    remove_entry(&path)?;
                    self.recovery.dropped_uncommitted += 1;
                }
                Err(Unusable::Corrupt) => {
                    remove_entry(&path)?;
                    self.recovery.dropped_corrupt += 1;
                }
            }
        }
        Ok(())
    }

    /// Reads one published entry, keeping it only if it is complete and
    /// self-consistent. Every record it cannot verify — undecodable,
    /// uncommitted, stored under another key, or declaring a payload
    /// length the file does not have — is unusable: the bytes cannot be
    /// served, so the entry is dropped and rebuilt from its sources.
    fn index_entry(
        &mut self,
        path: &Path,
        name: &str,
        digest: ContentHash,
    ) -> Result<(), Unusable> {
        let stored = read_header(&path.join(HEADER_FILE)).map_err(|_| Unusable::Corrupt)?;
        if stored.key.digest() != digest {
            return Err(Unusable::Corrupt);
        }
        let EntryState::Committed { payload_len, .. } = stored.state else {
            return Err(Unusable::Uncommitted);
        };
        let found = match fs::metadata(path.join(PAYLOAD_FILE)) {
            Ok(metadata) => metadata.len(),
            Err(_) => return Err(Unusable::Corrupt),
        };
        if found != payload_len {
            return Err(Unusable::Corrupt);
        }
        self.bytes += payload_len;
        self.next_sequence = self.next_sequence.max(stored.sequence + 1);
        self.entries.insert(
            name.to_owned(),
            IndexedEntry {
                payload_len,
                sequence: stored.sequence,
            },
        );
        Ok(())
    }
}

/// Why a published entry was not indexed.
enum Unusable {
    /// The record never reached `committed`: a partially written entry.
    Uncommitted,
    /// The record, its key or its payload length is inconsistent, or the
    /// entry could not be read at all.
    Corrupt,
}

/// Whether `name` is this store's own entry directory spelling: the
/// canonical 64-character lowercase hex of a SHA-256 digest.
fn is_entry_name(name: &str) -> bool {
    name.len() == 64
        && name
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

/// Creates `path` if it is missing, refusing a symbolic link in its place.
fn create_dir(path: &Path) -> Result<(), StoreError> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() || !metadata.is_dir() => {
            Err(StoreError::Io {
                path: path.to_path_buf(),
                source: io::Error::other(
                    "the store's own directory is a symbolic link or not a directory",
                ),
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            fs::create_dir(path).map_err(|source| StoreError::Io {
                path: path.to_path_buf(),
                source,
            })
        }
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Removes one entry directory, ignoring an already absent one.
fn remove_entry(path: &Path) -> Result<(), StoreError> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(source) => Err(StoreError::Io {
            path: path.to_path_buf(),
            source,
        }),
    }
}

/// Flushes a directory entry so a completed rename or creation survives a
/// power loss.
///
/// Unix can fsync a directory handle; Windows cannot, and there the
/// rename's own ordering is what the platform offers.
#[cfg(unix)]
fn sync_dir(path: &Path) -> io::Result<()> {
    fs::File::open(path)?.sync_all()
}

#[cfg(not(unix))]
fn sync_dir(_path: &Path) -> io::Result<()> {
    Ok(())
}

/// Writes the persisted record of one entry.
fn write_header(path: &Path, header: &StoredHeader) -> io::Result<()> {
    let mut text = String::with_capacity(512);
    text.push_str(HEADER_FORMAT);
    text.push('\n');
    text.push_str(match header.state {
        EntryState::Writing => "state writing\n",
        EntryState::Committed { .. } => "state committed\n",
    });
    let key = &header.key;
    text.push_str(&format!("key {}\n", key.digest()));
    text.push_str(&format!("install {}\n", key.install()));
    for input in key.inputs() {
        text.push_str(&format!("input {}\n", input.digest()));
    }
    text.push_str(&format!("decoder {}\n", key.converter().decoder));
    text.push_str(&format!(
        "decoder_version {}\n",
        key.converter().decoder_version
    ));
    text.push_str(&format!("ir {}\n", key.converter().ir.0));
    for option in key.options().as_slice() {
        text.push_str(&format!("option {} {}\n", option.name, option.value));
    }
    if let EntryState::Committed {
        payload_len,
        payload_sha256,
    } = header.state
    {
        text.push_str(&format!("length {payload_len}\n"));
        text.push_str(&format!("payload {payload_sha256}\n"));
        text.push_str(&format!("sequence {}\n", header.sequence));
    }
    let mut file = fs::File::create(path)?;
    file.write_all(text.as_bytes())?;
    file.sync_all()
}

/// Reads and strictly decodes a persisted record.
///
/// A record is kept only when the key rebuilt from its own facets hashes
/// to the digest the record declares; every other shape is
/// [`StoreError::CorruptHeader`], so a truncated, hand-edited or foreign
/// record can never become a served entry.
fn read_header(path: &Path) -> Result<StoredHeader, StoreError> {
    let unreadable = |source| StoreError::Io {
        path: path.to_path_buf(),
        source,
    };
    let text = fs::read_to_string(path).map_err(unreadable)?;
    let mut lines = text.lines();
    if lines.next() != Some(HEADER_FORMAT) {
        return Err(corrupt(path, "the first line is not the header format"));
    }
    // Single-valued fields, each of which may appear at most once.
    let mut fields: BTreeMap<&str, &str> = BTreeMap::new();
    let mut inputs: Vec<&str> = Vec::new();
    let mut options: Vec<(&str, &str)> = Vec::new();
    for line in lines {
        let Some((field, value)) = line.split_once(' ') else {
            return Err(corrupt(path, "a header line is not a field and a value"));
        };
        if value.is_empty() {
            return Err(corrupt(path, "a header field has no value"));
        }
        match field {
            "input" => inputs.push(value),
            "option" => {
                let Some((name, option)) = value.split_once(' ') else {
                    return Err(corrupt(path, "an option line is not a name and a value"));
                };
                if name.is_empty() || option.is_empty() {
                    return Err(corrupt(path, "an option field is empty"));
                }
                options.push((name, option));
            }
            _ if is_single_value_field(field) => {
                if fields.insert(field, value).is_some() {
                    return Err(corrupt(path, &format!("the {field} field is repeated")));
                }
            }
            _ => return Err(corrupt(path, "an unknown header field")),
        }
    }
    let field = |name: &str| fields.get(name).copied();
    let state = match field("state") {
        Some("writing") => EntryState::Writing,
        Some("committed") => EntryState::Committed {
            payload_len: number(path, "length", field("length"))?,
            payload_sha256: hash(path, "payload", field("payload"))?,
        },
        Some(_) => return Err(corrupt(path, "the state is neither writing nor committed")),
        None => return Err(corrupt(path, "the record has no state")),
    };
    if matches!(state, EntryState::Writing)
        && (field("length").is_some() || field("payload").is_some() || field("sequence").is_some())
    {
        return Err(corrupt(path, "a writing record declares committed fields"));
    }
    if inputs.is_empty() {
        return Err(corrupt(path, "the record lists no source input"));
    }
    let declared = hash(path, "key", field("key"))?;
    let rebuilt = rebuild_key(
        path,
        hash(path, "install", field("install"))?,
        &inputs,
        field("decoder"),
        field("decoder_version"),
        field("ir"),
        &options,
    )?;
    if rebuilt.digest() != declared {
        return Err(corrupt(
            path,
            "the recorded key facets do not hash to the recorded key",
        ));
    }
    Ok(StoredHeader {
        state,
        key: rebuilt,
        sequence: match field("sequence") {
            Some(value) => number(path, "sequence", Some(value))?,
            None => 0,
        },
    })
}

/// The record fields that may appear at most once.
const SINGLE_VALUE_FIELDS: [&str; 9] = [
    "state",
    "key",
    "install",
    "decoder",
    "decoder_version",
    "ir",
    "length",
    "payload",
    "sequence",
];

/// Whether `field` is a record field that may appear at most once.
fn is_single_value_field(field: &str) -> bool {
    SINGLE_VALUE_FIELDS.contains(&field)
}

/// Rebuilds a [`CacheKey`] from the facets a record declares, through the
/// contract's own constructors — so a record is only accepted when its
/// fields are a spelling the contract itself would have produced.
#[allow(clippy::too_many_arguments)]
fn rebuild_key(
    path: &Path,
    install: ContentHash,
    inputs: &[&str],
    decoder: Option<&str>,
    decoder_version: Option<&str>,
    ir: Option<&str>,
    options: &[(&str, &str)],
) -> Result<CacheKey, StoreError> {
    let inputs = inputs
        .iter()
        .map(|hex| Ok(SourceSpanHash::from_digest(hash(path, "input", Some(hex))?)))
        .collect::<Result<Vec<_>, StoreError>>()?;
    let decoder =
        DecoderId::new(decoder.ok_or_else(|| corrupt(path, "the record has no decoder"))?)
            .map_err(|error| corrupt(path, &error.to_string()))?;
    let converter =
        ConverterVersion {
            decoder,
            decoder_version: u32::try_from(number(path, "decoder_version", decoder_version)?)
                .map_err(|error| {
                    corrupt(
                        path,
                        &format!("the decoder_version is out of range: {error}"),
                    )
                })?,
            ir: IrVersion(u32::try_from(number(path, "ir", ir)?).map_err(|error| {
                corrupt(path, &format!("the ir version is out of range: {error}"))
            })?),
        };
    let options = options
        .iter()
        .map(|(name, value)| {
            ConversionOption::new(name, value).map_err(|error| corrupt(path, &error.to_string()))
        })
        .collect::<Result<Vec<_>, StoreError>>()?;
    let options =
        ConversionOptions::new(options).map_err(|error| corrupt(path, &error.to_string()))?;
    CacheKey::from_hashed_inputs(install, &inputs, converter, options)
        .map_err(|error| corrupt(path, &error.to_string()))
}

fn corrupt(path: &Path, reason: &str) -> StoreError {
    StoreError::CorruptHeader {
        path: path.to_path_buf(),
        reason: reason.to_owned(),
    }
}

fn hash(path: &Path, field: &str, value: Option<&str>) -> Result<ContentHash, StoreError> {
    let Some(value) = value else {
        return Err(corrupt(path, "a required header field is missing"));
    };
    ContentHash::from_hex(value)
        .map_err(|error| corrupt(path, &format!("the {field} field is not a digest: {error}")))
}

fn number(path: &Path, field: &str, value: Option<&str>) -> Result<u64, StoreError> {
    let Some(value) = value else {
        return Err(corrupt(path, "a required header field is missing"));
    };
    value
        .parse::<u64>()
        .map_err(|error| corrupt(path, &format!("the {field} field is not a number: {error}")))
}

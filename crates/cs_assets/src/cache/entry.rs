//! The stored-entry integrity contract: what a cache entry must record and
//! when it may be served (F15-A).
//!
//! Spec F15 non-negotiable behavior 3: "A partially written cache entry
//! fails integrity validation and is rebuilt. Cache corruption cannot
//! change campaign state." This module is the typed form of that rule:
//!
//! * [`EntryHeader`] is what a store persists about one entry — its
//!   [`CacheKey`] and an [`EntryState`] that is either `Writing` or
//!   `Committed` with the payload's declared length and digest. The commit
//!   flag is the atomic-publish boundary of the contract: a header that
//!   never reached `Committed` describes a write that did not finish,
//!   whatever bytes happen to sit behind it.
//! * [`verify_entry`] is the only way stored bytes become usable. It
//!   refuses an uncommitted header, a header stored under another key, a
//!   payload shorter than declared and a payload whose digest differs —
//!   every refusal is an [`IntegrityError`] that means *rebuild*, never
//!   *serve anyway* and never *repair in place*.
//! * [`VerifiedEntry`] is the product of a passed check: the only type that
//!   exposes payload bytes, so no consumer can accidentally serve bytes
//!   that were never verified.
//!
//! How the on-disk store makes the commit boundary atomic is F15-B's
//! decision: [`super::store`] stages every byte in a scratch directory and
//! publishes the finished pair with a single directory rename, so this
//! module fixes only the semantics a reader can rely on regardless.

use std::fmt;

use cs_types::evidence::ContentHash;

use crate::cache::key::CacheKey;
use crate::install::sha256;

/// The publish state a stored entry recorded.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EntryState {
    /// The write started but the commit boundary was never crossed: the
    /// process died, the write was aborted, or the store was never
    /// finished. The payload behind such a header is partial by contract
    /// and must be rebuilt.
    Writing,
    /// The write finished and the entry was published, declaring the
    /// payload's length and digest at commit time.
    Committed {
        /// Declared payload length in bytes.
        payload_len: u64,
        /// Declared SHA-256 of the payload.
        payload_sha256: ContentHash,
    },
}

impl EntryState {
    /// The stable label used in reports.
    pub const fn label(&self) -> &'static str {
        match self {
            Self::Writing => "writing",
            Self::Committed { .. } => "committed",
        }
    }
}

/// What a store persists about one derived entry: its key and its publish
/// state.
///
/// A header is built either as [`EntryHeader::writing`] — the state every
/// write starts in — or as [`EntryHeader::committed`], which computes the
/// declared length and digest from the payload being published. There is
/// no way to construct a `Committed` header that does not match the bytes
/// it was committed with.
#[derive(Clone, Debug)]
pub struct EntryHeader {
    key: CacheKey,
    state: EntryState,
}

impl EntryHeader {
    /// The header of a write in progress: not publishable, not verifiable.
    pub fn writing(key: CacheKey) -> Self {
        Self {
            key,
            state: EntryState::Writing,
        }
    }

    /// The header a completed write commits with, its declared length and
    /// digest computed from `payload`.
    pub fn committed(key: CacheKey, payload: &[u8]) -> Self {
        Self {
            key,
            state: EntryState::Committed {
                payload_len: payload.len() as u64,
                payload_sha256: sha256(payload),
            },
        }
    }

    /// The header a *streaming* write commits with, from a length and a
    /// digest the writer already computed.
    ///
    /// A bounded writer (F15-B's [`PendingStoreWrite`]) appends the payload
    /// in chunks and never holds it whole, so it cannot call
    /// [`EntryHeader::committed`]; it hashes the same bytes incrementally
    /// instead and declares what it wrote. That makes the invariant the
    /// caller's to keep — the digest is over exactly `payload_len` bytes —
    /// and [`verify_entry`] re-establishes it at read time: a writer that
    /// misreports its length or digest has its entry refused
    /// (`LengthMismatch`, `DigestMismatch`) and rebuilt, never served.
    pub fn committed_streaming(
        key: CacheKey,
        payload_len: u64,
        payload_sha256: ContentHash,
    ) -> Self {
        Self {
            key,
            state: EntryState::Committed {
                payload_len,
                payload_sha256,
            },
        }
    }

    /// The key this entry was stored under.
    pub fn key(&self) -> &CacheKey {
        &self.key
    }

    /// The publish state the entry recorded.
    pub fn state(&self) -> EntryState {
        self.state
    }
}

/// Why a stored entry failed integrity validation.
///
/// Every variant means the same recovery: rebuild the entry from its
/// sources. None of them is a licence to serve the bytes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum IntegrityError {
    /// The recorded state is `Writing`: a partially written entry. Spec
    /// F15 non-negotiable behavior 3 — it is rebuilt, not served.
    Uncommitted,
    /// The stored header's key is not the key that was asked for: the
    /// store returned an entry that belongs to another derivation.
    WrongKey {
        /// The key that was asked for.
        requested: ContentHash,
        /// The key the stored header records.
        stored: ContentHash,
    },
    /// The payload's actual length disagrees with the committed header:
    /// the write was truncated or extended after commit.
    LengthMismatch {
        /// The length the header committed.
        declared: u64,
        /// The length actually read.
        actual: u64,
    },
    /// The payload's digest disagrees with the committed header: the bytes
    /// are corrupt or belong to another entry.
    DigestMismatch {
        /// The digest the header committed.
        declared: ContentHash,
        /// The digest of the bytes actually read.
        actual: ContentHash,
    },
}

impl IntegrityError {
    /// The stable lowercase code used in reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Uncommitted => "uncommitted",
            Self::WrongKey { .. } => "wrong_key",
            Self::LengthMismatch { .. } => "length_mismatch",
            Self::DigestMismatch { .. } => "digest_mismatch",
        }
    }
}

impl fmt::Display for IntegrityError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Uncommitted => write!(
                f,
                "the entry was never committed: a partially written entry is rebuilt"
            ),
            Self::WrongKey { requested, stored } => write!(
                f,
                "the stored entry answers key {stored}, not the requested {requested}"
            ),
            Self::LengthMismatch { declared, actual } => write!(
                f,
                "the payload is {actual} bytes but the committed header declares {declared}"
            ),
            Self::DigestMismatch { declared, actual } => write!(
                f,
                "the payload hashes to {actual} but the committed header declares {declared}"
            ),
        }
    }
}

impl std::error::Error for IntegrityError {}

/// Stored bytes that passed integrity validation: the only form a cache
/// payload may be consumed in.
///
/// Constructed exclusively by [`verify_entry`]; there is no other way to
/// claim the bytes are the ones the entry committed.
#[derive(Clone, Debug)]
pub struct VerifiedEntry {
    key: CacheKey,
    payload: Vec<u8>,
}

impl VerifiedEntry {
    /// The key the verified entry serves.
    pub fn key(&self) -> &CacheKey {
        &self.key
    }

    /// The verified payload bytes.
    pub fn payload(&self) -> &[u8] {
        &self.payload
    }
}

/// The integrity gate between a store and a consumer.
///
/// `requested` is the key the lookup wanted, `header` the persisted record
/// and `payload` the stored bytes. The checks run in the order a corruption
/// is discovered: an uncommitted header is partial regardless of the bytes
/// behind it; a mismatched key means the store answered a different
/// derivation; a length mismatch precedes hashing; only a payload that
/// passes all of them becomes a [`VerifiedEntry`].
///
/// # Errors
///
/// Every [`IntegrityError`] variant means *rebuild from sources*; the
/// caller does not get partial bytes to recover with.
pub fn verify_entry(
    requested: &CacheKey,
    header: &EntryHeader,
    payload: &[u8],
) -> Result<VerifiedEntry, IntegrityError> {
    let EntryState::Committed {
        payload_len,
        payload_sha256,
    } = header.state()
    else {
        return Err(IntegrityError::Uncommitted);
    };
    if header.key() != requested {
        return Err(IntegrityError::WrongKey {
            requested: requested.digest(),
            stored: header.key().digest(),
        });
    }
    if payload.len() as u64 != payload_len {
        return Err(IntegrityError::LengthMismatch {
            declared: payload_len,
            actual: payload.len() as u64,
        });
    }
    let actual = sha256(payload);
    if actual != payload_sha256 {
        return Err(IntegrityError::DigestMismatch {
            declared: payload_sha256,
            actual,
        });
    }
    Ok(VerifiedEntry {
        key: requested.clone(),
        payload: payload.to_vec(),
    })
}

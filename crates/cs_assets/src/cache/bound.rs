//! The cache's location and size contract: private and bounded (F15-A).
//!
//! Spec F15 non-negotiable behavior 1: "Cache stores are private, bounded
//! and atomic; no writes to source installation." This module holds the
//! two halves of that rule that are contract-level rather than
//! store-mechanics:
//!
//! * [`CacheDirectory`] is the private location: an existing directory,
//!   not a symbolic link, whose canonical path does not lie inside the
//!   installation it caches. The check mirrors
//!   [`crate::vfs::export::ExportDirectory`]'s rule — a cache inside the
//!   mounted installation would write derived data into the owner's
//!   read-only source tree. Which directory is used is the caller's choice
//!   (typically a per-user private path); what this type guarantees is only
//!   that it cannot be inside the installation.
//! * [`CacheBudget`] is the bound: a maximum entry count and a maximum
//!   byte total, both nonzero, that every store write is checked against.
//!   F15-B decides the eviction policy; this type fixes that a store is
//!   always opened *with* a bound and that exceeding it is a typed refusal,
//!   never silent growth.
//!
//! The atomicity of a write is not expressible as a location or a number;
//! its contract lives in [`super::entry`] (`EntryState::Writing` never
//! verifies), and its mechanism is F15-B's.

use std::fmt;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

/// Why a directory cannot serve as the cache root.
#[derive(Debug)]
pub enum CacheLocationError {
    /// The root is missing, not a directory, a symbolic link, or could not
    /// be canonicalized.
    Unavailable {
        /// The requested root.
        path: PathBuf,
        /// Why it cannot be used.
        source: io::Error,
    },
    /// The root lies inside the installation it would cache: derived data
    /// would be written into the owner's read-only source tree.
    InsideInstall {
        /// The requested root.
        path: PathBuf,
        /// The canonical installation root that contains it.
        install: PathBuf,
    },
}

impl fmt::Display for CacheLocationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Unavailable { path, source } => {
                write!(f, "cache root {} is unusable: {source}", path.display())
            }
            Self::InsideInstall { path, install } => write!(
                f,
                "cache root {} lies inside the installation at {}; the cache never \
                 writes into the source installation",
                path.display(),
                install.display()
            ),
        }
    }
}

impl std::error::Error for CacheLocationError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Unavailable { source, .. } => Some(source),
            Self::InsideInstall { .. } => None,
        }
    }
}

/// A validated private cache directory.
///
/// The only way to obtain one is [`CacheDirectory::open`], which applies
/// the location rule once so no later write path needs to re-argue it.
#[derive(Clone, Debug)]
pub struct CacheDirectory {
    root: PathBuf,
}

impl CacheDirectory {
    /// Opens `root` as the cache directory for the installation at
    /// `install_root`.
    ///
    /// `root` must exist, be a directory and not a symbolic link. Both
    /// paths are canonicalized before the containment check, so a root
    /// that only *textually* escapes the installation (through `..` or a
    /// symbolic parent) is still refused.
    ///
    /// # Errors
    ///
    /// [`CacheLocationError::Unavailable`] or
    /// [`CacheLocationError::InsideInstall`].
    pub fn open(root: &Path, install_root: &Path) -> Result<Self, CacheLocationError> {
        let unavailable = |source| CacheLocationError::Unavailable {
            path: root.to_path_buf(),
            source,
        };
        let metadata = fs::symlink_metadata(root).map_err(unavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(unavailable(io::Error::new(
                io::ErrorKind::NotADirectory,
                "the cache root must be a directory, not a symbolic link",
            )));
        }
        let canonical = fs::canonicalize(root).map_err(unavailable)?;
        let install =
            fs::canonicalize(install_root).map_err(|source| CacheLocationError::Unavailable {
                path: install_root.to_path_buf(),
                source,
            })?;
        if canonical.starts_with(&install) {
            return Err(CacheLocationError::InsideInstall {
                path: canonical,
                install,
            });
        }
        Ok(Self { root: canonical })
    }

    /// The canonical cache root.
    pub fn root(&self) -> &Path {
        &self.root
    }
}

/// Why a [`CacheBudget`] was rejected.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetError {
    /// A bound of zero is not a bound; it is a refusal to cache disguised
    /// as configuration.
    Zero {
        /// Which bound was zero.
        field: &'static str,
    },
}

impl fmt::Display for BudgetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Zero { field } => write!(f, "cache budget {field} must be nonzero"),
        }
    }
}

impl std::error::Error for BudgetError {}

/// The bound a store is opened with: how many entries and how many bytes
/// it may hold.
///
/// Both limits are required and nonzero, so "unbounded" is not a state a
/// store can be configured into. What a store does when a write would
/// exceed the budget — evict, refuse or rebuild — is F15-B's policy; this
/// type fixes that the check exists and what it reports.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CacheBudget {
    max_entries: u64,
    max_bytes: u64,
}

impl CacheBudget {
    /// Builds a budget, refusing a zero bound.
    ///
    /// # Errors
    ///
    /// [`BudgetError::Zero`] naming which bound was zero.
    pub fn new(max_entries: u64, max_bytes: u64) -> Result<Self, BudgetError> {
        if max_entries == 0 {
            return Err(BudgetError::Zero {
                field: "max_entries",
            });
        }
        if max_bytes == 0 {
            return Err(BudgetError::Zero { field: "max_bytes" });
        }
        Ok(Self {
            max_entries,
            max_bytes,
        })
    }

    /// The entry-count bound.
    pub fn max_entries(&self) -> u64 {
        self.max_entries
    }

    /// The byte bound.
    pub fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Whether `entries` entries totaling `bytes` bytes fit the budget.
    ///
    /// # Errors
    ///
    /// [`BudgetExceeded`] naming the bound that would be crossed, the limit
    /// and the requested amount.
    pub fn check(&self, entries: u64, bytes: u64) -> Result<(), BudgetExceeded> {
        if entries > self.max_entries {
            return Err(BudgetExceeded::Entries {
                limit: self.max_entries,
                requested: entries,
            });
        }
        if bytes > self.max_bytes {
            return Err(BudgetExceeded::Bytes {
                limit: self.max_bytes,
                requested: bytes,
            });
        }
        Ok(())
    }
}

/// Which bound a store state would cross.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BudgetExceeded {
    /// The entry count would exceed the budget.
    Entries {
        /// The configured bound.
        limit: u64,
        /// The count that was requested.
        requested: u64,
    },
    /// The byte total would exceed the budget.
    Bytes {
        /// The configured bound.
        limit: u64,
        /// The total that was requested.
        requested: u64,
    },
}

impl BudgetExceeded {
    /// The stable lowercase code used in reports.
    pub const fn code(&self) -> &'static str {
        match self {
            Self::Entries { .. } => "entries",
            Self::Bytes { .. } => "bytes",
        }
    }
}

impl fmt::Display for BudgetExceeded {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Entries { limit, requested } => write!(
                f,
                "the cache would hold {requested} entries, over its budget of {limit}"
            ),
            Self::Bytes { limit, requested } => write!(
                f,
                "the cache would hold {requested} bytes, over its budget of {limit}"
            ),
        }
    }
}

impl std::error::Error for BudgetExceeded {}

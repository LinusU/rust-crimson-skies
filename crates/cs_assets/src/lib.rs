//! Read-only installation mounts, discovery, IO and derived caches.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. [`install`] defines the typed
//! inventory path (discovered host files to validated manifest) for F02,
//! including the F02-B path that walks a real installation and hashes bytes
//! (`discover`, `discover_with_cache`, streaming `Sha256`, `fingerprint`,
//! `content_fingerprint`, `AnalysisCache`). Allowed dependencies:
//! [`cs_types`] and [`cs_formats`]. The original installation at
//! `$CS_GAME_DIR` is read-only and nothing derived from it is committed to
//! Git.
//!
//! [`install`]: install
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats

pub mod install;

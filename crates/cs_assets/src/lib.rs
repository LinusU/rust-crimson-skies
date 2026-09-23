//! Read-only installation mounts, discovery, IO and derived caches.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. [`install`] defines the typed
//! inventory path (discovered host files to validated manifest) for F02;
//! walking a real installation and hashing bytes arrive with F02-B. Allowed
//! dependencies: [`cs_types`] and [`cs_formats`]. The original installation
//! at `$CS_GAME_DIR` is read-only and nothing derived from it is committed
//! to Git.
//!
//! [`install`]: install
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats

pub mod install;

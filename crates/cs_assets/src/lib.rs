//! Read-only installation mounts, discovery, IO and derived caches.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. No implementation yet: mounts
//! and discovery land with the F02+ tasks. Allowed dependencies: [`cs_types`]
//! and [`cs_formats`]. The original installation at `$CS_GAME_DIR` is read-only
//! and nothing derived from it is committed to Git.
//!
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats

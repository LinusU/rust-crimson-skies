//! Normalization, catalog, blueprints, localization and saves.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. No implementation yet: content
//! normalization lands with the F04+ tasks. Allowed dependencies: [`cs_types`]
//! and [`cs_formats`] (VFS interfaces from `cs_assets` only where needed). It
//! must never depend on Bevy or Avian.
//!
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats

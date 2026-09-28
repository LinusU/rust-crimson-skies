//! Normalization, catalog, blueprints, localization and saves.
//!
//! Owner crate per `docs/01-ARCHITECTURE.md`. Allowed dependencies: [`cs_types`],
//! [`cs_formats`] and, where a normalization needs the filesystem,
//! [`cs_assets`]. It must never depend on Bevy or Avian.
//!
//! [`loading`] is the F07-C work: the loading-plan adapter
//! (`specs/F07-interp-loading-script-container.md`, `### F07-C`). It reads a
//! container `cs_formats::decode_interp` validated, classifies its lines with
//! `cs_formats::plan_interp_loading` and resolves the registered loading
//! commands through a content session, producing the world's dependency closure
//! and the lines that fail it. It ships no command registrations: which
//! commands load resources is F07-D's measurement, so until then every line is
//! unclassified and every world's plan fails with the line's source offset and
//! the world it affects rather than reporting a loaded state.
//!
//! [`cs_types`]: cs_types
//! [`cs_formats`]: cs_formats
//! [`cs_assets`]: cs_assets

pub mod loading;

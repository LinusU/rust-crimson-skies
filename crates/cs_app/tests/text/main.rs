//! F51-A/F51-B acceptance tests: locale fallback, control-markup grammar, font
//! provenance, the fit-or-scroll layout, resource decoding and the screen text
//! pipeline.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stages `### F51-A` and `### F51-B`; shared contract
//! `docs/contracts/UI-NETWORK.md`. Task test prefixes: `accept_f51_a_` and
//! `accept_f51_b_`.
//!
//! These tests drive production code only: `cs_content::localization` owns the
//! declared records, their validation, the markup parser and the F12 resource
//! decode, and `cs_app::text` owns the measurement input, the layout and the
//! screen text pipeline. No test carries its own fallback walk, its own
//! grammar, its own decode or its own wrapping.
//!
//! The F51-B minimum scenario — *malformed markup and absent glyphs produce
//! visible diagnostics* — lives in `screen`, reached through the decoded F12
//! rows; `resource` owns the decode report.
//!
//! The F51-A minimum scenario — *long localized text fits or scrolls without
//! covering required buttons* — lives in `layout`. `catalog` owns the locale
//! fallback and the locale-independent identity, `markup` the grammar
//! validation and `fonts` the provenance and glyph coverage.
//!
//! No original data and no `CS_GAME_DIR` access: every value here is authored
//! development content, so these tests prove the interface and the contract,
//! never the original game.

mod catalog;
mod common;
mod fonts;
mod layout;
mod markup;
mod resource;
mod screen;

//! F51-A acceptance tests: locale fallback, control-markup grammar, font
//! provenance and the fit-or-scroll layout.
//!
//! Spec: `specs/F51-localization-fonts-text-layout-and-original-media-ids.md`,
//! stage `### F51-A`; shared contract `docs/contracts/UI-NETWORK.md`.
//! Task test prefix: `accept_f51_a_`.
//!
//! These tests drive production code only: `cs_content::localization` owns the
//! declared records, their validation and the markup parser, and `cs_app::text`
//! owns the measurement input and the layout. No test carries its own fallback
//! walk, its own grammar or its own wrapping.
//!
//! The stage's minimum scenario — *long localized text fits or scrolls without
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

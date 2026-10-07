//! Acceptance stage F45-A: the front-end state table
//! (`specs/F45-main-menu-pandora-cabin-briefing-and-flight-check.md`, section
//! `### F45-A`).
//!
//! The minimum scenario is AC01's shape — a fresh profile to first flight and
//! back, through buttons only — plus AC02's shape (Cancel/Back at each
//! preflight screen asks the domain for nothing), AC03's data half (a failed
//! load keeps the selection and retries without a restart) and the contract's
//! hotspot rule. Everything is authored synthetic data; no original screen,
//! coordinate or file is read, so this proves the table and its machine only,
//! never the original front end (F45-B/D).
//!
//! Acceptance stage F45-B (`mod screens`) adds the original-asset screens:
//! a validated `ScreenDeck` of artwork and hotspots presented by
//! `ScreenSession`, and AC02's minimum scenario driven through that authored
//! art. Its fixture data is authored for the same
//! reason as F45-A's: no original front-end layout is decoded anywhere in this
//! repository (see
//! `docs/findings/2026-10-07-f45-b-original-asset-screen-decks.md`), so the
//! original screens themselves are F45-D's capture.
//!
//! Acceptance stage F45-C (`mod wiring`) adds the flows themselves: the
//! `FrontEndFlow` that runs every transition's domain transaction *before*
//! the machine moves, the resource ledger that consumes the acquire/release
//! stream, and the loading screen's real `LoadingSession` over a content
//! session — AC03's minimum scenario, a load whose dependency is missing,
//! fails, is repaired and is retried in the same process.

mod flow;
mod layout;
mod screens;
mod table;
mod wiring;

use cs_types::content::{ContentId, ContentKind};

/// A synthetic content id.
pub fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id")
}

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

mod flow;
mod layout;
mod table;

use cs_types::content::{ContentId, ContentKind};

/// A synthetic content id.
pub fn id(kind: ContentKind, key: &str) -> ContentId {
    ContentId::from_source(kind, key).expect("test id")
}

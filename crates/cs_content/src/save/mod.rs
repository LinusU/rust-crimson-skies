//! Versioned profile save: byte encoding, bounds and the write/recovery
//! protocol (F48-A, `specs/F48-profiles-saves-settings-migration-and-
//! recovery.md`, contract `docs/contracts/STATE-TRANSACTIONS.md`).
//!
//! [`codec`] turns a `cs_types::profile::ProfileDocument` into a checksummed
//! line document and back, with hard size bounds, unsupported-major refusal
//! and verbatim preservation of unknown same-major fields. [`store`] defines
//! the five write phases, the [`store::SaveStorage`] seam, a deterministic
//! in-memory storage with crash injection, the commit sequence and the
//! recovery rule (highest valid revision, never a merge of two files).
//!
//! Real filesystem persistence, fsync and the Windows replacement test are
//! F48-B/F48-D; nothing here claims them. The format is newly authored
//! engine design, not an original-game format.

pub mod codec;
pub mod store;

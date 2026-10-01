//! Versioned profile save: byte encoding, bounds, atomic persistence, ids and
//! recovery (F48-A `specs/F48-profiles-saves-settings-migration-and-
//! recovery.md`, stage F48-B, contract `docs/contracts/STATE-TRANSACTIONS.md`).
//!
//! [`codec`] turns a `cs_types::profile::ProfileDocument` into a checksummed
//! line document and back, with hard size bounds, unsupported-major refusal
//! and verbatim preservation of unknown same-major fields. [`store`] defines
//! the five write phases, the [`store::SaveStorage`] seam, a deterministic
//! in-memory storage with crash injection, the commit sequence and the
//! selection rule (highest valid revision, never a merge of two files).
//! [`fs`] is the production path: [`fs::DirStorage`] maps each phase onto a
//! real file operation, [`fs::Registry`] persists the live ids, the active
//! pointer and the id high-water mark through those same phases, and
//! [`fs::Replacement`]/[`fs::directory_sync_supported`] report what this
//! platform actually does instead of assuming POSIX semantics.
//! [`library::ProfileLibrary`] is where the pieces meet: one population's
//! profiles, allocating persistent ids above the persisted high-water mark,
//! committing revisions and reporting every recovery as text a caller can
//! show.
//!
//! The formats are newly authored engine design, not original-game formats, so
//! nothing here is `verified_original`. The cross-platform crash/recovery
//! matrix and the Windows replacement measurement are F48-D; this stage
//! implements and tests the path on the platform it runs on and names what it
//! does not measure.

pub mod codec;
pub mod fs;
pub mod library;
pub mod store;

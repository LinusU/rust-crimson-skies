# F48-A: versioned profile and save schema — design notes and what stays open

Date: 2026-10-01. Task: F48-A "Define versioned profile and save schema"
(`specs/F48-profiles-saves-settings-migration-and-recovery.md`).
Capabilities used: ordinary build/test only. All data is newly authored
synthetic design; nothing derives from the original game.

## Files and the observable failure

- `crates/cs_types/src/profile.rs`: `ProfileId`, `ProfileKind`, `Revision`,
  `SchemaVersion`, `ProfileDocument`, `ProfileRegistry` (high-water id mark).
- `crates/cs_content/src/save/{mod,codec,store}.rs`: checksummed line codec,
  `SavePhase`, `SaveStorage`, `MemoryStorage` (crash injection), `commit`,
  `recover`.
- `crates/cs_app/src/profile.rs`: `slot_dir`, `SessionOrigin`.
- Wiring: `pub mod` lines in the three `lib.rs` files only.
- Failure without the implementation: crashing any phase of a second commit
  loses the state (no recoverable file) or recovery returns a torn file
  (`accept_f48_a_interrupt_every_write_phase_keeps_a_valid_revision`).

## Decisions

- Recovery takes the valid file with the highest revision and never merges
  fields. An unsupported *major* in any file fails recovery rather than
  falling back to an older file, so a newer build's save is never shadowed or
  overwritten. A newer *minor* is readable and unknown fields are kept.
- Commit refuses a revision not above the stored one (`RevisionConflict`) and
  a slot that holds another profile. A corrupt current is never rotated over
  a good backup. A complete interrupted temp file is installed before the next
  temp is written.
- The checksum is FNV-1a 64: corruption/torn-write detection only.

## Open / not claimed (resolving stages)

- Real filesystem IO, `fsync`, directory sync, Windows replacement semantics
  and per-platform behaviour are **not** exercised: `MemoryStorage` models
  crash-before-phase, a torn temp write and loss of an unsynced temp only. It
  does not model reordered or lost rename metadata. Resolving: F48-B
  (filesystem), F48-D (platform matrix).
- `ProfileRegistry::rebuild_from_live` cannot know a deleted higher id when
  the high-water file is lost; it can reissue it. Recovery from a missing
  high-water file therefore must also surface this limitation or store the
  mark redundantly in each profile save. Resolving: F48-B.
- Registry/active-pointer persistence format is not defined yet (F48-B).
- Setting keys and their live/restart classification are not enumerated; the
  schema only carries the label (F48-C wiring).
- User-data base directory choice: F61. Legacy import: F64.

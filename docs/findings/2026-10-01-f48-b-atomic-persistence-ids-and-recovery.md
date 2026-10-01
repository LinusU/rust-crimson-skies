# F48-B: atomic persistence, ids and recovery — what was built and what stays open

Date: 2026-10-01. Task: F48-B "Implement atomic persistence, ids and recovery"
(`specs/F48-profiles-saves-settings-migration-and-recovery.md`), contract
`docs/contracts/STATE-TRANSACTIONS.md` ("Persistence").

Capabilities used: ordinary build/test only. Every byte the tests write is
newly authored synthetic data in a temporary directory; no test reads
`$CS_GAME_DIR`, a real user profile directory or any original data. Nothing
here is `verified_original` and nothing asserts original-game behavior — the
save format is newly authored engine design.

## Files and the observable failure

- `crates/cs_content/src/save/fs.rs`: `DirStorage` (the five phases as real file
  operations), `SlotError`, `Registry` (live ids, active pointer, id
  high-water mark), its checksummed `CSREG 1.0` document, `load_registry`,
  `commit_registry`, `Replacement`/`directory_sync_supported`/`platform_note`.
- `crates/cs_content/src/save/library.rs`: `ProfileLibrary` — the production
  path over one population's directory, `LibraryStatus`/`LibraryNotice`,
  `slot_name`/`retired_name`, `load_profile_slot`.
- `crates/cs_content/src/save/store.rs`: the selection rule (`Survey`,
  `survey`), the phase sequence (`run_phases`), the offer check
  (`check_offer`/`SlotIdentity`), `RecoveryWarning: Display`,
  `Recovery::warning_lines`, `SaveFile::prefixed_name`.
- `crates/cs_content/src/save/codec.rs`: `sealed_body`/`check_line_count` (the
  shared framing), `parse_u64`/`parse_profile_id`, `DecodeError::Inconsistent`.
- `crates/cs_app/src/profile.rs`: `population_dir`, `LibraryOpenError`,
  `open_library`. Wiring only otherwise; no other `cs_app` file changed.
- Tests: `crates/cs_content/tests/accept_f48_b_atomic_persistence.rs` (20),
  `crates/cs_app/tests/accept_f48_b_profile_path.rs` (2).

Failure without the implementation: a commit interrupted at any phase leaves
either a half-written current file or a revision that is lost with nothing to
recover from, and deleting the highest profile id makes the next one reuse it
(`accept_f48_b_interrupting_any_phase_on_disk_keeps_a_whole_revision`,
`accept_f48_b_deleted_ids_are_never_reissued_across_a_restart`).

## Decisions

- **The five phases are one shared sequence.** `store::run_phases` is the write
  order; `DirStorage` maps each phase onto a file operation. A profile save and
  the registry are different documents with the same durability rules, so they
  commit through the same function rather than two copies of it.
- **The selection rule is one shared function.** `survey` reads current, temp
  and backup in that order, decodes each whole file, refuses an unreadable
  schema outright and takes the valid file with the highest revision, `Current`
  first on a tie. Two files are never combined; every file that did not decode
  is reported and kept.
- **The registry is a slot, not a special case.** `CSREG 1.0` uses the same
  `sealed_body` framing, the same size and line bounds and the same phases. Its
  identity is its population, so a registry of another kind in a directory is
  refused (`RecoverError::RegistryKindMismatch`) and never adopted or
  overwritten.
- **Ids are recoverable from the filesystem as well as from the registry.** A
  slot directory is named `profile-<id>`, and a deleted profile's slot is
  *renamed* to `retired-<id>` rather than removed. `ProfileLibrary::open`
  therefore reconciles: the mark in force is the highest of the registry's mark
  and every id the directory names, and a live slot the registry did not list is
  adopted. Without this, a damaged newest registry falls back to a revision an
  allocation or two behind and hands an already-issued id out again. The
  reconciliation is reported in `LibraryStatus::notices`, never applied
  silently.
- **A registry that cannot be read anywhere is an error, not an empty
  library.** An unreadable mark read as zero is exactly what reissues a deleted
  id. A population that *does* have slots is recovered from their own ids; one
  that has nothing is a `RecoverError`.
- **A registry that decodes but does not hold together is a corrupt file.** A
  mark below a live id or a repeated id is refused
  (`DecodeError::Inconsistent`), not adopted with a lower mark.
- **The install is a rename, and that is asserted.** `accept_f48_b_installing_a_
  revision_is_a_rename_of_the_synced_file` compares the inode of the temp file
  before the install with the inode of the current file after it, so a
  copy-based implementation fails. (The assertion is `#[cfg(unix)]` because it
  reads `MetadataExt::ino`; the byte-level and phase-level assertions run
  everywhere.)
- **Retiring rather than deleting keeps a player's files.** A deleted profile's
  save is not destroyed. The cost is disk: retired slots are never collected by
  this stage.
- **`cs_app::profile::open_library` is where the population rule is enforced.**
  The library does not know about sessions; it is handed one population's
  directory. The automation refusal is applied where the path is produced, so it
  cannot be gone around by a caller that builds the path itself.
- **Wiring edits outside the owner paths:** `crates/cs_content/src/save/mod.rs`
  gained `pub mod library;` and updated module docs;
  `crates/cs_content/src/save/{codec,store,fs}.rs` are inside the owner path.
  No `lib.rs`, `Cargo.toml` or `Cargo.lock` change was needed.

## Sensitivity

Each of these was applied to the implementation and the suite re-run; the
mutations were reverted afterwards.

| Mutation | Test that failed |
| --- | --- |
| `install_current` copies instead of renaming | `accept_f48_b_installing_a_revision_is_a_rename_of_the_synced_file` |
| selection ignores the revision and takes the first file read | `accept_f48_b_the_highest_whole_revision_wins_wherever_it_is` |
| backup rotation no longer requires a valid current | `accept_f48_b_interrupting_any_phase_on_disk_keeps_a_whole_revision` |
| an unsupported schema is treated as an ignorable corrupt file | `accept_f48_b_foreign_and_hostile_files_are_refused_not_overwritten` |
| allocation takes the lowest free index instead of the mark | 5 tests |
| `delete` removes the slot instead of retiring it | 2 tests |
| `ProfileLibrary::open` skips reconciliation | `accept_f48_b_the_registry_round_trips_and_is_bounded` |

## Open / not claimed

- **Windows replacement semantics are not measured.** `platform_note()` reports
  which rename call this build uses on this platform, and the contract refuses to
  assume POSIX behavior on Windows, but no Windows run happened here: this
  machine is macOS. What `MoveFileExW` does to a read-only or open destination,
  and whether a rename survives a real power loss there, is **F48-D**.
- **No crash was induced.** The interruption matrix injects a stop at each phase
  boundary through the production `SaveStorage` seam; the file operations that
  ran are the real ones, but the process was never killed and no `fsync` was
  observed to survive a power cut. A real crash/power-loss matrix per platform is
  **F48-D**.
- **Directory `fsync` is a no-op off unix.** `directory_sync_supported()` reports
  it, so a caller can see that a rename's durability is weaker there rather than
  reading a claim of atomicity. F48-D measures what that means.
- **Two live libraries over one directory are not prevented.** Each would
  believe it owned the population, and the second write would fail the revision
  check rather than corrupt anything. A lock is F48-C's runtime concern, or a
  later stage's; this one does not claim to hold one.
- **Retired slots are never collected.** Nothing prunes them, so a long-lived
  profile directory grows. Any collector must know that an id named in a retired
  slot may not be reissued, so it is deliberately not written here.
- **Settings are not wired.** This stage persists a `SettingEntry` list with its
  `live`/`restart` label through the same document; which settings exist, their
  keys and their restart classification are **F48-C**.
- **Mid-mission suspend is not implemented and not claimed.** The contract calls
  it optional, and nothing here snapshots a running mission.
- **The user-data base directory is still unchosen (F61).** `open_library` is
  handed a base and never picks one; the tests pass a temporary directory.
- **Legacy import is F64**; nothing here reads an original save.

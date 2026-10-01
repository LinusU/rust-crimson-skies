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
- `crates/cs_types/src/profile.rs`: the two counter bounds added in review (see
  below) — `ProfileFieldError::NoSuccessor` and the mark check in
  `ProfileRegistry::from_parts`.
- Tests: `crates/cs_content/tests/accept_f48_b_atomic_persistence.rs` (26),
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

The review added six more tests (20 → 26 here). Each behaviour they cover was
checked the same way — applied to the implementation, the suite re-run, the
mutation reverted:

| Mutation | Test that failed |
| --- | --- |
| the read is unbounded (`fs::read`, bound checked after) | `accept_f48_b_a_save_file_is_read_within_its_bound` — the read of a `/dev/zero` at the save name never ends, so the run is killed rather than finished |
| `write_temp` opens the temp name without looking at it | `accept_f48_b_a_save_is_never_written_through_a_symbolic_link` |
| `ProfileRegistry::from_parts` stops refusing a repeated id | `accept_f48_b_a_counter_with_no_successor_is_refused` |
| `ProfileRegistry::from_parts` stops refusing a mark with no successor | `accept_f48_b_a_counter_with_no_successor_is_refused` |
| `ProfileDocument::validate` stops refusing a revision with no successor | `accept_f48_b_a_counter_with_no_successor_is_refused` |
| the tie-break takes the last file read instead of the first | `accept_f48_b_a_tie_in_revisions_prefers_the_current_file` |
| adoption follows a link and accepts an empty slot | `accept_f48_b_only_a_writable_slot_with_a_save_is_adopted` |

`accept_f48_b_a_slot_with_no_whole_revision_is_still_writable` is a pin, not a
sensitivity case: it records a decision the shared `survey`/`check_offer`/`run_phases`
refactor introduced (a commit no longer goes through `recover`, so a slot with
no readable file at all is written rather than refused). See below.

## Review

The implementer and the reviewer of this stage are the same agent identity
(`bunny-2/bunny-2`), so this is a self-review and not independent evidence; the
reviewer context was fresh (a new session with no memory of the
implementation). What it changed, and why each is a defect rather than a taste:

1. **The size bound was checked after the file had been read.** `DirStorage::read`
   did `fs::read` — the whole file, of any size — and compared the length
   afterwards, while its own comment claimed the bound was enforced "before it is
   parsed". A large or hostile file at a save name was therefore loaded into
   memory in full before anything was decided about it. The read now refuses on
   the length the file reports and caps the read itself, so a path that is not a
   bounded regular file (a device, a pipe) cannot make it read without limit.
2. **A save could be written through a symbolic link.** `DirStorage::create`
   refused a slot *directory* that is a link, and `write_temp` opened the temp
   *file* with `create`+`truncate`, which follows one. A link left at
   `profile.tmp` would have had its target truncated and overwritten — a
   destructive write outside the slot. The temp name is now checked with
   `symlink_metadata` and a non-regular file there is refused. The current and
   backup names need no check: they are only ever replaced by a rename, which
   replaces the directory entry instead of following it.
3. **`Registry::rebuilt` could panic.** It called `ProfileRegistry::rebuild_from_live`,
   which used `.expect(...)`, and then had a second `.expect(...)` behind an
   `unwrap_or`. No production path could reach either — the ids come from a
   de-duplicated directory listing — but the constructor is `pub`, its own doc
   claimed the parts "are still checked by the same rule", and a repeated id
   would have panicked instead of being refused. `rebuilt` and
   `rebuild_from_live` now return the refusal.
4. **A counter with no successor was accepted and then bricked its slot.** A save
   whose `revision` was `u64::MAX` decoded, was installed as the whole newest
   state, and from then on every commit was a `RevisionConflict`: the profile
   could never be saved again, with no diagnostic and no way back except
   deleting the file. A registry whose `high_water` was `u64::MAX` was the same
   for a whole population. Both are now refused where their parts are validated
   (`ProfileDocument::validate`, `ProfileRegistry::from_parts`), so the encoder
   never writes one, the decoder never accepts one, and the previous whole
   revision stays in force instead. This is the F48-A schema gaining two range
   checks, in an owner path of this stage; no F48-A test depended on the old
   behaviour.
5. **Adoption accepted things the library could not write.** Reconciliation
   adopted any `profile-<id>` entry that `Path::is_dir` accepted — a symbolic
   link, or an empty directory left by a `create` killed between making the slot
   and writing its first revision. Both would be offered as pilots whose every
   later save fails. Adoption now requires the same plain-directory test
   `create` and `retire_slot` apply, and a slot that actually holds a save file.
   The ids those entries name still raise the high-water mark, so neither is a
   way to have an id issued again.
6. **The tie-break was asserted nowhere.** The selection rule documents that a tie
   between two files of equal revision resolves to `Current`; the comparator
   implements that (`right.cmp(left)` on the read-order index), and a test now
   pins it. Nothing was wrong with the code; the claim had no test behind it.
7. **`save_registry` reported refusals as IO errors.** Its `Result<_, SlotError>`
   turned a stale revision, a foreign population or an unreadable slot into
   `SlotError::Io`. It now returns the commit's own error.

Decisions taken rather than defects, recorded so a later stage does not
rediscover them:

- **A commit over a slot with no readable file is allowed.** The shared
  `survey`/`check_offer`/`run_phases` sequence reads the slot's three files
  itself, where F48-A's `commit` went through `recover`; so a slot whose current
  and backup are both unreadable is no longer refused. The write still goes
  through the temp file and the atomic install, the unreadable file stays on
  disk, and the alternative would leave a profile whose save and backup were
  both lost permanently unable to save. The offered revision then comes from the
  caller, because nothing on disk can say what it was.
  `accept_f48_b_a_slot_with_no_whole_revision_is_still_writable` pins it.
- **A read of a file that is over the bound is an error for the whole slot, not
  an ignorable corrupt file.** Recovery therefore does not fall back to the
  backup when the current file is oversized. That is the conservative reading of
  "validate size ... before use" and it never overwrites bytes it did not read;
  the cost is that an oversized current file also hides a good backup until it
  is moved aside.

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
- **No runtime consumer calls this yet.** `cs_app::profile::open_library` is
  exercised by the tests in this stage and by nothing else in the tree: the
  session that opens a library, writes a revision and tears it down is the
  wiring **F48-C** does. The path is production code, not a test-only
  implementation, but nothing in a running game calls it yet.
- **Mid-mission suspend is not implemented and not claimed.** The contract calls
  it optional, and nothing here snapshots a running mission.
- **The user-data base directory is still unchosen (F61).** `open_library` is
  handed a base and never picks one; the tests pass a temporary directory.
- **Legacy import is F64**; nothing here reads an original save.

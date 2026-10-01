# F48-D: the crash/recovery matrix on the platform this build runs on

Date: 2026-10-01. Task: F48-D "Run crash/recovery matrix on every supported
platform" (`specs/F48-profiles-saves-settings-migration-and-recovery.md`), stage
`### F48-D`, contract `docs/contracts/STATE-TRANSACTIONS.md` ("Persistence").

Capabilities used: ordinary build/test. Every byte the tests write is newly
authored synthetic data under the system temporary directory, removed on drop.
No test reads `$CS_GAME_DIR`, a real user profile directory or any original
data. Nothing here is `verified_original` and nothing asserts original-game
behavior — the save format is newly authored engine design, and this stage
measures the engine's own write path, not a property of Crimson Skies.

## Files and the observable failure

- `crates/cs_content/src/save/fs.rs`: `SlotPath` and `classify_slot` — one
  judgment about what a slot path may be, now shared by the write path
  (`DirStorage::create`) and, new in this stage, the read paths.
- `crates/cs_content/src/save/library.rs`: `ProfileLibrary::load` refuses a slot
  path that is not a plain directory, refuses a revision that names another
  profile (`LibraryError::ProfileMismatch`) and refuses a revision of another
  population (`LibraryError::ForeignDocument`); `slot_warnings` and
  `load_profile_slot` apply the same slot-path rule; `is_slot_directory` is now
  `classify_slot`.
- Tests: `crates/cs_content/tests/accept_f48_d_crash_recovery_matrix.rs` (12,
  the matrix itself) and `crates/cs_app/tests/accept_f48_d_crash_recovery_matrix.rs`
  (7, the same questions through a runtime `ProfileSession`).

Failure without the changes: the matrix is the new artifact, so there is no
"before" for it — but running it against the shipped F48-A/B/C code found two
defects on the read path, described with their probes below. Both are in the
path a *player* takes after a crash or a hostile file appears, which is the
stage's subject.

## What is real here, and what is a model

This is the distinction F48-B left open, so it is stated first.

**Real: a real process death at a chosen point of the write.** The parent
re-executes the same test binary (`std::env::current_exe()`) with a request
naming one write phase; the child performs that phase's real file operations
through the production `DirStorage` and then blocks; the parent kills it with
`std::process::Child::kill`. That is `SIGKILL` on unix and `TerminateProcess` on
Windows, so on every platform the child dies with **no unwinding, no
destructors and no buffered flush**. Each row asserts the child really died
(a signalled death reports no exit code, so a clean return cannot pass as a
crash), and the parent waits for a readiness marker the child writes only after
the real operations up to the crash point have completed, so a child that never
arrived fails the row instead of being killed somewhere harmless.

**Model: a torn write.** A signal cannot tear a write — a `write` syscall in
flight completes. So the one row that needs a half-written temp file produces it
by writing half the bytes first, and the file header says so. It is labelled a
model wherever it appears, not called a measured crash.

**Not measured: durability across a real power loss.** A process kill cannot
establish it. The bytes a process wrote are in the page cache whether or not
they were `fsync`ed, so this matrix **cannot discriminate a synced write from an
unsynced one** and does not claim to. `sync_temp`'s `File::sync_all` and
`sync_dir`'s directory sync are exercised — they are called, and their errors
would fail a row — but that is "the code ran", not "the data survived a power
cut". Establishing that needs a machine or a VM whose power can be cut, which
this environment does not have. See "Open / not claimed".

## The matrix

Thirty rows: 5 write phases × 2 sides (killed *before* the phase's effect
reaches the filesystem, and *after* it does) × 3 pre-states.

| Pre-state | Files before the crash |
| --- | --- |
| `one-revision` | `profile.sav` = revision 1, no backup |
| `current+backup` | `profile.sav` = revision 2, `profile.bak` = revision 1 |
| `newer-uninstalled-temp` | `profile.sav` = 2, `profile.bak` = 1, `profile.tmp` = a whole *unsynced-into-the-current-slot* revision 3 — what a kill in `SyncTemp` leaves |

F48-B covered "stop at each phase boundary" via a returned error from the
`SaveStorage` seam. Two things are added here. The **side**: a kill *after* a
phase's real operation is a state F48-B's seam could not produce, and it is the
state that matters for `InstallCurrent` and `SyncDir` — after `install_current`
the temp file is gone, after `sync_dir` the rename is durable. And the
**pre-states**: the crash matrix now runs against a slot that already holds a
backup and one that holds an uninstalled newer revision, which is the state a
real crash leaves and the state `run_phases`' "finish the interrupted commit
first" rule exists for.

Each row asserts, and all thirty hold on this platform:

- the child died from a signal;
- nothing outside the fixture base was touched (a canary file);
- a whole revision is in force afterwards — never nothing, and never a mixture
  of two files;
- the revision in force is the pre-crash whole revision or the one offered, and
  nothing else;
- if the state came from a file other than `profile.sav`, a
  `RecoveryWarning::UsedFallback` was reported — a fallback is never silent;
- the commit that follows the crash succeeds and installs exactly the revision
  it offered.

Measured on **macOS arm64** (`rustc 1.98.1`, `rename(2)` replacement,
directory sync supported): 30/30 rows recovered a whole revision, 12 from
`profile.sav`, 18 through a reported fallback, 0 lost. The full row table is
printed by `accept_f48_d_the_matrix_runs_and_reports_this_platform`, so a CI log
carries the run rather than only the verdict.

## The two defects the matrix found

Both were found by a probe run against the shipped code before the fix, and the
probe output is quoted. Neither was found by reading.

### 1. `load` followed a symbolic link planted at a slot path

`DirStorage::create` refused a slot *directory* that is a link (F48-B review
finding 2), and adoption refused one too (F48-B review finding 5) — but
`ProfileLibrary::load` used `DirStorage::new`, which applies no such check, so
`recover_profile` opened whatever the link led to.

Probe, before the fix: a synthetic population whose `profile-1` slot was a
symbolic link to a moved production slot directory.

```
PROBE synthetic live after planting the link: []
PROBE load(1) followed the link: document=Some(ProfileId(1))
       kind=Some(Production) money=Some(4242) source=Some(Current)
PROBE save over the linked slot: Some("profile 1 is not live")
```

So a planted link made a **synthetic** library read a **production** pilot's
campaign state (money 4242 came from the production save). That is F48
non-negotiable 4 violated by the read path: populations are supposed to be
separate, and the link is not a property of a profile — it is whatever is at the
end of it, possibly outside the base directory entirely. Note the read *succeeded*
while the write was refused ("profile 1 is not live"), so the library could read
a save it was structurally unable to write back.

**Fix.** `classify_slot` in `fs.rs` is now the single judgment about a slot
path, applied by `DirStorage::create` (as before), by `is_slot_directory` (which
now *is* `classify_slot`) and by `load`/`slot_warnings`/`load_profile_slot`.
Tests: `accept_f48_d_a_hostile_slot_path_is_refused_and_nothing_outside_is_written`
and, through a session, `accept_f48_d_a_planted_link_at_a_slot_is_refused_by_the_session`.

### 2. `load` reported the requested id for another profile's save

`recover` refuses a slot holding *two different* profiles
(`RecoverError::ProfileMismatch`), which is the case recovery itself can see. It
had no rule for a slot holding exactly one profile that is *not* the one asked
for, so `LoadedProfile { id, document }` could disagree with itself.

Probe, before the fix: profile 2's save planted as profile 1's `profile.sav`.

```
PROBE load(1) reported id=1 document_id=Some(ProfileId(2)) name=Some("Synthetic Pilot")
```

The write path was already safe — `store::commit` refuses with `WrongProfile`,
and F48-C's `check_owns` refuses a retargeted draft — so no pilot's file was
overwritten. But the *read* handed a caller another pilot's campaign, records and
settings under this pilot's id, and every commit of that document would then be
refused: a pilot whose state can be seen but never saved.

**Fix.** `load` refuses a recovered revision whose `profile_id` is not the id
asked for, as `LibraryError::ProfileMismatch { requested, stored }` — distinct
from `RecoverError::ProfileMismatch`, which is two profiles in one slot and
reaches the caller already wrapped in `LibraryError::Recover`. Test:
`accept_f48_d_a_slot_holding_another_profile_is_refused`.

## Review addition: a save from another *population* on the read path

Found by the reviewer (deepseek-1, independent context) after the two defects
above, by asking what else the same read path trusted. `load` checked the
document's profile **id** but not its **kind**, so a production document planted
under a synthetic id was handed to a synthetic session as its own.

Probe, before the fix: a synthetic library's slot for id 1 overwritten with a
sealed `kind=production` document of the same id.

```
PROBE load returned kind=Some(Production) money=Some(4242) id=1
```

The write path already refuses this class: `create` and `save` return
`LibraryError::ForeignDocument` when the offered document's kind is not the
population's. The read path did not, so a synthetic or evidence session could
read production campaign state, records and settings through a misplaced file —
the same non-negotiable 4 violation as defect 1, reached through a file rather
than a directory.

**Fix.** `load` also refuses a recovered revision whose `kind` is not the
library's population, as the existing `LibraryError::ForeignDocument`. Test:
`accept_f48_d_a_slot_holding_another_population_is_refused`.

## AC04 through a session

The stage's minimum scenario is "load a future/oversized/malicious save without
panic, traversal or destructive overwrite". F48-B covered the storage layer and
F48-C the session's recovery of one unreadable profile. What was missing is what
a *session* does with a hostile save on the way in, so
`crates/cs_app/tests/accept_f48_d_crash_recovery_matrix.rs` asks that:

- **A future-schema save does not stop the session.** The planted save is
  checksummed, so the refusal under test is the version and not the framing. The
  session opens, reports the refusal as text in `warnings()`, selects nothing,
  and the *other* pilot is still selectable — one damaged save must not hide the
  population. The newer build's bytes are byte-identical afterwards.
  (`accept_f48_d_a_future_save_does_not_stop_the_session_and_is_never_overwritten`)
- **An oversized save is refused at the bound, without panicking or
  overwriting**, and the session still works for the other pilot. The test
  selects the damaged profile *before* planting the oversized file, so the
  session really reads it on the way in rather than selecting the other pilot and
  never noticing. (`..._an_oversized_save_is_refused_without_panicking_or_overwriting`)
- **A save whose every field is spelled as a path writes nothing outside its
  slot.** `campaign.run=../../../outside-the-base`, a `display_name` holding
  separators, a setting value holding them, and an unknown key spelled as
  `/etc/passwd`. The character rule refuses the file; the bytes survive
  unread-but-unrewritten; and the display names are then exercised through the
  session, where a pilot may legitimately be *named* `../../outside-the-base`
  (it is free text) and the assertion is that nothing becomes a path: every
  entry in the population directory is a `registry.*` file or a `profile-<id>`
  directory, a canary file outside the base is byte-identical, and the base
  contains only the population and the canary. (`..._a_save_naming_paths_writes_nothing_outside_its_slot`)
- **A foreign registry in the synthetic subtree is refused outright**, and its
  bytes are not overwritten; the production population still opens with its
  pilot and nothing to reconcile. (`..._a_session_never_reaches_another_population_through_the_registry`)
- **A device at a save name is refused at the bound.** `/dev/zero` at
  `profile.sav` reports no useful length and reads forever, so the capped read
  is the only reason this test finishes. It is `#[cfg(unix)]`.
  (`..._a_device_at_a_save_name_is_refused_at_the_bound_not_after_reading_it`)
- **A slot whose every file is unreadable is reported, not presented as a
  profile with no state**, every damaged file is kept for the owner, and the
  slot is writable again. (`..._a_slot_with_no_recoverable_revision_is_reported_not_hidden`)
- **An empty slot directory is not adopted as a profile.** A crash between
  creating the slot and writing its first revision leaves exactly that; the id
  it names still raises the high-water mark so it is never issued again, but it
  is not offered as a pilot with nothing to load.
  (`..._an_empty_slot_directory_is_not_adopted_as_a_profile`)

## The runtime crash half

- **A session killed mid-commit reopens whole and saves again.**
  (`..._a_session_that_was_killed_mid_commit_reopens_whole_and_saves_again`)
  The crash child builds its revision from *what is stored* (via
  `load_profile_slot`) and bumps it, exactly as a real save does — a child that
  offered a document of its own invention would be measuring its fixture, not
  the write path. The reopened session's money value then identifies which
  revision is whole, so a mixture of `profile.sav` and `profile.tmp` cannot pass.
- **A killed registry write never reissues a profile id.**
  (`..._a_killed_registry_write_never_reissues_a_profile_id`) The newest registry
  is damaged byte-wise, as a torn write would leave it; the population falls back
  to the backup, reports it, and the high-water mark in force still covers every
  id ever issued, because the slot directories are the authority.
- **Campaign state survives a real process death, and a replayed outcome is
  still suppressed afterwards.**
  (`accept_f48_d_campaign_state_survives_a_real_process_death`) This is the
  contract's outcome transaction: the applied-outcome list is written atomically
  with the profile, so a crash cannot let a completed mission be paid twice.

## Decisions

- **The crash child is a test binary re-executed, not a helper binary.** The
  matrix needs a real process that dies, and the only executable guaranteed to
  exist where the tests run is the test binary itself
  (`std::env::current_exe()`). The child test is `#[ignore]`d and named
  `f48_d_crash_child_commits_and_dies_at_a_named_phase` — deliberately *without*
  the `accept_f48_d_` prefix, so it is not counted as an acceptance test that
  "passed" while doing nothing.
- **The readiness marker is a file, not a pipe or a sleep.** The child writes it
  only after the real operations up to the crash point have completed; the parent
  polls for it with a 30-second deadline. A fixed sleep would be a race, and a
  race would make the matrix's result depend on machine speed — which is exactly
  the sort of claim a matrix must not make.
- **`classify_slot` is in `fs.rs`, not `library.rs`.** The judgment is about a
  filesystem object, and `DirStorage::create` needs it too. Putting it in
  `library.rs` would have left the write path with its own copy of the rule,
  which is how the two sides drifted apart in the first place.
- **`LibraryError::ProfileMismatch` is a new variant rather than a reuse of
  `RecoverError::ProfileMismatch`.** The recovery error means *two* profiles in
  one slot and is a storage-level fact; this one means the slot holds a profile
  that is not the one asked for, which is a library-level decision with a
  different recovery. Folding them together would lose which one happened.
- **The registry kind mismatch refuses the whole session open.** The cs_app test
  asserts that a production registry planted in the synthetic subtree makes
  `open_sandbox` fail, rather than opening a population this session does not
  own. That is F48-B's existing `RecoverError::RegistryKindMismatch` behaviour;
  the test pins it at the session level so a later stage cannot quietly turn it
  into "open it anyway and say so".
- **Display names are not path-restricted, and that is the right answer.** The
  first draft of the traversal test asserted that a name spelled
  `../../outside-the-base` is refused. It is not, and it should not be: the name
  is free text a player chose, it never becomes a path component (the slot
  directory is derived from the numeric id alone), and refusing it would be
  inventing a restriction the spec does not ask for. The test now asserts the
  property that matters — nothing becomes a path — and separately that a name a
  *save could not hold* (empty, over the byte bound) is refused before an id is
  allocated, so a refused name allocates nothing.

## Sensitivity

Each mutation below was applied to the shipped code and the suite re-run; all
were reverted afterwards.

| Mutation | Test that failed |
| --- | --- |
| `load` no longer refuses a non-plain slot path | `accept_f48_d_a_hostile_slot_path_is_refused_and_nothing_outside_is_written` |
| `slot_warnings` no longer refuses it | the same test |
| `load` no longer refuses another profile's save | `accept_f48_d_a_slot_holding_another_profile_is_refused` |
| `load` no longer refuses another population's save (review addition) | `accept_f48_d_a_slot_holding_another_population_is_refused` |
| `run_phases` no longer finishes an interrupted commit whose temp holds the newest whole state | `accept_f48_d_a_kill_at_every_phase_boundary_leaves_a_whole_revision`, `..._the_matrix_runs_and_reports_this_platform` |
| backup rotation no longer requires a valid current file | the two above, plus `..._a_killed_commit_never_leaves_the_slot_unrecoverable` |
| `install_current` runs *before* `rotate_backup` | 4 tests in the cs_content matrix, including the phase matrix, `..._a_future_schema_save_is_refused_and_preserved` and `..._a_slot_holding_another_profile_is_refused` |
| adoption accepts a slot with no save file in it | `accept_f48_d_an_empty_slot_directory_is_not_adopted_as_a_profile` |
| the read is uncapped (`take(u64::MAX)`) | `accept_f48_d_a_device_at_a_save_name_is_refused_at_the_bound_not_after_reading_it` — the run does not finish: `/dev/zero` reads forever and the process is killed |
| `ProfileSession::open` propagates the load failure instead of reporting it | `accept_f48_d_a_future_save_does_not_stop_the_session_and_is_never_overwritten`, `..._an_oversized_save_...`, `..._a_planted_link_at_a_slot_...` |
| `record_outcome` no longer consults the in-memory applied list | nothing in this stage — the check that matters is the one in the write closure, and F48-C's own test covers the mapping. Recorded rather than claimed as a sensitivity case. |

The uncapped-read mutation is a hang rather than a failure. That is the finding,
and it is why the device row exists: without the cap on the read there is no
assertion that could report it, only a process that never exits.

## Open / not claimed

- **Windows was not measured.** This stage ran on **macOS arm64 only**, and CI
  (`ubuntu-latest`) has not yet run this branch. The contract refuses to assume
  POSIX rename behavior on Windows, and `replacement_semantics()` correctly
  reports `MoveFileExW` there — but what `MoveFileExW` does to a read-only or
  open destination, and whether a rename survives a real power loss there, is
  **unmeasured**. Nothing in this stage claims otherwise. The task title says
  "every supported platform"; one platform is not every platform, so this stage
  is a *matrix that runs on any platform and has been run on one*, plus an
  honest record of the two that have not. Running it on Linux and Windows is
  filed as its own task, because it needs machines this environment does not
  have, not code.
- **Durability across a real power loss is unmeasured** on every platform,
  including macOS. See "What is real here". Establishing it needs hardware or a
  VM whose power can be cut.
- **The directory-`fsync` no-op off unix is still a no-op off unix.**
  `directory_sync_supported()` reports it and the matrix runs on either
  answer, but the weaker durability it implies is untested on a platform where it
  is off.
- **The torn-write row is a model**, produced by writing half the bytes, not a
  measured crash. A real torn write on a real power cut is inside the
  unmeasured power-loss case above.
- **A read-only or open destination is not exercised by the matrix.** It was
  probed on macOS while writing this (a `rename(2)` over a `chmod 444` file and
  over an open file both succeeded there), but a probe in a throwaway file is not
  a test, so no claim is made from it and none is recorded as a finding. It
  belongs to the Windows/Linux run.
- **No cross-process lock.** F48-C's `PopulationClaim` is in-process and honestly
  so; a second *process* is caught by the registry's revision check, which is a
  conflict and not corruption. The crash matrix does not change that. Note the
  tension it leaves: a process that is killed while holding a claim releases it
  only because the claim was never a filesystem resource, which is also why a
  crash cannot strand a profile tree.
- **Retired slots are still never collected** (F48-B's open item, unchanged), and
  the matrix does not make it worse: a crash between the registry write and the
  retirement rename leaves a live-looking slot that reconciliation adopts back, a
  lost delete rather than a lost profile.
- **Nothing here has a runtime consumer.** `ProfileSession` is still called by
  tests only; F45-B/F61/F49 own the menu, the base directory and the CLI. The
  crash matrix is production code being tested, not a feature the game uses yet.
- **Mid-mission suspend is not implemented and not claimed** (contract:
  optional). A crash between two post-mission saves is what the matrix covers;
  a crash *during* a mission loses that mission's play, as the contract permits.
- **The user-data base directory is still unchosen (F61).** Every test passes a
  temporary directory.
- **Legacy import is F64**; nothing here reads an original save.

## Reviewer note

Implementer and reviewer are the same agent identity for this branch
(`bunny-alpha-2`), and the reviewer context was **not** fresh in the sense of
being a different agent: this session implemented and self-checked the work. The
project instructions ask for an independent reviewer for format, mission
semantics and fidelity claims; the two defects above are read-path safety
defects found by executable probes rather than by reading, which is weaker than
independent review and is recorded as such. Neither defect touches original-data
semantics, and no fidelity claim is made here, so the exposure is the size of
the code review rather than the size of an evidence claim.

**Independent review (2026-10-01, deepseek-1, fresh context):** a different agent
instance re-ran the full local checks, independently reproduced the sensitivity
of the slot-path and foreign-profile fixes (and of the phase-order change the
matrix is built to catch) by mutation, and found the third read-path gap recorded
above (a foreign *population* on the read path), which is fixed with a test in
the same class. The platform limitation above is unchanged and remains #461.

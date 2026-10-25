# F22-E: persisting bindings and calibration

Task #404, spec `specs/F22-input-bindings-devices-and-control-ownership.md`
(non-negotiable behaviors 1 and 5). Code: `crates/cs_app/src/input/profile.rs`.
Task-test prefix `accept_f22_e_`
(`crates/cs_app/tests/accept_f22_e_profile_persistence.rs`).

## What this stage delivers

`InputProfile` holds an `ActionMap` and a `CalibrationStore`.

- `to_text` / `parse`: a versioned, line-oriented text format. First line
  `cs-input-profile 1`, then `bind <source> <target>` and
  `calibration <class> <identity> <channel> deadzone= inverted= response=
  saturation= activation=` records, then `end`. Sources, targets and channels
  use the existing `label()` spellings. A device identity is percent-escaped
  (everything but ASCII alphanumerics and `._-:/`). Floats use Rust's shortest
  round-trip spelling, so a reload is bit-exact.
- Bounds: 1 MiB per file, 1024 bytes per line, 4096 bindings, 1024
  calibrations. Indices are canonical decimal (no sign, no leading zero).
- Refusals are named (`ProfileError`, `ProfileField`): missing header,
  unsupported version, truncation (no `end`, no final newline), trailing
  content, over-long line or file, unknown record, malformed source / target /
  class / identity / channel / calibration field (with the field name), a
  channel read by a different device class, a duplicate calibration record, and
  the underlying `DeviceIdError`, `CalibrationError` and `ActionMapError`.
  Nothing is clamped or repaired.
- Conflicts are visible: a profile whose bindings conflict returns
  `ProfileError::Bindings(ActionMapError::ConflictingBinding)` from `parse`;
  it is never resolved by order.
- Index-only devices: `to_text` / `save` refuse any record under
  `CalibrationStore::unstable_devices()` with `ProfileError::UnstableDevice`
  before touching the disk. `DeviceAdapters::adopt_identity` re-keys records
  onto the stable identity, and `InputProfile::capture` reads the store after
  that, so the re-keyed record is what is written.
- Atomic save: text is built first, written to `<file>.tmp` beside the target,
  `sync_all`ed and renamed over the target; a failure removes the temporary and
  leaves the previous profile. A stale temporary from a crash is never read and
  is truncated by the next save.
- Load failure preserves the session: `load` / `parse` only return a value;
  `InputProfile::apply` is the only step that changes a session, so a refused
  profile leaves the running bindings alone. `apply` replaces the session's
  map and its *stable* calibration records and keeps its index-keyed
  provisional ones (devices connected right now that the profile cannot know).

## Design decisions (new-engine, not original)

The original 2000 PC game's configuration location and format are **unknown**
(the owner-recovered default tables in F22-J say what the defaults are, not
how the game stores a player's changes). Nothing here claims parity. The
format and the `<file>.tmp` naming are this project's own choices. The profile
path is a parameter: `save`/`load` take a `&Path`, and choosing the file's
directory is left to the profile-directory owner (F48), not decided here.

## Known limits

- No caller yet loads the profile at startup or saves it from a settings
  screen; this stage delivers the library and its tests only. The settings UI
  and the startup path do not exist.
- The temporary name is fixed, so two processes saving the same profile at
  once would race; the game is a single process per profile directory.
- `sync_all` covers the file; the containing directory is not synced after the
  rename, so on power loss the rename itself may not survive (the previous
  profile then remains, which is the safe side).

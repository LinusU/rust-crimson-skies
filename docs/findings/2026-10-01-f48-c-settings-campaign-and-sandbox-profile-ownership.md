# F48-C: settings, campaign and sandbox profile ownership — what was wired and what stays open

Date: 2026-10-01. Task: F48-C "Wire settings, campaign and sandbox profile
ownership" (`specs/F48-profiles-saves-settings-migration-and-recovery.md`,
stage `### F48-C`), contract `docs/contracts/STATE-TRANSACTIONS.md` ("Session
reset", "Outcome and economy transaction", "Persistence").

Capabilities used: ordinary build/test only. Every byte the tests write is
newly authored synthetic data in a temporary directory; no test reads
`$CS_GAME_DIR`, a real user profile directory or any original data. Nothing here
is `verified_original` and nothing asserts original-game behavior — the save
format and the settings vocabulary are newly authored engine design.

## Files and the observable failure

- `crates/cs_content/src/save/settings.rs` (new): `ValueRule`, `SettingRule`,
  `SettingCatalog` (+`CatalogError`), `RefusalReason`, `SettingRefusal`,
  `SettingOutcome`, `SettingsState::open/set/entries`, `setting_line`.
- `crates/cs_app/src/profile.rs`: `PopulationClaim` (+`ClaimError`), `ProfileSession`
  (`open`, `open_sandbox`, `create`, `select`, `delete`, `set_setting`, `commit`,
  `commit_with`, `record_outcome`, `begin_campaign_run`, `document_mut`, `finish`),
  `SessionError`, `ChangeRefusal`/`ChangeRefusalReason`, `OutcomeRecord`,
  `TeardownReport`, `MAX_COMMIT_ATTEMPTS`.
- `crates/cs_content/src/save/mod.rs`: `pub mod settings;` and the module docs.
- Tests: `crates/cs_app/tests/accept_f48_c_profile_ownership.rs` (16).

Failure without the implementation: nothing opens a population as a runtime
resource — a profile's stored settings have no owner, so an unusable device
index is either applied or dropped silently; two sessions can hold one population
and interleave writes to one revision counter; a lost update restores an
obsolete copy of the profile and erases the other writer's progression; a replayed
outcome is applied twice; and an uncommitted change is flushed on the way out
instead of being reported. The five tests that pin those are
`accept_f48_c_a_population_has_one_live_owner_and_teardown_releases_it`,
`accept_f48_c_a_conflicting_commit_reapplies_to_the_stored_revision`,
`accept_f48_c_campaign_state_is_the_profiles_and_an_outcome_replays_once`,
`accept_f48_c_uncommitted_settings_are_dropped_and_reported_at_teardown` and
`accept_f48_c_a_refused_setting_value_recovers_to_the_value_in_force`.

## Decisions

- **The settings catalog is supplied by the caller, and this module declares no
  key.** F48-B persisted a `SettingEntry` list verbatim and left "which settings
  exist, their keys and their restart classification" to F48-C. It does *not*
  follow that F48-C should invent them: F52 owns
  `crates/cs_content/src/settings.rs` (accessibility and explicitly separated
  modern options), F22 owns device bindings and F17 owns display policy. So
  `save/settings.rs` is the machinery — what a rule is, what a catalog refuses,
  how a stored value is checked, what a refused value falls back to — and
  `ProfileSession::open` takes a `&SettingCatalog`. A session with
  `SettingCatalog::empty()` preserves every stored setting and interprets none of
  them, which is the correct state before a feature declares its own keys.
  Nothing in this file or its tests is a claim about an original setting; the
  tests say so in their own header.
- **A restart-required value is stored but not applied in the session that
  changed it.** `SettingsState::set` updates `stored` and reports
  `SettingOutcome::AppliedAfterRestart`, leaving `live` alone and listing the key
  in `pending_restart`. The value takes effect at the next session, because that
  session is a restart — the honest point where "requires restart" becomes true.
  The running session keeps the value it started with, so a device change cannot
  be half-applied mid-run.
- **The catalog's apply label wins over the save's, in both directions.** The
  label is read from the rule, never from the stored entry, so a build that
  mislabeled a key cannot make a restart-required change take effect while the
  game runs. `SettingsState::open` reports the mismatch, keeps the value stored,
  does not put it into force, and writes the catalog's label back on the next
  commit so the mislabel does not perpetuate itself.
- **An unusable *stored* value is recovered to the rule's declared default; an
  unusable *offered* value changes nothing.** The first is a save this build must
  open, and the spec's "safe recovery path" applies (non-negotiable 5); the
  second is a change this build refuses, so the last value known to be acceptable
  stays in force. A value that is merely *mislabeled* is left exactly as stored —
  a save is never rewritten behind the player's back, only its unusable values
  are replaced.
- **A key with no rule is preserved, reported and never interpreted.** It is
  carried through the save verbatim (like an unknown document field) and reported
  as `RefusalReason::UnknownKey`; `live_value` reports what the save says rather
  than a value this build would have invented, so a settings screen can display
  it. `set` on it is refused.
- **A boolean is a two-label `ValueRule::Choice`, not a third rule.** Two rules —
  a bounded integer and a declared label set — are what a persisted setting needs;
  a third rule would only add an invented spelling of its own.
- **A catalog is refused at build time when it could not be honored**: an empty
  value space, or a default its own rule would refuse. A setting that could never
  recover to anything is caught where it is declared, not at the moment a player
  has an unusable display.
- **`PopulationClaim` is the exclusivity F48-B left open.** Two live libraries
  over one directory are now refused in-process, so a menu, a mission teardown and
  a campaign task cannot each believe they own one population's revision counter.
  It is released on `Drop`, so a `?`, a panic or an early return cannot strand a
  profile tree permanently unopenable; `finish` releases it and *reports* what the
  session ended with. It is honestly in-process: a second *process* is caught by
  the registry's revision check, which is a conflict and not corruption. The key
  is the canonicalized directory, falling back to the path as given, so a
  symlinked spelling of one directory is not two populations — that is the
  in-process bound, not a filesystem lock.
- **`commit_with` retries; `commit` does not.** The retry is the contract's
  transaction: `change` is a *function* of the document, so re-reading the stored
  revision and applying the same function to it puts the intended change on top of
  whatever else is now there, instead of restoring an obsolete copy. The plain
  `commit` writes whatever the caller mutated in place through `document_mut`,
  which is not a function, so it reports the library's conflict rather than
  guessing — "conflicting revisions fail and refresh the view; they do not
  overwrite unrelated progression". After `MAX_COMMIT_ATTEMPTS` (3) conflicts
  `commit_with` also reports, with the stored revision the caller must refresh
  from, rather than retrying a profile that keeps moving.
- **Every commit writes the session's resolved settings into the draft.** A
  document that carried a hand-edited setting list could otherwise reintroduce a
  value the catalog refused, so the settings a session resolved are what the disk
  gets — including a recovery to a declared default.
- **An uncommitted settings change is dropped at teardown, not flushed.** The
  contract says persistent profile data receives only an explicit outcome
  transaction; committing on the way out would turn a failed run into a partial
  save. `TeardownReport::uncommitted_changes` says so and the caller decides.
- **A profile whose save cannot be read does not stop the population opening.**
  The active pointer naming an unreadable slot leaves the session with nothing
  selected and the reason in `warnings()`, so one damaged or future-schema save
  cannot hide every other pilot in the tree.
- **A refused display name allocates nothing.** The name is bounded before the
  library is asked for an id, so no id is issued, no slot directory is created and
  the high-water mark does not move.
- **Reward amounts are not decided here.** `record_outcome` owns the idempotence
  half of the contract's outcome transaction — an already-applied id returns
  `OutcomeRecord::AlreadyApplied` and writes nothing, so a replay after a crash
  before acknowledgment cannot pay twice — and the applied list is written
  atomically with the profile. What a mission is worth, mission unlock rules and
  the purchase/sell draft-and-expected-revision flow are F43-B's.
- **`library_mut` is public and is not a second owner.** It is the same
  `ProfileLibrary` the session holds, so the population rule and the revision
  check still apply; it exists because a caller sometimes has to reach the library
  directly (the tests use it to plant a mislabeled save and a foreign document).
- **Wiring edits outside the owner paths:** `crates/cs_content/src/save/mod.rs`
  gained `pub mod settings;` and updated module docs. No `lib.rs`, `Cargo.toml` or
  `Cargo.lock` change was needed.

## Sensitivity

Each mutation below was applied to the implementation and the suite re-run; all
were reverted afterwards. The whole file is production code the tests call.

| Mutation | Test that failed |
| --- | --- |
| the claim never refuses a second owner | `accept_f48_c_a_population_has_one_live_owner_and_teardown_releases_it` |
| the retry no longer refreshes the view before re-applying | `accept_f48_c_a_conflicting_commit_reapplies_to_the_stored_revision` |
| the conflict is retried forever and never reported | (the run hangs — the guard is what makes the loop terminate; the "starved profile" test is the one that reports it) |
| `commit_with` writes a draft that zeroes an unrelated field | `accept_f48_c_a_conflicting_commit_reapplies_to_the_stored_revision` |
| a replayed outcome is reported as `Recorded` | `accept_f48_c_campaign_state_is_the_profiles_and_an_outcome_replays_once` |
| `finish` reports `uncommitted_changes: false` | `accept_f48_c_uncommitted_settings_are_dropped_and_reported_at_teardown` |
| a name a save could not hold is not refused up front | `accept_f48_c_an_unusable_profile_name_is_refused_before_an_id_is_issued` |
| an unusable *offered* setting value is accepted | `accept_f48_c_a_refused_setting_value_recovers_to_the_value_in_force` |
| an unusable *stored* value is not recovered on open | `accept_f48_c_a_save_from_another_build_opens_with_its_unusable_values_recovered` |
| a mislabeled restart-required stored value is put into force | `accept_f48_c_a_mislabeled_stored_setting_keeps_the_catalogs_label` |
| a mislabeled entry keeps the save's own apply label on write | `accept_f48_c_a_mislabeled_stored_setting_keeps_the_catalogs_label` |
| an unreadable active profile opens silently, diagnostic dropped | `accept_f48_c_a_hostile_save_is_refused_without_overwriting_it` |

The "retried forever" mutation is a hang rather than a failure: that is the
finding, and it is why `MAX_COMMIT_ATTEMPTS` exists and why
`accept_f48_c_a_continuously_moved_profile_is_reported_not_retried_forever` asserts
the reported attempt count.

## Open / not claimed

- **No runtime consumer of a `ProfileSession` exists yet outside these tests.**
  The session is the production wiring F48-C adds — the menu that lists and
  creates profiles (F45-B), the campaign/session lifecycle that opens and tears
  it down, and the CLI `--profile-dir` mode are F61/F45-B/F49 — but nothing in a
  running game calls it yet. The path is production code, not a test-only
  implementation.
- **The user-data base directory is still unchosen (F61).** `ProfileSession::open`
  is handed a base and never picks one; the tests pass a temporary directory.
- **In-process exclusivity is not a filesystem lock.** Two *processes* are caught
  by the registry's revision check (a conflict, not corruption). A real
  cross-process lock file is not written, and the claim's key falls back to the
  path as given when the directory does not yet exist.
- **The population rule is enforced at the path.** An automated session is refused
  the production subtree, and the sandbox opener always selects the synthetic
  population. What is *not* enforced is that an automated run's base directory is
  not a player's real one: that is F61's choice of base, and this stage cannot
  check what it does not know.
- **Retired slots are never collected** (F48-B's open item, unchanged here): a
  long-lived profile directory grows, and any collector must know that an id named
  in a retired slot may not be reissued.
- **No crash was induced and no `fsync` was observed to survive a power cut.**
  The write path, the conflict path and the recovery path are the real file
  operations F48-B implemented, driven here by a runtime session; the
  crash/power-loss matrix per platform, the Windows replacement semantics and the
  directory-`fsync` no-op off unix are **F48-D**.
- **Settings beyond the catalog do not exist yet.** F52 declares the
  accessibility and presentation keys, F22 the device ones and F17 the display
  policy; until then a session preserves and ignores them. The
  "safe defaults startup flag" F52 names is the same recovery path this stage
  implements, so it needs a flag surface, not new machinery.
- **Mid-mission suspend is not implemented and not claimed** (contract: optional).
- **Legacy import is F64**; nothing here reads an original save.
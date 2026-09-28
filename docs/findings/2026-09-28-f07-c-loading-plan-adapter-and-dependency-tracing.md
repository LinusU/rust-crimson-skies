# F07-C: loading-plan adapter with dependency tracing

Date: 2026-09-28. Task: F07-C "Build loading-plan adapter with dependency
tracing" (`specs/F07-interp-loading-script-container.md`, section `### F07-C`).
Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capabilities used:
ordinary build/test. **No `CS_GAME_DIR` read**: the retail audit of which
commands load resources is F07-D, and this stage needed no installation to be
built or tested, so no evidence report is required here.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/interp.rs`: `LoadCommand`, `LoadCommandTable`,
  `KeyArguments`, `KeySpelling`, `KeyPart`, `KeyTokens`, `TableError`,
  `MalformedKey`, `ScriptOrigin`, `PlanLine`, `PlanLineKind`, `PlanScript`,
  `PlanStats`, `InterpLoadPlan` and `plan_interp_loading`; plus
  `DecodedInterp::bytes()`. F07-A's `read_interp` and F07-B's
  `decode_interp` are unchanged apart from the one accessor.
- `crates/cs_content/src/loading.rs` (new): `resolve_loading_plan`,
  `LoadingPlanReport`, `LoadingScript`, `ScriptState`, `LoadingDependency`,
  `DependencyState`, `DependencySite`, `LoadingFailure`, `LoadingScript`,
  `LoadingError`, and their accessors.
- `crates/cs_content/Cargo.toml`, `src/lib.rs` (wiring only): the
  `cs_assets` dependency edge the VFS resolution needs, and `pub mod
  loading;`.
- `crates/cs_formats/tests/interp.rs`: seven `accept_f07_c_*` tests.
- `tools/cs_inspect/src/interp.rs`: the `--plan`, `--commands`, `--cs-path`
  and `--world` flags, the plan JSON renderer, `build_plan`,
  `read_command_table` and five `accept_f07_c_*` unit tests.
- `tools/cs_inspect/Cargo.toml`, `src/lib.rs`, `src/main.rs` and the root
  `Cargo.lock` (wiring only): the `cs_content` edge, the help text and the
  module docs. The `Cargo.lock` change is those two edges and nothing else.
- This file.

**One observable failure:** a loading plan that reports a world as loaded when
a command in it was never understood. On a container whose two lines are
`LoadGameGen world c1/plane.flt` and `Quit`, with the workspace's
(empty) command table, `cs-inspect interp --file <c> --plan --cs-path <tree>
--world zbd/c1` must exit 3, report `"status": "incomplete"` with
`"unclassified_commands": 2`, `"dependencies": []`, and name each line's
source offset and `zbd/c1` — never `"state": "ready"`, never a resolved
dependency and never a loaded world
(`accept_f07_c_cli_reports_unclassified_commands_with_offsets_and_world`).

## Design decisions

- **The command table ships empty, and the plan fails closed.** Which tokens
  are loading commands is exactly what F07-D measures; the sheet's
  non-negotiable #3 says this container "does not by itself identify the full
  mission language". So `LoadCommandTable::new()` classifies nothing and every
  head token no registration names is `PlanLineKind::Unclassified` — a
  *failure*, not an assumption that the line loads nothing. A plan that
  defaulted to "this line is a value" would be a fabricated loading state, and
  a plan that defaulted to "this line loads something" would be worse. The
  table carries a `ClaimStatus` and a `source` string per rule so a claim is
  traceable, and `insert` refuses `ClaimStatus::VerifiedOriginal`: that status
  belongs to a fingerprinted evidence record, never to a table somebody typed.
  `cs-inspect interp --commands <file>` lets a researcher supply such a table
  without editing code, so the retail audit is a data exercise.
- **Classification is separated from resolution.** `cs_formats` cannot depend
  on the VFS (AGENTS.md rule 7), so `plan_interp_loading` is pure: it
  classifies lines and names argument positions, and
  `cs_content::loading::resolve_loading_plan` is the consumer that builds asset
  keys, resolves them in a session and records dependencies. That is the
  producer/consumer wiring the stage asks for, and it keeps the format crate
  free of a filesystem.
- **Registration is a claim that can be wrong, so a mismatch is reported.**
  A registered command whose stored line has too few arguments, or an empty
  one where the registration names a value, becomes
  `PlanLineKind::Malformed { reason }` with the position — never padded,
  skipped, or read out of a neighbouring argument. `LoadCommandTable` refuses
  the registrations that would make classification ambiguous (an empty
  spelling, a key argument at position 0, two parts naming one position, a
  duplicate spelling) and `extend` applies a whole set or none of it.
- **The plan owns a snapshot of the table it was built with.** `InterpLoadPlan`
  clones the registrations rather than borrowing the table, so a plan stays
  readable after the table is dropped or extended and a report cannot be read
  against a different table than the one that produced it. The test asserts the
  snapshot's status, source and variant position.
- **Identity is a content hash, over exactly the right bytes.**
  `LoadingScript::content_sha256` is SHA-256 of `script_offset..end` — the
  script's own stored bytes, terminator included. The name is not hashed (two
  scripts may share one) and the `timestamp` is not hashed (it is metadata,
  non-negotiable #5). `accept_f07_c_identity_is_a_content_hash_not_a_name_or_a_timestamp`
  pins the consequence: changing only the timestamp word moves the *container*
  hash (the index entry is part of the file) but leaves the *script* hash
  identical. `LoadingPlanReport::container_sha256` covers the whole container,
  so a report is tied to the exact bytes it was built from.
- **`Origin` and hash are complementary, and both are tested.** Two scripts
  with equal names, equal timestamps and different bodies keep distinct
  `ScriptOrigin`s *and* distinct hashes; two entries sharing one offset keep
  distinct origins (they really are one body) and one hash. Neither field is
  redundant, so the plan does not need a third identity concept.
- **Four dependency outcomes, and the difference between a failure and a
  dynamic lookup is explicit.** `Resolved` (with the immutable `SourceSpan`),
  `Unresolved` (a VFS refusal with its code and trace message, or `no_session`
  when no session was given), `Composed` (the registration says the key is
  assembled at run time, so nothing exists to resolve) and `Invalid` (the
  stored arguments are not a key). A `Composed` dependency is **counted, not
  failed** — `docs/contracts/SCRIPT-MISSION.md` asks for "dynamic lookups" as
  their own tally — but it does keep `is_complete()` false, because a world
  whose assets are not all known is not a fully known world. A `Ready` script
  may therefore hold dynamic lookups; the report's `dynamic_lookups` count and
  its `complete` flag are the check that accounts for them. This distinction is
  asserted in both the library and the CLI tests.
- **Argument bytes are never repaired.** A key is built through
  `AssetKey::from_spelling` from the bytes as stored, so a `..` path, an
  absolute spelling, a namespace that is not a lowercase label and a token that
  is not text are each a refusal naming the part that failed. Where the
  original engine resolved `%ZBD_DIR%`-style names from is unmeasured, so the
  plan records the `Composed` registration as a dynamic lookup rather than
  guessing a base directory or a substitution rule (F07-D's to measure).
- **Teardown and retry are the report's own contract.** The report holds no
  session reference and no file handle; its dependencies are stamped with the
  resolving session's generation, and `read_dependency` refuses a session that
  is not that one (`LoadingError::ForeignSession`) rather than re-resolving
  something the report never saw. `accept_f07_c_report_reads_only_through_the_session_that_resolved_it`
  walks the whole sequence: read through the resolving session, refused by a
  second session, the session closed (two mounts released), a second report
  built and read successfully, and the first report still refusing. In the CLI
  the session is closed by `PlanRun`'s `Drop`, so it is released on every path
  out of the command, including the ones that fail after the plan was built.
- **A plan paired with foreign bytes is refused.** `resolve_loading_plan` takes
  both the decoded container and the plan; if a plan's recorded script extent
  falls outside that container it returns `LoadingError::Extent` rather than
  hashing over a truncated range, because a plausible-looking wrong identity is
  worse than a loud failure. The decoder cannot produce such a plan, which is
  why the test pairs two containers deliberately.
- **The CLI adds a field, not a command.** `--plan` is additive to the
  existing `interp` report, and an incomplete plan exits 3 — the same code the
  command already uses for a reported anomaly. The plan report is
  shape-checked as JSON like the decode report, because a substring assertion
  alone would not notice a stray brace.

## Retail probe (read-only, not evidence)

Read-only, outside the repository, on the one installation this machine has.
`$CS_GAME_DIR/ZBD/interp.zbd`, SHA-256
`f5251cb559db1992320247b9674d159a149572e077bc8579ae34d5fbd16254c7` (the same
file F07-B's review probed). An independent Python walk — a different
implementation of the same layout, not this code — found 98 scripts, 5083 lines
and 85 distinct head spellings, with the most frequent being `FindNode` (1549),
`NodeSetActive` (1171), `LoadGameGen` (564), `SetIntersectSurface` (219) and
`SetAltitudeSurface` (219). Script names are installation-relative spellings
such as `support\load.gw` and `support\c1\init.gw`.

Two facts that shaped this stage and that **F07-D must measure properly**:

1. **The head-token vocabulary is scene-graph and terrain work, not loading.**
   `FindNode`, `NodeSetActive`, `AddChild`, `FindSubNode`, `Object3D*`,
   `Camera*`, `World*` and `WorldPartition*` are the majority. A table that
   registered "anything that is not one of the known loading verbs" as a
   loading command would invert the sheet's rule
   (`FindNode` really is game behaviour), so the empty table is the correct
   state, not a missing feature.
2. **A large share of the lines that look like loading commands name their
   target indirectly.** `LoadGameGen %dbFilePath% %dbName%`,
   `SetModelDirectory %DATA_DIR%\%CAMPAIGN_DIR%\models` and
   `RdrAddPath ..\\data\\%CAMPAIGN_DIR%\\zrdr` interpolate `%NAME%`
   variables that earlier lines (`set ZBD_DIR zbd`, `set DATA_DIR ..\data`)
   define. A literal-key resolver would refuse every one of these, which is
   why the plan has a `Composed` outcome and counts it: the retail corpus
   needs a variable model, not a bigger table, and building one is F07-D/F13
   work, not a decision this stage may make.

Nothing from this probe is committed, no derived file is in the repository, and
this paragraph certifies nothing about command semantics.

## Tests

`cargo test --workspace --locked -- accept_f07_c_ --include-ignored` — 19
tests, all passing; each also passes alone with `--exact`.

### `crates/cs_formats/tests/interp.rs` (7)

| Test | Covers |
| --- | --- |
| `equal_names_keep_distinct_origins` | **AC03 (this stage's minimum scenario).** Two scripts with the same name *and the same timestamp* keep distinct origins in index position, entry offset, script offset and byte range; the two ranges are the two different bodies. Two entries on one offset keep distinct origins and one shared script offset |
| `unregistered_command_is_unclassified_with_its_offset` | the empty table: every line `Unclassified`, `is_complete()` false, the stats, and the line's and head token's absolute offsets cross-checked against the container bytes |
| `registered_command_spellings_its_key_from_named_arguments` | byte-exact matching, the key's three tokens with their own absolute offsets, the table snapshot the plan carries, and a duplicate spelling refused |
| `mismatched_arguments_are_malformed_not_repaired` | four cases (missing and empty arguments, with and without a variant position) each `Malformed` with the position, naming the rule that did not match |
| `command_table_refuses_ambiguous_registrations` | empty spelling, empty source, self-awarded `verified_original`, head arguments, repeated positions, duplicate spellings; and `extend` applying all or none |
| `stats_tally_every_script_and_head` | the SCRIPT-MISSION tally over two scripts and three lines, the sum identity across the three classifications, and an empty container |
| `plan_reads_the_container_without_resolving_or_executing` | a `..` path survives into the plan unrepaired and the line stays lossless; the plan resolves nothing |

### `crates/cs_content/src/loading.rs` (7 unit tests)

| Test | Covers |
| --- | --- |
| `report_reads_only_through_the_session_that_resolved_it` | read through the resolving session; a foreign session refused with both generations; teardown releasing both mounts; a retry building and reading a second report; the first report still refusing |
| `missing_key_is_a_failure_not_a_default_file` | one resolved and one `not_found` line in one script, the failure's source offset derived from the bytes, its world, the VFS's attempts, the script `incomplete`, and the unresolved dependency refusing to be read |
| `invalid_keys_and_composed_keys_are_not_repaired` | a `..` path, a non-label namespace and non-text bytes each `invalid` naming the part, a `composed` line counted as a dynamic lookup with no key, and the counts |
| `without_a_session_nothing_resolves_and_the_reason_is_recorded` | no session: the plan still builds, the key is still reported, the dependency is `no_session`, the world and installation are `None`, and the summary says both |
| `identity_is_a_content_hash_not_a_name_or_a_timestamp` | the container hash and each script's own-extent hash against independently computed digests; a timestamp change moves the container hash and not the script hash |
| `equal_names_keep_distinct_origins_and_hashes` | **AC03 through the adapter.** Equal names, equal timestamps, different bodies: distinct origins, distinct hashes, and one resolved plus one `not_found` dependency so the two are distinguishable in the report too |
| `a_plan_against_foreign_bytes_is_refused` | each plan builds against its own bytes; pairing one plan with another container's bytes returns `LoadingError::Extent` naming the script and the length |

### `tools/cs_inspect/src/interp.rs` (5 unit tests)

| Test | Covers |
| --- | --- |
| `cli_reports_unclassified_commands_with_offsets_and_world` | **AC04.** Exit 3, `"status": "incomplete"`, the counts, `"dependencies": []`, the world, the session generation, both lines' offsets, `"state": "blocked"`, no `"resolved"`, and the same failures on stderr |
| `cli_resolves_registered_commands_through_the_session` | exit 0, `"complete": true`, the registration with its status and source, the dependency's key, site, `resolved` status, `ZBD/c1` span and member length, the script's content hash, the timestamp labelled metadata, and the container fingerprint |
| `cli_counts_composed_keys_as_dynamic_lookups` | a `composed` table entry: one dynamic lookup, zero resolved, no key, the script `ready` while the plan is `incomplete` |
| `cli_refuses_a_malformed_command_table` | six refused tables (too few fields, a non-numeric position, an unknown spelling kind, `verified_original`, a head argument, a duplicate), an unreadable table, and an unsupported flag — all exit 2 with no report |
| `cli_refuses_an_undecodable_container_before_planning` | `--plan` on bytes the decoder refuses exits 3 with the decoder's code and no report |

### Mutation probes (each reverted)

Every one was applied to production code, observed, and reverted from a
copy taken before the probes.

| Probe | Effect |
| --- | --- |
| `PlanLineKind::is_blocking` returns `false` (an unclassified line treated as harmless) | `unregistered_command_is_unclassified_with_its_offset`, `stats_tally_every_script_and_head` and `mismatched_arguments_are_malformed_not_repaired` fail |
| `ScriptOrigin::index` filled with a constant | `equal_names_keep_distinct_origins` fails |
| `InterpLoadPlan::commands` left empty (the plan not carrying its own table snapshot) | `registered_command_spellings_its_key_from_named_arguments` and `mismatched_arguments_are_malformed_not_repaired` fail |
| the script content hash taken over `bytes[..end]` instead of the script's own range | `identity_is_a_content_hash_not_a_name_or_a_timestamp` and `a_plan_against_foreign_bytes_is_refused` fail |
| the session-generation check in `read_dependency` disabled | `report_reads_only_through_the_session_that_resolved_it` fails |
| the failure's `world` rendered as `null` in the report | `cli_reports_unclassified_commands_with_offsets_and_world` fails |
| an incomplete plan no longer forcing exit 3 | `cli_reports_unclassified_commands_with_offsets_and_world` and `cli_counts_composed_keys_as_dynamic_lookups` fail |

One probe found a real redundancy rather than a test gap: dropping
`dynamic_lookups == 0` from `is_complete()` changed nothing, because a
`Composed` dependency already fails the `is_resolved()` check. The rule was
folded into the states (`is_resolved` / `is_failure` on `DependencyState`) with
a comment saying why, and the test now asserts those two predicates directly,
so the property is stated once instead of twice. That is the only change the
probes produced.

## Commands run

| Command | Result |
| --- | --- |
| `cargo fmt --all -- --check` | pass |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | pass |
| `cargo test --workspace --locked` | pass, no failures |
| `cargo test --workspace --locked -- accept_f07_c_ --include-ignored` | 19 tests, all pass |
| each of the 19 with `--exact` | all pass alone |
| `cargo run -p cs_inspect --bin cs-inspect -- interp --file <authored container> --plan` | exit 3, plan `incomplete`, every line `unclassified`, as designed |
| `cargo run -p cs_inspect --bin cs-inspect -- interp --file fixtures/synthetic/synthetic.interp --plan` | exit 3, `SYNTHETIC` and `VALUE` unclassified, report parses as JSON |

## Unknowns (not guessed)

- **Which commands load resources.** Unmeasured; F07-D's task. The table is
  empty and the read-only probe above (scene-graph and terrain commands
  dominate) says the answer is not "most of them".
- **What a loading command's arguments mean.** Argument *positions* are a
  registration's claim, checked against the stored line, never inferred. A
  retail command that names its key through a variable is `Composed` by
  registration; the table's author must have measured that.
- **The variable model.** The container clearly defines variables with `set`
  and interpolates them with `%NAME%`. F07-C records that shape as a dynamic
  lookup and implements no substitution, no environment and no scope; F13/F38
  own the language.
- **Whether a world is selected per container or per command.** The retail
  container holds every world's loading scripts, so "the affected world" here
  is the world the *resolving context* selected, not one derived from a script
  name. Deriving a world from a name would be a guess about naming policy and is
  not done.
- **Whether a `Composed` dependency makes a *script* unusable.** Not decided:
  the script is `ready` (nothing failed) and the *plan* is incomplete. If
  F07-D establishes that a world cannot load with outstanding dynamic lookups,
  the script state should follow the plan; recorded rather than assumed now.
- **The encoding of names and arguments.** Unchanged from F07-A/B and still
  unknown. No `String` is built from container bytes; the CLI report renders
  them as length and hex, and the adapter only decodes a token when it is
  building a key, where non-text is a refusal with a reason.

Every one of these is F07-D's or F13's scope, so no new task was filed; they
are listed here because a reader of the plan needs to know which parts are
measured and which are a structure waiting for a measurement.

## Sources

`specs/F07-interp-loading-script-container.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`,
`docs/contracts/IDENTITY-CONTENT.md`, `docs/research/FORMAT-NOTES.md`
("INTERP observed subset" [S07]), `fixtures/synthetic/README.md`, and
`docs/findings/2026-09-28-f07-a-interp-raw-records-and-golden-fixture.md` and
`docs/findings/2026-09-28-f07-b-lossless-token-decoder-and-validation.md`
(which named exactly this stage's outstanding work: "the loading plan, its
producer/consumer wiring and the dependency tracing").

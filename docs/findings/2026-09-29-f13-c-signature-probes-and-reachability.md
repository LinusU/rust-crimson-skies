# F13-C: resolve instruction/native signatures with isolated probes

Date: 2026-09-29. Task: F13-C "Resolve instruction/native signatures with
isolated probes"
(`specs/F13-mission-language-discovery-and-compatibility-closure.md`, section
`### F13-C`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`. Capability
used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F13-C/acceptance.json`, committed as
`docs/findings/evidence/F13-C.json`.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/probe.rs` (new): `ClaimError`,
  `SignatureShape`, `SignatureClaim`, `SignatureTable`, `ProbeError`,
  `ProbeConfig`, `ProgramProbe`, `ProbeSession`, `ProbeReport`,
  `RecordProbeError`, `RecordProbeStats` and `probe_records`. The typed place
  a **measured instruction/native signature** is recorded, the isolated probe
  session that walks one located program at a time, and the reachability
  probe that attaches a verdict to every inventory record.
- `crates/cs_formats/src/script_raw/inventory.rs`: `ScriptRecord` gains the
  `reachability` field with `reachability()`, `set_reachability()`,
  `reachability_evidence()` and `is_unused_unknown()`; the new
  `RecordReachability` enum with its `evidence()`, the
  `STRUCTURE_UNUSED_REASON` / `UNREACHED_UNUSED_REASON` reasons and
  `FormatDiscriminator::label()`.
- `crates/cs_formats/src/script_raw/discovery.rs`: `ProgramKind::from_label`,
  the inverse of `label()` used by the claims file reader.
- `crates/cs_formats/src/script_raw/mod.rs`: module declaration and
  re-exports only.
- `tools/cs_inspect/src/script_discovery.rs`: the `scripts` command gains
  `--signatures <file>`, `--word-bytes <n>` and `--budget <n>`, runs
  `probe_records` over every inventoried container and a `ProbeSession` over
  every located program, and renders the new top-level `probe` section, the
  new `summary` counters and the per-container `records` array; plus the
  `accept_f13_c_*` tests and the evidence harness.
- Wiring only: `tools/cs_inspect/src/main.rs` (help text and the module doc
  paragraph for the three new flags).
- `crates/cs_formats/tests/script_raw/main.rs`: the `accept_f13_c_*` tests.
- This file, plus `docs/findings/evidence/F13-C.json`.

**One observable failure:** an unused unknown record is a byte range no
reader decodes, no located program covers and no signature claim names —
exactly the range a "resolved signatures" report is tempted to drop. With
`probe_records` removed from `scripts_command_result`, the `Unclaimed` tail
of an INTERP container carries no reachability evidence and never appears in
the report's `records` section, so
`accept_f13_c_unused_unknown_record_stays_visible_with_reachability_evidence`
fails: the record either disappears or is reported without reachability
evidence. The mirrored failure — a probe session that reports a stopped walk
as a resolved one — is
`accept_f13_c_signature_claims_resolve_a_program_in_isolation`.

## What this stage wires

**Producer → probe → consumer.** `inventory_scripts` and
`discover_container` produce the records and the programs; `probe_records`
joins them in one direction (a record is *used* when a located program's byte
range overlaps it) and stores the verdict on the record; `SignatureTable`
turns caller-measured claims into the `OpcodeLedger` that `walk_program`
consumes; the `scripts` command renders both halves. Nothing is inferred from
a scan, and no byte of the installation is copied into the report.

| Piece | Behaviour |
| --- | --- |
| `SignatureClaim` | opcode, spelling, `ProgramKind`, arity, and a `SignatureShape` (signature/effects/timing/errors) plus `ScriptEvidence`. Refuses an empty field (`empty_field`) and evidence that cannot establish a meaning (`weak_evidence`): a `ContainerScan` lead or anything below `documented`. |
| `SignatureTable` | refuses a second claim for one opcode value (`duplicate_opcode`); `ledger()` builds the `OpcodeLedger`. Ships **empty**. |
| `ProbeConfig` | `word_bytes` and `budget` are explicit inputs; a width outside `1..=4` is `invalid_word_width`. A zero budget is legal and decodes nothing. |
| `ProbeSession` | one `probe()` per located program, each recorded as a `ProgramProbe` (mission, locator, kind, confidence, attempts, reached opcodes, stop). A stop never aborts the run. `extend()` adds a measured claim, `retry()` re-runs **only** a probe that stopped at an `unknown_opcode`, `teardown()` snapshots the `ProbeReport` and closes the session — later work fails `session_closed`. |
| `probe_records` | attaches `Used { programs }` / `Unused { reason }` to every record of one container and counts them; a mismatched container is refused (`path_mismatch`) instead of joined wrongly. |
| `ScriptRecord::is_unused_unknown` | true only for an undecoded record with an unidentified format, an unestablished instruction status **and** a measured `Unused` verdict — so "unused" is never a default for a record nobody looked at. |

The report is `cs-inspect-scripts/2`: the F13-B fields are unchanged and the
`probe` section, the `records` counters and the per-container `records` array
are new.

### The claims file (`--signatures`)

Plain text, `#` comments, one claim per line:

```text
<opcode> <spelling> <program> <arity> <signature> <effects> <timing> <errors> <citation> [note]
```

`opcode` is decimal or `0x`-prefixed, `program` is a `ProgramKind::label`,
every field but the note is one token, and the note is the rest of the line.
The evidence is always `document_review` / `documented` citing `citation`
(a claims file is a cited document, exactly the provenance F07-D's
`--classes` file carries). A malformed line, an unknown program kind, a
duplicate opcode or an unreadable file is **invalid input, exit 2**, and the
refusal names `<file>:<line>:`. The workspace ships no such file: the
mission opcode table is unmeasured, so the `retail` runs use an empty table.

## The F13-C minimum scenario (AC03)

An INTERP container with a trailing byte range no reader claims, after
`probe_records`:

- the `unclaimed` record is still in the inventory and in the report
  (4 records in, 4 out) with `discriminator: "unknown"` and
  `instructions: "unestablished"`;
- `is_unused_unknown()` is true, because the verdict is measured rather than
  defaulted — before the probe every record reports `reachability: null` and
  the predicate is false;
- its evidence is `structural_decode` / `observed_tool` at a `container_span`
  locator of exactly that record, reading *"no located program overlaps these
  bytes, so no probe reaches this record"*;
- the script body reports `used` with *"1 located program(s) overlap this
  record"*; the header and index entry report `unused` with the structure
  reason and are **not** unknown records.

Tests: `accept_f13_c_unused_unknown_record_stays_visible_with_reachability_evidence`
(synthetic) and `accept_f13_c_cli_resolves_signatures_and_keeps_an_unused_unknown_record`
(through the command).

## Retail corpus result (`cs-inspect scripts --coverage`)

`$CS_GAME_DIR` read-only; no file written inside it:

| Count | Value |
| --- | --- |
| `.zbd` containers routed | 184 |
| script containers (interp + reader + animation) | 124 (1 + 62 + 61) |
| families excluded from the search | 60 (listed, never searched) |
| located programs | 1452 (98 loading, 629 mission, 106 mission_animation, 16 camera_animation, 603 reader_entry, 0 unknown) |
| inventory records | 320 (197 in `interp.zbd` = 1 header + 98 index entries + 98 script bodies, 62 reader, 61 animation) |
| records a located program reaches | 221 (98 script bodies + 62 reader containers + 61 animation containers) |
| records no located program reaches | 99 (the INTERP header and its 98 index entries: decoded structure) |
| **unused unknown records** | **0** |
| signature claims | 0 |
| programs resolved | 0 |
| programs stopping at `unknown_opcode` | 1452 (all at their first counter) |
| `probe.complete` | `false` |

So the retail corpus contains **no** unused unknown record: every byte range
no reader decodes is covered by a located program, and the only unreachable
records are decoded container structure. That is a measured result, not a
pass — AC03's scenario is exercised on the authored INTERP tail above and
through the command, and the retail assertion pins the zero so a change in
either direction fails.

Coverage still exits 0 with `--coverage` alone (the F13-B check, unchanged).
Adding `--signatures` makes signature completeness part of the check: an
incomplete table exits 3, never 0.

## Design decisions

- **Resolution requires evidence that can establish meaning.** A claim built
  from a `ContainerScan` lead, or at `inferred`, is refused (`weak_evidence`).
  This is why the table stays empty on retail instead of being filled with
  plausible guesses (spec F13 non-negotiable #4).
- **A stop is data.** `ProgramProbe` records `stop: Option<ProgramError>` and
  `reached` only for a walk that ran to the end, because `walk_program`
  returns no partial progress and this probe never pretends it did.
- **Retry is narrow on purpose.** Only `unknown_opcode` is retryable: it is
  the one stop more measured signatures fix. `truncated_opcode`,
  `empty_program`, `budget_exceeded` and `invalid_word_width` are structural
  and are refused with `not_retryable` rather than silently re-run.
- **Reachability is a join, not a decode.** A record is used when a located
  program's byte range overlaps it; the reason separates decoded structure
  from a body nobody reaches. It never marks a record as instructions and
  never rewrites the F13-A instruction status.
- **The evidence is derived from the verdict**, not appended to the record's
  kind evidence, so a re-probe replaces the verdict instead of leaving two
  notes that disagree.
- **Assumptions are visible.** `--word-bytes` and `--budget` default to 4 and
  4096 and the report states `assumed: true` when they were not given; both
  are reported in `probe` so no width hides in code (the F13-B finding that
  the instruction unit is unmeasured still stands).

## Test inventory (`accept_f13_c_*`)

`crates/cs_formats/tests/script_raw/main.rs` (3 unignored + 1 retail) and
`tools/cs_inspect/src/script_discovery.rs` (2 unignored + 1 retail + the
evidence harness):

| Test | Covers |
| --- | --- |
| `unused_unknown_record_stays_visible_with_reachability_evidence` | **AC03 minimum scenario**: the unclaimed tail stays listed with its reachability evidence; body used, structure unused; a mismatched container is refused without changing anything |
| `signature_claims_resolve_a_program_in_isolation` | the resolution loop: empty table stops at `0x0a`, `extend` + `retry` moves the stop to `0x0b`, the second claim resolves it, a resolved probe is `not_retryable`, and `teardown` closes the session (`session_closed`) |
| `probe_session_refuses_weak_claims_and_structural_stops` | invalid word widths, `empty_field`, `weak_evidence`, `duplicate_opcode`, a structural stop that is `not_retryable`, `no_such_probe`, `mismatched_program`, and two containers probed independently (one stop never aborts the run) |
| `retail_reachability_covers_the_campaign_programs` | `$CS_GAME_DIR`: 200 records across four containers all carry a verdict and evidence, 0 unused unknown, 0 of >98 programs resolved |
| `cli_resolves_signatures_and_keeps_an_unused_unknown_record` | the command end to end: 6 claims resolve both programs, exit 0, and the unused unknown record appears in `scripts.json` with its evidence |
| `cli_fails_an_incomplete_table_and_refuses_a_malformed_file` | exit 3 for an incomplete table (and exit 0 without `--signatures`), exit 2 for five malformed files, a missing file and bad `--word-bytes` / `--budget` values |
| `retail_cli_probes_every_program_and_reaches_every_record` | `$CS_GAME_DIR`: 1452 probed, 0 resolved, 320 records with evidence and none `null`, and exit 3 for a supplied one-claim table |

`cargo test --workspace --locked -- accept_f13_c_ --include-ignored` discovers
and executes 7 tests (0 ignored at that point), all passing; each also passes
when run alone with `--exact`.

## Mutation probes

Each mutation was applied, the task selection run, and the file restored:

- `set_reachability` removed from `probe_records` →
  `accept_f13_c_unused_unknown_record_stays_visible_with_reachability_evidence` fails;
- the `establishes_semantics` check removed from `SignatureClaim::new` →
  `accept_f13_c_probe_session_refuses_weak_claims_and_structural_stops` fails;
- a stopped walk reported as resolved in `run_probe` →
  `accept_f13_c_signature_claims_resolve_a_program_in_isolation`,
  `accept_f13_c_probe_session_refuses_weak_claims_and_structural_stops` and
  `accept_f13_c_retail_reachability_covers_the_campaign_programs` fail;
- `passes` ignoring the signature check →
  `accept_f13_c_cli_fails_an_incomplete_table_and_refuses_a_malformed_file` fails.

## Recorded unknowns (not guessed)

- **No mission opcode is resolved.** The table ships empty, all 1452 retail
  programs stop at their first counter and `probe.complete` is `false`. F13-C
  delivers the machinery and the honest report; the measurement is F13-D's
  and F38's work and needs an owner-supplied original run for anything
  dynamic (SCRIPT-MISSION "Source adapter acceptance").
- **The instruction unit is still unmeasured.** `--word-bytes` defaults to 4
  and says so (`assumed`); it is not a format claim.
- **Whether INTERP loading bodies share the mission language is still
  unestablished** (F13-B); probing them with the mission table would be an
  assumption, so the table is one flat namespace exactly as `OpcodeLedger`
  documents and no cross-family relation is asserted.
- **The animation payloads' encodings are unmeasured**; a `cam_anim.zbd` /
  `mis_anim.zbd` program is probed like any other and stops at its first
  counter.
- **The excluded GameZ / sound / texture families keep no records**, so they
  contribute nothing to the reachability counts; whether they hold script
  data stays unmeasured (F13-A/F13-B) and is not answered here.
- **No original run was observed.** Everything here is structural: decoding,
  byte-range overlap and caller-supplied claims. Nothing is
  `verified_original`, and the evidence claim is `implemented`.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f13_c_ --include-ignored
```

## Sources

`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the
F13-A and F13-B findings (record schema, mission scope, the empty ledger and
the unmeasured instruction unit), the F07-D findings (the caller-supplied
classification file this claims file follows), and the read-only
`$CS_GAME_DIR` listing.

# F02-C: cs-inspect inventory and dependency-impact reports

Date: 2026-09-24. Task: F02-C "Add cs-inspect inventory and dependency
impact reports"
(`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`).
Capabilities used: `retail` (production discovery and the `cs-inspect
inventory` consumer over the owner's installation at `$CS_GAME_DIR`) and
`synthetic` (authored fixture trees under the system temporary directory).

## Files and the one observable failure

Pre-edit state: `tools/cs_inspect/src/install.rs` held only the F02-A
`synthetic_install_fixture`; `main.rs` rejected every subcommand;
`cs_inspect` depended on `cs_types` alone, so the F02-B production path in
`cs_assets` was unreachable from the tool.

Functions added to `tools/cs_inspect/src/install.rs`:

- `INVENTORY_REPORT_VERSION`, `EXPECTED_ZBD_ARCHIVES`,
  `EXPECTED_GROUP_ARCHIVES`, `EXPECTED_MISSION_ARCHIVES` (expected-set
  constants).
- `ArchiveKind` (+ `label`), `ExpectedArchive` (+ `available`),
  `DependencyImpact` (+ `expected_count`, `available_count`,
  `unavailable_count`, `impacted_dependents`), `dependency_impact`.
- `inventory_report_json`, private `diagnosis_json`, `role_json`,
  `parse_state_json`, `jstr`.
- `InventoryError` (`Usage`, `MissingInstallation`, `Discovery`,
  `Output`) with `Display`, `Error`, `From<DiscoveryError>`;
  `inventory_command`, private `InventoryArgs`, `parse_inventory_args`,
  `inventory_command_result`, `write_report`.

`crates/cs_assets/src/install.rs`: `Diagnosis` gains
`directories: Vec<RelativePath>` — the real observed directory set, sorted
by logical key — populated by `diagnose`. Mission-directory detection reads
it so a mission directory that carries no regular file still reports its
expected archives as unavailable instead of being silently omitted.

`tools/cs_inspect/Cargo.toml`: `cs_assets` dependency added (the command
consumes `cs_assets::install::discover`); `Cargo.lock` updated by cargo.
`tools/cs_inspect/src/main.rs` dispatches `inventory`;
`tools/cs_inspect/src/lib.rs` doc line refreshed. Both are wiring touches
outside the listed owner paths but required by the task's "wire the
implemented path into its actual producer and consumer".

Tests (all selected by `accept_f02_c_`, 8 in total):
`tools/cs_inspect/tests/common/mod.rs` (`TempTree` fixture helper),
`accept_f02_c_dependency_impact.rs` (3, the spec's minimum scenario AC03),
`accept_f02_c_inventory_command.rs` (4, end-to-end through
`CARGO_BIN_EXE_cs-inspect`), `accept_f02_c_retail.rs` (1,
`#[ignore = "requires CS_GAME_DIR"]`).
`tools/cs_inspect/tests/evidence_report_f02_c.rs` is the evidence harness;
it is deliberately *not* prefixed `accept_f02_c_` and is `#[ignore]`d.

Observable failure if the implementation is removed or stubbed: if
`dependency_impact` dropped unavailable expected archives, a fixture tree
missing `zbd/c3/gamez.zbd` would report `unavailable: 0` and lose the
`{"expected": "zbd/c3/gamez.zbd", ..., "available": false}` row — all three
dependency-impact tests fail (verified by mutation, below).

## Mutation verification (test sensitivity)

One plausible-shortcut mutation was applied to
`tools/cs_inspect/src/install.rs`, run, then reverted (the tree was
`git status --porcelain`-clean afterwards):

1. `dependency_impact` retains only `available()` rows before returning
   → all 3 tests in `accept_f02_c_dependency_impact.rs` FAILED
   (0 passed; 3 failed). AC03 is discriminating: a missing archive must be
   a counted unavailable row, not an omission.

## Design decisions

- **The consumer is the shipped binary.** `cs-inspect inventory
  [--cs-path <dir>] [--out <file>]` runs production `discover` (F02-B) and
  renders `inventory_report_json` — the F02-C deliverable. `--cs-path`
  wins over `CS_GAME_DIR` (spec "Deliverable and interfaces"); with
  neither, the exit is 4 (missing capability). Malformed input is 2;
  discovery and output failures are 1 with the host path named by the
  propagated `DiscoveryError`/`io::Error`. `--out` is atomic (sibling
  `.tmp-<pid>` file renamed into place, removed on failure) and its final
  path is reported on stderr; without `--out` the JSON goes to stdout.
  No success is ever logged for a failure (CLI-EVIDENCE).
- **The expected set is fixed by the layout, not by the files present.**
  Expected archives: `zbd/interp.zbd` and `zbd/planes.zbd` at the
  ZBD level (planes.zbd is named by spec non-negotiable 2; interp.zbd is
  the F07 loading-script container observed at `ZBD/interp.zbd`);
  `cam_anim.zbd`, `gamez.zbd`, `texture.zbd`, `zrdr.zbd` per expected
  world group; `mis_anim.zbd` and `zrdr.zbd` per observed mission
  directory. The `rtexture*.zbd` names vary per group (observed:
  rtexture2/4/6/8 plus one of 9/10/11/12/14/15), so they are inventoried
  but not "expected". Expected groups are the union of observed groups
  and the `REFERENCE_WORLD_GROUP_LEADS` — a group absent entirely still
  reports its four archives as unavailable (non-negotiable 3: absent
  expected groups are reported).
- **Impact is structural, not guessed semantics.** Each expected row
  names its `dependent` scope — the container that loses the archive:
  `zbd`, `zbd/<group>` or `zbd/<group>/<mission>`. `impacted_dependents`
  is the sorted set of scopes with at least one unavailable archive. No
  claim is made about what the archives contain (that is F06/F10/F13
  work); `kind` labels (`zbd-archive`, `group-archive`,
  `mission-archive`) describe only the structural slot.
- **Observed conventions behind the expected set (evidence class
  `observed`, from the retail installation, read-only):** all 8 world
  groups carry `cam_anim.zbd`, `gamez.zbd`, `texture.zbd`, `zrdr.zbd`
  (8/8); every directory under a world group is a mission directory
  carrying exactly `mis_anim.zbd` + `zrdr.zbd` (53/53). A partial/demo
  installation that deviates reports the deviation as unavailable rows —
  the report's purpose — never a hard failure.
- **Case-insensitive availability, preserved spellings.** Availability
  resolves through logical keys; an available row carries the preserved
  on-disk spelling in `observed` (`"ZBD/C1/TEXTURE.ZBD"` satisfies
  `zbd/c1/texture.zbd`).
- **`Diagnosis.directories` is real observation.** Mission directories
  are the walk's observed directories exactly one component below an
  expected group — not inferred from file paths — so `ZBD/C1/EMPTY/`
  with no files still expects both mission archives (verified by test).
- **JSON is hand-rolled with escaping** (`jstr`), no new dependency; the
  retail report parses under `python3 json.load` (checked while
  inspecting the evidence artifact).

## Evidence

Harness: `tools/cs_inspect/tests/evidence_report_f02_c.rs`. From the
workspace root, after `git fetch && git rebase origin/main`:

```sh
set -o pipefail
cargo test --workspace --locked -- accept_f02_c_ --include-ignored \
  2>&1 | tee private/evidence/F02-C/cargo-test.log
# -> 8 tests discovered, 8 executed, 8 passed, 0 failed, 0 ignored

CS_EVIDENCE_DIR=private/evidence/F02-C \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f02_c_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 \
  cargo test --locked --test evidence_report_f02_c -- --ignored

python3 tools/validate_evidence.py private/evidence/F02-C/acceptance.json \
  --artifact-root private/evidence/F02-C --require-pass
```

The harness derives every report field from real inputs — the recorded
log, `rustc --version`, `Cargo.lock`, `git rev-parse 'HEAD^{tree}'` (it
refuses a stale `CS_CANDIDATE_TREE`), production
`discover`/`fingerprint`/`content_fingerprint` over `$CS_GAME_DIR` — and
the **consumer trace**: it runs the shipped `cs-inspect inventory
--cs-path $CS_GAME_DIR --out` binary and asserts the report carries the
same `install_sha256` the suite measured and `unavailable: 0`, then
references `inventory-report.json` as an artifact (spellings, sizes,
digests and availability only; original file bytes are never copied). It
asserts the retail test actually ran and passed before declaring the
`retail` capability, and fails (not passes) when the acceptance run
failed. The committed copy lives at
`docs/findings/evidence/F02-C.json`; artifacts stay in `private/`.

Retail result at submission: 228 files, 67 directories, 842 048 219
bytes inventoried; 140 expected archives (2 zbd + 8 groups × 4 +
53 mission dirs × 2), all available, zero impacted dependents; 2 ROF
candidates; all 8 reference-lead groups observed.

Note on `candidate_tree`: it is the tree of the commit whose tests
produced `cargo-test.log`. The evidence copy commits on top of it, so the
committed report references the tested tree, not the report's own commit.

## Recorded unknowns (not guessed, none blocking this stage)

- **What the expected archives contain and which are truly load-bearing**
  — `zrdr.zbd`, `gamez.zbd`, `cam_anim.zbd`, `mis_anim.zbd` member
  semantics are F06/F10/F13 scope. The report deliberately names only
  structural dependents (`zbd/<group>` etc.), so a "missing" flag is an
  availability fact, not a playability verdict.
- **Whether mission directories beyond the observed convention exist**
  (dirs carrying other members, or missions addressed outside
  `zbd/<group>/<mission>`) — the expected set flags any such deviation as
  unavailable rather than adapting silently; F02-D's full audit will
  confirm or extend the convention.
- **`interp.zbd` as an expected archive** — observed at `ZBD/interp.zbd`
  and matching the F07 interp container's documented location; an install
  without it reports `zbd/interp.zbd` unavailable, which is the intended
  signal, not a claim that every SKU carries it.
- Covered by already-queued tasks (F02-D audit, F05/F06/F07 format
  readers, F13 script inventory); `create_tasks` was not needed.

## Sources

`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`
(F02-C slice, AC01–AC04, non-negotiable behaviors 1–5);
`docs/contracts/IDENTITY-CONTENT.md` (collections cannot exclude failed
entries; lookup contract);
`docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`
(command surface, exit codes, atomic `--out`, evidence record);
`docs/research/FORMAT-NOTES.md` (interp/gamez container leads);
`docs/findings/2026-09-24-f02-b-safe-discovery-hashing-diagnosis.md`
(the producer this stage consumes); the owner's original installation at
`$CS_GAME_DIR` (read-only) for the observed layout conventions
(8/8 group archives, 53/53 mission-directory pairs).

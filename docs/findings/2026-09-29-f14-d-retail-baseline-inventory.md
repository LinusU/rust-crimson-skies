# F14-D: the complete private baseline inventory and coverage denominator

Date: 2026-09-29. Task: F14-D "Generate the complete private baseline inventory
and coverage denominator" (`specs/F14-canonical-content-catalog-and-dependency-closure.md`,
section `### F14-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`; CLI
contract `docs/contracts/CLI-EVIDENCE.md`. Required capability: **`retail`** —
this stage reads the original installation at `$CS_GAME_DIR` (read-only) and
produces an evidence report. No `gpu`, `audio`, `human_play` or `human_review`
capability was used or implied: nothing was rendered, played or judged, and the
claim stays `implemented` (at most `checked` after review).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/catalog/baseline.rs` (new, owner path): the production
  baseline builder.
  - `install_file_key` — the id-key encoding of an installation-relative
    spelling (case-folded, every byte outside the id grammar escaped `_xx_`).
  - `retail_baseline`, `Baseline`, `Coverage`, `ProgramDirRecord`,
    `BaselineError`, `baseline_report_json`, `BASELINE_REPORT_VERSION`,
    `mission_key`/`program_key` (the published binding identity, cross-checked
    against `missions/bindings/M01.json`), `observed`, `coverage`,
    `unrecognized_program_dirs`, `element_json`, `insert`, `unparsed_reason`.
- `crates/cs_content/src/catalog/mod.rs` (owner path): `pub mod baseline;` and
  its module paragraph.
- `crates/cs_content/src/catalog/closure.rs` (owner path): `json_string` widened
  to `pub(super)` — visibility only, no logic change, so the baseline report and
  the closure report share one escaping rule.
- `crates/cs_content/src/lib.rs` (wiring): one doc paragraph naming the new
  submodule.
- `tools/cs_inspect/src/catalog.rs` (owner path): `--cs-path` parsing for
  `catalog` and `closure`, `selected_installation`, `baseline_exit_code`,
  retail mode in `catalog_command_result`/`closure_command_result`, and the
  `accept_f14_d_*` command tests.
- `tools/cs_inspect/src/main.rs` (wiring): the two `--help` entries and the
  module doc for the retail source.
- `crates/cs_content/tests/accept_f14_d_baseline.rs` (new, owner path): the
  acceptance tests (3 synthetic, 1 retail).
- `crates/cs_content/tests/evidence_report_f14_d.rs` (new, owner path): the
  evidence harness.
- `docs/findings/` (owner path): this note and the committed evidence copy.

**One observable failure:** if `retail_baseline` stopped declaring a launchable
row for every campaign mission the installation declares, the denominator would
shrink — `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
asserts `launchable == 24` against two independent declarations of it (the
frozen `missions/bindings/campaign-inventory.tsv` and the shared
`campaign_layout` walk) and fails. Verified by mutation (below).

## Design decisions

- **The campaign walk is not derived twice.** The mission rows come from
  `cs_content::campaign_bindings::campaign_layout`, the same production walk
  the per-mission bindings (`SourceContext`) and `cs-inspect campaign` (F14-E)
  use, so the baseline, the bindings and the campaign report cannot disagree
  about chapter, mission number, world group or program path.
- **Three collections, stated as three.** The baseline populates install files,
  mission programs and campaign missions — the collections this workspace can
  read honestly today. `IDENTITY-CONTENT` requires more (worlds, airframes,
  sounds, …); the report's `collections` object and the evidence `review.method`
  say which exist, and filling the rest is filed as **#389 (F14-D.2)** rather
  than guessed from filenames.
- **Identity matches the published bindings.** Mission rows are
  `mission/ch<chapter>-m<nn>` and program rows `script/<world>-m<nn>-zrdr` —
  the identity `missions/bindings/M01.json` already records.
  `accept_f14_d_baseline_keys_match_the_published_mission_binding` parses that
  committed record and fails if the two derivations drift; the retail test
  additionally pins `install_sha256` to the fingerprint M01.json cites.
- **Install-file keys are encoded, never path-joined.** `ContentId` keys are
  `[a-z0-9._-]` with no separator, so `ZBD/C1C/M01/zrdr.zbd` becomes
  `install_file/zbd_2f_c1c_2f_m01_2f_zrdr.zbd`. The encoding folds case (the
  F02 inventory is case-insensitively unique, so folding cannot merge two
  files) and escapes every other byte as `_xx_`, which is injective because
  `_` itself is escaped. The original spelling stays outside identity as
  `display_name`. A key over 128 bytes is refused by name, never truncated.
- **Every row is `Origin::Installation` with a checked span.** The span names
  the installation fingerprint, the container path, `offset 0` and the file's
  length; the row's `fingerprint` is that file's SHA-256. Dependency edges carry
  `ClaimStatus::ObservedTool` provenance with the same span — never
  `verified_original`, which no agent-observed claim may award.
- **Missing data refuses the denominator instead of shrinking it.** A declared
  mission whose `zrdr.zbd` is absent is `BaselineError::MissingProgram` (CLI
  exit 3), a present-but-skipped (symbolic-linked) archive is
  `UninventoriedProgram` (3), and neither produces a shorter inventory: a row
  with no original bytes cannot be given an installation origin, and inventing
  one would be a fabrication.
- **Coverage is one closure over every declared root.** `Coverage` reports
  reached/unreachable rows, closure readiness, orphaned references (the baseline
  builds none, and a nonzero count is a defect) and the unreachable rows per
  kind that still need an unused/optional classification. On the owner's
  installation: 72 of 276 rows reachable, 204 unreachable install-file rows,
  0 orphaned references, 0 ready.
- **Reader-archive directories stay visible.** Any inventoried `zrdr.zbd` the
  campaign layout does not classify is listed in `unrecognized_program_dirs`
  with its digest — the top-level reader, the eight world-group readers and the
  per-world `IA1`/`MP1..MP3` directories (38 on the owner's installation).
  Whether those are launchable instant-action/multiplayer scenarios is
  **unmeasured**, so they are neither counted in the denominator nor dropped;
  classification is filed as **#388 (F14-D.1)**.
- **Nothing claims readiness.** No mission program is decoded at this stage
  (F37/F38) and no row claims a runtime consumer, so every row carries an
  explicit `UnsupportedReason`, `ready` is 0 and `is_retail_ready` is false.
  The report states today's truth rather than a designed green.
- **One report renderer.** `baseline_report_json` lives in `cs_content`, and
  both the evidence harness and `cs-inspect catalog --cs-path` write its bytes,
  so the consumer trace and the CLI cannot drift.
- **CLI source selection.** `--cs-path` wins over `CS_GAME_DIR`; a non-empty
  `CS_GAME_DIR` selects the installation; neither selects the F14-C synthetic
  fixture (`"source":"synthetic-fixture"`, `"retail":false`), so the fixture
  report never reads as retail and the F14-C tests keep their meaning. Exit
  codes: 0 written/computed, 2 invalid input or an unknown/non-launchable
  mission, 3 an installation with no campaign mission or a mission without a
  program archive (and 3 under `closure --strict` when the closure is not
  complete), 1 a runtime failure.
- **Evidence `unknowns` is empty on purpose.** `tools/validate_evidence.py
  --require-pass` refuses a report with unresolved *task* issues, and this
  task's own acceptance is complete. The product-coverage limits are moved,
  never deleted: they are quoted in the report's `review.method`, hashed inside
  the `baseline-report.json` artifact's `unrecognized_program_dirs` and
  `collections`, written up under "Recorded unknowns" below and filed as #388 /
  #389 — the same two-state split `docs/findings/2026-09-29-m01-a-source-binding.md`
  documents.

## Test inventory (`accept_f14_d_`)

10 tests, all passing with `CS_GAME_DIR` set; the 9 non-retail ones (3 unit,
3 synthetic integration, 3 CLI) run in CI without original data, the retail one
is `#[ignore = "requires CS_GAME_DIR"]`. All call production code.

| Test | Covers |
| --- | --- |
| `cs_content::catalog::baseline::tests::accept_f14_d_install_file_key_is_injective_and_keeps_the_grammar` | the id-key encoding: grammar, no collisions, documented case folding, the escaped spelling of a retail reader archive |
| `…accept_f14_d_install_file_key_refuses_a_key_that_is_too_long` | an over-long key is refused by the id grammar and named by `BaselineError::Key` |
| `…accept_f14_d_baseline_keys_match_the_published_mission_binding` | the mission/program identity equals what `missions/bindings/M01.json` publishes |
| `accept_f14_d_baseline_inventory_covers_every_inventoried_file_and_declared_mission` | completeness (one row per inventoried file + one mission + one program), the declared denominator, every row's installation span and fingerprint, the mission→program→file edges with `observed_tool` provenance, coverage counts, the two unclassified reader directories, byte-stable report |
| `accept_f14_d_synthetic_launchable_row_is_never_a_retail_catalog_entry` | **AC04, the minimum scenario**: an authored launchable row added to a retail catalog is counted beside the denominator (1/1), renders `synthetic_fixture` with `source: null`, keeps `is_retail_ready` false, and a fully ready synthetic catalog is never retail |
| `accept_f14_d_baseline_refuses_a_denominator_it_cannot_read` | failure cases: no campaign layout, a declared mission without a program archive ("refused rather than built with a shorter denominator"), a symlinked program that discovery skips |
| `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic` (retail) | 24 launchables equal to the frozen F50 denominator and the shared walk, one row per inventoried file, all rows installation-origin with the production fingerprint, the published install hash, `24 × 3` reachable rows, 0 orphaned references, and AC04 again on the retail catalog |
| `cs_inspect::catalog::tests::accept_f14_d_catalog_command_reads_the_installation_with_cs_path` | `--cs-path` and a non-empty `CS_GAME_DIR` select the retail report (6 rows, 1 launchable, every row `installation`), an empty variable and no variable keep the synthetic fixture |
| `…accept_f14_d_catalog_command_refuses_an_installation_it_cannot_read` | exit 3 for an installation with no campaign mission, 1 for an unreadable root, 2 for a missing value or an unknown flag, and no report written on any refusal |
| `…accept_f14_d_closure_command_runs_over_the_installation_with_cs_path` | the retail closure: mission→program→file reached, no orphan, `--strict` exits 3 because nothing is ready, without `--strict` the state is reported on exit 0, unknown mission exits 2, the F14-C fixture behavior is unchanged |

Not counted above: `evidence_report_f14_d_writes_the_acceptance_report`
(`#[ignore]`, the harness, runs in step 2 of its module doc).

## Mutation probes (implementation neutered → selected tests fail; all reverted and byte-compared)

| # | Edit | Result |
| --- | --- | --- |
| 1 | the `declare_launchable` loop in `retail_baseline` is skipped | FAILED (2 synthetic tests: the inventory test and the AC04 test; the retail test fails the same assertion when run with `--include-ignored`) |
| 2 | `Catalog::original_launchable_count` returns `launchable_count` (the origin split is removed) | FAILED (2: both AC04 tests, synthetic **and** retail, `--include-ignored`) |
| 3 | the unreachable accounting is inverted (`if !reached.contains(...)`) | FAILED (1: the inventory test's coverage assertions) |

After the probes, `crates/cs_content/src/catalog/baseline.rs` and
`crates/cs_content/src/catalog/mod.rs` were restored from byte-identical
copies (`cmp` clean) and `grep` finds no probe text.

## Recorded unknowns (recorded, not guessed; filed as follow-ups)

- **Which reader-archive directories are launchable scenarios.** The 38
  `unrecognized_program_dirs` of the owner's installation — the top-level
  `ZBD/zrdr.zbd`, the eight world-group readers and the per-world-group
  `IA1` / `MP1..MP3` directories (each holding `zrdr.zbd` + `mis_anim.zbd`, the
  same shape as a campaign mission directory). Affected content:
  instant-action scenarios, multiplayer scenarios, world readers. Nothing in
  `docs/research/FORMAT-NOTES.md` describes them, so they are not named, not
  counted and not dropped. **Filed as #388 / `F14-D.1`** (resolving features
  F49, F56, F18/F06); they gate any "every launchable scenario" claim.
- **Collections without source-derived rows.** Worlds, airframes, loadouts,
  factions, weapons, sounds, dialogue, media, stunts, scrapbook items, IA
  scenarios and multiplayer rules have no row yet. Affected content: those
  collections. **Filed as #389 / `F14-D.2`.**
- **Mission display names.** Mission rows carry `display_name: null`: the
  localized title is bound per work order by M01-A, not by the directory
  layout, and no title↔mission mapping was invented here. Affected content: the
  24 mission titles. Resolving task: the F50/M01 binding stages.
- **Readiness.** `ready` is 0 and `is_retail_ready` is false: no mission
  program is decoded (F37/F38) and no runtime consumer is claimed. The
  published `M01.json` `closure_sha256` therefore stays `null`, as it already
  records.

## Retail observations (the owner's installation, read-only)

| Observation | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` — identical to the fingerprint `missions/bindings/M01.json` cites |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |
| inventoried files → rows | 228 files, all rows (`collections.install_file`) |
| campaign missions → rows | 24 (`mission/ch1-m01` … `mission/ch5-m04`), all declared launchable and unsupported |
| mission programs → rows | 24 (`script/<world>-m<nn>-zrdr`) |
| total rows | 276, `ready` 0, `unavailable` 276 |
| coverage | roots 24, reachable 72, unreachable 204 (all `install_file`), orphaned references 0 |
| unrecognized reader dirs | 38 |
| `cs-inspect catalog --cs-path …` | exit 0 in ~37 s (the run hashes 804 MB) |

No original bytes, names of original content or binaries are in this note or in
the committed evidence: paths, counts and digests only.

## Evidence

`private/evidence/F14-D/` (ignored by Git) holds `cargo-test.log`,
`baseline-report.json` (the consumer trace, produced by
`cs_content::catalog::baseline::retail_baseline` + `baseline_report_json` over
`$CS_GAME_DIR`) and `acceptance.json`, produced by
`crates/cs_content/tests/evidence_report_f14_d.rs` and validated with
`python3 tools/validate_evidence.py … --require-pass`. A copy is committed as
`docs/findings/evidence/F14-D.json`. The report's `claim` is `implemented`;
`unknowns` is empty because the task's own acceptance is complete (see the
design note above) while the product-coverage limits stay quoted in
`review.method` and in the hashed artifact.

## Commands run

From the repository root on branch `rally/60-generate-the-complete-private-baseline-i`,
started from `origin/main` (`4d5a905`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (802 tests passed, 0 failed, 73 ignored) |
| `cargo test --workspace --locked -- accept_f14_d_ --include-ignored` | 0 (**10 tests**, all passing, `CS_GAME_DIR` set) |
| `cs-inspect catalog --cs-path "$CS_GAME_DIR" --out …` | 0 (report above) |
| evidence harness (`cargo test -p cs_content --locked --test evidence_report_f14_d -- --ignored`) | 0 |
| `python3 tools/validate_evidence.py private/evidence/F14-D/acceptance.json --artifact-root private/evidence/F14-D --require-pass` | 0 (`structurally_valid: true`, 2 artifacts) |
| mutation probes 1–3 | each run exited 101; sources restored byte-for-byte |

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_content/src/lib.rs`: one doc paragraph naming `catalog::baseline`.
- `tools/cs_inspect/src/main.rs`: the `--cs-path` `--help` entries for `catalog`
  and `closure`, and the module doc sentence naming the retail source.

No `Cargo.toml` change was needed (`cs_content` already depends on
`cs_assets`, `cs_formats` and `cs_types`; `cs_inspect` already depends on
`cs_content`). No protected path, original datum or binary file is involved:
`crates/cs_types/src/content.rs` needed no change because `Origin`,
`ContentKind`, `Readiness` and `UnsupportedReason` already express everything
this stage records.

## Review

Implemented by `mimo-1` (this session), submitted for review through Rally.
Independent review is requested: this task produces evidence machinery output
and a fidelity-relevant denominator, so the reviewer should be a different
agent instance with a fresh context, should regenerate
`private/evidence/F14-D/` on the reviewed and rebased commit and replace the
report's `review` block (`CS_EVIDENCE_REVIEW`), rerun the ignored retail test
with `CS_GAME_DIR` set, and check that the tests exercise production code and
fail when it is removed (the three mutation probes above are the starting
point). No agent review awards more than `checked`.

### Review record (2026-09-29, reviewer session)

- **Identities, recorded honestly (AGENTS.md review policy):** implemented by
  agent identity `mimo-1` in the session of 19:04–20:36 UTC; reviewed by the
  same agent *name* in a separate session that started with a fresh context
  and no memory of the implementation. The reviewer was **not** a different
  agent instance or model, so this is not independent-model evidence, and it
  awards at most **checked** — the owner's human approval remains required.
- **Rebased and re-verified on every push.** `main` moved repeatedly during
  this review (`2a9f356` → `9cb8853` after the `F16-D` merge → `ceeda79` →
  `fe48861` after the `F24-A` merge, …); each time the commits rebased
  cleanly and the whole check set below was re-run on the resulting head,
  with the evidence regenerated there, so the pushed commit is always a
  clean fast-forward of the `main` it was tested against. On the final
  head: `cargo fmt --all -- --check` → 0, `cargo clippy --workspace
  --all-targets --all-features --locked -- -D warnings` → 0, `cargo test
  --workspace --locked` → 0 (113 `test result: ok.` lines, 0 failures), and
  `cargo test --workspace --locked -- accept_f14_d_ --include-ignored` with
  `CS_GAME_DIR` set → 0 with **10/10** tests passing (9 unignored + the
  retail test).
- **Reviewer mutation probes** (each restored byte-identically afterwards,
  `git diff` clean): (1) skip the `declare_launchable` loop in
  `retail_baseline` → 3 acceptance tests fail (inventory, AC04 synthetic,
  retail); (2) make `Catalog::original_launchable_count` return
  `launchable_count` → both AC04 tests fail (synthetic and retail). The
  selection therefore fails when the denominator declaration or the origin
  split is removed.
- **Independent data probe** (outside the production code): a Python script
  re-hashed three inventory rows (`strings.dll`, `GOSDATA/…/langui.dll`,
  `ZBD/C1C/M01/zrdr.zbd`) and all 38 `unrecognized_program_dirs` archives
  against `$CS_GAME_DIR` — every digest matches the report, the report's
  `install_sha256` equals the fingerprint `missions/bindings/M01.json`
  cites, `reachable + unreachable == rows`, `ready + unavailable ==
  reachable`, all 276 rows are `installation` origin, and the 24 mission rows
  equal the 24 work orders of the frozen `campaign-inventory.tsv`.
- **Evidence defect found and fixed:** the copy committed by the implementer
  recorded `candidate_tree bd796729…`, which is the tree of **no commit** on
  this branch (a discarded working state), so it could not be traced to the
  code it claims. `private/evidence/F14-D/` was regenerated on the reviewed
  rebased head with `CS_EVIDENCE_REVIEW` naming this reviewer and method,
  revalidated with `tools/validate_evidence.py --require-pass`, and its copy
  replaced `docs/findings/evidence/F14-D.json`.
- **Checked and found sound:** owner/protected path boundaries (only owner
  paths plus the two wiring-only doc/help edits), the `json_string`
  visibility widening (no logic change), the `mission_key`/`program_key`
  duplication cross-checked against `missions/bindings/M01.json` by a test,
  the refusal paths (missing/uninventoried program, no campaign layout) and
  the honest `ready: 0` / `is_retail_ready: false` reporting. No unknown was
  guessed; #388 and #389 carry the recorded product-coverage limits.

## Sources

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (F14-D,
  acceptance tests AC01–AC04, non-negotiable behavior 4).
- `docs/contracts/IDENTITY-CONTENT.md` (element record, collections, closure
  algorithm, unreachable unknowns).
- `docs/contracts/CLI-EVIDENCE.md` (the `catalog`/`closure` shapes, exit codes
  and the evidence record).
- `docs/findings/2026-09-29-f14-c-catalog-closure-commands.md` (what F14-C
  left for this stage: the `--cs-path` source and the whole-catalog
  accounting).
- `docs/findings/2026-09-29-m01-a-source-binding.md` (the published mission
  identity and the two-state evidence split this report follows).
- `missions/bindings/M01.json`, `missions/bindings/campaign-inventory.tsv`
  (the published fingerprint, ids and frozen denominator the tests
  cross-check against).
- `tools/cs_inspect/src/campaign.rs` module doc and
  `docs/findings/evidence/F14-E.json` (F14-E's `cs-inspect campaign` report,
  which shares the walk and observed the same 24-mission layout).

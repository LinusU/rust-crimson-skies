# F14-C: expose the catalog, closure and readiness inspection commands

Date: 2026-09-29. Task: F14-C "Expose catalog, closure and readiness inspection
commands" (`specs/F14-canonical-content-catalog-and-dependency-closure.md`,
section `### F14-C`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`; CLI contract
`docs/contracts/CLI-EVIDENCE.md`. Required capability: ordinary build/test
(the machine also has `retail`, `gpu` and `audio`; **none was used** — this
stage reads no original data, renders nothing and plays nothing).

## Files and the one observable failure (listed before editing)

- `tools/cs_inspect/src/catalog.rs` (owner path): the F14-C commands.
  - `inspection_catalog_fixture` — the F14-A `synthetic_catalog_fixture`
    plus one deep ready branch (`mission/m01 → airframe/scout →
    material/scout_skin → image/scout_paint`), the authored input the
    commands consume without touching `$CS_GAME_DIR`.
  - `catalog_summary`, `catalog_report`, `element_json`,
    `catalog_command`, `catalog_command_result`, `CatalogRun`,
    `CatalogSummary`, `CATALOG_REPORT_VERSION`, `SYNTHETIC_SOURCE_LABEL`.
  - `closure_run`, `closure_report_json`, `closure_command`,
    `closure_command_result`, `ClosureRun`, `ClosureSummary`,
    `CLOSURE_REPORT_VERSION`, `parse_closure_args`.
  - `parse_catalog_args`, `report_run`, `write_atomic`, `json_string`,
    `parse_state_label`, `normalize_state_label`.
- Wiring only (no logic): `tools/cs_inspect/src/main.rs` gains the
  `catalog`/`closure` dispatch arms, their help text and the updated command
  lists; `tools/cs_inspect/src/lib.rs` documents the two commands.

**One observable failure:** if `closure_report_json` stopped rendering the
predecessor chain of an orphaned reference, the AC03 case — a texture deleted
several edges deep — would report the orphan's parent but not the
`mission/m01 → airframe/scout → material/scout_skin → image/scout_paint`
path, and `accept_f14_c_closure_command_reports_the_deep_mission_to_texture_chain`
fails. Verified by mutation (below).

## Design decisions

- **The commands consume the validated F14-A fixture, not the retail
  installation.** F14-A's finding records that the fixture exists "so tests
  and the later `catalog` command (F14-C) have a real, validated input
  without touching the owner's installation at `$CS_GAME_DIR`". Building the
  complete private baseline inventory from the original installation is
  F14-D (required capability `retail`), and it owns the same paths to add
  that source. So the F14-C commands take `--out` (and `--mission` /
  `--strict` for `closure`) and read the fixture; the report states
  `"source": "synthetic-fixture"` and `"retail": false`, and every row is
  `origin: synthetic_fixture`, so a synthetic row is never presented as a
  retail catalog entry (AC04). The `CLI-EVIDENCE` `--cs-path` shape is left
  to F14-D with the installation source it needs.
- **`catalog` is an inspection, not a check.** It renders every row —
  including failed and unavailable ones; collections cannot exclude failed
  entries — with its parse, normalize, consumer, readiness and reason
  state, plus the declared-baseline accounting (`launchable`,
  `unsupported_launchable`, `original_launchable`, `synthetic_launchable`,
  `is_fully_ready`, `is_retail_ready`). It exits 0 when the report is
  written; the readiness verdict is data in the report, not a hidden pass.
- **`closure --strict` is the failing check.** The command computes the
  transitive closure of one declared launchable root with the production
  `Closure::compute` and `CompatibilityOptions::strict()` (follow dynamic
  candidates, require a runtime consumer, refuse orphaned references). It
  exits 2 for an unknown, malformed or non-launchable root, 3 for an
  ownership cycle, and — only under `--strict` — 3 when the computed closure
  is incomplete. Without `--strict` the same data is reported on exit 0, so
  the report is the evidence of the failure rather than silence
  (`CLI-EVIDENCE`: "Never return zero after only logging a failure").
- **The consumer completes the chain the producer's JSON leaves out.**
  `Closure::to_json` reports a reached node's chain, but a deleted leaf is
  an *orphan*: it has no node row, so its chain is absent from the F14-B
  payload. The F14-C consumer calls the production `Closure::chain_to` for
  each unresolved reference and renders `unresolved_chains`, so the
  mission-to-texture path required by the minimum scenario is in the report.
  The nested `closure` object is still the unmodified `Closure::to_json`, so
  this consumer cannot drift from F14-B's report.
- **Errors propagate; the temp file is cleaned up.** `--out` is written
  atomically through a sibling `*.tmp-<pid>` file that is removed when the
  write or rename fails, and a write failure turns the run into exit 1 with
  a diagnostic instead of a silent success.
- **Deterministic output.** Rows are enumerated through `Catalog::elements`
  (canonical id order); `catalog_report` and `closure_report_json` are
  byte-stable for the same rows, and `unresolved_chains` is sorted.

## Test inventory (`accept_f14_c_`)

7 tests, all ordinary build/test, none ignored; all select production code
(the command bodies and the production catalog/closure API they call).

| Test | Covers |
| --- | --- |
| `cs_inspect::catalog::tests::accept_f14_c_catalog_command_reports_readiness_and_synthetic_origin` | the `catalog` report: counts, baseline accounting, `source`/`retail`, every row synthetic, the unsupported launchable mission stays visible and unavailable, the deep texture row is present |
| `…accept_f14_c_catalog_command_writes_out_and_is_byte_stable` | `--out` atomic write, byte-stable repeats, missing value and unknown flag are exit 2 |
| `…accept_f14_c_closure_command_reports_the_deep_mission_to_texture_chain` | **AC03 minimum scenario**: a texture deleted several edges deep yields the full mission→texture chain, an explicit orphan, an unavailable mission and exit 3 under `--strict`; exit 0 without `--strict`; the same graph is complete when the row exists |
| `…accept_f14_c_closure_command_is_strict_over_the_fixture` | the CLI wired to the fixture: a reachable all-ready mission is a complete strict closure (exit 0), the unsupported mission fails it (exit 3) |
| `…accept_f14_c_closure_command_refuses_unknown_and_non_launchable_roots` | missing/unknown/malformed/non-launchable root and unknown flag all exit 2 with no report |
| `…accept_f14_c_closure_command_writes_out` | `--out` atomic closure write and report schema |
| `…accept_f14_c_closure_reports_each_referrers_own_orphan_chain` | review regression: two elements reference the same deleted texture and each orphan reports its own `root → referrer → texture` chain, not the first-discovery chain of the target |

## Mutation probes (implementation neutered → selected tests fail; all reverted and byte-compared)

| # | Edit | Result |
| --- | --- | --- |
| 1 | the orphan chain is neutralized (`chain_to(...).filter(\|_\| false)`) | FAILED (exit 101): the AC03 chain assertion |
| 2 | the strict verdict is removed (`if false { 3 }`) | FAILED (101): two strict tests |
| 3 | the unavailable row count is forced to `0` | FAILED (101): the catalog report test |

After restoring, `tools/cs_inspect/src/catalog.rs` is byte-identical to the
pre-probe state (`shasum -a 256`), `grep` finds no probe text and the
`accept_f14_c_` selection is green (6 tests).

## Recorded unknowns (recorded, not guessed)

- **The retail catalog source is F14-D.** This stage does not read
  `$CS_GAME_DIR` and implements no `--cs-path` mode: turning the installation
  inventory into content-kind rows with real dependencies needs the
  format/mission/script parsers and the `retail` capability, and the
  complete private baseline inventory and coverage denominator are F14-D's
  deliverable. The commands' core takes a `Catalog`, so F14-D adds the
  installation source without changing the report consumers.
- **The fixture content is authored, not measured.** The rows, ids, kinds,
  edge kinds and the deep chain are synthetic fixture data; they prove the
  command, closure and report wiring, not retail behavior or the real
  mission-to-texture dependency shapes.
- **`CompatibilityOptions::strict` is a designed default** (F14-B), not a
  measured compatibility profile; `closure` exposes it as the fixed
  `--strict`/relaxed verdict only.
- **Unreachable-unknown classification is not this stage.** The contract's
  whole-catalog accounting of unreachable unknowns is F14-D, not the
  per-root closure report rendered here.

None of these is a new task: they are covered by the already-queued F14-D, so
`create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/56-expose-catalog-closure-and-readiness-ins`, started from
`origin/main` (`04ad7df`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (96 test binaries green) |
| `cargo test --workspace --locked -- accept_f14_c_ --include-ignored` | 0 (**6 tests**, all passing) |
| `cs-inspect catalog` / `cs-inspect closure --mission mission/m01 --strict` / `--mission mission/m02 --strict` | 0 / 0 / 3 |
| mutation probes 1–3 | 0: every probe run exited 101 and the file was restored byte-for-byte |

No command needed `CS_GAME_DIR`; `CS_CAPABILITIES` (`retail,gpu,audio`) was
not exercised by this stage.

## Wiring edits (outside owner paths, logic-free)

- `tools/cs_inspect/src/main.rs`: the `catalog`/`closure` dispatch arms, their
  `--help` entries, the missing/unsupported-command lists and the module doc.
- `tools/cs_inspect/src/lib.rs`: a doc paragraph naming the two commands.

No `Cargo.toml` change was needed (`cs_inspect` already depends on
`cs_content`, `cs_assets` and `cs_types`). No protected path, original datum
or binary file is involved.

## Review (deepseek-1, 2026-09-29)

Reviewed by `deepseek-1` in a fresh session (implementer `glm-1/deepseek-1`),
against the F14-C section, `IDENTITY-CONTENT` and `CLI-EVIDENCE`. Re-ran
`cargo fmt --all -- --check`, `cargo clippy --workspace --all-targets
--all-features --locked -- -D warnings`, `cargo test --workspace --locked` and
`cargo test --workspace --locked -- accept_f14_c_ --include-ignored`.

- **One defect found and fixed.** `unresolved_chains` rendered
  `Closure::chain_to(target)`, the first-discovery chain to the missing id.
  When two elements reference the same missing id only the first referrer's
  path is stored in `predecessors`, so the second orphan entry paired a
  `from` with a chain that did not pass through it. The consumer now builds
  each reference's own chain (`chain_to(from)` plus the target); a regression
  test (`accept_f14_c_closure_reports_each_referrers_own_orphan_chain`) covers
  the two-referrer case and fails against the old computation. The nested
  `Closure::to_json` payload is untouched.
- Everything else checked out: owner paths only (plus logic-free `main.rs`/
  `lib.rs` wiring), no protected path, the `accept_f14_c_` selection exercises
  production code, and a mutation probe (orphan chains neutralized) still
  fails
  `accept_f14_c_closure_command_reports_the_deep_mission_to_texture_chain`.

## Sources

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (F14-C,
  minimum scenario; acceptance tests AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (element record, closure algorithm,
  readiness).
- `docs/contracts/CLI-EVIDENCE.md` (`catalog`/`closure` shapes and exit
  codes).
- `docs/findings/2026-09-29-f14-a-content-ids-and-provenance-schema.md` (the
  fixture F14-C consumes).
- `docs/findings/2026-09-29-f14-b-normalization-and-graph-validation.md` (the
  closure and normalizer this stage wires up).

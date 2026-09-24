# F02-D: audit of the full private installation — zero unclassified gameplay files

Date: 2026-09-24. Task: F02-D "Audit the full private installation with
zero unclassified gameplay files"
(`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`).
Capabilities used: `retail` (production discovery and the `cs-inspect
audit` consumer over the owner's installation at `$CS_GAME_DIR`) and
`synthetic` (authored fixture trees under the system temporary
directory).

## Files and the one observable failure

Pre-edit state: `crates/cs_assets/src/install.rs` ended at F02-B's
`AnalysisCache`; every freshly discovered row carried `family: None`,
`FileRole::Unknown`, `ParseState::Unparsed` by design, and nothing could
say whether an installation was complete. `tools/cs_inspect/src/install.rs`
held the F02-C `inventory` command; `main.rs` rejected `audit`.

Functions added to `crates/cs_assets/src/install.rs`:

- `FileRoleKind` (+ `to_role`), `Classification`,
  `PLATFORM_BINARY_EXTENSIONS`, `PLATFORM_SUPPORT_FILES`,
  `GAMEPLAY_SCOPE_ROOTS`, `in_gameplay_scope`, `classify`.

Functions added to `tools/cs_inspect/src/install.rs`:

- `AUDIT_REPORT_VERSION`, `AUDIT_SCOPE_ALL`, `AuditFinding`,
  `InstallAudit` (+ `unclassified_gameplay`, `unclassified_other`,
  `role_count`), `install_audit`, `ReadinessCheck`,
  `full_content_readiness`, `audit_report_json`.
- `AuditError` (`Usage`, `MissingInstallation`, `Discovery`, `Output`)
  with `Display`, `Error`, `From<DiscoveryError>`; `audit_command`,
  private `AuditArgs`, `parse_audit_args`, `audit_command_result`,
  `write_audit_report`.

`tools/cs_inspect/src/main.rs` dispatches `audit` and documents it in
`--help`; `src/lib.rs` doc refreshed. Both are wiring touches outside the
listed owner paths but required by the CLI-EVIDENCE command surface
(`cs-inspect audit --scope all --strict --out`), the same pattern the
F02-C change established for `inventory`.

Tests (all selected by `accept_f02_d_`, 13 in total):
`crates/cs_assets/tests/accept_f02_d_classification.rs` (3),
`tools/cs_inspect/tests/accept_f02_d_readiness.rs` (5, covering the spec's
minimum scenario AC04), `accept_f02_d_audit_command.rs` (4, end-to-end
through `CARGO_BIN_EXE_cs-inspect`), `accept_f02_d_retail.rs` (1,
`#[ignore = "requires CS_GAME_DIR"]`).
`tools/cs_inspect/tests/evidence_report_f02_d.rs` is the evidence harness;
it is deliberately *not* prefixed `accept_f02_d_` and is `#[ignore]`d.

Observable failure if the implementation is removed or stubbed: if
`full_content_readiness` stopped counting unclassified gameplay files, a
fixture carrying `ZBD/C1/odd.dat` would pass readiness with an unknown
gameplay dependency present — verified by mutation below.

## Mutation verification (test sensitivity)

One plausible-shortcut mutation was applied to
`tools/cs_inspect/src/install.rs`, run, then reverted (the tree was
`git status --porcelain`-clean afterwards):

1. `full_content_readiness` drops the unclassified-gameplay check
   (`gameplay` bound to an empty vec) →
   `accept_f02_d_unclassified_gameplay_file_fails_readiness` FAILED
   (4 passed; 1 failed). Unknown gameplay dependencies must fail
   completeness (non-negotiable behavior 4); the AC04 partial-installation
   tests discriminate the archive half of the same check.

## Design decisions

- **The audit is a report plus a readiness verdict.** `install_audit`
  produces one `AuditFinding` per manifest row — logical key, preserved
  spelling, role kind, materialized role, rule basis and gameplay scope —
  and composes the F02-C `dependency_impact` alongside it. Nothing is
  dropped: an unclassified file is a finding that names its file
  (IDENTITY-CONTENT: collections cannot exclude failed entries).
- **Classification rules are bound to observed shapes only.**
  `classify` assigns: `dll`/`exe`/`icd` anywhere → platform-support
  (native binaries are never parsed as game data); the named installer
  leftovers `00000409.016`, `00000409.256`, `ebusetup.sem` and `*.rtf`
  documents → platform-support; `zbd/**` members ending `.zbd` →
  needed-unimplemented (the F06 reader is not implemented);
  `gosdata/assets/*.rof` → needed-unimplemented (F05);
  `gosdata/assets/graphics/*.tga` → needed-unimplemented (F08);
  `gosdata/assets/graphics/mpg/*.mpg` → optional-media (cutscene videos;
  absence does not block gameplay content); `crimsonff.ifr` →
  needed-unimplemented (force-feedback effect resource, no consumer yet).
  A file matching no rule stays `Unknown` — recorded and reported, never
  guessed (non-negotiable 4, "Unknown means unknown").
- **`FileRoleKind` is a copyable mirror of `FileRole`.** The classifier
  returns rule-owned `&'static str` bases; `to_role` materializes the
  schema type. `UnusedWithReason` carries its reason in the kind, so no
  classification can ever produce the empty reason the schema rejects.
  No retail file is classified `unused` — the observed tree has no
  leftover content — and `consumed` stays at 0 because no format reader
  exists yet (F05/F06/F07/F08 land later; the roles are honest).
- **Readiness is a check, not a guess.** `full_content_readiness` fails
  when the F02-C expected set reports unavailable archives (a denominator
  the present files cannot shrink) **or** when any unclassified file sits
  inside the gameplay scope (`zbd/`, `gosdata/`). `--strict` additionally
  fails on unclassified files outside the scope: not proven gameplay
  dependencies, but a strict audit classifies every file. AC04 follows:
  a partial installation can never pass. Every failure names the exact
  impacted dependents or file keys.
- **The verdict is a class, not a claim of completeness.** The report
  emits `classes: ["full"]` on a pass and `["partial"]` on a failure —
  the spec's recognized installation classes. `patched`, `localized` and
  `demo-like` are deliberately absent: no evidence rule for them was
  established here, so they are recorded as unknowns below rather than
  asserted.
- **Exit codes follow CLI-EVIDENCE.** `0` ready, `3` failed validation
  (the report is still produced and lists the failures), `2` invalid
  input/unsupported `--scope`, `4` no installation, `1` discovery or
  output failure. `--out` reuses the sibling-temp atomic write; without
  it the JSON goes to stdout and human output stays on stderr.
- **JSON is hand-rolled** (`jstr`), no new dependency; the retail
  audit report parses under `python3 json.load` (checked while inspecting
  the evidence artifact).

## Retail audit result

Over the owner's installation at `$CS_GAME_DIR` (228 files, 842 048 219
bytes, install `b4e780ab…`, content `a0223506…`):

- **189 needed-unimplemented**: 184 `.zbd` archives under `ZBD/`
  (6 zbd-level + 8 groups × archives incl. `rtexture*` + 53 mission
  directories × 2), `crimson.rof` + `crimptch.rof`, `font.tga` +
  `arial8.tga`, `crimsonff.ifr`.
- **10 optional-media**: the `GOSDATA/ASSETS/GRAPHICS/MPG/*.mpg`
  cutscenes.
- **29 platform-support**: 21 root DLLs/EXEs/ICD, `EBUSetup.sem`,
  `00000409.016`, `00000409.256`, `EULA.RTF`, `Readme.rtf`, and the four
  `GOSDATA/ASSETS/BINARIES/*.dll` libraries.
- **0 unknown — zero unclassified gameplay files**, the task's claim.
- Dependency impact: 140/140 expected archives available, no impacted
  dependents; `full_content` readiness passes under `--strict`, class
  `full`.

The retail test re-derives the role counts with its own walk (extension
and path rules written independently of the production table) and
asserts the production counts match; the command's `--out` report is
byte-identical to the library-rendered report.

## Evidence

Harness: `tools/cs_inspect/tests/evidence_report_f02_d.rs`. From the
workspace root, after `git fetch && git rebase origin/main`:

```sh
set -o pipefail
cargo test --workspace --locked -- accept_f02_d_ --include-ignored \
  2>&1 | tee private/evidence/F02-D/cargo-test.log
# -> 13 tests discovered, 13 executed, 13 passed, 0 failed, 0 ignored

CS_EVIDENCE_DIR=private/evidence/F02-D \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f02_d_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 \
  cargo test --locked --test evidence_report_f02_d -- --ignored

python3 tools/validate_evidence.py private/evidence/F02-D/acceptance.json \
  --artifact-root private/evidence/F02-D --require-pass
```

The harness derives every report field from real inputs — the recorded
log, `rustc --version`, `Cargo.lock`, `git rev-parse 'HEAD^{tree}'` (it
refuses a stale `CS_CANDIDATE_TREE`), production
`discover`/`fingerprint`/`content_fingerprint` over `$CS_GAME_DIR` — and
the **consumer trace**: it runs the shipped `cs-inspect audit --cs-path
$CS_GAME_DIR --scope all --strict --out` binary and asserts the report
carries the same `install_sha256` the suite measured, `full_content:
true` and `unknown: 0`, then references `audit-report.json` as an
artifact (spellings, roles, bases and availability only; original file
bytes are never copied). It asserts the retail test actually ran and
passed before declaring the `retail` capability, and fails (not passes)
when the acceptance run failed. The committed copy lives at
`docs/findings/evidence/F02-D.json`; artifacts stay in `private/`.

Note on `candidate_tree`: it is the tree of the commit whose tests
produced `cargo-test.log`. The evidence copy commits on top of it, so
the committed report references the tested tree, not the report's own
commit.

## Recorded unknowns (not guessed, none blocking this stage)

- **`patched`, `localized` and `demo-like` detection.** `crimptch.rof`
  is present and its name suggests patch data, and `00000409.*` suggests
  an English (0x0409) installer — but no verified mechanism ties either
  to an installation *class*, so the audit emits only `full`/`partial`
  and leaves the other classes unasserted. Detection rules need
  reference evidence (different SKUs/patch states), which is
  unavailable.
- **What the classified files contain.** `zbd`, `rof`, `tga`, `mpg` and
  `ifr` members are classified by observed location and naming
  convention only; their member semantics are F05/F06/F08 scope. The
  `needed-unimplemented` role is a statement about our readers, not a
  claim about the bytes' meaning.
- **`crimson.icd` vs `crimson.exe`.** Both classify platform-support
  (native binaries, never parsed); which loader chain used which file is
  DRM/platform trivia this engine does not need and did not investigate.
- **Mission directories that carry unexpected members.** The observed
  convention (exactly `mis_anim.zbd` + `zrdr.zbd`, 53/53) means a
  deviation lands either as an unavailable expected archive or as an
  unclassified extra file — both fail readiness, which is the intended
  signal rather than silent adaptation.
- Covered by already-queued tasks (F05/F06/F07/F08 format readers, F13
  mission inventory); `create_tasks` was not needed.

## Sources

`specs/F02-installation-discovery-versions-and-exhaustive-inventory.md`
(F02-D slice, AC04, non-negotiable behaviors 1–5);
`docs/contracts/IDENTITY-CONTENT.md` (collections cannot exclude failed
entries; unknown stays unknown);
`docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`
(command surface, exit codes, evidence record);
`docs/findings/2026-09-24-f02-c-cs-inspect-inventory-dependency-impact.md`
(the expected-set denominator this audit consumes);
`docs/findings/2026-09-24-f02-b-safe-discovery-hashing-diagnosis.md`
(the producer this stage audits);
the owner's original installation at `$CS_GAME_DIR` (read-only) for the
observed layout and file shapes.

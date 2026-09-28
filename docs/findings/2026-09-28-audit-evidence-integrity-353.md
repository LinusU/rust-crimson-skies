# AUDIT-EVIDENCE-INTEGRITY (#353): reject dangling evidence references and incomplete test accounting

Date: 2026-09-28
Task: #353 `AUDIT-EVIDENCE-INTEGRITY` (Rally branch `rally/353-reject-dangling-evidence-references-and`)
Implementing agent: deepseek-1 (resumed the owner handover notes #911/#912; owner-authored source, locally re-run and re-verified here).

## Scope

Structural validation only. This change makes `tools/validate_evidence.py` reject reports whose
assertion evidence points at something that is not a declared, existing, hash-matching artifact, and
reports whose selected test counts do not add up. It does not certify gameplay, visuals or original
behavior, does not relax any unknown/claim gate, and does not make positive reports match the old
validator. Schema-v1 `unknowns`/`claim` semantics are deliberately unchanged; #354 owns the versioned
task-success/product-readiness split and #355 owns regenerating historical producer reports.

## What changed

- `tools/validate_evidence.py`: replaced the validator. Every nested object is shape-checked before
  field access; assertion ids and artifact declarations are unique; every `assertions[].evidence`
  string is a nonempty canonical relative path that must resolve to a declared artifact; declared
  files are hash-checked; symlinks on any path component, absolute/Windows/backslash/`..`/control-char
  spellings and duplicate paths are rejected; exact test accounting
  `discovered == executed + ignored` and `executed == passed + failed`; `--require-pass` also requires
  `exit_code == 0`, a nonempty selection, `failed == 0`, `ignored == 0`, and all assertions passing
  with evidence. The return value still carries `claims_semantically_verified: false`.
- `tools/tests/test_validate_evidence.py`: new production-path unittest regressions
  (`accept_audit_evidence_integrity_` prefix) that import the real `validate()` and drive the real CLI.
- `docs/contracts/CLI-EVIDENCE.md`: documented the v1 structural rules above.
- `.github/workflows/ci.yml`: added a "Evidence validator regression tests" step to the existing
  `pack` job; every existing Rust, fixture-reproducibility and binary-content check is untouched.
- `schemas/evidence.schema.json` was not modified; its v1 shapes already match the validator.

## Commands and results (all run locally on this checkout)

Identities of the reviewed files:

```
$ git hash-object tools/validate_evidence.py
39ccd268108e3f315c62ca81fc9aa01d913191c9   # matches owner note #912
$ git hash-object tools/tests/test_validate_evidence.py
47db18e0532ef778c93b277426ffd5e7a3c0e632   # matches owner note #912
```

Task-specific discovery and execution (owner clarification: Python discovery, no invented Rust shim):

```
$ python3 -m unittest discover -s tools/tests -p 'test_validate_evidence.py' -v
Ran 16 tests in 0.133s
OK            # 16 tests, 0 failures, 0 errors, 0 skipped
```

The two confirmed audit failures are now rejected (CLI, `--require-pass`):

```
dangling (assertion -> missing.log, artifacts=[]):
  exit=3  Invalid evidence: Undeclared evidence artifact(s): missing.log
unaccounted (discovered=10 executed=1 passed=1 failed=0 ignored=0):
  exit=3  Invalid evidence: Contradictory or incomplete test counts
positive complete manifest:
  exit=0
```

The same 16-test suite run against the pre-change validator (`git show HEAD:tools/validate_evidence.py`)
reports `FAILED (failures=29, errors=21)`, so the regressions fail when the new implementation is removed.
The new validator was restored and its blob hash re-checked afterwards.

Full local project gates (Rust changes are unrelated; run to prove the branch is green):

```
cargo fmt --all -- --check                                          -> 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings -> 0
cargo test --workspace --locked                                     -> 0
```

## Compatibility with committed evidence

A scan of `docs/findings/evidence/*.json` found no noncanonical evidence references and no
contradictory selected test counts, so the stricter structure does not retroactively invalidate the
committed summaries' fields. Their artifact files are not committed (they live in ignored `private/`),
so full hash verification still requires the producing harness; that regeneration is #355.

## Remaining limitations

- No original-game/retail, GPU, audio or human evidence is claimed or needed here. This is a
  synthetic, Python-only structural task.
- The symlink regression is skipped on Windows (`os.name == 'nt'`) because creating symlinks there
  needs privileges; it runs on Linux CI. Real Windows coverage is #361.
- GitHub CI was not run by this session directly; it runs on the pushed commit and must be green
  before merge. The local Rust gates above plus the Python suite stand in only as local checks.
- `unknowns` still rejects `--require-pass`; that v1 behavior is intentionally preserved. Separating a
  gated task success from product limitations is #354 (`AUDIT-EVIDENCE-MODEL`).
- Historical reports whose producing harness is unavailable must be rerun rather than hand-edited;
  #355 (`AUDIT-EVIDENCE-PRODUCERS`) owns that. No historical evidence was rewritten here.

Reviewer note: the code is owner-authored and re-verified by this session; this is not independent
original-reference evidence. The Rally reviewer should reproduce the Python suite, re-check the two
blob hashes and run the Rust gates on the rebased commit.

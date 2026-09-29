# F14-B: normalization and graph validation

Date: 2026-09-29. Task: F14-B "Implement normalization and graph validation"
(`specs/F14-canonical-content-catalog-and-dependency-closure.md`, section
`### F14-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Required
capability: ordinary build/test (the machine also has `retail`, `gpu` and
`audio`; **none was used** — this stage reads no original data, renders
nothing and plays nothing).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/content.rs` (owner path): adds
  `DependencyKind::Ownership` (+ `is_ownership`), the canonical `Unit`
  vocabulary (`ALL`, `label`, `symbol`, `from_label`), `PermittedRange`
  (+ `new`, `min`, `max`, `contains`) and `RangeError`, and the
  `UnsupportedReason::{NotParsed, NotNormalized}` variants used by the
  closure. `ContentKind` and the rest of the F14-A schema are unchanged.
- `crates/cs_content/src/catalog/normalize.rs` (new, owner path):
  `UnitScale` (`Canonical`, `Declared`, `Unknown`), `QuantityRule`
  (`canonical`, `converted`, `unknown_unit`), `FieldInput`,
  `NormalizedField`, `NormalizeError`, `normalize_field`,
  `ElementNormalization` and `normalize_element`.
- `crates/cs_content/src/catalog/closure.rs` (new, owner path):
  `CompatibilityOptions` (`strict`, `encode`), `ClosureError`
  (`UnknownRoot`, `NotLaunchableRoot`, `OwnershipCycle`),
  `UnresolvedReference`, `NodeStatus` and `Closure`
  (`compute`, `roots`, `options`, `hash`, `node_ids`, `is_reached`, `status`,
  `is_ready`, `reasons`, `unavailable`, `unresolved`, `is_complete`,
  `chain_to`, `to_json`).
- `crates/cs_content/tests/accept_f14_b_dependency_closure.rs` (new):
  the AC02 minimum scenario plus the chain, cycle, propagation, options and
  root-validation tests.
- `crates/cs_content/tests/accept_f14_b_normalization.rs` (new): the
  normalization acceptance tests.
- Wiring only (no logic): `crates/cs_content/src/catalog/mod.rs` gains
  `pub mod closure;` / `pub mod normalize;` and a doc paragraph;
  `crates/cs_content/src/lib.rs` gains a doc paragraph;
  `crates/cs_types/src/content.rs`'s module doc names F14-B.

**One observable failure:** if `Closure::compute` stopped folding
dependencies into a node's readiness (`dependencies_ready` returns `true`),
`accept_f14_b_closure_propagates_unsupported_dependencies_along_the_chain`
and `accept_f14_b_unnormalized_and_unsupported_nodes_block_their_dependents`
would report a ready mission that references an unsupported texture several
edges deep — the exact failure F14-B must prevent. Verified by mutation
(below). The second observable failure for the normalization half is
`normalize_field` treating an unrecognized unit as canonical (assuming SI),
which `accept_f14_b_normalization_refuses_missing_unknown_nonfinite_and_out_of_range`
pins.

## Design decisions

### Normalization (`cs_content::catalog::normalize`)

- **A rule declares the unit, the range and how the raw unit is known.**
  `QuantityRule` carries the canonical `Unit`, the approved `PermittedRange`
  and a `UnitScale`. A `Canonical` scale means the raw number is already in
  the canonical unit; a `Declared` scale multiplies by an evidence-carrying
  `canonical_per_source` factor; an `Unknown` scale is a refusal. Nothing
  assumes SI from a spelling.
- **A refusal is named, never defaulted.** `normalize_field` returns
  `MissingValue` for an explicitly unknown critical value, `UnknownUnit` for
  an unrecognized unit, `InvalidConversionFactor` for a non-positive or
  non-finite factor, `NonFinite` for NaN/infinity (before and after
  conversion) and `OutOfRange` quoting the field, value, range and unit. No
  path returns a zero, a `Default::default()` or a raw value under a
  different unit than the rule declared.
- **Element normalization is deterministic and all-or-failed.**
  `normalize_element` sorts raw fields by name, refuses duplicate names and
  undeclared fields, and returns `NormalizeState::Normalized` with the fields
  in canonical name order only when every field normalized. One failure
  yields `NormalizeState::Failed` with that failure's diagnostic and no
  fields. Parsing is a different state and is untouched.
- **The normalized value keeps the raw value's provenance.** A converted
  value's `NormalizedField::provenance` is the raw `Resolved`'s provenance;
  the conversion factor's own provenance lives on the `Declared` scale, so a
  measured original unit and an authored ratio cannot be conflated.

### Graph validation (`cs_content::catalog::closure`)

- **The closure is a value, not a mutation.** `Closure::compute` reads the
  catalog and returns a separate value; parsing, normalization, dependency
  validation and runtime readiness stay separate states (non-negotiable
  behavior 1). The catalog's own `readiness`/`unsupported_reasons` are never
  rewritten.
- **Readiness is a fixpoint over the reached subgraph.** A node is ready when
  its own rules pass (its `Readiness`, parse state, normalize state and — by
  default — at least one runtime consumer) and every reached dependency is
  ready. Reference cycles are legitimate, so this is an iterative
  downgrade-to-fixpoint (start all own-ready, mark any node with an unready
  dependency unready until stable), not a recursive topological walk. A cycle
  containing an unsupported node becomes unready; an all-ready reference
  cycle stays ready.
- **An unsupported dependency is propagated, not hidden.** A node that is
  own-ready but has an unready or orphaned dependency gets
  `UnsupportedReason::UnsupportedDependency { target }` for each such
  dependency in canonical order. `NormalizeState::NotNormalized` is
  `not_normalized` and `ParseState::Unparsed` is `not_parsed`, so an
  unvalidated node cannot be silently ready.
- **Orphaned references are explicit.** A dependency target with no catalog
  row becomes an `UnresolvedReference` with its edge kind and provenance, and
  (by default) keeps its parent unavailable. The reference is never skipped
  as if it were "no dependency".
- **The predecessor chain survives a deleted leaf.** The walk records the
  first predecessor (in canonical BFS order) for every reached id *and* for
  every orphaned target, so `chain_to` still reports
  `mission → … → texture` after the texture row is deleted.
- **Ownership cycles are invalid; reference cycles are not.**
  `DependencyKind::Ownership` marks parent/child hierarchy edges. A
  deterministic iterative DFS over the ownership subgraph refuses any cycle
  with `ClosureError::OwnershipCycle { cycle }`. Plain reference/speculative
  cycles are allowed.
- **Deterministic order everywhere.** Roots are a `BTreeSet`; each node's
  dependencies are sorted by `(target, kind)` before traversal; edges,
  unresolved references and nodes all use canonical order. The closure hash
  and `to_json` therefore do not depend on insertion or enumeration order
  (AC02).
- **The hash is over ids, fingerprints, state and options.**
  `Closure::hash` hashes the sorted roots, every reached node id with its
  closure state and content fingerprint, its reason strings, the orphaned
  references and the compatibility options. Different
  `CompatibilityOptions` are a different closure, so the options are part of
  the hash. `to_json` emits the same data as canonical JSON with escaped
  strings.
- **Dynamic candidates are followed by default.** `follow_dynamic_candidates`
  defaults to `true` because a bounded candidate set is still a dependency;
  not following it would let an unbounded lookup look like "no dependencies".
  The option is exposed and included in the hash so a caller can state the
  choice.

## Test inventory (`accept_f14_b_`)

15 tests, all ordinary build/test, none ignored; 15 select production code
(6 unit tests in the two owner modules and 9 integration tests).

| Test | Covers |
| --- | --- |
| `cs_content::catalog::normalize::tests::accept_f14_b_normalize_canonical_and_converted_values` | canonical and declared-conversion normalization, unit/range and provenance |
| `cs_content::catalog::normalize::tests::accept_f14_b_normalize_refuses_missing_unknown_and_out_of_range` | missing, unknown-unit, NaN, out-of-range and bad-factor refusals |
| `cs_content::catalog::normalize::tests::accept_f14_b_normalize_element_is_ordered_and_all_or_failed` | canonical field order, all-or-failed, duplicate and undeclared fields |
| `cs_content::catalog::closure::tests::accept_f14_b_closure_is_stable_under_reordering` | closure hash and `to_json` stable across input order |
| `cs_content::catalog::closure::tests::accept_f14_b_closure_propagates_unsupported_dependencies_along_the_chain` | deep chain readiness propagation and `chain_to` |
| `cs_content::catalog::closure::tests::accept_f14_b_orphans_and_ownership_cycles_are_reported` | orphan reference and ownership-cycle refusal |
| `…accept_f14_b_randomizing_input_enumeration_keeps_ids_and_serialized_order_stable` | **AC02 minimum scenario**: three permutations × two dependency orderings of a diamond graph give one id set, one hash and one JSON; the canonical chain picks `airframe/alpha` regardless of list order |
| `…accept_f14_b_deeply_deleted_texture_keeps_its_mission_chain_and_blocks_readiness` | AC03 mechanism: mission → airframe → material → deleted texture |
| `…accept_f14_b_reference_cycles_are_allowed_and_ownership_cycles_are_invalid` | the cycle policy both ways |
| `…accept_f14_b_unnormalized_and_unsupported_nodes_block_their_dependents` | `not_normalized` is a separate state and propagates |
| `…accept_f14_b_compatibility_options_change_the_closure_hash_and_dynamic_edges` | options are hashed; dynamic candidates followed/suppressed |
| `…accept_f14_b_roots_must_be_present_and_launchable` | unknown and non-launchable root refusals |
| `cs_content::tests::accept_f14_b_normalization_converts_declared_units_with_provenance` | integration: unit conversion and provenance |
| `…accept_f14_b_normalization_refuses_missing_unknown_nonfinite_and_out_of_range` | integration: every refusal path |
| `…accept_f14_b_element_normalization_is_deterministic_and_all_or_failed` | integration: element normalization |

## Mutation probes (implementation neutered → selected tests fail; all reverted and byte-compared)

| # | Edit | Result |
| --- | --- | --- |
| 1 | `dependencies_ready` ignores unready dependencies (`Some(_) => {}`) | FAILED (exit 101) |
| 2 | orphaned references are never recorded (`if false`) | FAILED (101) |
| 3 | ownership adjacency is never populated | FAILED (101) |
| 4 | the canonical dependency sort is removed | FAILED (101) |
| 5 | an unrecognized unit is treated as canonical | FAILED (101) |
| 6 | the permitted-range check is removed | FAILED (101) |
| 7 | the compatibility options are excluded from the hash | FAILED (101) |

After restoring, `crates/cs_content/src/catalog/closure.rs` and
`normalize.rs` are byte-identical to the pre-probe state (`shasum -a 256`),
`grep -rn "MUTATION PROBE" crates tools` prints nothing and the
`accept_f14_b_` selection is green.

The unit test count is 15 rather than the 12 an earlier draft had because the
first probe run's `P3` pattern did not match after `cargo fmt` reflowed the
statement; the probe was re-run against the formatted text and caught. The
probe script's failure to find a pattern is recorded here, not hidden: a
silent miss would have been a false "not caught".

## Recorded unknowns (recorded, not guessed)

- **The original units, ranges and conversion factors are unmeasured.**
  `Unit` is the contract's canonical vocabulary; the only conversions here
  are authored test declarations (`0.3048 m/ft`). Which original value is in
  which unit, and what its measured range is, arrive with the format/mission
  tasks and F14-D. No original conversion is asserted.
- **The global accounting report is not this stage.** `Closure` covers the
  reached subgraph of the declared roots. The contract's "unreachable
  unknowns remain in the global accounting report and need an unused/optional
  classification" is the F14-C/F14-D report over the whole catalog, not the
  per-root closure.
- **`CompatibilityOptions` are designed defaults.** Follow dynamic
  candidates, require a runtime consumer and refuse to tolerate orphaned
  references are engineering choices; the sheet requires them to be explicit,
  not that these particular defaults match the original. A measured
  compatibility profile can change them through an approved design update.
- **Which kinds own which, and which edges are ownership/static/dynamic, is
  unmeasured.** The tests' graphs are authored fixtures; the format, script
  and mission tasks decide the real edge kinds and F14-D the retail baseline.
- **The closure hash's exact input list is an engineering design.** It
  includes ids, fingerprints, closure state, reasons and options; a consumer
  that needs a different compatibility-option vocabulary can extend it
  without changing the deterministic ordering rules.

None of these is a new task: they are covered by F14-C, F14-D and the
format/mission tasks already in the plan, so `create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/55-implement-normalization-and-graph-valida`, Rust 1.98.1, started from
`origin/main` (`9aec234`).

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (95 test binaries green) |
| `cargo test --workspace --locked -- accept_f14_b_ --include-ignored` | 0 (**15 tests**, all passing) |
| `python3 /private/.../mutate_f14b.py` (probes 1,2,4–7) | 0: every probe's run exited 101 and both files were restored byte-for-byte |
| the `P3` probe, re-run against the formatted text | 0: exited 101 |

No command needed `CS_GAME_DIR`; `CS_CAPABILITIES` (`retail,gpu,audio`) was
not exercised by this stage.

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_content/src/catalog/mod.rs`: `pub mod closure;` /
  `pub mod normalize;` plus a doc paragraph.
- `crates/cs_content/src/lib.rs`: a doc paragraph naming the two F14-B
  modules.

No `Cargo.toml` change was needed (`cs_content` already depends on
`cs_assets`, whose `Sha256` the closure hash uses). No protected path,
original datum or binary file is involved. `tools/cs_inspect/src/catalog.rs`
is an owner path but was left untouched: the `catalog`/`closure` commands are
F14-C's, and F14-A already records that `main.rs` keeps refusing them.

## Sources

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (F14-B,
  acceptance tests AC02/AC03, non-negotiable behaviors 1–5).
- `docs/contracts/IDENTITY-CONTENT.md` (dependency closure algorithm, lookup
  and numeric contracts).
- `docs/findings/2026-09-29-f14-a-content-ids-and-provenance-schema.md` (the
  schema this stage normalizes and walks).
- `docs/TASK-SPLITTING.md` (the bounded-slice rule this stage keeps).

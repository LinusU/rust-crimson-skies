# F14-A: stable content ids and a provenance-bearing schema

Date: 2026-09-29. Task: F14-A "Define stable content ids and provenance-bearing
schema" (`specs/F14-canonical-content-catalog-and-dependency-closure.md`,
section `### F14-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Required capability: ordinary build/test
(the machine also has `retail`; it was **not** used — this stage reads no
original data).

## Files and the one observable failure (listed before editing)

- `crates/cs_types/src/content.rs` (new): `ContentKind` (+ `ALL`, `label`,
  `from_label`, `is_launchable`), `ContentIdError`, `ContentId`
  (`from_source`, `parse`, accessors), `Origin` (+ `is_original`, `label`,
  `source`), `Provenance` (+ `new`, `designed`, `unknown`),
  `ProvenanceError`, `Known<T>`, `Resolved<T>` (+ `unknown`, `is_known`,
  `known`, `provenance`), `ResolvedError`, `ConsumerKind`, `RuntimeConsumer`,
  `DependencyKind`, `Dependency`, `NormalizeState`, `Readiness`,
  `UnsupportedReason` (+ `code`, `detail`), `ElementError`,
  `CatalogElement` (+ `validate`, `is_ready`, `unsupported_codes`), plus the
  `accept_f14_a_*` schema tests.
- `crates/cs_content/src/catalog/mod.rs` (new): `Catalog` (`insert`, `get`,
  `elements`, `sorted_ids`, `len`, `is_empty`, `declare_launchable`,
  `launchable_count`, `unsupported_launchables`, `unsupported_count`,
  `is_fully_ready`, `original_launchable_count`,
  `synthetic_launchable_count`, `is_retail_ready`) and `CatalogError`.
- `crates/cs_content/tests/accept_f14_a_catalog_readiness.rs` (new): the
  catalog acceptance tests.
- `tools/cs_inspect/src/catalog.rs` (new): `synthetic_catalog_fixture` and
  its `accept_f14_a_*` fixture tests.
- Wiring only: `crates/cs_types/src/lib.rs` gains `pub mod content;`,
  `crates/cs_content/src/lib.rs` gains `pub mod catalog;` (and a doc
  paragraph), `tools/cs_inspect/src/lib.rs` gains `pub mod catalog;` (and a
  doc paragraph). No logic in those edits.
- `tools/cs_inspect/src/main.rs` is **not** touched: the `catalog` and
  `closure` commands need a mounted installation and the closure walk, which
  arrive with F14-B/F14-C, so the binary keeps refusing them.

**One observable failure:** adding an unsupported launchable mission while
neutering the baseline accounting (`Catalog::unsupported_count` returns `0`)
leaves `accept_f14_a_adding_unsupported_mission_increases_unsupported_count`
failing on the exact count; the AC01 minimum scenario would report a fully
ready catalog that is not ready. Verified by mutation (below).

## Design decisions

- **Identity is namespace + semantic key, normalized at construction.**
  `ContentId` is `ContentKind` (`namespace`) plus a key normalized to
  lowercase `[a-z0-9._-]` (max 128 bytes). It is built by
  `from_source(kind, source_key)` — a mission id, an airframe name — never
  from an enumeration index, so the same content keeps one id regardless of
  input order (AC02). The key grammar excludes `/`, `\`, `:` and blank
  components, so a key can never be joined to a path or escape its namespace
  (`IDENTITY-CONTENT`: "validate at construction; no unchecked path join").
  Equality, hashing and ordering use the canonical `namespace/key` text, so
  catalog iteration is deterministic. Original display names live outside
  identity in `CatalogElement::display_name`.
- **One canonical namespace enum, open about the mapping.** `ContentKind`
  is the union of the `IDENTITY-CONTENT` required collections and spec F14's
  deliverable ("worlds, missions, airframes, loadouts, factions, weapons,
  sounds, dialogue, media, stunts, scrapbook items, IA scenarios and
  multiplayer rules"). `from_label` scans `ALL`, so `label` and `from_label`
  cannot disagree; a test asserts the labels are unique and round-trip. The
  kind is engine-authored vocabulary: which original file backs a kind is
  the format/mission tasks' measurement, not guessed here.
- **Provenance reuses the F01 epistemic vocabulary.** The contract sketches
  an `EvidenceClass`; its seven states are exactly `evidence::ClaimStatus`,
  so `Provenance.class` is a `ClaimStatus` instead of a second enum that
  could drift. A `verified_original` provenance must name a source span
  (F01-A's rule), which `Provenance::new` enforces.
- **Values are known or explicitly unknown.** `Resolved<T>` is
  `Known(Known<T>)` or `Unknown { claim_id, reason }`; building an unknown
  with an empty reason is refused. There is no `Default::default()` path to
  a critical value (non-negotiable behavior 3).
- **Origins keep synthetic apart from installation data.** `Origin` is
  `Installation { source }`, `SyntheticFixture` or `Designed`, and only
  `Installation` is `is_original`. The catalog counts original and synthetic
  launchable rows separately, and `is_retail_ready` requires a fully ready
  baseline with no synthetic row — so a synthetic launchable row is never
  mistaken for a retail catalog entry (AC04).
- **Parsing, normalization and readiness are separate fields.** Parse state
  reuses `install::ParseState`; `NormalizeState` and `Readiness` are their
  own types; `CatalogElement::validate` refuses a ready element with
  reasons, an unavailable element with none, empty failed diagnostics and
  empty reason details. A visible but unavailable row stays in the
  collection with its reasons.
- **Readiness is measured over a declared baseline, not the supported
  subset.** The launchable mission/scenario ids are declared explicitly via
  `declare_launchable`; `unsupported_count` counts declared rows whose
  element is not ready and `is_fully_ready` is that count being zero. An
  unsupported mission therefore increases the count and blocks readiness
  instead of being filtered out of the denominator (non-negotiable behavior
  4, AC01). `declare_launchable` refuses an unknown id and a non-launchable
  kind.
- **Duplicate identities fail visibly.** `Catalog::insert` refuses a second
  row with the same id rather than merging or overwriting; contradictory
  duplicates must be resolved explicitly (non-negotiable behavior 5).
- **The fixture is entirely synthetic.** `synthetic_catalog_fixture` builds
  a ready world/airframe/image/sound, a ready launchable mission, an
  unsupported launchable mission and an unsupported non-launchable resource,
  all `Origin::SyntheticFixture`, so tests and F14-C have a validated input
  without touching `$CS_GAME_DIR`.

## Test inventory (`accept_f14_a_`)

| Test | Covers |
| --- | --- |
| `content::tests::…content_id_is_namespaced_normalized_and_path_safe` | AC02/id: spelling folds, parse round-trips, path-like/`:`/blank keys refused, unknown namespace and over-long key refused, kind labels unique |
| `…values_are_known_with_provenance_or_explicitly_unknown` | known/unknown resolution, empty unknown reason refused, `verified_original` without a span refused |
| `…origin_distinguishes_synthetic_from_installation` | AC04 at the type level |
| `…element_states_are_independent_and_reasons_are_mandatory` | kind/id mismatch, unavailable without reason, ready with reasons, empty diagnostics |
| `catalog::…adding_unsupported_mission_increases_unsupported_count` | AC01 minimum scenario: count rises, full readiness falls, mission stays in the denominator |
| `…synthetic_launchable_row_is_not_a_retail_entry` | AC04 through the catalog counts and `is_retail_ready` |
| `…ids_and_order_are_stable_under_reordering` | AC02: three input enumerations give one canonical id order, equal catalogs |
| `…duplicate_identities_are_refused` | non-negotiable 5 |
| `…launchable_baseline_requires_a_launchable_element` | unknown id and non-launchable kind refused |
| `…catalog_refuses_an_invalid_element` | insertion validates the row |
| `cs_inspect::catalog::…fixture_keeps_unsupported_rows_and_counts_them` | fixture: unsupported rows kept, count one, not fully ready |
| `cs_inspect::catalog::…fixture_is_synthetic_and_never_retail` | AC04 through the fixture |

**12 tests**, all passing under `cargo test --workspace --locked --
accept_f14_a_ --include-ignored`. None needs `CS_GAME_DIR`, so CI runs them
too.

## Mutation probes (implementation removed → tests fail; all reverted)

| Probe | Edit | Result |
| --- | --- | --- |
| 1. Baseline accounting neutered | `unsupported_count` returns `0` | `…adding_unsupported_mission…` FAILED (exit 101) |
| 2. Key normalization removed | `from_source` stores the raw key | `…content_id_is_namespaced…` FAILED |
| 3. Duplicate refusal removed | `insert` never checks for an existing id | `…duplicate_identities_are_refused` FAILED |
| 4. Origin classification broken | `Origin::is_original` returns `true` always | `…origin_distinguishes…`, `…fixture_is_synthetic_and_never_retail` FAILED |

After restoring, `cargo fmt --all -- --check` is clean, the prefix run is
green and `grep -rn "MUTATION PROBE" crates/ tools/` prints nothing.

## Recorded unknowns (recorded, not guessed)

- **The original catalog collections and their semantic source keys are
  unmeasured.** `ContentKind` is engine-authored vocabulary; which original
  file backs which kind, and what a stable source key is for each
  collection (mission id, airframe name, …), arrive with the format/mission
  tasks and F14-D's retail baseline. No id table is invented here.
- **The dependency closure is not implemented in this stage.** `is_fully_ready`
  reflects each declared launchable row's own readiness, not the transitive
  closure of its dependencies; the closure walk, per-edge validation, cycle
  policy and the closure hash are F14-B, and the `closure` command is F14-C.
  A mission that is itself ready but references an unsupported texture is
  not yet detected as unavailable.
- **Which kinds are launchable** (campaign missions, IA scenarios,
  multiplayer scenarios) is a designed classification from the sheet's
  deliverable; whether the original exposes more or fewer launchable entry
  points is F14-D.
- **`is_retail_ready` is a stage-A guard**, not the campaign/release
  readiness criterion; the full denominator and the usable-unavailable
  classification are F14-D and the release tasks.
- The `catalog` and `closure` CLI commands are unimplemented; `main.rs`
  still refuses them, and F14-C/F14-B own them.

None of these is a new task: they are covered by the already-queued F14-B,
F14-C and F14-D, so `create_tasks` was not used.

## Commands run

All commands from the repository root on branch
`rally/53-define-stable-content-ids-and-provenance`, Rust 1.98.1.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f14_a_ --include-ignored` | 0 (12 tests) |

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_types/src/lib.rs`: `pub mod content;`.
- `crates/cs_content/src/lib.rs`: `pub mod catalog;` plus a doc paragraph.
- `tools/cs_inspect/src/lib.rs`: `pub mod catalog;` plus a doc paragraph.

No protected path, original datum or binary file is involved.

## Sources

`specs/F14-canonical-content-catalog-and-dependency-closure.md`,
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`, and
the F01-A, F02-A, F04-A and F12-A findings for precedent.

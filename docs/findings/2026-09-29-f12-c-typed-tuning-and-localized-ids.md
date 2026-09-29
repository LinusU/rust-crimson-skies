# F12-C: resolving typed tuning and localized ids through the catalog

Date: 2026-09-29. Task: F12-C "Resolve typed tuning and localized ids through
the catalog" (`specs/F12-text-configuration-strings-and-pe-resources.md`,
section `### F12-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Required capability: ordinary build/test. No original data was read for this
stage: it wires the F12-B readers into their consumers and is exercised by
newly authored synthetic fixtures only. Nothing from `$CS_GAME_DIR` is
committed.

## Files and the one observable failure (listed before editing)

F12-B implemented the producers — `cs_formats::read_pe_resources`, the
resource-id header reader, `ValueWidth`/`FieldSpec`/`Tuning` — and explicitly
deferred `tools/cs_inspect/src/config.rs` to this stage. F12-C is the
integration: build the consumers and make them reachable.

- `crates/cs_content/src/config.rs` (owner path): adds the **string catalog**
  (`StringRow`, `StringLookup`, `StringAccounting`, `StringCatalog`,
  `StringCatalogError`) over `cs_formats::PeResources`, and the **declared
  tuning** adapter (`FieldBinding`, `TuningOutcome`, `ResolvedTuning`,
  `TuningReport`, `resolve_tunings`) over `ConfigDocument`. Existing
  `ConfigDocument`/`TuningSchema` are unchanged in behaviour.
- `tools/cs_inspect/src/config.rs` (new, owner path): the `config` command —
  route a file by its observed member rule, read a PE image into the catalog
  or a keyed list into the document, and report the accounting plus every
  requested string lookup and tuning declaration.
- Wiring only: `tools/cs_inspect/src/lib.rs` (module declaration and doc
  paragraph) and `tools/cs_inspect/src/main.rs` (dispatch, help text). Neither
  contains logic.

**One observable failure:** a catalog that keeps the text but not the *id's
language* would answer a lookup for the wrong translation; a consumer that
silently picks the first of two strings sharing `(id, language)` would hide a
contradictory localized install; and a malformed PE resource offset handled by
loading the image would invoke a platform loader instead of a structured
refusal. Each is pinned by a test and by the mutation probe below.

## Design decisions

### The string catalog (`StringCatalog`)

- **The id is the identity, the language is part of it, the text is not.** A
  localized install ships the same `(id, language)` with different code units
  (spec AC04). `resolve(id, language)` answers `Found`, `Missing` or
  `Ambiguous(count)`; `language: None` means "any language" and is how two
  translations of one id are distinguished from one another.
- **Two strings for one `(id, language)` are ambiguous, never merged.**
  Matching `ConfigDocument::lookup` and the `.H` reader's duplicate-name rule,
  `resolve` returns `Ambiguous` rather than the first row, and
  `StringAccounting::duplicate_ids` counts the pairs.
- **Non-string leaves are retained and counted.** `other_leaves()` exposes
  every leaf the string reader does not own, and the accounting counts them.
  This is spec non-negotiable #5 ("unknown keys are retained and counted")
  applied to the resource tree: `strings.dll` carries `RT_VERSION` and an
  unassigned type beside `RT_STRING`, and a catalog that dropped them would
  hide two resources.
- **Undecodable units are recorded, not replaced.** A unit whose code units
  hold an unpaired surrogate keeps its exact `code_units` and has `text:
  None`; the accounting counts them.
- **Provenance is per block, at the block's byte extent.** Each `StringRow`
  carries a `SourceSpan` built from the block's checked `file_offset`/`size`,
  the catalog's container and the caller's installation hash. The spans are
  file offsets, not RVAs (the reader reports both; the span uses the mapped
  offset).
- **The catalog is built only from the bounded reader.** `StringCatalog::read`
  checks the byte length against the source, then hands the bytes to
  `read_pe_resources`. There is no dynamic-loading surface, so a hostile
  offset is `StringCatalogError::Pe` with the reader's own code
  (`outside_table`, `directory_cycle`, ...) — never a load (AC03).

### The declared tuning adapter (`resolve_tunings`)

- **A consumer declares the schema.** `FieldBinding` names who wants the value
  (`consumer`), which entry (`section`, `key`), which field (`index`) and the
  `FieldSpec` (width, signedness, approved range). A `Tuning` is produced only
  when `TuningSchema` accepts the value; every other case is an explicit
  `Missing`, `Ambiguous` or `Refused(code)`.
- **Consumption is booked.** Each found lookup goes through
  `ConfigDocument::lookup`, so the document's own `consumed`/`unconsumed`
  accounting reflects what the consumers actually used; an ambiguous or
  missing declaration consumes nothing. This is what lets F12-D gate parity on
  the unconsumed entries.

### The `config` command

- **Routing is by observed rule, never by extension** (spec non-negotiable
  #1). `dialect_for_member(container, member)` decides; `--container` and
  `--member` let a loose export be read as the surveyed member it came from
  (a member exported by `cs-inspect rof --export-dir`, for example), and a
  member no rule covers is refused.
- **Two shapes, one report.** `"kind": "pe_resources"` carries the layout, the
  accounting, the languages and every string row; `"kind": "keyed_list"`
  carries the entry accounting and the unconsumed entries. A requested
  `--string`/`--field` that does not resolve still produces a report and moves
  the exit code to 3: the file was read, and the report is the anomaly's
  record.
- **The loose-file provenance sentinel.** A loose `--file` belongs to no
  fingerprinted installation. Without `--install-sha256`, its `SourceSpan`
  carries `UNAFFILIATED_INSTALL_SHA256` (the 32 ASCII bytes of
  `cs-inspect:unaffiliated-install0`), a documented sentinel rather than an
  invented digest; the report names it, and a caller that knows the
  installation overrides it. A member read with `--member` also records the
  SHA-256 of the bytes actually read as its `member_sha256`.
- **Exit codes** follow `docs/contracts/CLI-EVIDENCE.md`: 0 read and every
  request resolved; 2 invalid input; 3 a refused route/bytes or an unresolved
  request; 1 a runtime failure. A refusal is never reported as success.

## Test inventory (`accept_f12_c_*`)

Eleven tests, all ordinary build/test, none ignored; all call production code.

| Test | Covers |
| --- | --- |
| `cs_content::config::…_string_catalog_resolves_ids_languages_and_provenance` | id → text, language, code page and provenance; an empty unit is present-and-empty; the language is part of the identity |
| `cs_content::config::…_string_catalog_retains_other_types_and_reports_duplicates` | a non-string leaf is retained and counted; two strings for one `(id, language)` are `Ambiguous` and counted |
| `cs_content::config::…_malformed_pe_resource_offsets_are_refused_not_loaded` | AC03: an unmapped data RVA, an out-of-table subdirectory and a self-referential directory are structured refusals; the uncorrupted shape reads |
| `cs_content::config::…_string_catalog_refuses_a_length_mismatch` | a truncated image is refused before parsing |
| `cs_content::config::…_no_platform_loader_in_the_string_path` | AC03's other half: the production sources reference no `LoadLibrary`/`GetProcAddress`/`dlopen`-class identifier |
| `cs_content::config::…_declared_tuning_fields_resolve_through_the_document` | known/negative/overflow/missing/ambiguous declarations; consumption accounting; the bytes stay in the document |
| `cs_inspect::config::…_config_reports_pe_strings_and_resolves_a_lookup` | the CLI reads a PE image through the bounded reader, reports the accounting and resolves id + language |
| `cs_inspect::config::…_config_refuses_a_malformed_pe_offset` | AC03 through the CLI: a cycle is exit 3 with no report, while the same shape with a sound offset reads |
| `cs_inspect::config::…_config_resolves_declared_tuning_fields` | a loose keyed list routed via `--container`/`--member`; known/negative/overflow/missing and the consumed/unconsumed counts |
| `cs_inspect::config::…_config_refuses_an_unrouted_member` | a member no observed rule covers is refused (an extension alone routes nothing) |
| `cs_inspect::config::…_config_reports_an_unresolved_lookup` | a request that does not resolve is reported with exit 3, not swallowed |
| `cs_content::config::…_string_catalog_counts_a_name_keyed_leaf_as_another_leaf` | a three-level `RT_STRING` leaf whose second level is a *name* is not a block; the reader keeps it plain, so the catalog counts it as another leaf rather than dropping it from the accounting (review fix) |
| `cs_inspect::config::…_config_refuses_a_request_of_the_wrong_shape` | a `--string` against a keyed list or a `--field` against a PE image is invalid input (2), never a silently ignored request that exits 0 (review fix) |

## Mutation probes

- **`StringCatalog::resolve` returns the first of two duplicates** (the
  `(Some(_), _many)` arm was made to return `Found`): the duplicate test fails
  (`accept_f12_c_string_catalog_retains_other_types_and_reports_duplicates`,
  assertion at the `Ambiguous(2)` check). Reverted; the suite is green again.
- The `no_platform_loader` scan reads `config.rs` and
  `cs_formats/src/pe_resources.rs` with `include_str!` and asserts both are
  non-empty before scanning, so the guard cannot pass by scanning nothing.
- Removing `StringCatalog`/`resolve_tunings` breaks the consumers that call
  them, so the tests cannot pass against a stub.

## Review corrections (2026-09-29, deepseek-1)

Independent review found and fixed two acceptance defects; no behaviour the
existing tests pin was weakened.

- **`is_string_leaf` did not match the reader's own string rule.** It checked
  only `path.len() == 3` and the outermost `RT_STRING`, but the F12-B reader's
  `string_leaf` requires **all three** levels to be ids. A three-level leaf
  under `RT_STRING` whose second or third level is a *name* is therefore not a
  block: the reader keeps it as a plain leaf, while the catalog classified it
  as a string leaf and excluded it from `other_leaves`. The leaf vanished from
  the accounting (`strings` and `other_leaves` both counted nothing for it),
  contradicting spec F12 non-negotiable #5 ("unknown keys are retained and
  counted"). The predicate now requires all three levels to be ids.
  `accept_f12_c_string_catalog_counts_a_name_keyed_leaf_as_another_leaf` pins
  it and fails under the old predicate (`other_leaves` 0 vs 1).
- **A request shape the member cannot answer was silently dropped.** `--string`
  addresses a PE image's catalog and `--field` a keyed list's document, but the
  command ignored the other shape's requests and still exited 0 with an empty
  `lookups`/`tunings` array — a lookup that never happened reported as success.
  The dispatch now refuses a `--string` against a keyed list, or a `--field`
  against a PE image, as invalid input (exit 2). The mirror test
  `accept_f12_c_config_refuses_a_request_of_the_wrong_shape` fails (exit 0 vs 2)
  when the guard is removed. The command's module doc records the rule.

Reviewer identity: `deepseek-1` (same model family as the F12-B reviewer);
context was fresh for this task (the F12-C implementation was read from the
branch, not carried over). This is an agent review and awards at most
`checked`, not `verified_original`.

## Recorded unknowns

- **Whether the shipped game reaches these strings through the Win32 resource
  API, through its own `RESOURCE.H` table, or through both is still unknown**
  (F12-B). This stage only makes the strings resolvable; it does not claim
  which path the original engine used.
- **A loose `--file` has no installation fingerprint.** The command records
  the sentinel above; it cannot and does not claim the bytes are part of a
  fingerprinted installation. Reading a member in place (through a mounted
  session) would carry the real fingerprint and is `resolve`/`rof` work, not
  this command's.
- **The `.H` resource-id header dialect has no F12-C consumer.** `--field`
  declarations and `--string` lookups address keyed lists and PE resources
  only; correlating a header's ids with a `RT_STRING` id (F12-B's open
  question) is filed as follow-up #368.
- **Fractional configuration values remain unobserved** (F12-B); follow-up
  #369.
- **`Reader` still has no absolute-offset window**; the PE reader does its own
  bounded random access. Follow-up #367.

No new tasks were filed: every unknown above is already filed (#367, #368,
#369) or is a documented scope boundary of this command.

## Commands

- `cargo fmt --all -- --check` — 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — 0.
- `cargo test --workspace --locked` — 0.
- `cargo test --workspace --locked -- accept_f12_c_ --include-ignored` — 13
  tests discovered, all passing (11 from the implementation plus the 2 review
  regressions below).
- Mutation probe above: the duplicate test fails under the mutation and passes
  after the revert.

## Sources

- `specs/F12-text-configuration-strings-and-pe-resources.md` (F12-C,
  acceptance tests AC02/AC03/AC04, non-negotiable behaviour 1, 2, 5).
- `docs/contracts/IDENTITY-CONTENT.md` (provenance, stable-id and numeric
  contracts).
- `docs/contracts/CLI-EVIDENCE.md` (exit codes, negative-test rule).
- `docs/findings/2026-09-29-f12-b-pe-resource-reader-and-typed-values.md`
  (the reader this stage consumes; the deferral of `tools/cs_inspect/src/config.rs`).
- `docs/findings/2026-09-28-f12-a-text-dialects-and-lossless-config-nodes.md`
  (the dialect inventory the routing uses).
- Public Win32 `RT_STRING`/resource-directory layout (`Documented`); the
  fixtures are newly authored from it.

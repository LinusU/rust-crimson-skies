# F50-A: complete mission binding and campaign coverage records

Date: 2026-09-29. Task: F50-A "Define complete mission binding/coverage
records" (`specs/F50-per-mission-compatibility-and-full-campaign-closure.md`,
section `### F50-A`, plus the owner ruling of 2026-09-28 in
`specs/README.md` and the `### F50-A` paragraph of the sheet). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`; identity rules:
`docs/contracts/IDENTITY-CONTENT.md`. Capability used: ordinary build/test
only — no `retail`, no `gpu`, no evidence report is produced, and
`$CS_GAME_DIR` was not read.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/campaign_bindings.rs` (new): `LabelError`,
  `validate_upper_identity`, `MissionLabel`, `SubsystemId`,
  `REQUIRED_SUBSYSTEMS`, `BindingCategory` (`ALL`, `label`, `from_label`,
  `required_roles`), `BindingTarget`, `BindingError`, `validate_role`,
  `BindingRow` (`new`, `content`, `evidence`, `unknown`, `role`, `target`,
  `is_known`, `content_target`, `evidence_target`), `CategoryState`
  (`rows`, `unresolved`, `unsupported`), `CellState`, `cell_state`,
  `DependencyState`, `SubsystemDependency`, `Progression`,
  `MissionBinding` (`unresolved`, `catalog_identity`,
  `category`, `cells`, `validate`, `is_placeholder`), `InventoryError`,
  `InventoryLineError`, `CampaignInventory` (`parse`, `load`, `len`,
  `is_empty`, `iter`, `labels`), `ClosureError`, `ClosureReport`,
  `CoverageReport`, `CampaignBindings` (`from_inventory`, `insert`,
  `declare`, `bind`, `get`, `missions`, `declared`, `coverage`,
  `closure`, `closures`) and the private traversal `CampaignBindings::visit`.
- `missions/bindings/campaign-inventory.tsv` (new): the declared
  denominator, 24 `label<TAB>title` lines plus a `#` header.
- `missions/bindings/README.md` (new): what this directory holds now and
  what M01-A … M24-A still add.
- `crates/cs_app/tests/campaign/` (new): `main.rs` plus `common.rs`,
  `inventory.rs`, `coverage.rs`, `closure.rs`, `identity.rs` — the
  `accept_f50_a_*` tests.
- Wiring only (AGENTS rule 1): `crates/cs_content/src/lib.rs` (module
  declaration and one doc paragraph), `crates/cs_app/Cargo.toml`
  (`[dev-dependencies] cs_content`), root `Cargo.lock` (the new
  `cs_app -> cs_content` edge).

**One observable failure:** with `cell_state` reporting a category the
binding never recorded as `Complete` instead of `Missing`, the cell total
no longer accounts for the absent child: `accept_f50_a_a_missing_child_stays_in_the_totals_and_blocks_ready`
fails on `missing_cells == 1` and on the campaign staying unready, and
`accept_f50_a_cycle_duplicate_and_dangling_identities_are_reported` fails on
its missing `actors` cell. That is the failure the whole stage exists to
prevent — a child that disappears rather than staying red.

## What the records hold

`MissionBinding` is engine-independent data: no subsystem's concrete type
appears, so the record keeps its shape while F18 … F47 are unwritten.

| Piece | Rule | Where it comes from |
| --- | --- | --- |
| `MissionLabel` | uppercase `[A-Z0-9._-]`, max 32; a work-order discovery label, never a retail identity | owner ruling: "discovery labels M01-M24 are not automatically verified retail identities" |
| `BindingCategory` (closed, 7) | `mission_identity`, `actors`, `assets_media`, `objectives`, `interactions`, `rewards_progression`, `reference_evidence` | owner ruling's category list, kept verbatim |
| `BindingRow.role` (open) | `[A-Za-z0-9._:-]`, max 64; free vocabulary, because which roles original data needs is discovered later | `IDENTITY-CONTENT`: kinds/labels are engine vocabulary |
| `mission_identity` required roles | `mission`, `world`, `program`, each with the matching `ContentKind` (`Mission`, `World`, `Script`) | owner ruling names all three in one category |
| `BindingRow.target` | `Resolved<BindingTarget>` = content id or evidence claim, or an explicit unknown with claim id + reason | `IDENTITY-CONTENT` `Resolved<T>` |
| `SubsystemDependency` | one row per entry of `REQUIRED_SUBSYSTEMS` (the 23 F50 prerequisite stages), state `Resolved`/`Unresolved`/`Unsupported`; an empty or partial list is refused | owner ruling: "a stable identity and an explicit unresolved dependency row" |
| `Progression` | `Known { next }` or `Unknown { reason }`; empty is not a stand-in for unknown | `IDENTITY-CONTENT` closure rule |
| `CampaignInventory` | strict `label<TAB>title` reader; comments/blank lines skipped, one trailing `\r` tolerated; wrong field count, blank title, bad or repeated label fails the whole file; an inventory with no mission is refused | F50 non-negotiable 5, "the denominator cannot shrink" |

### Coverage and closure

`CampaignBindings::coverage()` counts every `(recorded mission, required
category)` cell exactly once — `complete + unknown + missing + unsupported
== cells == total_missions × 7` — plus every row, every subsystem row and
every progression. `is_ready()` requires all cells complete, all subsystem
rows resolved and all progressions recorded; an empty campaign is vacuously
ready, exactly like `Catalog::is_fully_ready`. Missions recorded beyond the
declared denominator are counted too and reported as `discovered_extra`;
`declare()` is the only way to add to the denominator and no call removes
one.

`closure(root, Option<&Catalog>)` walks the recorded progression graph,
counts the cells, rows and subsystem rows of everything it reaches, and
refuses to guess: `Cycle` (with the offending chain), `UnknownMission`,
`DanglingProgression` and `DanglingContent` (only when a catalog is
supplied) are returned as errors. `closures()` runs one closure per
recorded mission in canonical order — a shared visited set cannot shrink a
report. `insert()` refuses duplicate labels, missing/unknown/duplicated
subsystem rows, wrong-kind identity rows and empty categories; `bind()`
replaces a placeholder exactly once and reports `AlreadyBound` afterwards.

`is_ready()` also requires a frozen denominator: no declared missions, or a
recorded mission still outside the declared denominator, is never ready. See
the review record below for why.

### The declared denominator

`missions/bindings/campaign-inventory.tsv` declares the 24 work orders with
the titles from `missions/README.md`. The inventory is data rather than a
Rust constant so a discovered mission can be added without a code change,
and `accept_f50_a_denominator_matches_the_owner_work_orders` asserts the
file still equals the owner-authored (protected) work-order list, so it
cannot shrink without a failing test. Building it yields, at this stage:
24 declared missions, 168 cells, 0 complete, 168 unknown, 552 subsystem rows
all unresolved, 24 unknown progressions, 0 discovered extras, not ready.

## Format decision: a two-column TSV, not JSON

The workspace has no JSON reader — `serde` appears nowhere in any
`Cargo.toml`, and the only JSON the Rust code produces is hand-written
emission in `cs_inspect`/`cs_app`. `schemas/mission-binding.schema.json` is
a *contract* validated by Python tooling, not something this crate can
parse. Adding a JSON dependency (or hand-writing a JSON parser) to define
one flat denominator list would be a second, larger slice than this stage,
so the inventory is the smallest strict line format that a ~90-line reader
can reject loudly. The per-mission `missions/bindings/M01.json` … files
named by the work-order sheets stay on `schemas/mission-binding.schema.json`
and are M01-A … M24-A's output; note that the protected schema has
`additionalProperties: false`, so the complete record of this stage (extra
categories, subsystem rows, progression) deliberately does **not** fit in
it and is not stored there.

## Test inventory (23 tests, prefix `accept_f50_a_`)

Eighteen tests were added by the implementing agent and five by the review
round (marked **review**); the review record is below.

| Test | What it pins |
| --- | --- |
| `inventory::accept_f50_a_denominator_matches_the_owner_work_orders` | the declared inventory equals `missions/README.md` (24 labels and titles) |
| `inventory::accept_f50_a_a_malformed_inventory_never_drops_a_mission_silently` | wrong field count/blank title/bad label/duplicate/empty file/unreadable file all fail with the line number |
| `coverage::accept_f50_a_the_declared_campaign_starts_unresolved_and_unready` | the shipped campaign: 168 unknown cells, 552 unresolved subsystem rows, nothing ready, every identity still a discovery label |
| `coverage::accept_f50_a_a_missing_child_stays_in_the_totals_and_blocks_ready` | removing a category changes no total and blocks readiness |
| `coverage::accept_f50_a_an_unknown_child_stays_in_the_totals_and_blocks_ready` | an unknown row keeps its cell, its row count and blocks readiness |
| `coverage::accept_f50_a_an_unresolved_subsystem_row_blocks_ready` | one open subsystem row blocks readiness with every category complete |
| `coverage::accept_f50_a_an_unsupported_category_stays_in_the_totals_and_blocks_ready` | unsupported is counted, never treated as unused |
| `coverage::accept_f50_a_discovered_content_stays_visible` | a mission beyond the denominator grows the cell total, blocks readiness and only an explicit `declare` joins the baseline |
| `coverage::accept_f50_a_a_fully_bound_campaign_is_ready` | the ready path exists at all (3 missions × 7 cells, 39 rows) |
| `coverage::accept_f50_a_identity_rows_carry_the_kind_their_role_means` | `mission`/`world`/`program` rows carry `Mission`/`World`/`Script` |
| `coverage::accept_f50_a_forced_airframes_are_a_first_class_row` | forced airframes are a row of the closed `actors` category |
| `coverage::accept_f50_a_category_labels_round_trip` | every required category's stable label maps back to it, and an unknown label maps to nothing |
| `coverage::accept_f50_a_reference_evidence_rows_carry_evidence_claims` | a reference/evidence row resolves to an evidence claim, never to a content id |
| `coverage::accept_f50_a_readiness_requires_a_frozen_denominator` | **review** readiness needs a non-empty declared denominator covering every recorded mission |
| `closure::accept_f50_a_every_declared_mission_closure_omits_nothing` | AC01's minimum scenario: one closure per declared mission, roots and reach are the full 24, every closure accounts for every required category and all 23 subsystem rows |
| `closure::accept_f50_a_a_bound_chain_is_traversed_in_full` | a bound `M01 → M02 → M03` chain is walked and counted; a tail closure counts only itself |
| `closure::accept_f50_a_cycle_duplicate_and_dangling_identities_are_reported` | duplicate insert, unknown root, cycle chain, dangling successor, dangling content identity, missing category and blank reason |
| `closure::accept_f50_a_required_subsystems_match_the_sheet` | **review** `REQUIRED_SUBSYSTEMS` equals the F50 sheet's 23 prerequisite features, read from the sheet |
| `closure::accept_f50_a_every_mission_carries_a_row_for_every_required_subsystem` | **review** every declared mission carries exactly the required subsystem rows, each an explicit unresolved row |
| `identity::accept_f50_a_a_discovery_label_never_stands_in_for_a_retail_identity` | placeholder → `bind()` once → `AlreadyBound`; the catalog identity only appears once bound; lowercase label refused |
| `identity::accept_f50_a_invalid_records_are_refused_on_admission` | wrong identity kind, missing subsystem, unknown subsystem, duplicated subsystem, empty rows, empty category, blank reasons |
| `identity::accept_f50_a_a_second_identity_row_is_checked_for_its_kind` | **review** a wrong-kind identity row is refused wherever it sits, not only when it is the first row of its role |
| `identity::accept_f50_a_every_explicit_unknown_and_identity_grammar_is_refused` | **review** `declare`/`bind` of an unrecorded label, the remaining blank-reason constructors, row-role grammar and mission/subsystem identity grammar |

Mutation probes (implementation removed, test must fail, then reverted):

| Mutation | Result |
| --- | --- |
| `cell_state` reports an absent category as `Complete` | 2 tests fail (`a_missing_child…`, `cycle_duplicate_and_dangling…`) |
| `closures()` returns only the first root | `every_declared_mission_closure_omits_nothing` fails |
| `CampaignInventory::parse` skips malformed lines instead of failing | `a_malformed_inventory_never_drops_a_mission_silently` fails |
| `MissionBinding::validate` inspects only the first row per identity role (review) | `a_second_identity_row_is_checked_for_its_kind` fails |
| `CoverageReport::is_ready` drops the frozen-denominator guard (review) | `readiness_requires_a_frozen_denominator` fails |
| one entry removed from `REQUIRED_SUBSYSTEMS` (review) | `required_subsystems_match_the_sheet` fails |
| `BindingRow::new` maps a bad role back to `BindingError::Label` (review) | `every_explicit_unknown_and_identity_grammar_is_refused` fails |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 (93 test binaries/doctest groups green) |
| `cargo test --workspace --locked -- accept_f50_a_ --include-ignored` | 0 (23 tests after the review round, all passing; 18 before it) |

No `#[ignore]` test belongs to this stage: nothing here needs `CS_GAME_DIR`,
and CI (which has no original data) runs these tests unchanged.

## What this stage does not do

- It does not bind anything. Every identity, world, program, actor,
  objective and reward is still `Resolved::Unknown`; nothing here reads the
  installation, normalizes original fields or claims gameplay success.
- It does not create `missions/bindings/M01.json` … `M24.json`. Those are
  the per-mission binding outputs of M01-A … M24-A against
  `schemas/mission-binding.schema.json`; `missions/bindings/README.md`
  says so.
- It does not traverse the *content* dependency closure, hash a closure or
  validate parsers/handlers/consumers — that is F14-B/C/D, and this module
  only checks content ids against a catalog when a caller passes one.
- It does not integrate a production consumer of these records and does not
  run a campaign. Production normalization/consumer integration and the
  full campaign runs remain F50-B/C/D with unchanged acceptance, and no
  `verified_original` or `release_approved` claim is made or implied.

## Open points for later stages

- Which row roles the original program actually needs (beyond the three
  identity roles) is discovery work for M01-A … M24-A; the vocabulary is
  open and needs no change here.
- Whether the campaign progression is a chain, a graph or per-difficulty is
  unmeasured: `Progression::Unknown` records that honestly today.
- Whether one mission may legitimately record **two** rows for the same
  identity role (two world variants, an intro program plus the mission
  program) is unmeasured. `MissionBinding::validate` now checks the kind of
  *every* row that claims an identity role, so a second contradictory row is
  refused, but multiplicity itself is deliberately not restricted: a second
  well-formed row of the right kind is allowed until original data says
  otherwise. M01-A … M24-A settle this; do not add a singleton rule before
  then.
- If the owner later wants the complete record to live in JSON next to the
  per-mission files, `schemas/mission-binding.schema.json` (protected) would
  have to grow with it; that is an owner decision, recorded here rather
  than worked around.

## Review record (bunny-1, 2026-09-29)

Reviewer: **bunny-1**, a different agent instance from the implementer
(**opencode-1**) and working from a fresh context: the implementer's
submission summary and this findings document were read, but the code was
judged against the F50 sheet, the 2026-09-28 owner ruling,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/IDENTITY-CONTENT.md`
and the owner-authored `missions/README.md`, not against the implementer's
description of it. Not an independent *original-reference* observation: this
stage reads no original data and no agent review replaces owner approval.

Reviewed and accepted as sound: the closed seven-category set and its
mapping onto the ruling's list; `Resolved<BindingTarget>` rows with explicit
unknowns; the strict inventory reader; the frozen-denominator
`CampaignBindings` API with no removal path; the closure traversal, where
the cycle check runs *before* the visited check so an already-explored node
can never hide a cycle reachable from it; and the honest "what this does not
do" framing. The TSV-instead-of-JSON decision is sound and is recorded as
an owner decision rather than worked around.

Four defects were found and fixed on this branch.

### 1. `validate` checked only the first row of each identity role

`MissionBinding::validate` looked up each identity role with
`rows.iter().find(...)`, so only the *first* `world`, `mission` or `program`
row had its [`ContentKind`] verified. A binding carrying a well-formed
`world` row followed by a second `world` row pointing at a `Mission` was
admitted, and the cell then read `Complete` because both rows were known.
The module documents "An identity row points at a catalog element of the
wrong kind" as a refused admission rule, so this was a hole in a stated
rule, not a design choice. The loop now walks every row and checks the kind
of each one that claims an identity role. Multiplicity is left open on
purpose (see the open points above).

Observed failure before the fix:
`identity::accept_f50_a_a_second_identity_row_is_checked_for_its_kind` failed
with `a second world row of the wrong kind is refused: ()`.

### 2. `BindingError::InvalidRole` was a dead variant

`BindingRow::new` validated the role with `validate_role(role)?`, which goes
through `From<LabelError>`, so every bad role produced
`BindingError::Label(LabelError::…)` — indistinguishable from a bad mission
label — while four doc comments promised `BindingError::InvalidRole` and no
code path constructed it. The variant existed precisely to distinguish a bad
row role, and the stage's other error variants all follow the same
"name the specific failure" convention. `BindingRow::new` now maps the role
error to `InvalidRole`; the docs are true and the variant is reachable.

Observed failure before the fix: the new identity-grammar test failed with
`role "" must be refused`.

### 3. `is_ready` ignored the frozen denominator

`CoverageReport::is_ready` compared only cell and subsystem counts, and its
doc justified an empty campaign as "vacuously ready, exactly like
`Catalog::is_fully_ready`". That is the "filtering to the working subset"
failure spec F50 non-negotiable behavior 5 forbids: a campaign built
without a denominator, or one whose recorded set no longer matches its
declared set, reported itself complete. The owner ruling also puts the
denominator first ("Aggregate completion fixes the denominator before
testing"), so the denominator is part of the question rather than a
precondition of it. `is_ready` now additionally requires a non-empty
declared denominator that covers every recorded mission, and the
`Catalog::is_fully_ready` precedent was dropped as a misleading analogy.
`discovered_content_stays_visible` still holds: a discovered mission keeps
the campaign unready until `declare` freezes it into the baseline.

Observed failure before the fix:
`coverage::accept_f50_a_readiness_requires_a_frozen_denominator` failed.

### 4. Nothing pinned `REQUIRED_SUBSYSTEMS` to the sheet

Every subsystem assertion in the implementer's tests compared a count
against `REQUIRED_SUBSYSTEMS.len()` on *both* sides, so the set could
shrink silently: deleting `"F38"` from the constant would have made every
mission report one dependency row fewer and every test still pass — a
dropped prerequisite feature becoming a silently lighter mission. The
implementer already established the right pattern for the denominator
(comparing the committed inventory against the protected
`missions/README.md`); the same guard is now applied to the prerequisite
list, read from `specs/F50-...md` and compared by identity, with the sheet's
23 features counted explicitly.

Observed failure before the fix (mutation applied, then reverted):
`closure::accept_f50_a_required_subsystems_match_the_sheet` failed.

### Also corrected

- The `# Errors` sections of `CampaignBindings::insert` and `bind` listed
  `DuplicateId`/`UnknownMission` before `MissionBinding::validate`, while
  the code validates first. The doc now states the real order, so a record
  that is both invalid and a duplicate is documented as reported as the
  invalid record it is.

### Review checks (bunny-1, 2026-09-29)

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `RUSTDOCFLAGS="-D warnings" cargo doc -p cs_content --no-deps --all-features` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f50_a_ --include-ignored` | 0 (23 tests, all passing) |

No protected path is touched: `git diff --name-only origin/main...HEAD`
lists only `missions/bindings/`,
`crates/cs_content/src/campaign_bindings.rs`, `crates/cs_content/src/lib.rs`
(wiring), `crates/cs_app/Cargo.toml` and `Cargo.lock` (wiring),
`crates/cs_app/tests/campaign/` and this file. The new tests read two
protected files (`missions/README.md`, already read by the implementer, and
`specs/F50-...md`) read-only, to assert agreement.


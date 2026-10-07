# F64-C: import validation and the user-facing migration report

Date: 2026-10-07. Task: F64-C "Build import validation and user-facing
migration report" (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`,
section `### F64-C`). Shared contract: `docs/contracts/STATE-TRANSACTIONS.md`.
Evidence report: `docs/findings/evidence/F64-C.json` (validated with
`tools/validate_evidence.py --require-pass`).

Owner paths used: `crates/cs_app/src/ui/import.rs` (new), `tests/`
(`crates/cs_app/tests/{f64_c_support,accept_f64_c_import_report,evidence_report_f64_c}`),
`docs/findings/`. **Wiring edit:** `crates/cs_app/src/lib.rs` gained the
module declaration `pub mod import;` inside the existing `pub mod ui { .. }`
block and one doc paragraph naming `[ui::import]` — no logic was added there.
`crates/cs_formats/src/legacy_profile/` and `crates/cs_content/src/legacy_import.rs`
needed no change: F64-B's producer already exposed everything the consumer
reads (`plan_import`, `assess_imported_blueprints`, `ImportRefusal::code`,
`UnresolvedReason::code`, the inventory rows).

Capabilities used: **`retail`** (read-only reads of `$CS_GAME_DIR` in the
retail acceptance test and in the evidence harness) plus ordinary build/test.
The original executable was never run; nothing here is a claim about original
runtime behaviour.

## The boundary this stage adds

`crates/cs_app/src/ui/import.rs` is the **consumer** F64-B deliberately left
open ("the import UI file of the task's owner paths is not wired"). It is the
value-driven boundary a screen drives; it holds no file handle, no writer and
no host path, so it is structurally incapable of touching the source or a
save:

- **`ImportContext` / `ImportOffer`** — the inputs, all by shared reference:
  the declared id table, catalog, stock rules/policy/price book, admission
  policy and the `cs.profile.legacy_save_import` switch (context); the
  candidate source with its declared fingerprint, the bytes, the layout the
  screen found for the class (`Option`), the optional `BlueprintFieldMap` and
  the **new** `TargetProfile` (offer).
- **`ImportFlow`** — `offer` runs the production pipeline
  (`plan_import` → `read_legacy_profile` → `assess_imported_blueprints`) and
  stores the outcome; `dismiss` is the teardown; `retry` is that teardown
  followed by a fresh run of the whole pipeline; `confirm` is the explicit
  owner action of non-negotiable 4. The attempt counter is monotone and
  survives teardown, so a log line still names the attempt it came from.
- **`MigrationView` / `ReportLine`** — the user-facing migration report: a
  verdict of full / partial / unsupported / refused, the retained verified
  `SourceFingerprint`, one coded line per source, class, inventory-row
  evidence, layout, version, record (with its resolved identities), unresolved
  row, blueprint verdict, limit breach, constraint and notice. Every line has
  a stable machine `code()` plus a `Display` text, and the typed variants
  carry `LimitBreach` and `ConstraintViolation` **as values**, so a rejection
  shows the exact `limit`/`total` pair rather than a reformatted sentence.
- **`RefusedView` / `FlowRefusal`** — every refusal is propagated whole with
  the producer's own code (`source_too_large`, `layout_evidence`,
  `enhancement_disabled`, `unreadable`, `map_invalid`, ...) plus a
  `nothing_written` notice, and one new condition this stage must be able to
  state honestly: `NoMeasuredLayout { class, inventory_evidence }`, raised
  **before any byte is judged** when the screen has no layout for the class —
  which is the real state of every class today, since
  `LEGACY_LAYOUT_INVENTORY` is `Unknown` on all five rows.
- **`ConfirmedImport`** — the outcome transaction: target profile, retained
  migration report, blueprint verdicts, fixture marker and
  `importable_records()`, the intersection of *planned* and *stock-conforming*
  record indices. A rejected or refused blueprint is never in that list.
  `confirm()` refuses `NothingToConfirm`, `Refused`, `Unsupported` (the plan
  carries nothing — never a blank profile called imported) and
  `NoConformingRecord` (nothing the stock rules accept).

## What the tests pin (`accept_f64_c_*`, 11 tests)

| test | sheet criterion / behaviour |
| --- | --- |
| `..._reordered_catalog_never_remaps_an_imported_weapon` | **AC03, the stage's minimum scenario** |
| `..._hostile_or_oversized_offer_touches_neither_source_nor_destination` | **AC01** |
| `..._rejected_blueprint_is_reported_with_breach_fields_and_cannot_be_confirmed` | **AC02** at the consumer |
| `..._optional_save_import_can_be_disabled_while_new_profiles_still_work` | **AC04** |
| `..._a_refused_attempt_is_torn_down_and_the_retry_runs_clean` | teardown/retry |
| `..._confirm_retains_the_fingerprint_and_the_importable_records` | explicit owner action + retention |
| `..._a_partial_import_names_what_it_could_not_carry` | non-negotiable 5 (partial ≠ full) |
| `..._a_document_resolving_nothing_is_never_confirmed_as_a_blank_profile` | non-negotiable 5 (unsupported) |
| `..._a_field_map_that_cannot_describe_the_layout_propagates_its_code` | blueprint-stage error propagation |
| `..._production_admission_refuses_the_fixture_layout_by_code` | measured-only default + `NoMeasuredLayout` |
| `..._retail_every_shipped_file_is_refused_by_name_and_nothing_is_written` | `retail`, `#[ignore]`, run with `--include-ignored` |

### How AC03 is made discriminating

A `Catalog` stores rows in a `BTreeMap` keyed by `ContentId`, so *insertion*
order can never change enumeration — asserting "same catalog filled twice in
two orders" would be vacuous. The catalog's own contract is that rows
enumerate in canonical id order, which means what a reordering of entries does
to a positional consumer is move every identity to a different **position**.
The test therefore runs the same document twice and proves the positions
really differ first:

- catalog *Base* holds the four bound components; catalog *Shifted* adds an
  unrelated airframe (`fixture.synthetic_alpha`) and an unrelated weapon
  (`fixture.synthetic_autocannon`) in front of them, so `position_of(gun)`,
  `position_of(missile)`, `position_of(airframe)` and `position_of(engine)`
  all differ between the two runs — asserted with `assert_ne!` before the
  report comparison;
- the second run also declares the id table's five bindings in the opposite
  order (`id_map_reordered()`), covering the other place a positional reader
  could hide;
- then both rendered reports must be `==` line for line **and** the record's
  identities must equal the declared ones: legacy weapon id 1 → the gun,
  ordnance id 5 → the missile, in both runs, with zero unresolved rows.

A positional implementation fails this in both runs (position 1 holds the
engine in *Base* and the airframe in *Shifted*, neither of which is the gun).

### The retail test (capability `retail`)

`accept_f64_c_retail_...` discovers the installation through the production
`cs_assets::install::discover`, offers **every** inventoried file whose size
fits the 4 MiB designed import cap to `ImportFlow` under the production
default (`LayoutAdmission::MeasuredOnly`, no layout, because none exists), and
asserts each one is refused with `no_measured_layout` naming
`inventory_evidence: Unknown`, that its bytes re-read identically afterwards,
that files over the cap are unproposable, that a probe destination directory
is byte-for-byte unchanged, and that one file offered *with* a layout in hand
is refused with `layout_evidence`. It passed locally with `CS_GAME_DIR` set.

## Deliberately not done

- **No profile-store write.** `crates/cs_app/src/profile.rs` is not in this
  task's owner paths, so a `ConfirmedImport` is a staged outcome value the
  persistence layer will consume, not a persisted profile. Recording this as
  a follow-up (see below) rather than reaching into another task's files.
- **No front-end screen.** `crates/cs_app/src/ui/front_end/` is not an owner
  path either; the module is the boundary the screen will draw, exactly as
  `ui::instant_action` is the boundary the IA screens drive.
- **No save-format or byte-layout claims.** Nothing original was read beyond
  the installation's own inventory: the byte layout, version field and id
  encoding of a stored plane or save remain `Unknown` in
  `LEGACY_LAYOUT_INVENTORY`, and the retail test proves the consumer refuses
  everything today rather than pretending otherwise.
- **No change to `plan_import`.** A plan still reports only the first reason
  of an `Unsupported` document (the F64-A shape), so the view renders what the
  producer retains; extending the producer's report is a separate slice.

## Follow-ups filed (Rally `create_tasks`)

- **#756** — persist a `ConfirmedImport` into a new profile atomically
  (`STATE-TRANSACTIONS`), source fingerprint and migration report retained.
  Needs a task whose owner paths include `crates/cs_app/src/profile.rs`.
- **#757** — draw `ui::import`'s report lines in the front-end import dialog
  (F45/F51 screen wiring).
- The stored-plane/save byte layouts, and with them a real offer of original
  data, still need an original run or an owner-captured file (F64-D and its
  capture follow-up).

# F64-A: legacy-import inventory and validation contracts — design notes and what stays unknown

Date: 2026-10-01. Task: F64-A "Define legacy-import inventory and validation
contracts" (`specs/F64-legacy-custom-aircraft-and-optional-save-import.md`,
section `### F64-A`). Shared contract:
`docs/contracts/STATE-TRANSACTIONS.md`.

Capabilities used: **ordinary build/test only**. `$CS_GAME_DIR` was not read,
no original file was opened, no GPU/audio/human capability was used and no
evidence report is required or produced at this stage. Every byte in the tests
is newly authored synthetic fixture content written under the system temporary
directory.

**No claim of original verification.** Nothing in this task is
`verified_original`, and the design explicitly refuses to import through
anything but measured evidence (`LayoutAdmission::MeasuredOnly` is the
default). The single shipped layout, `cs_formats::legacy_profile::
synthetic_layout`, is `ClaimStatus::Designed` fixture data whose magic, field
names, widths and version were chosen to exercise the reader.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/legacy_profile/inventory.rs` (new):
  `MAX_LEGACY_SOURCE_BYTES`, `LegacyArtifactClass`,
  `ImportRequirement`, `LegacyLayoutRecord`, `LEGACY_LAYOUT_INVENTORY`,
  `layout_record`, `ArtifactProposal`, `ArtifactProposalError`.
- `crates/cs_formats/src/legacy_profile/document.rs` (new): `LegacyIdClass`,
  `LegacySlotType`, `LegacySlot`, `LegacyIdSlot`, `TrailingPolicy`,
  `LegacyLayout` (+ `with_record_size`), `LegacyLayoutError`, `LegacyLimits`,
  `LegacyValue`, `LegacyField`, `LegacyRecord`, `LegacyProfileDocument`,
  `LegacyProfileErrorKind`, `LegacyProfileError`, `check_slot_widths`,
  `read_legacy_profile`, `synthetic_layout`, `LEGACY_MAGIC_BYTES`.
- `crates/cs_formats/src/legacy_profile/mod.rs` (new): module doc and
  re-exports.
- `crates/cs_content/src/legacy_import.rs` (new): `LayoutAdmission`,
  `SourceFingerprint`, `TargetProfile`, `LegacyIdBinding`, `LegacyIdMap`,
  `LegacyIdMapError`, `UnresolvedReason`, `UnresolvedRow`, `ImportClass`,
  `ImportRefusal`, `ImportedRecord`, `MigrationReport`, `ImportRequest`,
  `ImportPlan`, `plan_import`, `refusal_is_read_failure`.
- Wiring only: `pub mod` lines, re-export lists and one module-doc paragraph in
  `crates/cs_formats/src/lib.rs` and `crates/cs_content/src/lib.rs`, plus a
  `[dev-dependencies] cs_assets` entry in `crates/cs_content/Cargo.toml` (the
  AC01 test compares the retained fingerprint against the production
  `cs_assets::install::sha256`, so it asserts through the real hashing path
  instead of a copy of it). No logic in either `lib.rs`; no `Cargo.lock` change.
- `crates/cs_formats/tests/accept_f64_a_legacy_profile_contracts.rs` (new): 12
  `accept_f64_a_*` tests.
- `crates/cs_content/tests/accept_f64_a_legacy_import_plan.rs` (new): 9
  `accept_f64_a_*` tests, including the AC01 filesystem scenario.
- This file.

**One observable failure:** a reader that trusts the document's own record
count. A legacy profile that claims 4 294 967 295 records is refused here, as
`too_many_records`, *before* a `Vec` is reserved; the same claim lowered inside
`max_records` but past the end of the file is refused as
`record_table_out_of_range`; and a plan offered an oversized payload is refused
as `source_too_large` with the source bytes, the source's modification time and
the entire destination profile directory bit-identical afterwards. Removing the
count cap, the table-extent check or the size gate makes
`accept_f64_a_hostile_record_count_is_refused_before_any_table_is_reserved` and
`accept_f64_a_malicious_or_oversized_old_profile_fails_without_touching_source_or_new_saves`
fail; that was verified by mutating each check in turn.

## Decisions

- **The layout is data, the reader is generic.** `LegacyLayout` carries the
  magic, the header slots, the record-count field, the record slots, the id
  slots, the record stride, the supported version major and the layout's
  evidence state. Nothing about a layout is compiled in, so F64-B's measurement
  is an added value rather than a rewrite, and a layout that could not describe
  a document is refused by `LegacyLayout::validate` before any byte is read.
- **Every extent is declared, never read from the input.** The input therefore
  controls exactly one number — the record count — which is checked against
  `LegacyLimits::max_records` *and* booked against an F03 `AllocationBudget`
  *and* checked against the document length with overflow-checked arithmetic
  before the first `Vec::with_capacity`. Field widths are checked against the
  limits before the first read, so a layout declaring a 1 MiB text field is
  refused without interpreting anything.
- **Unknown is retained, not dropped and not interpreted.** Record bytes past
  the last declared slot are kept in `LegacyRecord::undeclared` (which is why
  `LegacyLayout::with_record_size` exists: a measured stride may be wider than
  its accounted fields), bytes past the table are kept in
  `LegacyProfileDocument::trailing`, and a `Text` slot that is not valid UTF-8
  is a refusal rather than a lossy conversion. The reader attaches no meaning
  to any value; the content layer is the only place a value can become an
  aircraft, a weapon or a mission.
- **Classification is derived, never asserted.** A record is imported only when
  every declared id slot resolved and it has no undeclared bytes; anything else
  becomes a named `UnresolvedRow`. `ImportClass` is `Full`, `Partial` (with its
  resolved indices and its unresolved rows) or `Unsupported` (with its first
  reason). A document with zero records is refused outright as
  `not_importable { no_records }`, so an empty legacy profile can never be
  reported as a successful import.
- **Ids resolve through identities.** `LegacyIdMap` is keyed by
  `(LegacyIdClass, raw)` and holds a `ContentId`; `resolve` looks that identity
  up in the `Catalog` and requires it to be **ready**. There is no index
  arithmetic and no positional constructor anywhere in the module, and a
  binding whose target is in the wrong namespace is refused rather than coerced.
  `LegacyIdClass::Ordnance` shares the `weapon` namespace on purpose, because
  F44's ordnance fitment is a `ContentKind::Weapon` id — the *declared
  binding* is what separates a rocket from a gun, not its namespace.
- **The plan is plan-only.** `plan_import` is a pure function over borrowed
  bytes; `ImportRequest` has no mutable handle, no writer and no host path, and
  `ImportPlan` has no method that writes. F64-B owns the verified read-only
  subset and F64-C owns the transaction, and both consume this plan.
- **The optional save import is a typed switch, not a UI flag.**
  `ImportRequirement::OptionalEnhancement` carries the label
  `legacy-save-import` and the switch `cs.profile.legacy_save_import`; with the
  switch off a save source is refused as `enhancement_disabled` and no plan
  exists, while a custom-aircraft source is still planned. AC04's "new
  campaigns still work" is F64-D's to demonstrate; this stage only fixes the
  switch's name and its refusal.

## Open / not claimed (resolving stages)

- **Every legacy layout is unmeasured.** No original profile, save,
  custom-aircraft or settings file was opened: the inventory rows' evidence is
  `Unknown` and the byte layout, version field, id encoding, storage location
  and even the *existence* of a custom-aircraft reference are listed row by row
  in `LEGACY_LAYOUT_INVENTORY[..].unknowns`. Because no referencing path has
  been measured, custom-aircraft import is *not required yet* — and the
  requirement is a rule that a measurement switches on, not an assumption.
  Resolving: F64-B (retail measurement), F64-D (original evidence).
- **AC02** (a legacy blueprint violating current stock constraints is rejected
  with specific fields) is F64-B's minimum scenario and is **not** implemented
  here. This stage does not even model a staged blueprint: F44-B's
  constraint rules do not exist yet, so a validator here would be a second,
  divergent implementation. The seam F64-B needs is `ImportedRecord::
  resolved_ids` plus F44's `ConstructionRules::assess`.
- **AC03** (reordering catalog entries does not remap an imported weapon) is
  F64-C's end-to-end minimum scenario. What F64-A owns is the *type* that makes
  it hold — `LegacyIdMap`, with no positional API — and
  `accept_f64_a_ids_resolve_by_identity_so_catalog_order_changes_nothing`
  pins that the mapping is independent of catalog order. That is a contract
  test, not an AC03 verdict.
- **AC04** (optional old-save migration can be disabled while new campaigns
  still work) needs the settings catalog and the profile transaction to exist as
  a running system; F64-D.
- **The 4 MiB source cap and the `max_records`/`max_text_bytes` limits are
  designed values, not measured ones.** They are the tested configuration
  surface; a measured original file that exceeded one would raise the cap with
  the measurement recorded.
- **No user-facing report, no UI, no transaction.** `crates/cs_app/src/ui/
  import.rs` (an owner path) is deliberately not created: F64-C is the stage
  that wires the plan into a producer and consumer with a migration report the
  player sees, and a UI shell here would be a placeholder. The same reasoning
  as F44-A's and F07-A's recorded "not created at this stage".
- **Where an original legacy artifact actually lives is unknown**, so this
  module has no notion of a source *directory*: it takes an `ArtifactProposal`
  (spelling + size + SHA-256 + declared class) and borrowed bytes. F64-B
  discovers the real locations and produces the proposals.

## Sensitivity checks run

Each of these was applied, observed to fail the named tests, and reverted:

| Mutation | Test that failed |
| --- | --- |
| drop the `record_count > max_records` check in `read_legacy_profile` | `accept_f64_a_hostile_record_count_is_refused_before_any_table_is_reserved` |
| drop the planner's `bytes.len()` size gate | `accept_f64_a_malicious_or_oversized_old_profile_fails_without_touching_source_or_new_saves` |
| make `LegacyIdMap::resolve` return the first binding regardless of the key | 4 tests, including `..._ids_resolve_by_identity_...` and `..._not_ready_target_...` |
| drop the retained undeclared record bytes | `accept_f64_a_unmapped_ids_and_undeclared_bytes_stay_unresolved` |

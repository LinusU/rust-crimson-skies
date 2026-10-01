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
- `crates/cs_formats/tests/accept_f64_a_legacy_profile_contracts.rs` (new): 13
  `accept_f64_a_*` tests (1 added in review, 1 extended).
- `crates/cs_content/tests/accept_f64_a_legacy_import_plan.rs` (new): 14
  `accept_f64_a_*` tests, including the AC01 filesystem scenario (3 added in
  review, 1 added in each of the second and third review passes; see "Review
  findings" below).
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
  reason). Two ways a document could be reported as an import that carries
  nothing are refused by name instead: zero records is
  `not_importable { no_records }`, and zero **carried identities** is
  `Unsupported { NoResolvableIdentity { records } }` — the classification
  counts what the plan holds, not how many records were visited, so a layout
  that declares no id slot cannot turn a blank profile into a full import.
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

## Third review pass (bunny-alpha-2, same agent instance, after a second lander rebase conflict)

The second approval could not land either: the lander's automatic rebase hit
another conflict with main. This pass rebased by hand onto `a832a98` (one real
conflict, in `crates/cs_content/src/lib.rs`, where main had added the F45-A
`ui_layout` module-doc paragraph in the same place this branch added the F64-A
one; both were kept) and then reviewed the rebased branch again. The context was
again **not** fresh, so this is a third defect-hunting pass, not independent
review. See the handover note.

7. **A blank profile could still be reported as a *partial* import.** The
   carried-identity check added in the second pass ran only when the document
   had **no** unresolved rows (`unresolved.is_empty() && carried == 0`). A
   document read through a layout that declares its record fields and no id
   slot, which *also* carried an unexplained tail, therefore classified as
   `Partial { resolved: [0, 1], unresolved: [undeclared_trailing_bytes] }` —
   the report claimed two records were imported while the plan carried no
   content identity at all. That is the same "silent reset to a blank profile
   called imported" the `Full` branch was closed for, reached through an
   unrelated leftover: the second pass closed the all-clean path and left the
   others. `carried == 0` is now tested on its own in the `Unsupported` branch,
   so a leftover can no longer mask it; the leftover is still reported, as the
   branch's `unresolved` rows. The regression test reads the same tailed
   document through an id-slot-less layout (asserting it is **not** `Partial`),
   the same document without the tail (asserting the named
   `no_resolvable_identity`), and the tailed document through
   `synthetic_layout()` (asserting it *is* `Partial`), so the assertion is about
   the missing identity rather than about the tail.

8. **A layout could declare a record table with a zero stride.** `validate()`
   refused an empty record slot list (`NoRecordFields`) but accepted a record
   whose only declared field has zero extent (`Bytes { len: 0 }` or
   `Text { len: 0 }`), so `record_size()` was 0. Every record then read the
   same bytes at the same offset and the record count was bounded by nothing in
   the document: a **20-byte** file could claim 4096 records and
   `read_legacy_profile` faithfully reported 4096 identical records, each with
   its own index and offset — 4096 records the file does not contain. Probed and
   confirmed before the fix (`read Ok records=4096` from 20 bytes). Now refused
   as `LegacyLayoutError::ZeroRecordExtent` at the declaration, like every other
   declaration that could not describe a table. The test also asserts that a
   **one-byte** stride still validates, so the refusal is about the zero stride
   and not about small records.

Note on a limit that was checked and is *not* a defect: `plan_import` takes the
caller's `LegacyLimits` verbatim, so a caller may raise `max_records` or
`max_bytes` past the designed values. That is the caller choosing its own
policy, not a hole — the document length is still checked against the limit and
the F03 budget is still charged against `max_bytes` before any `Vec` is
reserved, and `MAX_LEGACY_SOURCE_BYTES` is enforced independently of the
`limits` struct. Probed with `max_records: u32::MAX` and `max_bytes: u64::MAX`:
the same documents plan and the same oversized document is still refused with
`max = MAX_LEGACY_SOURCE_BYTES`. Left as is.

## Sensitivity checks run

Each of these was applied, observed to fail the named tests, and reverted:

| Mutation | Test that failed |
| --- | --- |
| drop the `record_count > max_records` check in `read_legacy_profile` | `accept_f64_a_hostile_record_count_is_refused_before_any_table_is_reserved` |
| drop the planner's `bytes.len()` size gate | `accept_f64_a_malicious_or_oversized_old_profile_fails_without_touching_source_or_new_saves` |
| make `LegacyIdMap::resolve` return the first binding regardless of the key | 4 tests, including `..._ids_resolve_by_identity_...` and `..._not_ready_target_...` |
| drop the retained undeclared record bytes | `accept_f64_a_unmapped_ids_and_undeclared_bytes_stay_unresolved` |
| narrow the version field with `as u32` instead of `u32::try_from` | `accept_f64_a_wide_version_field_is_refused_never_truncated_onto_a_supported_major` |
| clamp an out-of-range legacy id back into range with `unwrap_or(u32::MAX)` | `accept_f64_a_legacy_id_wider_than_the_id_space_is_unresolved_not_clamped` |
| drop the document-level undeclared-trailing-bytes row | `accept_f64_a_undeclared_trailing_bytes_stay_unresolved_instead_of_a_full_import` |
| drop the declared-digest verification in `plan_import` | `accept_f64_a_same_length_source_with_a_changed_digest_is_refused` |
| drop the carried-identity check in `plan_import` | `accept_f64_a_document_resolving_no_identity_is_never_reported_as_a_full_import` |
| drop the duplicate-id-slot check in `LegacyLayout::validate` | `accept_f64_a_invalid_layout_declarations_are_refused_by_name` |
| drop `carried == 0` from the `Partial` branch in `plan_import` | `accept_f64_a_carrying_nothing_is_named_even_when_a_leftover_is_present` |
| drop the zero-record-extent check in `LegacyLayout::validate` | `accept_f64_a_invalid_layout_declarations_are_refused_by_name` |

One test-quality fix came with the above. The AC01 filesystem test ended with
`assert_eq!(destination_before.len(), destination_files_before, ...)`, which
compares the pre-call snapshot's length with itself and therefore proves nothing
— and because the `tree` helper swallows a `read_dir`/`read` failure, the
whole-tree comparison around it would have passed vacuously if the snapshot had
been empty. The pre-call snapshot is now pinned to the one save that is really
on disk, so the after-call comparison cannot pass by observing nothing.

**An honest limitation of AC01, recorded rather than papered over.** What makes
"without touching the source or a new save" true at this stage is *structural*:
`ImportRequest` carries the legacy bytes and a `RelativePath` spelling and has no
field through which a caller could hand the planner a host path, a file handle
or a writer. The AC01 test offers a real hostile payload on a real tree and
compares the source bytes, size and modification time and the whole destination
directory before and after, but that comparison is a guard against a later stage
introducing a path, not the mechanism — no test could catch a writer that
`ImportRequest` cannot express. The test's discriminating power is in the
*refusal*: removing the planner's size gate or the reader's record-count cap
makes it fail, which is what the sensitivity table above records. A reviewer
should not read the filesystem assertions as proving more than that.

Probed and found correct (no change made): `LegacyIdMap::insert` refuses a
duplicate `(class, raw)`; `TrailingPolicy::Reject` refuses an unexplained tail
as `trailing_bytes`; a saturating `slot.end()` is caught by `validate()` as
`SlotExtentOverflow` before `header_bytes()` is used; a
`with_record_size`-widened stride still charges the undeclared bytes per record
and keeps them verbatim.

## Second review pass (bunny-alpha-2, same agent instance, after the lander hit a rebase conflict)

The first approval could not land: the lander's automatic rebase onto a newer
main conflicted. This pass rebased by hand (one real conflict, in
`crates/cs_content/src/lib.rs`, where main had added the F53-A `mods`
module-doc paragraph in the same place this branch added the F64-A one; both
were kept) and then reviewed the rebased branch again. The context was **not**
fresh — it carries this agent's own implementation reasoning — so this is a
defect-hunting pass, not independent review. See the handover note.

Two further defects were found and fixed, both with a regression test observed
to fail when the fix is reverted.

5. **A document resolving no identity at all was reported as a full import.**
   `ImportClass::Full` was decided by the *record* bookkeeping alone: a layout
   that declares its record fields but **no id slot** makes every record resolve
   "successfully" with an empty `resolved_ids`, so `unresolved` stayed empty and
   the plan reported `Full` — a full import of a profile carrying nothing. That
   is precisely the "silent reset to a blank profile called imported" that
   non-negotiable 5 forbids, reached by the path the module believed it had
   closed when it refused a zero-record document. Now the outcome counts the
   identities the plan actually **carries**, not the records that were visited:
   zero carried identities is a named `UnresolvedReason::NoResolvableIdentity`,
   reported as `Unsupported`. The regression test reads the same two-record
   document through an id-slot-less layout and through `synthetic_layout()`,
   asserting `Unsupported { NoResolvableIdentity { records: 2 } }` for the first
   and `Full` for the second, so the assertion is about the missing identity and
   not about the document.

6. **One record field could be declared as an id slot of two classes.**
   `LegacyLayout::validate` checked that every id slot names a declared record
   field, but not that each field was claimed only once. A layout declaring
   `airframe_id` as both `LegacyIdClass::Airframe` and `LegacyIdClass::Weapon`
   validated cleanly, and the planner then resolved that one stored value twice —
   into two different namespaces — so a single number was read as two different
   pieces of content. That is the same class of silent misreading as the clamp
   fixed in defect 2, arrived at through the declaration instead of through the
   value. Now refused as `LegacyLayoutError::DuplicateIdSlot { field }`. The
   regression test declares the duplicate and asserts the named refusal.

One diagnostic was corrected: the out-of-width version refusal named "version
major {raw}" for **both** the major and the minor field, so a wide *minor* field
reported the wrong field in its message. It now names the field it refused.

## Review findings (bunny-alpha-2, reviewing its own implementation)

The reviewer re-read the whole diff against the sheet, the shared contract and
`AGENTS.md`, and probed the behaviour with throwaway tests before changing
anything. Four defects were found and fixed. All four were live behaviours, not
style: each one made the contract the module documents **not** hold, and each has
a regression test that fails when the fix is reverted.

1. **A wide version field truncated onto the supported major.**
   `read_legacy_profile` narrowed the declared version with `? as u32`. A
   layout declaring a 64-bit version and a document carrying `0x1_0000_0001`
   was reported as version major 1 and **read as if supported** — the one check
   that decides whether an unknown format version is admitted. Now narrowed
   with `u32::try_from`; an out-of-width version is refused as
   `unsupported_version` naming the value it read. *This is the most serious of
   the four: it defeats the "clearly distinguish unsupported version" rule of
   non-negotiable 5 for any layout whose version field is wider than 32 bits.*

2. **A legacy id wider than the id space was clamped into range.**
   `plan_import` did `u32::try_from(raw).unwrap_or(u32::MAX)`. An id slot
   declared wider than a `u32`, carrying a value that does not fit, was silently
   clamped to `u32::MAX` and then looked up — so it resolved to whatever element
   happened to be bound at the clamp boundary. A hostile or merely damaged file
   would import **one weapon as another**, which is exactly what non-negotiable
   3 forbids. Now a named `UnresolvedReason::IdOutOfRange` carrying the value as
   read. The regression test deliberately binds `u32::MAX`, so a clamping
   implementation would report a full import and fail.

3. **Undeclared bytes past the record table were dropped from the report.**
   The reader retained them in `LegacyProfileDocument::trailing` and
   `TrailingPolicy::Retain` is the default, but `plan_import` never looked at
   them: a document with a fully resolvable record table plus 4 unexplained
   trailing bytes planned as `ImportClass::Full`. Non-negotiable 2 requires
   unknown fields to stay unresolved, and a `Full` report claims nothing was
   left over. Now a document-level `UndeclaredTrailingBytes` row, exactly as a
   record's undeclared bytes already were. The test also asserts the *same*
   document without the tail is `Full`, so the assertion is about the tail.

4. **The declared SHA-256 was never verified against the bytes.**
   `ArtifactProposal` carries a fingerprint and `validate_against` only compared
   **length**; `SourceFingerprint::new` then recomputed the digest from the
   supplied bytes, so the plan always reported a correct digest — and a
   source that changed between being inventoried and being read was imported
   silently under the identity the inventory gave it. Non-negotiable 1 requires
   the source fingerprint to be retained, and retaining a *recomputed* digest
   while ignoring the declared one cannot detect a changed source. Now the
   digest is compared and a mismatch is refused as `HashMismatch` before the
   document is read; `SourceFingerprint` retains the verified declared digest,
   so a report traces back to the inventoried file rather than to whatever
   bytes arrived. The regression test substitutes a **same-length** document,
   which is the case a length check cannot see.

Two smaller corrections came with the fixes:

- **`admitted_designed_layout` under-reported.** It was `evidence == Designed`,
  so a plan made through `AllowDesignedFixtures` from an `observed_tool` layout
  reported `false` — a report could present tool-observed fixture data as
  measured. It is now "the strict policy would have refused this layout and the
  caller named the fixture admission instead", which is what the flag is for.
- **The test fixtures declared a placeholder digest.** The F64-A fixtures
  declared `hash(0x5a)` for every source regardless of the bytes, which is
  precisely why defect 4 was invisible: the tests passed *because* the declared
  digest was never checked. The fixture helper now declares the production
  `cs_assets::install::sha256(bytes)`, so the suite would have caught defect 4
  rather than accommodating it.

`SourceFingerprint::new` lost its `bytes` parameter as a result of fix 4: it
retains the declared fingerprint rather than digesting the bytes again. A public
API of an unmerged stage, so the signature change is in place rather than
deprecated.

Nothing else in the diff was changed: the inventory, the refusal taxonomy, the
allocation bounding, the declared-layout reader's other checks, the identity-based
id map, the full/partial/unsupported split and the AC01 filesystem scenario were
re-read and are correct as written. The `crates/cs_app/src/ui/import.rs` decision
(not created at this stage, F64-C owns the wiring) is recorded above and stands.

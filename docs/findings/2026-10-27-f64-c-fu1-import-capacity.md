# F64-C-FU1: how many imported records a profile can persist

Date: 2026-10-27. Task: #1530 "Persist imports with more stock-conforming
records than a save's extra list holds" (F64-C-FU1), a follow-up of #756. The
affected content is F64's two import classes: **custom-aircraft imports**
(`LegacyArtifactClass::CustomAircraft`, the blueprint path) and **campaign-save
imports** (`LegacyArtifactClass::CampaignSave`, records without blueprints).
Synthetic fixture evidence only: this build has no measured legacy layout, so
nothing here is a claim about an original save format. The persisted document
fields are this build's own vocabulary.

## The measured problem

#756 persisted a confirmed import into `ProfileDocument::extra`:

* six fixed fields (`import.source.spelling`, `.size`, `.sha256`,
  `import.verdict`, `import.layout`, `import.fixture_admitted`);
* one `import.record.<index>.<slot>` field per resolved identity of every
  `ConfirmedImport::importable_records()` index, as a `class:id` pair.

A profile document's lists are bounded by `cs_types::profile::MAX_LIST_ENTRIES`
= 512 (`extra` is checked against it in `ProfileDocument::validate`), and the
designed blueprint layout declares eight id slots per record
(`synthetic_blueprint_layout`). `extra` therefore holds at most 506 identity
fields, i.e. **63 records at eight slots**; the 64th importable record refused
as `SessionError::Field(TooManyEntries { field: "extra" })` before any
allocation or write. The F64 reader's own designed limit is
`LegacyLimits::designed().max_records` = 4,096, so a file that is legal to read
could be impossible to persist. Nothing was truncated or misread, but a real
gap between "readable" and "storable" is a product refusal the import screen
would have to explain.

## Why a compact spelling inside `extra` is not enough

An identity value is `class:id`, and `ContentId`'s canonical text is
`namespace/key` with a key of up to `MAX_CONTENT_KEY_LEN` = 128 bytes, so one
identity can be about 150 bytes. `MAX_VALUE_BYTES` is 256, so **two identities
cannot be packed into one value** without either truncating a legal identity or
refusing legal ids by length. One identity per field is the only spelling that
round-trips every identity the reader and the id table can produce, and the
field count is what bound the import at 63. A dictionary plus per-record
indices was also rejected: its worst case (every record carrying eight distinct
long ids) still needs roughly a megabyte of identity text, i.e. thousands of
fields, and it would store the identity table twice (once in the dictionary,
once nowhere else) in a spelling no consumer has been built for yet.

Two further hard bounds sit behind the field count: `MAX_SAVE_LINES` = 4096
lines and `MAX_SAVE_BYTES` = 256 KiB for the whole encoded document
(`cs_content::save::codec`). Eight identities for 4,096 records is 32,768
identity rows at up to ~280 bytes each — about 9 MiB — so **the reader's full
4,096-record limit cannot be persisted into a save document of the old shape at
any encoding**, whatever the list bound. Reaching it would need a different
storage shape (a validated sidecar in the profile slot, or a save cap forty
times larger whose every revision rewrites megabytes). That is not a bound
tweak; it is a storage-shape decision this task does not take.

## The decision

Extend the save schema with a **dedicated, bounded, typed list for imported
identities**, and keep the existing on-disk spelling of each identity:

1. `cs_types::profile::ProfileDocument` gains
   `import_identities: Vec<ImportedIdentity>`, where `ImportedIdentity` is
   `{ record_index: u32, slot: u32, value: String }` and `value` keeps the
   `class:id` text the previous revision wrote into `extra`.
   `ProfileDocument::validate` bounds the list by
   `MAX_IMPORTED_IDENTITIES` = 4,096, validates each value with
   `MAX_VALUE_BYTES`, and refuses a repeated `(record_index, slot)` — a
   document cannot name two identities for one slot.
2. The codec encodes one line per identity, still
   `import.record.<index>.<slot>=<class>:<id>`, and decodes that prefix into the
   typed list instead of `extra`. A save written by the previous build (which
   put those lines in `extra`) decodes unchanged: same spelling, one more
   revision of the same format.
3. `cs_content::save::codec` raises the document bounds derived from that list,
   documented at the constant: `MAX_SAVE_LINES` 4,096 → 8,192 and
   `MAX_SAVE_BYTES` 256 KiB → 2 MiB. At the chosen cap the worst-case document
   is 4,096 identity lines (~1.14 MiB with maximal 256-byte values) plus 512
   blueprint lines and the fixed fields; 8,192 lines and 2 MiB hold it with
   room for the other lists.
4. `cs_app::profile::import_document` writes the typed list, and
   `ProfileSession::persist_confirmed_import` refuses an import whose
   `importable_records()` exceed `MAX_IMPORTED_RECORDS` = 512 with the named
   `SessionError::ImportTooLarge { records, max }` — before the document is
   built, before any allocation and before any write. The refusal names both
   numbers so the import screen can explain it.

## The bound this leaves

**512 importable records per import** (4,096 identities at eight slots). The
number is derived, not invented: 512 is the save schema's own per-list bound,
which already bounds `blueprints` — a custom-aircraft import with more than 512
conforming records could never have owned its blueprints either. The identity
list is therefore bounded to `512 × 8` = 4,096 entries, the widest layout this
build declares having eight id slots per record.

The reader still admits up to 4,096 records, so a campaign-save or
custom-aircraft import whose importable set is larger than 512 records remains
**unsupported to persist**: it is refused, never truncated, and the refusal is
`ImportTooLarge`. No release or fidelity claim may call such imports covered.
The remaining gap closes only with a storage shape that holds ~9 MiB of
identity text per profile (a validated profile-slot sidecar, or a much larger
save cap). Recorded as the unresolved bound of F64-C-FU1.

Compatibility note: a profile written at the new cap, read by a build from
before this change, decodes those `import.record.*` lines into `extra` and is
refused over `MAX_LIST_ENTRIES`; the newer build reads the older file exactly.
Nothing is stripped or silently dropped in either direction.

## Verification

`crates/cs_app/tests/accept_f64_c_import_capacity.rs` (task-test prefix
`accept_f64_c_fu1_`) drives the production offer → confirm → persist chain:

* `..._the_chosen_maximum_persists`: 512 fixture records at the eight-slot
  blueprint layout persist through `ProfileSession::persist_confirmed_import`,
  every one of the 4,096 identities and the source fingerprint round-trip
  through a close and reopen, the 512 conforming blueprint ids are owned, and
  the encoded at-cap document is checked against the raised document bounds (it
  is about 272 KB and has more lines — 4,096 identities plus 512 blueprints —
  than the previous 256 KiB / 4,096-line bounds; a maximal-value identity list
  is about 1.14 MiB).
* `..._one_record_over_the_maximum_is_refused`: 513 records confirm (the reader
  admits them) and persist refuses with `ImportTooLarge { records: 513,
  max: 512 }`; the population is unchanged and no slot directory is created.
* `..._the_identity_list_is_bounded_and_unique`: the document schema refuses a
  4,097-entry identity list and a repeated `(record, slot)` — the list bound and
  the one-identity-per-slot rule are enforced where a hand-written save would
  hit them, not only on the import path.

The existing `accept_f64_c_persist_` suite keeps its coverage; its assertions
read the identities from the typed list rather than `extra`, and every case it
proved before (source untouched, exact importable set, partial and
stock-rejected filtering, torn-write recovery) still holds.

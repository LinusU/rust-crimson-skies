# F64-C follow-up: how a confirmed import is persisted

Date: 2026-10-17. Task: #756 "Persist a F64-C confirmed import into a new
profile atomically" — a follow-up of task #256 (F64-C). Synthetic fixture
evidence only — no retail or original-behavior claim. The persisted document
fields are this build's own vocabulary, not a claim about an original save
format.

## The problem

F64-C produced `cs_app::ui::import::ConfirmedImport`: the verified source
fingerprint, the retained `MigrationReport`, the optional
`BlueprintImportReport`, the allocated `TargetProfile` and the exact record
set a persistence layer may carry (`importable_records()` — planned *and*
stock-conforming). It deliberately carries no path and no writer, so nothing
stored it. The task was to write it into a **new** profile through the
existing store, under `docs/contracts/STATE-TRANSACTIONS.md`, never over the
source file and never over an existing profile.

## The decision

`ProfileSession::persist_confirmed_import(&ConfirmedImport) -> Result<
ProfileId, SessionError>` in `crates/cs_app/src/profile.rs`:

1. **Population gate.** `target.kind()` must equal the session's population
   kind, refusing as `Library(ForeignDocument)` before anything is read —
   the same separation every other write path enforces.
2. **The target id must be the next allocation.** `ProfileLibrary::create`
   allocates from the persisted high-water mark; it does not honour a caller's
   id. The persist therefore refuses (`SessionError::ImportTargetMismatch`)
   unless `target.id()` is exactly `high_water + 1` — the id the library is
   about to hand out. Anything below the mark was already issued, live or
   retired, and an issued id is never reissued, so "persist over an existing
   profile" is refused rather than merged or overwritten; an id ahead of the
   mark means the offer's allocation went stale while the owner read the
   report, which is also refused rather than silently landing elsewhere.
   This makes the normal path a comparison, not a new allocation primitive:
   the caller mints the target from the same mark before offering.
3. **The document is built from the confirmed value alone** by
   `import_document` and passed to `library.create`, so the write is the
   store's own sequence — temp file, flush/fsync, backup rotation, rename,
   directory fsync — and the registry records the allocation under the same
   transaction discipline. The new profile becomes the persisted active
   pointer and the session's selection, exactly as `create` leaves one.
4. **Validation happens before the write.** `ProfileDocument::validate` runs
   on the built document, so a retained value that cannot fit a save's
   bounds refuses as `SessionError::Field` instead of failing mid-commit.

## What the retained report becomes (spec F64 non-negotiable 1)

`ProfileDocument` has no migration-report field; the report lands in `extra`
under this build's own keys, which the codec round-trips verbatim:

| Key | Value |
| --- | --- |
| `import.source.spelling` | the source's declared spelling |
| `import.source.size` | the source's byte size, decimal |
| `import.source.sha256` | the verified digest, full hex |
| `import.verdict` | `MigrationVerdict::label()` |
| `import.layout` | the layout id the document was read through |
| `import.fixture_admitted` | `yes`/`no` — whether the report could only exist through `AllowDesignedFixtures` |

Each resolved identity of every importable record lands in its own
`import.record.<index>.<slot>` field as a `class:id` pair. A single joined
value per record was the first sketch; it overflowed `MAX_VALUE_BYTES` on an
8-slot record, which is what pushed the per-identity fields — one `class:id`
pair always fits, and `MAX_LIST_ENTRIES` bounds the whole import the same
way it bounds every other save list. The record set a stored profile carries
is exactly `importable_records()`: a record that was unresolved or
stock-rejected names no field.

`document.settings` is the session's catalog seed — the same
`SettingsState` entries `ProfileSession::create` writes into a made profile —
so an imported profile's first revision records its resolved defaults rather
than opening with an empty list.

`document.blueprints` carries the catalog `ContentId` of each conforming
blueprint inside that same set — the deterministic `blueprint.legacy.<layout>.
<index>` ids the assessment minted. The blueprint's composition is not
duplicated into the document: it is reconstructable from the retained
`import.record.<index>.*` fields (the same resolved identities the assessment
used) and a future task can rebuild `AircraftBlueprint`s from them.

## What is not claimed, and edges deliberately left open

- The source file is never written — the value carries no path to it — and
  the test proves bytes *and* mtime are untouched.
- A torn write (leftover undecodable `profile.tmp`) recovers the previous
  committed revision through the store's existing recovery, reported as
  `RecoveryWarning::Corrupt` — exercised on the imported slot itself.
- **No fixture-into-production gate was added.** A `fixture_admitted` import
  can target a `Production` profile through an interactive session; this task
  retains the marker honestly instead of inventing a product decision. If the
  front-end later needs to keep designed-fixture imports out of live
  profiles, `import.fixture_admitted` is persisted for exactly that check.
- `FingerprintEntry` was not used: its `u64` hash cannot hold the 32-byte
  SHA-256 the spec requires verbatim, and an import source is not the
  "compatibility fingerprint the save was written against" that field
  describes.

## Tests

`crates/cs_app/tests/accept_f64_c_persist_import.rs` (prefix
`accept_f64_c_persist_`, reusing `f64_c_support` fixtures through the
production `ImportFlow` offer → confirm chain):

- `..._a_confirmed_import_lands_in_a_new_profile`: source bytes+mtime
  untouched, an existing profile's slot tree byte-identical, the retained
  report round-trips through close/reopen, exactly the importable set carried.
- `..._carries_only_the_importable_records`: a partial import stores only
  index 0; the unresolved record names no field and mints no blueprint id.
- `..._refuses_to_write_over_an_existing_profile`: an issued id and an
  ahead-of-mark id both `ImportTargetMismatch`; a foreign-kind target
  `ForeignDocument`; nothing written.
- `..._recovers_the_previous_profile_after_a_mid_write_crash`: a torn temp
  file leaves revision 1 installed and is reported, not adopted.

Both mutations were verified: without the next-allocation check the refusal
test fails; with the carry dropped the round-trip and importable-set tests
fail.

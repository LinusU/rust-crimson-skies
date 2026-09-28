# F06-B: the reader and sound container subset, with bounds

Date: 2026-09-28. Task: F06-B "Implement reader and sound container subset
with bounds" (`specs/F06-zbd-families-reader-archives-and-sound-containers.md`,
section `### F06-B`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — **no `$CS_GAME_DIR` read**, no evidence report required, nothing derived
from original game data.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/zbd/archive.rs` (new): the bounded container layer
  both F06-B readers share — `CONTAINER_ENTRYPOINT`, `MEMBER_ROW_BYTES`,
  `MemberExtent`, `MemberTable` (`from_dispatch`, `named`), `FamilyOrigin`,
  `MemberError`, `MemberStatus`, `MemberRow`, `ContainerStatus`,
  `UnsupportedRecord`, `ArchiveListing`, `ContainerError`, `FamilyMismatch`,
  `require_family`, `list_members`, `undeclared_reason` and the span helpers
  `check_extent` / `merge_ranges`.
- `crates/cs_formats/src/zbd/reader_archive.rs` (new): `EncodingEvidence`,
  `ReaderEntry`, `ReaderArchive`, `ReaderError`, `read_reader_archive`.
- `crates/cs_formats/src/zbd/sound_archive.rs` (new): `SoundField`,
  `SoundDescriptor`, `SoundEntry`, `SoundArchive`, `SoundError`,
  `read_sound_archive`.
- `crates/cs_formats/src/zbd/mod.rs` (wiring): the three module declarations,
  the matching `pub use` re-exports, and the module-doc sentence that says no
  reader parses container bytes yet (F06-B is that stage).
- `crates/cs_formats/tests/zbd/readers.rs` (new): the `accept_f06_b_*` tests.
- `crates/cs_formats/tests/zbd/main.rs` (wiring): one `mod readers;` line so
  the F06-A test target also builds the F06-B tests.
- `docs/findings/2026-09-28-f06-b-reader-and-sound-container-bounds.md`
  (this file).

Not created in this stage (later F06 stages, nothing to wire yet):
`crates/cs_assets/src/zbd.rs` (turning a VFS key/container into a member table
and mounting the result is F06-C) and `tools/cs_inspect/src/zbd.rs` (the
`cs-inspect zbd` command needs F06-D's retail pass before it can print an audit
with a nonzero strict status).

**One observable failure:** with the family gate removed from
`read_reader_archive` (or with the gate replaced by a fallback that adopts
whatever family the table happens to declare), the container at
`zbd/interp.zbd` whose bytes carry the **documented** INTERP signature
`0x08971119` / version `7` — a valid header — is read by the reader-family
reader and `accept_f06_b_valid_header_with_incompatible_family_data_fails_explicitly`
fails at `assert_eq!(mismatch.actual(), ZbdFamily::Interp)`: the reader-archive
reader claims a container whose dispatch decided the `interp` family, which is
exactly the silent fallback spec F06 AC02 forbids. The mirrored failure — the
reader family's own container handed to the sound reader, or a member whose
declared extent runs past the container — is covered by the same test and by
`accept_f06_b_member_extents_are_bounds_checked`.

## Why this slice is bounds first

F06-A recorded the constraint this stage works under: the committed research
pack documents a header layout for exactly one of the six ZBD families
(`INTERP.ZBD`), and **none** for the reader or sound family
(`docs/research/FORMAT-NOTES.md` "INTERP observed subset" [S07];
`docs/findings/2026-09-28-f06-a-zbd-family-inventory-and-dispatch.md`,
"Recorded unknowns"). Task #340 owns reading those layouts from the pinned
mech3ax 0.6 source and checking them against the installation; this stage
declares ordinary build/test only and must not define a field offset the pack
does not document (spec F06 non-negotiable #1 and its research boundary,
"Precise sound/reader header variants must be read from the pinned source").

So the subset F06-B implements is the half of a family reader that is
**independent of the member-index layout**: which family the container is (the
F06-A gate), that every member the container's own index declares is inside the
container, what the reader consumed, what it could not support, and that a
container of another family is refused instead of being re-read by a different
parser. The member index is an **input**
(`MemberTable` + `MemberExtent`), not an assumption: F06-B claims no knowledge
of which bytes of a reader or sound archive hold that index or how a member's
name, id and extent are laid out inside it.

## What was implemented

`crates/cs_formats/src/zbd/archive.rs` — the bounded container layer:

* `MemberExtent` (name bytes, optional id, `SourceSpan`) and `MemberTable`
  (`from_dispatch`, `named`) carry the container's declared member index
  together with the evidence that decided the container's family.
  `FamilyOrigin` records **which** key decided it: `Dispatched { basis }` (F06-A's
  two-key dispatch) or `NamedByCaller`.
* `require_family` is the gate. It returns `FamilyMismatch` (container, expected
  family + reader slot, actual family, origin and the header status that *did*
  validate) and there is no code path from it to another reader.
* `list_members` walks the index once, checks `offset + length` per member with
  checked arithmetic against the container's length, records a per-member
  `MemberError` and keeps going, merges the consumed ranges, and books the row
  table *and* the range list against the parse's allocation budget before either
  `Vec` exists.
* `ArchiveListing` reports `rows()`, `member_bytes()`, `consumed_ranges()`,
  `uncovered_ranges()`, `failures()` and `status()`; `MemberError` has stable
  codes `extent_overflow` / `member_out_of_bounds`; `ContainerStatus` is
  `Clean` / `Failed { failures }`.

`reader_archive.rs` and `sound_archive.rs` — the two readers. Each gates its
family, lists the container and exposes `entry(index)` / `entries()` /
`consumed_ranges()` / `uncovered_ranges()` / `failures()` / `status()` /
`unsupported_records()`. Reader entries carry verbatim content plus
`EncodingEvidence::Undeclared` (`ClaimStatus::Unknown`); sound entries carry
verbatim content, their span and a `SoundDescriptor` whose four fields are
`SoundField::Unknown { reason }`.

## Design decisions

- **The member index is an input, not a guess.** The reader and sound families
  have no documented header layout, so F06-B cannot know which bytes of those
  archives hold the index or how a name/id/extent is laid out inside it. The
  tests therefore hand the index in, exactly as F06-A's tests hand in a probe
  header. What the reader *is* responsible for — the family gate, the bounds,
  the retained bytes and spans, the consumed/uncovered ranges, the strict status
  and the unsupported-record list — needs no field offset and is fully
  implemented here. Claiming offsets would have been the one thing this stage
  must not do (spec F06 non-negotiable #1).
- **`NamedByCaller` exists because the sound family is unreachable by dispatch.**
  F06-A recorded that no `.zbd` basename in committed evidence is tied to sound
  bytes, so `ZbdFamily::Sound` owns no role rule and `dispatch` cannot route to
  it. Rather than inventing an archive name, `MemberTable::named` is the
  documented route and it still reports `HeaderStatus::Unvalidated` with the
  family's own reason: naming a family is not validating its bytes.
- **Per-member errors, not a fatal listing.** Spec F06 non-negotiable #4
  requires a listing to continue past an invalid member and show every error,
  so `MemberError` lives on the row and `ContainerError` is reserved for the
  parse itself (the allocation budget). This also mirrors `RofError`, whose
  name-table checks are row-level for the same reason.
- **`unsupported_records()` lists everything today.** That is the honest state
  of the evidence, not an oversight: no reader encoding and no sound header is
  documented, so no entry can be interpreted, and the IDENTITY-CONTENT contract
  forbids dropping an entry from a collection. The list shrinks as #340 lands.
- **No playability API.** `SoundArchive` has no `playable()`, `decoded()` or
  `samples()` accessor. With every descriptor field unknown, such a method would
  be exactly the "advertising playability" non-negotiable #4 forbids; the
  `accept_f06_b_sound_entries_retain_spans_and_unknown_descriptor_fields` test
  pins the alternative (every field `Unknown`, reason equal to the family's
  recorded one).
- **Spans are the contract's own type.** `MemberExtent` carries
  `cs_types::evidence::SourceSpan`, so a member's place in the container can be
  cited by a catalog row or an evidence record without a second span scheme.
- **Borrowing, not copying.** Entry content is a borrow of the container bytes,
  so listing an archive allocates only the row table and the range list — both
  inside the budget.
- **One byte-level entrypoint.** `CONTAINER_ENTRYPOINT` (`zbd.container`) is the
  single parse scope the family readers use; they read no bytes of their own, so
  there is no second scope to add (spec F03-C). A budget failure is reported as
  `zbd.container.members`.

## Test inventory (`accept_f06_b_*`)

All in `crates/cs_formats/tests/zbd/readers.rs`, built into the `zbd` test
target by one `mod readers;` line in `tests/zbd/main.rs`. Every one calls
production code (`read_reader_archive`, `read_sound_archive`, `list_members`
through them, `MemberTable`, dispatch).

| Test | Covers |
| --- | --- |
| `valid_interp_header_is_not_reader_family_data` | **AC02, the stage's minimum scenario**: a container whose INTERP header *validated* is refused by the reader reader with `family_mismatch`, carrying the validated header status; no archive is produced |
| `each_reader_refuses_the_other_family_in_both_directions` | the mirror: reader-family data refused by the sound reader; both gates are real |
| `reader_entries_retain_bytes_names_ids_and_spans` | non-negotiable #2/#3: verbatim content, raw name bytes, ids, `SourceSpan`, declared order, whole-body accounting, `ContainerStatus::Clean` |
| `duplicate_member_names_and_ids_are_preserved` | two rows with the same name *and* id stay distinct rows with distinct spans and bytes |
| `sound_entries_retain_spans_and_unknown_descriptor_fields` | non-negotiable #2 for sound: span + bytes retained; format/channels/rate/loop metadata all `Unknown` with the family's recorded reason |
| `a_corrupt_member_fails_its_content_but_not_its_siblings` | non-negotiable #4 + "with bounds": out-of-bounds and overflowing extents yield no bytes, valid siblings stay readable, both failure codes/offsets, `Failed { failures: 2 }`, consumed ranges exclude the failures |
| `gaps_and_overlaps_are_reported_in_ranges` | merged consumed ranges, interior gap and tail reported as uncovered, overlap is not corruption |
| `an_empty_index_lists_nothing_and_claims_nothing` | zero-member container: nothing consumed, whole body uncovered, still clean |
| `unsupported_records_are_listed_with_their_spans` | the deliverable's "unsupported records", both readers, with identity, span and a non-empty reason |
| `the_listing_is_bounded_by_the_parse_allocation_budget` | F03: a zero-budget parse refuses the row table with `AllocationBudgetExceeded`, scoped `zbd.container.members`; the retry on a healthy context succeeds (no stale charge) |
| `nothing_this_stage_produces_claims_documented_bytes` | evidence honesty: entries stay `ClaimStatus::Unknown`, spans are the contract's `SourceSpan` |

The eleven F06-A tests in `tests/zbd/main.rs` still pass unchanged.

## Mutation probes

Each mutation was applied to production code, the `zbd` test target run, and the
file restored with `git checkout`. `cargo test -p cs_formats --test zbd`, 22
tests:

| Mutation | Failing tests |
| --- | --- |
| `require_family` removed from `read_reader_archive` | `valid_interp_header_is_not_reader_family_data` |
| `require_family` removed from `read_sound_archive` | `each_reader_refuses_the_other_family_in_both_directions` |
| `check_extent` accepts every extent | `a_corrupt_member_fails_its_content_but_not_its_siblings` |
| `offset + length` saturates instead of failing | `a_corrupt_member_fails_its_content_but_not_its_siblings` |
| the bound comes from the member count, not the container length | 7 tests (`reader_entries_…`, `duplicate_…`, `sound_entries_…`, `gaps_and_overlaps…`, `unsupported_records…`, `nothing_this_stage…`, `a_corrupt_member…`) |
| a failed member also counts as consumed | `a_corrupt_member_fails_its_content_but_not_its_siblings` |
| `merge_ranges` removed | `gaps_and_overlaps_…`, `reader_entries_…` |
| `uncovered_ranges` always empty | `gaps_and_overlaps_…`, `an_empty_index_…` |
| duplicate names/ids collapsed in `ReaderArchive::entry` | `duplicate_member_names_and_ids_are_preserved` |
| `SoundDescriptor::undeclared` invents `pcm16le`/1/`22050` | `sound_entries_retain_spans_and_unknown_descriptor_fields` |
| the row table is not booked against the budget | `the_listing_is_bounded_by_the_parse_allocation_budget` |
| `MemberTable::named` claims `HeaderStatus::Validated` | `sound_entries_retain_spans_and_unknown_descriptor_fields` |
| `ContainerStatus` ignores `failures` | `a_corrupt_member_fails_its_content_but_not_its_siblings` |

One probe changed **no** result and is recorded rather than hidden: removing the
`is_readable()` guard from `ArchiveListing::member_bytes` still returns `None`
for every failed row, because `bytes.get(start..end)` refuses exactly the same
ranges. The guard is kept because it is the explicit statement of the invariant
("bytes are handed out only for an extent that was checked"), but it is not the
enforcement point on this target and no test distinguishes it.

## Recorded unknowns (not guessed)

- **Where a reader or sound archive keeps its member index, and how a name, id
  and extent are laid out inside it.** Unknown in the committed pack; task #340
  reads it from the pinned mech3ax 0.6 source and checks it against the
  installation. Until then `MemberTable` is an input, and no `MemberExtent` in
  this crate is described by an offset.
- **Reader entry encoding.** Unknown (spec F06 research boundary). Every entry
  is `EncodingEvidence::Undeclared`.
- **Sound sample format, channels, rate and loop metadata.** Unknown; no
  `.zbd` archive name has been tied to sound bytes either. Every descriptor field
  is `SoundField::Unknown`.
- **Whether real member extents may overlap, alias or leave gaps.** Unknown: the
  listing *reports* overlaps as merged ranges and gaps as uncovered, and refuses
  neither, because neither has been observed. F06-D, with real bytes, decides.
- **The four unnamed `zbd`-level archives** and the sound family's archive names
  stay F06-A's recorded unknown (#340).

No follow-up task was filed: #340 already owns the header layouts and the
archive names, and this stage adds no new unknown beyond what it records here.

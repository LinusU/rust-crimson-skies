# Task #343: the ZBD version-one member index

Date: 2026-09-28. Task #343 "Read the ZBD version-one trailer member table for
sound and reader archives", a follow-up to task #340
(`docs/findings/2026-09-28-t340-zbd-family-headers-and-archive-names.md`).
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t343_`. Evidence:
`docs/findings/evidence/T343.json`.

## Sources

- **Pinned reference source**: mech3ax v0.6.0, commit
  `d3521a9721be731d365504568ddcd78e3f9846bb` (`docs/research/SOURCES.md` S02),
  `crates/mech3ax-archive/src/archive.rs` (`read_table`, `EntryC`) and
  `crates/mech3ax-common/src/string/mod.rs` (`str_from_c_padded`). I read it
  from the private clone task #340 made and copied no code (EUPL-1.2).
- **Retail installation**: install fingerprint
  `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`, content
  fingerprint
  `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` (F02
  production discovery). The per-archive table (spelling, SHA-256, size, member
  count, table start, duplicate names, anomalies, failures, uncovered bytes,
  overlaps) is the evidence artifact `zbd-member-index.json`. It is private and
  the report references it by digest.

## Layout read (as task #340 recorded it)

All words are little-endian `u32`.

| Offset | Bytes | Field |
| --- | --- | --- |
| `size - 8` | 4 | version, must be `1` |
| `size - 4` | 4 | member count |
| `table_start = size - 8 - 148 × count` | 148 × count | entries |
| entry + 0 | 4 | member start |
| entry + 4 | 4 | member length |
| entry + 8 | 64 | name: ASCII, NUL-terminated, zero-padded (`str_from_c_padded`) |
| entry + 72 | 76 | **unknown**: `garbage` in the source, no meaning assigned |

The source asserts `start < start + length <= table_start` for every entry.

## Code (`crates/cs_formats/src/zbd/trailer.rs`)

`read_version_one_index(context, dispatch, bytes)` returns a `VersionOneIndex`:

- **Only the sound and reader families** (`indexed_by_trailer`). Any other
  dispatched family gets `not_indexed_by_trailer` before a byte is read.
- **Checked arithmetic and F03 budgets.** An archive shorter than 8 bytes
  fails with `unexpected_eof` (`zbd.trailer.version`). A version other than 1
  gives `unsupported_trailer_version`, and a count whose entries do not fit in
  front of the trailer gives `index_out_of_bounds`. Both are checked before
  anything is reserved. The entry table and the derived `MemberExtent` table
  are booked against the allocation budget (`INDEX_ROW_BYTES +
  MEMBER_EXTENT_BYTES` per member) before either `Vec` exists. A refused
  index is rolled back.
- **Unknown bytes kept verbatim.** `IndexEntry::unexplained()` returns the
  76 bytes with their span, `ClaimStatus::Unknown` and `UNEXPLAINED_REASON`.
- **Names verbatim.** A name is the bytes before the first NUL, never decoded.
  Duplicate names stay separate entries. The entries carry no numeric id, so
  every extent's id is `None`.
- **Source assertions recorded per entry**, not fatal: `empty_extent`,
  `unterminated_name` (the name is then the whole field), `non_zero_name_padding`,
  `non_ascii_name`.
- **Wiring in front of the F06-B readers.** `index.member_table()` builds the
  `MemberTable` from the dispatch that decided the family. `index.data()` is the
  data region `[0, table_start)`, which is what the reader is handed. A member
  that reaches into the index therefore fails on its own row as
  `member_out_of_bounds` while its siblings stay readable. `index_span()` is the
  consumed index-plus-trailer range.

Changes outside `trailer.rs`: the module declaration and re-exports go in
`zbd/mod.rs`. Doc comments only in `archive.rs`, `sound_archive.rs`,
`reader_archive.rs` and `lib.rs`. `MemberTable` stays an input, so the bounds
layer does not depend on one index layout. The test helpers in
`tests/zbd/t340.rs` are now `pub(super)`, and `parse_suite` takes the test
prefix, so `tests/zbd/t343.rs` reuses them.

## Retail result (`accept_t343_retail_every_sound_and_reader_archive_lists_all_members`)

These come from production code over all 64 archives: 2 sound
(`ZBD/soundsl.zbd`, `ZBD/soundsh.zbd`) and 62 reader (`zrdr.zbd`).

- Every archive's index lists exactly the declared count: 6334 members in
  total.
- No entry anomaly: every length is non-zero and every name is terminated,
  zero-padded ASCII.
- **No gaps and no overlaps.** In every archive the members tile
  `[0, table_start)` exactly, in index order. So the data region has no
  uncovered byte, and the index and trailer cover the rest of the file. The
  whole file is accounted for.
- **29 duplicated names** across the 64 archives, each kept as separate
  members.
- Every sound member starts `RIFF` … `WAVE`.

One ad-hoc read-only observation, not a production test: none of the 6334
76-byte unexplained fields is all zero, and 5912 of them are distinct. They
are not padding. What they mean stays **unknown**.

## Tests (`crates/cs_formats/tests/zbd/t343.rs`, `accept_t343_*`)

| Test | Covers |
| --- | --- |
| `sound_members_come_from_the_trailer` | names, spans, record spans, verbatim unknown bytes, ids `None`; the sound reader over `data()` is clean and fully covered |
| `reader_archives_read_their_trailer_at_every_level` | root, world-group and mission `zrdr.zbd`; duplicate names; the sound reader refuses the reader index |
| `families_without_a_trailer_are_refused` | texture dispatch → `not_indexed_by_trailer`, no charge |
| `other_trailer_versions_are_refused` | versions 0, 2, `u32::MAX` |
| `truncated_and_oversized_indexes_fail_before_allocating` | 0–7 byte archives; counts 4, 1000, `u32::MAX`; an empty index |
| `the_index_is_charged_exactly_against_the_budget` | exact charge fits, one byte less is refused and rolled back |
| `entry_anomalies_are_recorded_without_hiding_siblings` | each anomaly on its own entry; all entries still listed |
| `a_member_reaching_into_the_index_fails_on_its_own_row` | `member_out_of_bounds` against `table_start`; sibling readable; uncovered range reported |
| `retail_every_sound_and_reader_archive_lists_all_members` (ignored without `CS_GAME_DIR`, fails loudly then) | the retail result above |

Mutation probes (each applied to `trailer.rs`, `accept_t343_` run with
`--include-ignored`, then restored). Every probe failed at least one test:
version check disabled, table start computed without the entries, the raw
64-byte field used as the name, the empty-extent check disabled, the whole
archive handed out as `data()`.

## Recorded unknowns

- The **76 unexplained bytes** of each index entry (see above).
- **Sound sample descriptors** are still `SoundField::Unknown`. Reading the
  RIFF/WAVE headers is task #344.
- **Reader entry encoding** is still `EncodingEvidence::Undeclared`.
- What `soundsl` and `soundsh` mean is still unknown (task #340).

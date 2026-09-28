# F05-D: resolve compressed length semantics and audit all private ROF members

Date: 2026-09-28. Task: F05-D "Resolve compressed length semantics and audit
all private ROF members" (`specs/F05-rof-directory-trees-and-compressed-members.md`,
section `### F05-D`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`;
evidence contract: `docs/contracts/CLI-EVIDENCE.md`. Capabilities used:
`retail` (the read-only installation at `$CS_GAME_DIR`) and ordinary
build/test.

## The answer

**A ROF record's first length word (`+4`, `raw_length`) is the member's
*decoded* byte count. Its second length word (`+8`, `raw_length_on_disk`) is
the *stored* byte count: how many bytes at `start` the member occupies in
the container.**

That is the opposite of the order the two English names suggest, and the
opposite of what the reference extractor reads ([S05]: it hands the first
word to zlib and ignores the second). The measured consequences, all on the
two containers of the original installation:

| | `crimson.rof` (60,236,221 B) | `crimptch.rof` (797 B) |
| --- | --- | --- |
| file members | 846 | 1 |
| directory records | 21 | 2 |
| compressed members | 418 | 1 |
| uncompressed members | 428 | 0 |
| every compressed extent is a complete zlib stream | 418/418, `trailing_len == 0` | 1/1 |
| every compressed extent decodes to exactly `raw_length` | 418/418 | 1/1 |
| every uncompressed member has `raw_length == raw_length_on_disk` | 428/428 | n/a |
| blocks + stored extents tile the file exactly (0 gaps, 0 overlaps, 0 tail bytes) | yes | yes |
| blocks + *first-word* extents | **418 overlapping spans** | **one extent 971 B past the end** |
| total stored / decoded bytes | 60,198,862 / 96,063,812 | 670 / 1641 |
| largest expansion (stored → decoded) | 6,045 → 479,764 (`ASSETS/GRAPHICS/PX_ICON_1_9_3.TGA`, ×79.4) | 670 → 1,641 (×2.45) |

SHA-256 of the two containers, and of the installation fingerprint
`cs-inspect` reports for every span:

| | SHA-256 |
| --- | --- |
| `GOSDATA/ASSETS/crimson.rof` | `acc9946874110e9741183384010bc02fd48923ae3d608fcf02a96498c3731174` |
| `GOSDATA/ASSETS/crimptch.rof` | `7ae28cfb65645a3e22aadc64a68badc23e2543bc56356d599fb59375d9ebbefe` |
| installation (`install_sha256` in the report) | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |

Both refusals F05-C recorded for this task are explained by this, and both
are gone: `crimptch.rof` claimed a 1641-byte extent in a 797-byte file
because 1641 is a *decoded* count, and `crimson.rof` appeared to have two
members sharing bytes because every compressed member's decoded count is
larger than the bytes it occupies.

## Why the reference extractor is not wrong about the content

The pinned script is *tolerant*, not right about extents. It does

```python
f.seek(entry["start"])
decompressed = zlib.decompress(f.read(entry["length"]))
```

`entry["length"]` is the first word, which is too long for the member (it
usually reaches into the following members, and past the end of the file for
the last member of a container). `zlib.decompress` stops at the end of the
stream and ignores whatever follows, so the **bytes it writes are the
member's** — every time. Its extents are not the members' extents, which is
the only thing F05-D had to change.

Verified, not assumed: the reference tool was run unmodified except for its
`ROF_PATH` constant (and an empty `data/` output directory it assumes but
never creates), on both containers, and its 847 output files were compared
by SHA-256 with the digests the production reader produced. **0
mismatches.** The reference is `extract_rof.py` from
`github.com/rozab/crimsonskies2blend`, commit
`214b170bf330041b411634dbb9fb392d54c2db7a` (2024-02-17), file SHA-256
`96374b5df3911e1bbaa7400b4637cfe5876afb37e674a735b43541157903588e`. The
script stays in `private/research/` and is **not** committed: it is an
external implementation, and `docs/research/FORMAT-NOTES.md` is explicit
that none belongs in this pack.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/rof.rs`: the reader. `RofError::DecodedLengthMismatch`
  (new) with its `code`/`container`/`offset`/`Display` arms, `RofMemberRead::declared_len`
  (new), `RofMember::{stored_len, declared_decoded_len, is_compressed}` and
  `RofMember::stored_end` (replacing `length_end` / `length_on_disk_end`),
  the walker's stored-extent bounds check and overlap spans, and
  `read_member`'s profile and its declared-length check.
- `crates/cs_assets/src/rof.rs`: `RofMemberInfo::declared_decoded_len`
  (new), `RofCoverage` and `RofSource::{blocks, coverage}` (new), the mount's
  stored extent and `RofSource::read`'s rebuilt member.
- `tools/cs_inspect/src/rof.rs`: `--audit`, `AuditReport`, `audit_container`,
  `spans_json`, `RofOutcomes`, the new `declared_decoded_len` in the member
  list, and the `cs-inspect rof --audit` help text.
- `crates/cs_formats/Cargo.toml` (**wiring**): `miniz_oxide` as a
  dev-dependency so the retail test can re-read a member the way the
  reference profile does. The crate already depends on the same version, so
  **`Cargo.lock` has no diff** and the dependency graph is unchanged.
- `crates/cs_formats/tests/rof.rs`: the F05-D tests, and the F05-A/B tests
  re-authored for the resolved profile.
- `docs/findings/evidence/F05-D.json` (this stage's evidence report) and
  this file. Artifacts stay in `private/evidence/F05-D/`.

**One observable failure:** with the stored/declared roles swapped back in
`read_member` and in the walk, `GOSDATA/ASSETS/crimptch.rof` is refused again
as `extent_out_of_bounds` ("declares extent [127, 1768) outside the 797-byte
container") and `GOSDATA/ASSETS/crimson.rof` is refused as
`unsupported_layout` (the extent `[37359, 93507)` overlaps `[52437, 87591)`)
— the exact two refusals F05-C handed to this task, reproduced from the
synthetic fixtures by `accept_f05_d_the_decoded_word_is_not_a_range_of_the_container`
and `accept_f05_d_overlaps_are_computed_from_the_stored_extent`, and
reproduced on retail by the same commands F05-C recorded.

## Design decisions

- **The two words are kept, never collapsed.** `RofRawRecord` still carries
  both verbatim; `RofMember` exposes them as `stored_len()` and
  `declared_decoded_len()` and only the stored one is a range of the file
  (`stored_end`). The decoded word is *not* bounds-checked against the
  container any more, because it is a count of output bytes: that check is
  the bug (a member whose decoded count runs past the end of the file is
  ordinary — it is the whole of `crimptch.rof`). It is checked where it is a
  real claim instead: the read must reproduce it exactly.
- **A wrong declared length is refused, not repaired.** A read that produces
  a different number of bytes than the record declares is
  `DecodedLengthMismatch`, for a compressed *and* an uncompressed member.
  Truncating, padding or trusting the stream instead would each be a silent
  reinterpretation, which is precisely what spec F05 non-negotiable #4
  forbids. Trailing bytes inside a stored extent are *reported*
  (`trailing_len`) rather than refused: they make the stream's boundary
  visible, and every observed member has zero of them.
- **The directory record's words stay unknown.** All 23 directory records of
  both containers author both words as `0`, so their meaning there is
  unobserved and this reader never uses them. The walk still refuses a
  directory record whose (meaningless) word reaches past the end, because a
  record it cannot interpret is data it must not walk on; the word is not
  read as a length.
- **The mount spans the stored bytes, as before, but now for a measured
  reason.** `SourceSpan` describes a range of a file, so it describes the
  stored extent and its digest is the digest of those bytes. The decoded
  count travels beside it in `RofMemberInfo::declared_decoded_len` rather
  than being folded into the span: no range of the container holds the
  decoded bytes. (This is also the collision hazard task #345 was warned
  about: two containers holding equal content compressed differently have
  different *stored* digests and the same decoded content.)
- **The audit is a plain read.** `cs-inspect rof --audit` reads every member
  through the production `RofSource::read` and reports both words, the
  decoded count, trailing bytes, the stored and decoded digests, the
  container census and the byte coverage. It writes nothing but the report.
  A member the reader refuses is *named* in the report with its code and
  raises the exit code to 3; it never hides its siblings and never refuses
  the container, because one bad member is not a broken archive. A
  container that does not tile exactly is also exit 3.
- **`RofSource` holds the block extents.** A mount walks every directory
  block, so recording their extents costs nothing and lets
  `RofSource::coverage()` answer "do the container's bytes add up?" — the
  question the resolution was measured with — without re-reading anything.

## Test inventory (6 tests, prefix `accept_f05_d_`)

| Test | What it pins down |
| --- | --- |
| `accept_f05_d_the_decoded_word_is_not_a_range_of_the_container` | the `crimptch.rof` shape (670 stored / 1641 decoded in a 797-byte file) walks and reads; the stored word is still bounds-checked |
| `accept_f05_d_overlaps_are_computed_from_the_stored_extent` | the `crimson.rof` shape: two compressed members stored back to back tile the container exactly and read to their declared lengths; the same bytes with the words exchanged are refused as `unsupported_layout` (overlap), with nothing booked |
| `accept_f05_d_a_declared_length_that_disagrees_with_the_bytes_is_refused` | a compressed and an uncompressed member whose declared count is wrong are refused with the code, container, offset, declared and observed counts; the ledger is untouched and the refusal repeats; the agreeing case still reads; trailing bytes are reported, not refused |
| `accept_f05_d_the_mount_records_stored_extents_that_tile_the_container` *(cs_assets)* | the mount keeps both words, indexes and digests the stored extents, spans the stored length, `coverage()` is exact, both members read through the production reader, a session resolves both to spans of the stored length, and the exchanged-word container is refused at the mount with a fresh builder untouched |
| `accept_f05_d_rof_audit_reports_both_words_for_every_member` *(cs_inspect)* | the audit's profile string, census, exact coverage, per-member words/digests/trailing count, a member the reader refuses named with its code and exit 3 while its sibling is still audited, and nothing written but the report |
| `accept_f05_d_retail_members_tile_their_container_and_match_the_reference_profile` *(retail, `#[ignore]`)* | **AC04 on private data:** both containers walk; blocks and stored extents tile each exactly; every compressed member is an exact stream with zero trailing bytes; every uncompressed member's words agree; and every member's bytes equal what the reference extractor's read profile produces, member for member |

The F05-A/B fixtures were re-authored for the resolved profile (a
compressed fixture now authors `raw_length` = 204 and
`raw_length_on_disk` = 62), and the two F05-C compressed fixtures with it.
`accept_f05_b_compressed_member_stored_and_decoded_lengths_differ` (AC02) now
asserts the resolved profile *and* that both words are load-bearing: the
extent check refuses the decoded word as an extent, and a container that
states the wrong decoded count is refused instead of being read.

## Commands and exit codes

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f05_d_ --include-ignored` | 0 (6 tests: 4 `cs_formats`, 1 `cs_assets`, 1 `cs_inspect`; 1 ignored without `CS_GAME_DIR`) |
| `cargo run -p cs_xtask --locked -- test-select --prefix accept_f05_d_` | 0 (6 selected, 6 passed, each re-run alone with `--exact`) |
| `cs-inspect rof --cs-path "$CS_GAME_DIR" --container GOSDATA/ASSETS/crimptch.rof --audit --out …` | 0 |
| `cs-inspect rof --cs-path "$CS_GAME_DIR" --container GOSDATA/ASSETS/crimson.rof --audit --out …` | 0 |
| `python3 private/research/run_reference.py` (the pinned S05 script, unmodified but for `ROF_PATH`) | 0 (846 + 1 files, 0 digest mismatches) |
| `python3 tools/validate_evidence.py private/evidence/F05-D/acceptance.json --artifact-root private/evidence/F05-D --require-pass` | 0 |

## Mutation probes (implementation removed → tests fail)

Applied one at a time, the `accept_f05_d_` suite run with
`--include-ignored`, the source restored from a copy afterwards (the suite
was green again after every restore, and `git status` is clean):

| Mutation | Exit | Failing test(s) |
| --- | --- | --- |
| M1: `read_member` slices `declared_decoded_len()` again (`cs_formats`) | 101 | `…_the_decoded_word_is_not_a_range_of_the_container`, `…_overlaps_are_computed_from_the_stored_extent`, `…_a_declared_length_that_disagrees_with_the_bytes_is_refused`, `…_retail_members_tile_their_container_and_match_the_reference_profile` |
| M2: the overlap check compares decoded-word spans again (`cs_formats`) | 101 | `…_overlaps_are_computed_from_the_stored_extent`, `…_retail_…` |
| M3: the declared-length check is disabled (`cs_formats`) | 101 | `…_a_declared_length_that_disagrees_with_the_bytes_is_refused` |
| M4: the decoded word is bounds-checked as a range again (`cs_formats`) | 101 | the same four as M1 |
| M5: the mount records the decoded word as the stored extent (`cs_assets`) | 101 | `…_the_mount_records_stored_extents_that_tile_the_container` |
| M6: an unreadable member does not raise the audit's exit code (`cs_inspect`) | 101 | `…_rof_audit_reports_both_words_for_every_member` |

M1 and M4 also fail the retail test, which is the point: they are the two
regressions this stage exists to repair, and only retail data shows them as
what they are.

## Recorded unknowns (not guessed)

1. **A directory record's two length words.** `0` in all 23 observed
   directory records. Meaning unknown; the reader never uses them and only
   refuses one that reaches past the end of the container.
2. **Flags other than 1 and 2.** Only `0x0` and `0x2` occur in 867 file
   records. A bit with no observed meaning is still `unsupported_layout`.
3. **Non-UTF-8 names.** No retail name contains a byte above 127 (846 + 1
   members, longest name 34 bytes, deepest path 4), so `NonUtf8Name` is
   still unexercised by original data. The refusal stays.
4. **Trailing bytes inside a stored extent.** Zero in all 419 observed
   compressed members, so the stored word is their *exact* boundary rather
   than an upper bound. The count is reported for any container that has
   them; a non-zero value is not refused.
5. **A container that does not tile.** No observed container has a gap, an
   overlap or a tail byte. `RofSource::coverage()` reports one instead of
   guessing which word made it so, and the audit exits 3.
6. **The other length words of a *sibling* format.** Not in scope; nothing
   here says anything about the ZBD family.
7. **Memory.** `RofSource` holds a whole container: 60,236,221 bytes for
   `crimson.rof` for as long as its session or command lives. Measured only
   as "the audit completes", not as a peak figure.

## Follow-ups filed (not fixed here)

- Task #345 (crimptch.rof collisions over crimson.rof) can now mount both
  containers; its comparison must use the **decoded** digest, because the
  stored digests of equal content compressed differently differ by
  construction. Recorded as a note on that task.
- The `crimson.rof` audit is 32 s of wall clock in a debug build (846 zlib
  streams, 96 MB decoded). A caller that wants the census without the
  content still has to read every member; a future stage that needs the
  census often may want a `--audit-structure` that skips the decodes. Not
  needed for anything today, so it is not built.

## Sources used

- `specs/F05-rof-directory-trees-and-compressed-members.md` (whole sheet,
  `### F05-D`), `docs/contracts/IDENTITY-CONTENT.md` and
  `docs/contracts/CLI-EVIDENCE.md`.
- `docs/research/FORMAT-NOTES.md` ("ROF observed structure [S05]"), which
  states the unresolved question this stage closed, and
  `docs/research/RESEARCH-PLAYBOOK.md` ("compare actual stored stream
  boundary and decoded size, not a filename").
- `docs/findings/2026-09-28-f05-a-…`, `…-f05-b-…` and
  `…-f05-c-mount-rof-into-vfs-and-expose-inspection.md` for the fixture
  conventions, the recorded unknowns and the two retail refusals this task
  inherited (the F05-C note on task #24).
- The pinned S05 script (`github.com/rozab/crimsonskies2blend`, commit
  `214b170bf330041b411634dbb9fb392d54c2db7a`) run read-only from
  `private/research/` as the independent reference.
- `$CS_GAME_DIR` (read-only) for both containers.

## Status

**Checked, not recreated** (AGENTS.md rule 8). The six `accept_f05_d_*`
tests, `cargo fmt`, `cargo clippy -D warnings` and `cargo test --workspace`
pass on this commit, and the six mutation probes above show which
production lines they depend on. What this stage certifies is the
*semantics of the two length words* and that the resolved profile yields
byte-identical member content to the pinned reference extractor on all 847
members of the original installation. It certifies nothing about how the
game uses those members.

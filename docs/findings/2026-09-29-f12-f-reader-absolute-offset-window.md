# F12-F: an absolute-offset window for `Reader`, and the PE reader ported onto it

Date: 2026-09-29. Task: #367 / F12-F "Give Reader an absolute-offset window and
port the PE resource reader to it", a follow-up to F12-B. Shared contract:
none beyond `docs/contracts/IDENTITY-CONTENT.md`'s provenance rule (every
refusal keeps container, absolute offset, field and the expected/observed
conditions). Required capability: **ordinary build/test only** — no `CS_GAME_DIR`
read, no original byte, name, count or text is used or committed. The
`#[ignore = "requires CS_GAME_DIR"]` F12-B tests were run unchanged where a
retail read was possible and are not affected by this change either way; see
"Commands".

EvidenceClass: the whole change is `Designed` (engineered plumbing). It moves
no layout constant, no tuning value and no game rule. The only factual claims
about the original game in this document are inherited from
`docs/findings/2026-09-29-f12-b-pe-resource-reader-and-typed-values.md` and are
not restated as new measurements.

## The problem this closes

F12-B shipped `crates/cs_formats/src/pe_resources.rs` with a module-private
`Image` type whose doc comment explained, in its own words, why it existed:

> `Reader` is a forward-only cursor: it has no seek …, so the random-access
> walk cannot borrow its sequencing.

That is a real limitation — a container that points into its own middle (a PE
resource directory, a ROF member, a level inside a texture package) has to be
read at an offset the parser did not walk to — but the *solution* was a second
implementation of the F03 bounded-read primitives: `Image::slice` re-did the
checked extent, the `usize` conversion and the `UnexpectedEof` /
`LengthOverflow` construction; `Image::u16` and `Image::u32` re-did the
byte-wise little-endian decode. `specs/F03-bounded-binary-parsing-primitives.md`
asks for "a safe byte reader over immutable byte slices with … checked ranges",
and its value is that there is **one** such reader, audited once. A second one
inside one parser is exactly what F12-B recorded as an unknown
("A `Reader` with absolute-offset random access (`io.rs`, a follow-up task),
which would remove the module's private `Image` accessors").

## What was added (`crates/cs_formats/src/io.rs`)

| API | What it is | Refuses |
| --- | --- | --- |
| `Reader::window(&self, offset, len, field)` | A `Reader` over `[offset, offset + len)`, rebased so `position()` is the absolute `offset`, borrowing the input. The random-access twin of `sub_reader`. | A range outside the reader's own range, as `UnexpectedEof` at the requested `offset`; an overflowing `offset + len` as `LengthOverflow`. |
| `Reader::window_bytes(&self, offset, len, field)` | The same checked range handed out as `&'a [u8]` into the input, for a caller that wants the bytes of a whole record rather than sequential fields through a reader. | Identical: both are the one private `window_range`. |
| `Reader::range_end(&self)` | Absolute offset one past the last byte of the reader's range — the outer bound a window is checked against. | — |

Design points worth recording, because they are decisions a later caller has to
know:

- **The window is bounded by the range the reader was given, not by the
  container it came from.** `sub_reader` narrows a reader, and F03 rule 4 keeps
  a nested range inside its parent; a window that could reach back out would
  undo that. So `offset < base` is refused as well as `offset + len > range_end`.
- **The refusal is the refusal a `skip` gives.** For a window that starts
  inside the range the anchor is the window's own `offset`, `expected` is
  `"{len} bytes available"` and `observed` is the number of bytes the range
  really holds from that offset — identical, field for field, to
  `Reader::skip` at the same offset (`tests/accept_f12_f_reader_absolute_offset_window.rs`
  asserts `assert_eq!(window_error, skip_error)` for five over-long windows and
  for `window_bytes`). For a window that starts *before* the range, the count
  falls to the overlap (usually `0`): this reader holds no bytes at all before
  its own start, and the error says so instead of implying the container is
  short.
- **Nothing is clamped.** A window longer than the range is an error, never a
  short window: a parser handed a short window would read a truncated record as
  if it were whole, which is the failure F03's truncation corpus exists to
  prevent.
- **The provenance label is now `Arc<str>`, not `String`.** A random-access
  parser opens one window per field, and each `Reader` owns its label; with a
  `String` that is one allocation per field, and the PE walk's structure pass
  is documented as allocating nothing. `Arc` also removes the per-`sub_reader`
  clone that already existed. No public signature changed: `Reader::new` still
  takes `impl Into<String>` and `container()` still returns `&str`.

## What was ported (`crates/cs_formats/src/pe_resources.rs`)

`struct Image` is **deleted**. The module keeps one `Reader` over the whole
image and opens windows at the absolute offsets the structures name:

- `read_header_fields`, `read_pe_offset`: the scattered DOS/COFF/optional-header
  words go through `u16_at` / `u32_at`, which are two-line compositions of
  `Reader::window` + `Reader::read_u16`/`read_u32` and hold no checks of their
  own.
- `section_span` and `read_layout`: one window per 40-byte
  `IMAGE_SECTION_HEADER`, read forward (eight name bytes, then the four numbers).
  This also removed the `let head = …` arithmetic the old accessors needed.
- `Walker::table` / `Walker::table_bytes`: the directory-size check the spec
  F12 non-negotiable #3 demands is now `check_table`, returning the absolute
  offset; the image bound is `Reader::window`'s. Splitting the two is what
  makes it visible that a table is checked against *the resource directory's
  declared size* and then against the image — two different bounds, not one.
- `Walker::key`, `Walker::data`, `Walker::walk`: a name length word, a 16-byte
  `IMAGE_RESOURCE_DATA_ENTRY` and a directory header plus its entry table are
  each read forward out of one window, in the order the format lays them out.
- `image_len` became `Reader::range_end()`; the `MZ` and `PE\0\0` reads now
  slice once and reuse the bytes for the error message instead of reading the
  same range twice.

**The module decodes no bytes of its own any more**: there is no
`u32::from_le_bytes([bytes[0], bytes[1], …])` left in it, and no private
extent check. The only `ParseError`s it still builds by hand are the two the
format's *own* rules need and `Reader` cannot express — an `offset + len`
overflow while computing a table's position, and the `PeError` domain refusals
(`Malformed`, `OutsideTable`, `DirectoryCycle`, `StringBlock`), which are not
`ParseError`s at all.

**Behaviour is unchanged:** the 16 `accept_f12_b_*` tests pass **unmodified**
(the test file was not touched), including the truncation case
(`accept_f12_b_pe_layout_and_directory_presence_are_checked`, which cuts the
fixture image by 0x200 bytes), the cycle case, the bounds cases, the
allocation-budget case, and — with `CS_GAME_DIR` set on this machine — the
retail survey of `strings.dll`, `language.dll` and `langui.dll` through the
ported walk.

## The audit the task asked for: `rof.rs`, `zbd/`, `interp.rs`, `texture/`

Read for a *second implementation of the F03 bounded-read primitives*, and
for absolute random access a window would express. Result: **no other module
duplicates the primitives**; four sites are adoption candidates, each of which
is a follow-up rather than part of this slice.

| Site | What it does | Verdict |
| --- | --- | --- |
| `crates/cs_formats/src/rof.rs:1572` | `Reader::new(self.container, &file[offset as usize..])`, then `RofError::in_block(offset, …)` re-bases the failure's offset by hand. | **Adoption candidate.** A window would hand back the rebased reader and drop the arithmetic. Not done here: `in_block` also rebases the walk's *domain* errors (`NameTableLength`, `EmptyName`, `UnterminatedName`, `InteriorNul`) and applies the `directory` field scope, so a window removes the offset arithmetic but not the scoping, and the F05 error contract would need its own review. |
| `crates/cs_formats/src/rof.rs:1858` | `&file[start as usize..end as usize]` for a member extent whose ends came from checked reads. | **Adoption candidate.** `Reader::window_bytes` states the bound instead of asserting it with a cast. |
| `crates/cs_formats/src/texture/tga.rs:647`, `crates/cs_formats/src/texture/zbd.rs:860` | The same shape: `&bytes[a as usize..b as usize]` after a checked `Reader` walk, with a comment saying the ends were reached by checked reads. | **Adoption candidate**, same reasoning, same risk (the surrounding F08 error types are not `ParseError`s). |
| `crates/cs_formats/src/zbd/wave.rs:365-373`, `zbd/header.rs:261`, `zbd/archive.rs:640`, `zbd/sound_sample.rs:599`, `zbd/trailer.rs:316` | `u16_at`/`u32_at`/`id_at`/`le_u32`/`.get(start..end)` over an **already-bounded slice**, several returning `Option` or `None`. | **No change wanted.** These are accessors over a range the reader already proved, deliberately shaped as `Option` (a probe reports "absent", not a parse error). They are not a second bounded-read implementation, and turning them into `Result`-shaped windows would change their API for nothing. |
| `crates/cs_formats/src/interp.rs` | Sequential only: `Reader` plus `skip`/`read_*` over a validated container; no absolute re-entry anywhere. | **Nothing to port.** |

Recorded as follow-up tasks rather than fixed inside this slice: **#378 (F05-G)**
for the three `as usize` re-slices, and **#379 (F05-H)** for the ROF block
rebasing, which is listed separately because it changes an F05 error contract
and so must not be bundled with the mechanical replacements.

## Mutation probe (run, not argued)

Both mutations were applied to the committed tree, the suite was run, and the
tree was restored from a copy of the file afterwards.

1. **The window silently clamps to the end of the input** (`window_range`'s
   refusal disabled and both ends clamped to `bytes.len()`).
   `accept_f12_f_window_past_the_container_is_refused_exactly_as_a_skip`,
   `accept_f12_f_window_refuses_every_truncated_prefix_of_the_f03_corpus` and
   `accept_f12_f_pe_resource_reads_go_through_the_shared_window` **fail**; the
   other two still pass. The `accept_f12_b_*` tests and the F03 suites still
   pass, because they never open a window — they always went through the
   cursor. That is the honest limit of this probe: the window's own
   sensitivity is proven by the new tests, not by the older ones.
2. **The shared cursor check clamps** (`Reader::take`, which every `skip`,
   `read_bytes` and `read_u16`/`read_u32` goes through).
   `accept_f03_d_truncation_corpus_fails_at_every_boundary` **fails**, an
   F03-B budget test fails, and so do
   `accept_f12_f_window_reports_absolute_positions_and_borrows_the_input` and
   `accept_f12_f_window_past_the_container_is_refused_exactly_as_a_skip` — the
   latter because it compares the window's refusal against `skip`'s.

Together these say what the two implementations are: the cursor rule is what
the F03 corpus certifies, the window rule is what the F12-F tests certify, and
the F12-F tests deliberately assert that the two *agree* on every refusal they
can express, so a change to one that the other does not follow is a failure
rather than a silent divergence.

## Commands

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0 (every binary reports `ok`; 0 failed)
- `cargo test --workspace --locked -- accept_f12_f_ --include-ignored` → 0
  (5 tests, all synthetic, none ignored)
- `cargo test --workspace --locked -- accept_f12_b_ --include-ignored` → 0
  (16 tests, **all 16 run**: the 9 synthetic ones plus the 2 retail ones in
  `cs_formats` and the 5 in `cs_content`, because `CS_GAME_DIR` was set on this
  machine. The two retail resource surveys — `strings.dll`,
  `GOSDATA/ASSETS/BINARIES/language.dll` and `langui.dll` read through the
  ported walk — pass with the counts, ids, languages and code pages F12-B
  recorded.)
- `cargo test --workspace --locked -- accept_f03_ --include-ignored` → 0
  (34 tests, including the truncation corpus and the budget suites)


## Still unknown (unchanged by this task)

Nothing here resolves an unknown about the original game. The F12-B
question — *whether the game reaches these strings through the Win32 resource
API, through its own `.H` headers, or through both* — is exactly as open as
before, and the recorded unknown about the private accessors is now closed as a
code fact (there are none) rather than as a fidelity claim.

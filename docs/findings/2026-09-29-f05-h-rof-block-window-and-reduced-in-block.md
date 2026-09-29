# F05-H: the ROF block reader is a `Reader::window`, and `in_block` is only the field scope

Date: 2026-09-29. Task: #379 / F05-H "Rebase the ROF block reader with
`Reader::window` and keep the domain-error scoping `in_block` does". Follow-up
to #367 / F12-F, whose audit table named `crates/cs_formats/src/rof.rs:1572`
as the remaining hand-rebased block reader. Shared contract: none beyond
`docs/contracts/IDENTITY-CONTENT.md`'s provenance rule (every refusal keeps
container, absolute offset, field and the expected/observed conditions).
Required capability: **ordinary build/test only**. `CS_GAME_DIR` was set on
this machine, so the `#[ignore = "requires CS_GAME_DIR"]` F05-D retail
survey really ran, unchanged, through the new code path; it is *compatibility*
evidence for this port, not a new measurement, and no original byte, name,
count or text is read, derived from, or committed by this change.

EvidenceClass: the whole change is `Designed` (engineered plumbing). It moves
no layout constant, no length-word semantics and no game rule, and the F05-D
resolution of the two length words is untouched.

## What changed

Two things, both in `crates/cs_formats/src/rof.rs`:

1. **`Walker` holds one `Reader` over the whole container** (built in
   `Walker::new` from the bytes `read_tree` already has) and **`visit` opens
   the block as a `Reader::window` at `offset`** instead of
   `Reader::new(self.container, &file[offset as usize..])`. The walk is
   random access by nature — every block but the root is named by an absolute
   `start` read out of an earlier block — so the reader that expresses that
   is now the shared F03 one rather than a re-sliced range with a hand-written
   rebasing step in front of it. The label is now allocated once per walk
   rather than once per visited block.
2. **`RofError::in_block(delta, scope)` is now `in_block(scope)`**: it applies
   the `directory` field scope to the `Parse` variant and returns everything
   else unchanged. The four arms that added `delta` to `NameTableLength`,
   `EmptyName`, `UnterminatedName` and `InteriorNul` are gone, because the
   window already reports the absolute position those errors are anchored on
   (`borrow_block` reads `names_start` from `reader.position()`, and
   `validate_names` offsets from there). The traversal's own variants
   (`Cycle`, `ExtentOutOfBounds`, `UnsupportedLayout`, `ExpansionBomb`,
   `DecodeFailure`, `DecodedLengthMismatch`) were never rebased and still are
   not: they are constructed with absolute offsets and no scope.

The doc comments on `RofError::offset` and `in_block` were rewritten to say
what is true now: `read_directory`'s failures are relative to the range it was
handed, and `read_tree`'s are absolute because every block is a window.

## The one decision that needed care: what the block's window *asks for*

The window's length is `max(file_len - offset, DIRECTORY_HEADER_BYTES)`, and
its refusal is mapped to the `ExtentOutOfBounds` the walk has always raised,
with the same four numbers (block offset, `start`, the header a block needs,
the container length). Three properties of that choice are worth recording,
because each is a way it could have gone wrong:

* **It reproduces the hand-written check exactly, for every input.** The old
  code refused when `offset + DIRECTORY_HEADER_BYTES > file.len()`. The window
  refuses when `offset + max(file_len - offset, 8) > file_len`, which is the
  same condition stated as a range: for an `offset` inside the container with
  at least a header's worth of bytes left, the ask is exactly the rest of the
  container and succeeds; otherwise the ask is the header and is refused. No
  input exists where the two disagree, and the F05-B and F05-H suites both
  pass **unmodified** in their offset and field-path assertions.
* **It never hands back a short window.** A window that clamped to the end of
  the input would let a three-byte "block" be read as a block, and the refusal
  would come back as a structural `UnexpectedEof` on
  `rof.tree.header.entry_count` instead of the documented domain error. That
  is the mutation probe below, and it is why the refusal is mapped rather
  than propagated: the numbers the walk has always reported are the contract,
  the window's own message is not.
* **`offset + len` cannot overflow, so the window's only refusal is the
  end-of-range one.** `len` is bounded by the container length and a nested
  block's offset comes from a `u32` record field, so `LengthOverflow` is not
  reachable here. The mapping asserts exactly that in a `debug_assert!`, so a
  future edit that makes some other refusal reachable fails in debug builds
  instead of silently re-labelling it.

## What the port did *not* remove (a finding, not a cleanup)

The block window is **not the only bound on a nested block**, and that decides
where the window's own refusal is observable. `visit`'s record loop already
refuses a directory record whose `start + DIRECTORY_HEADER_BYTES` runs past the
container (`rof.rs`, the "A directory record's extent is the nested block"
check), *before* the recursion into `visit`. So for every nested block the
record loop's comparison is what fires, and the only block whose window is the
last word before a byte is read is the **root** block at offset zero, which
`read_tree` calls directly.

Two consequences, both recorded because a reader of this change would
otherwise get the test's design wrong:

* A test that pins the block window's refusal must build the case at the
  **root**: a container of fewer than eight bytes, or an empty one. A
  "directory pointer past the end" fixture (`root_directory_pointing_at(5000)`)
  never reaches the window at all, and a test written that way would pass
  under a clamping window while pinning nothing about it. The first draft of
  `accept_f05_h_the_block_is_a_window_of_the_container` made exactly that
  mistake and **passed** under the clamping mutation; the probe caught it,
  and the test was rewritten around the root block.
* The walk's directory traversal is otherwise *insensitive* to a clamping
  window: under that mutation the only F05-B tests that fail are the two that
  go through `read_member`'s `window_bytes` (F05-G's window), not the block
  reader. The F03 and F12-F suites certify the window primitive itself; the
  new F05-H test is what certifies the block reader's use of it.

## The new test

`crates/cs_formats/tests/rof.rs`, `accept_f05_h_the_block_is_a_window_of_the_container`,
one test with nine cases, no existing test edited:

| case | what it pins |
| --- | --- |
| 1 | A root block with less than a header of bytes (3 bytes, and an empty container) is refused as `ExtentOutOfBounds` at offset `0` with `length = DIRECTORY_HEADER_BYTES` and the container's own length — the window refuses, nothing is clamped, nothing is read. |
| 2 | A nested block past the end of the container keeps the record loop's own `ExtentOutOfBounds` at `5000`. |
| 3 | A nested block without its record table is refused at the absolute offset of the missing table with field `rof.tree.directory.records`. |
| 4 | A nested block whose name table runs three bytes short is refused at the absolute offset of the table with field `rof.tree.directory.name_table`, expecting 8 bytes and observing 3. |
| 5 | The root block's own structural failure keeps `rof.tree.records` at offset 8 — neither scope missing nor applied twice. |
| 6-8 | `EmptyName`, `UnterminatedName` and `InteriorNul` in a **nested** block report the block's *own* name table offset (36 + 8 + 24 = 68), counted once. No test pinned these in a nested block before: F05-A pins them through `read_directory`, where offsets are range-relative by contract, and F05-B pins only `NameTableLength`. |
| 9 | The window is the *whole* rest of the container, not just the header it asked for: the authored tree's three blocks, seven record tables and name tables all still read through it, and the walk books exactly what that tree holds. |

Case 1 is the acceptance criterion's "a directory block whose bytes run past
the container is refused at the same absolute offset"; cases 3 to 5 are "a
nested block still reports `rof.tree.directory.…`"; cases 6 to 8 are the part
only this change could get wrong, because they are the offsets that the
removed `delta` arithmetic used to produce and the window now produces
instead.

## Mutation probes (run, not argued)

Both mutations were applied to the working tree, the selections were run with
`--no-fail-fast`, and the tree was restored from a copy of the file afterwards.
This is a reproduction by the agent that made the change, **not** an
independent review.

1. **The window silently clamps to the end of the input** — `window_range`'s
   refusal disabled and both ends clamped to the range (the same mutation
   F12-F probe #1 describes, applied to the same function).
   `accept_f05_h_the_block_is_a_window_of_the_container` **fails** (1 of 1), at
   its case 1: the three-byte container is read as a three-byte block and
   refused as `parse` instead of `extent_out_of_bounds`. Also failing: 1 of 1
   `accept_f05_g_` (F05-G's `window_bytes`), 3 of 5 `accept_f12_f_` (F12-F's own
   probe), and 2 of 12 `accept_f05_b_` — both of which are `read_member`
   cases, not traversal cases. The 34 `accept_f03_` tests and the rest of
   `accept_f05_b_` still pass, because they never open a window.
2. **The window rebases once and `in_block` rebases again** — the four domain
   arms of `in_block` restored on top of the window, which is the most likely
   way this change would be made wrongly (adding the window and leaving the
   old arithmetic in place).
   `accept_f05_h_the_block_is_a_window_of_the_container` **fails** (1 of 1), at
   its case 6: the nested `EmptyName` is reported at `68 + 36 = 104` instead of
   `68`. Also failing: 1 of 12 `accept_f05_b_`
   (`accept_f05_b_invalid_name_table_fails_a_nested_block`, which pins
   `NameTableLength` at `36 + 8 + 24` and would see `104`). The F03, F05-A,
   F05-C, F05-D, F05-G and F12-F selections are unaffected.

Together the two probes say what the test is worth: it is the only test that
fails when the block reader stops being an absolute window (probe 2), and it
shares the clamp sensitivity of the F05-G and F12-F window tests (probe 1).

## What is *not* evidence for this port

Recorded because a reader of this document could otherwise over-read it, and
this is the same limitation F12-F recorded for its own port.

**No runtime test can distinguish this window from the re-sliced range it
replaced, and none is claimed to.** The two implementations agree on every
input by construction (the ask reproduces the old check exactly), so putting
the old code back makes every test in the repository pass again. The evidence
for the port is: (a) the code — `&file[offset as usize..]` and the `delta`
arithmetic are gone, the walk holds one `Reader` and opens windows from it,
and the `as usize` left in the module are `u32` field conversions (name
lengths, entry counts), never a slice re-derivation; (b) the whole F05, F03
and F12-F selection passing **unmodified** through the new path, including the
F05-D retail survey of both containers with their directory blocks, member
extents and reference-profile byte comparisons; and (c) the new F05-H test,
which pins the refusals the window's own bound decides.

The one observable difference the two implementations could have on a
32-bit `usize` target (a `LengthOverflow` from the old `as usize` cast where
the window reports `UnexpectedEof` with 0 bytes observed) is untested, because
CI builds `x86_64` only.

## Commands

All run on this machine with `CS_GAME_DIR` set.

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0 (801 passed, 0 failed, 71 ignored; these
  are the counts of the branch **as it stands rebased onto `main`**. An earlier
  run, made before the branch was rebased, reported 778/0/70, and that stale
  pair is what this list used to record. These totals move whenever `main`
  moves, so re-run the command rather than trusting the number here.)
- `cargo test --workspace --locked --no-fail-fast -- accept_f05_h_ --include-ignored` → 0 (1 test)
- `cargo test --workspace --locked --no-fail-fast -- accept_f05_ --include-ignored` → 0
  (47 tests, 0 ignored: the synthetic F05-A/B/G ones, the F05-C ones in
  `cs_assets` and `cs_inspect`, and `accept_f05_d_retail_members_tile_their_container_and_match_the_reference_profile`
  against `GOSDATA/ASSETS/crimson.rof` and `crimptch.rof` through the ported walk)
- `cargo test --workspace --locked --no-fail-fast -- accept_f03_ --include-ignored` → 0 (34 tests)
- `cargo test --workspace --locked --no-fail-fast -- accept_f12_ --include-ignored` → 0 (76 tests)
- the two mutation probes above, with `--no-fail-fast` over each selection, so
  the counts are complete rather than first-failure

## Reviewer reproduction (not an independent review)

**Who:** reviewer and implementer are the **same agent identity**,
`bunny-alpha-1/bunny-alpha-1`. The review session had a fresh context — it had
not seen the implementation while writing it — but per `AGENTS.md` a review by
the same agent is **not** independent evidence, and nothing below should be
read as a second pair of eyes. Both mutation probes above were reproduced, and
one extra check was run that the implementing agent had not.

* **Probe 1 (window clamps to the end of the input)** — reproduced.
  `accept_f05_h_the_block_is_a_window_of_the_container` fails at its case 1 with
  `"parse"` where the test requires `"extent_out_of_bounds"`, exactly as
  reported, plus 1/1 `accept_f05_g_`, 2/5 `accept_f12_f_` and 1/12
  `accept_f05_b_`. The `accept_f05_b_` count is 1 here rather than the 2 above,
  because this reproduction's clamp still refused a window that clamped to
  *nothing*; the mutation's exact shape decides how many of those cases it
  reaches, and the case the acceptance rests on is the same either way.
* **Probe 2 (the window rebases once and `in_block` again)** — reproduced
  exactly: the nested `EmptyName` of case 6 is reported at `104` instead of
  `68`, and 1/12 `accept_f05_b_`
  (`accept_f05_b_invalid_name_table_fails_a_nested_block`) fails with it.
  F05-A, F05-C, F05-D, F05-G, F12-F and F03 are unaffected.
* **Differential check, run for this review and not committed.** The section
  above argues the two implementations agree by construction. That argument can
  be *checked* instead: a throwaway harness generated 15,870 containers —
  24 uniform-random byte strings at every length from 0 to 96 and 24
  block-shaped ones (a small plausible header, then random record words) at
  every length from 8 to 160, so every header, table and name-table boundary
  is hit at every offset; hand-built root-plus-nested-block trees with each
  record word perturbed; clean trees of nesting depth 0 to 4 with 0 to 4
  members each, duplicate basenames on and off; and the random corpus again
  through `read_directory`. It recorded the full outcome of every call
  (every `RofError` variant with all of its numbers, or the whole walked tree:
  block offsets, record words, member extents, paths, ids). It was run once on
  this branch and once with `crates/cs_formats/src/rof.rs` replaced by
  `origin/main`'s, and the two transcripts are **byte-identical**: 15,870 lines,
  no difference. 100 of those calls succeed, 50 of them clean multi-directory
  trees whose nested blocks are read through windows.
  This is a *review-time* check, not a committed test: it compares two
  revisions of the module rather than asserting a property of one, and it
  writes its transcript to a file, so it belongs to no test suite. It supports
  the argument; it does not replace it, and it is not portable evidence about
  the original game.

## Still unknown (unchanged by this task)

Nothing here resolves an unknown about the original game. The F05 unknown that
survives is the one the module has always recorded: the two length words of a
**directory** record are `0` in every directory record of both containers, so
their meaning there is still unknown and is still never used. Both ROF
containers of the original installation walk and read through the new code
path with the F05-D profile unchanged, which says the port is compatible with
the observed data; it is not a new measurement of the format.

## Note on paths

This task's owner paths are `crates/cs_formats/src/rof.rs` and
`docs/findings/`. The acceptance test is in
`crates/cs_formats/tests/rof.rs`, which is the F05 feature sheet's own test
owner path and the only file holding the synthetic ROF fixtures the test
needs (`tree`, `valid_block`, `block`, `name_table`, `RawRecord`); a new test
file would have had to duplicate them. No existing test in it was edited.

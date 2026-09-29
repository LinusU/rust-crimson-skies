# F05-G: the three absolute re-slices, through `Reader::window_bytes`

Date: 2026-09-29. Task: #378 / F05-G "Express the ROF member extent through
`Reader::window_bytes` instead of a checked-then-cast slice", the first of the
two follow-ups recorded by F12-F in
[`2026-09-29-f12-f-reader-absolute-offset-window.md`](2026-09-29-f12-f-reader-absolute-offset-window.md)
(§"The audit the task asked for"). Required capability: `retail`, used
**read-only**.

**Owner paths used:** `crates/cs_formats/src/rof.rs`,
`crates/cs_formats/src/texture/tga.rs`, `crates/cs_formats/src/texture/zbd.rs`,
`docs/findings/` (this file), and the three test files the task's acceptance
criterion requires a new test in (`crates/cs_formats/tests/rof.rs`,
`crates/cs_formats/tests/texture/tga.rs`,
`crates/cs_formats/tests/texture/zbd_package.rs`). No existing test
assertion was changed: the only lines removed from a test file are the two
`use` statements that had to grow an import. #379 / F05-H is the second
follow-up (the ROF block rebasing) and is **not** started here.

## What the three sites became

| Site (F12-F row) | Before | After |
| --- | --- | --- |
| `rof.rs` `read_member` | `start.checked_add(stored_len)`, then `end > file.len()`, then `&file[start as usize..end as usize]` | one `Reader::window_bytes(start, stored_len, "member.extent")` over a short-lived whole-container `Reader`, with the window's `UnexpectedEof` mapped back onto `RofError::ExtentOutOfBounds` |
| `texture/tga.rs` `read_trailer` | `&bytes[pixels_end as usize..footer_at as usize]` | `reader.window_bytes(pixels_end, footer_at - pixels_end, "tga.extension")?` on the reader already built for the footer |
| `texture/zbd.rs` `read_texture` | `&bytes[level_start as usize..level_end as usize]` | `reader.window_bytes(level_start, level_end - level_start, "texture.level")?`; the now-redundant `bytes: &'a [u8]` parameter is gone (private fn, one call site) |

F12-F's "no change wanted" rows were left alone: `zbd/wave.rs`'s
`u16_at`/`u32_at`/`id_at`, `zbd/header.rs::le_u32`, `zbd/archive.rs::member_bytes`,
`zbd/sound_sample.rs`'s `member.get(start..end)` and `zbd/trailer.rs::data` all
index a range the reader has already proved and deliberately answer `Option`
("absent", not a parse error), and `interp.rs` has no absolute re-entry at
all. Two more sites are in an owner path and are still casts, and stay
candidates rather than part of this stage: `tga.rs`'s `&bytes[..footer_at as usize]`
(the sub-reader prefix the size word is read through) and `rof.rs:1572`'s
`Reader::new(self.container, &file[offset as usize..])`, which F12-F assigned to
#379 because it moves an F05 error contract.

## The F05 error contract is unchanged, with one wording difference

`read_member` is a `Result`-shaped domain, so the window's refusal had to come
back out as what the parser used to raise:

* **A container that does not hold the extent** is still
  `RofError::ExtentOutOfBounds` with the same container, `offset`, `start`,
  `length` and `file_len` (`file_len` is now `Reader::range_end()`, which for a
  whole-container reader is the old `file.len() as u64`). The extent is still
  established *before* the flag checks, so spec F05's non-negotiable #3 — the
  order — is untouched.
* **`start + raw_length_on_disk` overflowing `u64`** is still
  `RofError::Parse` at `start` with the field `member.extent`. The **message
  text changed**: it is now the shared `"offset + length to fit in u64"` /
  `"offset N plus length M"` wording of `Reader::window_range` rather than this
  module's own `"start + stored length to fit in u64"`. Kind, offset and field
  are the same; no test, spec or caller referenced the old wording
  (`rg "start \+ stored length" crates/ specs/ docs/` matches nothing but the
  shared primitive). No test pins that arm either way: `stored_len` is a `u32`
  widened to `u64`, so `start + stored_len` can only overflow a `start` no
  container can hold, and the only way to reach it is the hand-built
  `RofMember` the public `read_member` explicitly accepts.

## What the new tests can and cannot prove

One new test per touched parser, all three mutation-sensitive:

* `accept_f05_g_member_extent_is_a_window_of_the_container` (tests/rof.rs) —
  an uncompressed member that **ends before the container does** and a
  compressed one whose successor starts at its `stored_end` (so a window that
  ran to the end of the input would hand back the next payload as extra bytes
  or as `trailing_len`), plus a container cut 1 and 4 bytes inside the extent
  refused as the same `ExtentOutOfBounds` with the same numbers, plus a
  hand-built member whose `start + raw_length_on_disk` overflows `u64` — the
  one arm whose *message* the port changed — still refused as
  `RofError::Parse` carrying `LengthOverflow` at that start, field
  `member.extent`. (That last case was added by the review pass: nothing
  pinned the arm before, and it is the only observable difference the port
  makes.)
* `tga::accept_f05_g_extension_area_is_a_window_of_the_container` — a full
  495-byte area, and areas of 2/100/494 bytes whose refusal reports
  `at most {n} (the footer follows)`, which is the window's own length. A
  window clamped to the end of the file would report `n + 26`, the footer.
* `zbd_package::accept_f05_g_texture_level_is_a_window_of_the_container` — a
  palette texture (whose palette follows the level) and two direct-colour
  textures, asserting both the level's length and that it is a borrow of the
  container *at the level's own offset* (`stored.as_ptr() ==
  bytes[at..].as_ptr()`), plus a container cut 0/3/11 bytes into the texel run.

**The honest limit:** the TGA and ZBD windows **cannot fail by construction** —
their ends are positions a checked read already reached inside the same
reader's range, so `window_bytes` is a bound that is proved rather than a
check that can fire. A truncated container is therefore refused by the *earlier*
check in both parsers: `trailing_bytes` at the end of the pixels for a TGA
whose cut also takes the footer, and `unexpected_eof` at the level's start for
a ZBD cut inside the texel run. The new tests pin those refusals at the same
code and offset they had before the port (and the 0-byte TGA area is already
pinned by `accept_f08_b_04`). What the tests prove for these two sites is the
window's *length and start*, not a refusal path that the port introduced — the
same limit F12-F recorded for its own port, and the reason no test can prove a
port at all.

## Mutation probe (run by the review pass, then reverted)

The three windows were each changed to clamp to the end of the input
(`file_len - start`, `range_end() - pixels_end`, `range_end() - level_start`),
the suites were run with `--no-fail-fast`, and the sources were restored from
copies (`git status` clean afterwards). **All three new tests fail:**

| Mutation | Failing assertion |
| --- | --- |
| `rof.rs` window clamped | `tests/rof.rs:2233` — the uncompressed member decodes to the *next* payloads' bytes as well |
| `tga.rs` window clamped | `tests/texture/tga.rs:746` — `"at most 2 (the footer follows)"` becomes `"at most 28"` |
| `zbd.rs` window clamped | `tests/texture/zbd_package.rs:736` — the palette texture's level swallows its palette |

A second, narrower probe on the ROF window — asking for
`file_len.saturating_sub(start)` instead of `stored_len`, which cannot
overflow and so silently folds the overflow refusal into
`ExtentOutOfBounds` — fails the ROF test on the added case 5, which is what
makes that case load-bearing rather than decorative.

This reproduction was run by the same agent identity that implemented the
stage (`bunny-alpha-1`), in a review session with no memory of the
implementation, so it confirms the probes the implementer recorded rather than
constituting an independent review. The AGENTS review policy still asks for a
different agent instance on format/mission semantics; this stage changes no
format semantics and asserts no fidelity claim.

## Commands (all exit codes from the review pass, after the rebase onto `f079264`)

Run with a worktree-private `CARGO_TARGET_DIR="$PWD/target"` (task #383: a
shared target directory makes a test result no longer evidence about the tree
it ran in).

- `cargo fmt --all -- --check` → 0
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` → 0
- `cargo test --workspace --locked` → 0 (104 test binaries, cold build)
- `cargo test --workspace --locked -- accept_f05_g --include-ignored` → 0 (3 selected, 3 passed)
- `cargo test -p cs_formats --locked --test rof --test texture -- --include-ignored`
  with `CS_GAME_DIR` set → 0, including the ignored retail surveys
  (`accept_f05_d_retail_members_tile_their_container_and_match_the_reference_profile`,
  `retail::accept_f08_b_retail_zbd_texture_packages_read_and_decode_with_the_recorded_census`,
  `retail::accept_f08_b_retail_tgas_keep_the_declared_alpha_bits`)
- `cargo test -p cs_formats --locked -- --include-ignored` → 101, and the only
  failure is `evidence_report_f10_b_gamez_writes_the_acceptance_report`, which
  panics by design without `CS_EVIDENCE_DIR` and is ignored in CI. It is not
  related to this stage.

## Evidence

None required: this stage changes no fidelity claim and needs no capability
beyond `retail`, which the F05/F08 retail tests above exercise. No claim
status is asserted here.

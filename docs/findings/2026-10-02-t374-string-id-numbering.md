# T374: the numbering a PE `RT_STRING` block's units carry

Date: 2026-10-02. Task: #374 (no F-key) "String-id numbering: Win32
`name*16+index` vs cs_formats' `(block-1)*16+index`". Owner paths used:
`crates/cs_formats/` and `docs/findings/`. Required capability: `retail`,
used **read-only**; the synthetic half of the acceptance runs without it.

**Verdict: the production formula is correct as written.**
`cs_formats::string_id(block, index) = (block - 1) * 16 + index` is the Win32
string-table numbering. The task description's `name * 16 + index = 3496` is a
misreading — it substitutes the resource directory-entry name (`218`) for the
string *identifier* — and the code must not be changed to match it. This
document records the evidence, the corrected `string_id` doc comment and the
rejected hypothesis.

Installation fingerprint (unchanged from F12-G; this task re-derived it from
the same production discovery):

| Fingerprint | Value |
| --- | --- |
| `install_sha256` | `b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978` |
| `content_sha256` | `a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d` |

Nothing from the installation is committed beyond paths, byte spans, sizes,
SHA-256 digests, resource ids, language ids, code pages and counts. **No
original string text is reproduced here**, and the acceptance tests assert no
string text either: the measured row is identified by its block entry, index
and id.

## The two candidate numberings

Let `b` be the **second-level resource directory-entry name** of an
`RT_STRING` leaf (its "name/id" in the resource tree at
`[type 6, b, language]`) and `i` the position of the string inside that leaf.

- **Win32 numbering** (`cs_formats::string_id`): `id = (b - 1) * 16 + i`.
- **The task description's reading**: `id = b * 16 + i`.

The two differ by exactly one block (`+16`) for every row.

## Why `(b - 1) * 16 + i` is the Win32 numbering

Microsoft documents the section rule in terms of the string **identifier**,
not the directory entry:

> RC allocates 16 strings per section and uses the identifier value to
> determine which section is to contain the string. Strings whose identifiers
> differ only in the bottom 4 bits are placed in the same section.
>
> — [`STRINGTABLE resource`](https://learn.microsoft.com/en-us/windows/win32/menurc/stringtable-resource),
> Microsoft Learn (retrieved 2026-10-02).

So an identifier `n` lives in section `n >> 4` counted from zero. Which
resource directory entry holds that section is not stated by that page, nor by
the `LoadString` reference (its `uID` is "the identifier of the string to be
loaded" and no block-index formula is given). It is therefore **measured**, not
cited: the resource entry `b` holds identifiers `(b - 1) * 16 ..= (b - 1) * 16
+ 15`, so section `n >> 4` is resource entry `(n >> 4) + 1`. Inverting,
`id = (b - 1) * 16 + i`. The `.rc` `stringID` a header defines is the
identifier `n`; the directory entry `b` is one greater than `n / 16`.

The task description's `name * 16 + index` uses `name = 218`, the directory
entry, where the documented rule's `identifier` (the string id) belongs:
`(218 - 1) * 16 + 8 = 3480` is what the compiler stored, and `218 * 16 + 8 =
3496` names a *different* string. The formula is not "the Win32 rule"; it is
the Win32 rule with the wrong quantity substituted.

## Measured in the installation (read-only)

Two independent measurements agree, and both are re-derivable from the
committed tests.

1. **The M01-A row.** The production PE resource reader reaches the leaf
   `[type 6, name 218, language 1033]` in
   `GOSDATA/ASSETS/BINARIES/langui.dll` at file offset **92088**, size
   **610** bytes. Under the production numbering its unit at index **8** is
   id **3480**. The production `cs-inspect config --string 3480:1033` resolves
   that span; `--string 3496:1033` resolves a different, later unit, so 3496
   is a real string and not the title. (Task #368 recorded the same leaf and
   the same 92088/610 span.)
2. **The contiguous header run.** The two resource-compiler headers inside
   `crimson.rof` (`RESOURCE.H`, `RESRC1.H`) name the contiguous identifier
   run **40000..=40170**. Under the Win32 numbering those identifiers need
   `langui.dll` sections **2501..=2511**, all of which exist, each with 16
   counted units; section **2500** does not exist. Under the contrary reading
   the run needs section **2500**, which is absent — a deterministic
   discriminator, measured by task #368
   (`docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`)
   and re-read here through the production reader.

The second measurement is the decisive one for the numbering: it is a
build-provenance fact (the header's identifiers versus the compiled image's
section entries), so it fixes which section each `stringID` was stored in
without any dependence on the project's own formula.

## The contrary hypothesis, recorded and rejected

**Hypothesis (task description):** `id = resource_name * 16 + index`, i.e.
`(218, 8) -> 3496`.

**Evidence against it:**

- The word "identifier" in Microsoft's documented rule is the string id (the
  `.rc` `stringID`), not the resource directory-entry name. `218` is the
  entry; the identifier of the M01-A row is 3480 under the one-based rule.
- The 40000..=40170 run is stored under sections 2501..=2511 in `langui.dll`;
  the contrary reading puts it in 2500, which the image does not have.
- `cs-inspect config --string 3496:1033` resolves to a different string than
  the M01-A title row, so 3496 is not the title's id.

**Test that would have caught a wrong implementation:** replacing
`(b - 1) * 16 + i` with `b * 16 + i` makes both `accept_string_id_*`
acceptance tests fail (the synthetic one at `string_id(1, 0)` and the retail
one at the hard-coded `3480`); the literal ids, not `string_id(...)`
comparisons, are what make the retail test sensitive.

## What is established, and what is not

- **Established:** the section rule (Microsoft-documented) and the one-based
  section entries (measured); therefore the production formula is the Win32
  numbering and the task description's premise is wrong. On `string_id` the
  `Documented` part is the section rule; the one-based entry numbering is
  `ObservedTool`, and the doc comment now states both precisely instead of
  calling the entry numbering documented.
- **Not established (deferred, not a blocker):** which API the original
  engine itself calls to resolve these ids. `LoadString` consumes the same
  identifier space as the resource compiler used, so any consumer must agree
  with the numbering, but no original run was observed. This is the same
  boundary F12-G recorded ("Which resource API the original engine uses") and
  it does not bear on the numbering itself.
- **Not touched:** `cs_content`'s id comments already say
  `(block - 1) * 16 + index` and are correct; no production code was changed
  — only the `string_id` doc comment.

## Tests

`crates/cs_formats/tests/string_id.rs` (new), prefix `accept_string_id_`:

- `accept_string_id_win32_sections_are_one_based` (synthetic; runs in CI):
  pins hard-coded ids (`string_id(1,0) == 0`, `(2,0) == 16`,
  `(218,8) == 3480`, `(u16::MAX,15) == 1_048_559`), the block-218 run, and
  that the contrary `block * 16 + index` value (3496) is not produced.
- `accept_string_id_retail_langui_rows_use_the_win32_block_numbering`
  (`#[ignore = "requires CS_GAME_DIR"]`, retail; fails loudly without it):
  reads `langui.dll` through the production reader, pins the leaf
  `[6, 218, 1033]`, the unit at index 8 to the literal id **3480**, section
  **2500** absent, and sections **2501..=2511** each full.

The existing `accept_f12_b_` regression in `crates/cs_formats/src/text/
tests_f12_b.rs` already asserted `string_id(1,0)==0`, `(2,0)==16` and the
`u16::MAX` bound; it is left in place.

## Evidence

`docs/findings/evidence/T374.json` is the CLI-EVIDENCE report for
`private/evidence/T374/acceptance.json` (validated with
`tools/validate_evidence.py --require-pass`). Its artifact
`string-id-langui-blocks.json` carries, per section, presence, unit count and
non-empty count, plus the M01-A block/index/measured-id/contrary-id and
code-unit count — **no string text**. `cargo-test.log` is the recorded
acceptance run.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_string_id_ --include-ignored
python3 tools/validate_evidence.py private/evidence/T374/acceptance.json \
  --artifact-root private/evidence/T374 --require-pass
```

## Review

Implemented by `deepseek-1` (DeepSeek V4.1 Flash, session of 2026-10-02T03:03Z).
Reviewed by `deepseek-1` as the Rally reviewing agent on the review claim of
2026-10-02T03:58Z: the same agent and model, in a fresh session that re-read the
tree, the installation and the task history. The reviewer re-ran the acceptance
suite and regenerated `docs/findings/evidence/T374.json` on the rebased commit.
A fresh context does not make the review independent — same-agent review is not
independent evidence, and no agent review replaces the owner's approval. This
task does not award `verified_original`.

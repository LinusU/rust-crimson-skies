# M16-A-FU1: the mission-binding title source span is the matched row's own bytes

Date: 2026-10-02. Task: M16-A-FU1 "Record the title row's own bytes as the
mission-binding title source span" (#478), raised by the M16-A review (#303).
Feature: F50 per-mission compatibility, `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: `retail` (`$CS_GAME_DIR` read-only, never written).
Implementer: **deepseek-1** (DeepSeek V4.1 Flash, session of 2026-10-02T01:12Z).
No reviewer yet; an implementer's own run is not independent review and no
agent review replaces the owner's human approval.

## The measurement: what the reader knows about a row

`SourceContext::bind` used to take the confirmed title's span from
`StringRow::span`. That field is documented as "where the block's bytes are",
and it is the span of the whole PE `RT_STRING` **block**, which the
installation fills with up to sixteen unrelated strings. The reader did *not*
record a per-row range, so the only byte range a binding could cite for a
title was the block.

The bytes needed to recover the row's own range are, however, already read
and retained. `StringCatalog::rows()` flattens the decoded blocks into
`StringRow`s, and `StringCatalog::resources()` keeps every `StringBlock`,
whose `data.file_offset` is the block's byte offset and whose `units` are the
decoded code units in block order. Each unit is encoded as a little-endian
`u16` code-unit count followed by that many UTF-16 code units, so unit `i` of
a block occupies

    offset = block.data.file_offset + sum over j < i of (2 + 2 * units[j].len())
    length = 2 + 2 * units[i].len()

bytes. Recovering a row's range is therefore arithmetic over bytes the reader
already decoded — not a second parse of the image, and not an invention.
`StringCatalog` is unchanged: the range is computed in
`crates/cs_content/src/campaign_bindings.rs` from the catalog's public
surface, so no `cs_content::config` contract moves.

Measured on `$CS_GAME_DIR/GOSDATA/ASSETS/BINARIES/langui.dll`
(282 624 bytes, language 1033), by an independent throwaway PE parser and
cross-checked against the production derivation:

| binding | confirmed row id | row text | row offset+length | enclosing block offset+length |
| --- | --- | --- | --- | --- |
| M01 | 3480 | `[AB14I]The Lost Treasure` | 92292 + 50 | 92088 + 610 (block 218) |
| M12 | 3491 | `[AB14I]The Great Plane Robbery` | 95446 + 62 | 95304 + 876 (block 219) |
| M16 | 3495 | `[AB14I]Raid on the Rocky Express` | 95676 + 66 | 95304 + 876 (block 219) |

Before this stage every one of those bindings cited the block: M01
`92088 + 610` and M12/M16 `95304 + 876`. The M01 block holds the long names
of M23/M24 and the short names of M01..M08; the M12/M16 block holds the short
names of M09..M24. Neither block identifies a single string.

## The fix

`SourceContext::bind` now records:

- `SourceBinding::title_source` — the confirmed row's own range, when
  recoverable, and the enclosing block only as the fallback;
- `SourceBinding::title_enclosure` — the `RT_STRING` block the row was
  decoded from, kept distinct and clearly named so a block span can never be
  read as the title;
- the title's entry in `SourceBinding::source_spans` — the row's own range
  (the `sha256` in that entry is unchanged: it is still the whole-asset
  digest, see below);
- an explicit `unknowns` entry when a confirmed row's own range cannot be
  located in its block, so the record says the cited span is the enclosing
  block rather than implying it is the row. That arm is unreachable on the
  retail installation (every confirmed row is a decoded unit) and exists so
  the fallback cannot silently read as a recovered row.

The enclosing block is deliberately **not** added to `source_spans`.
`schemas/mission-binding.schema.json` gives a span no name — only
`asset_id`/`offset`/`length`/`sha256`, with `additionalProperties: false` —
so an unnamed block entry would recreate exactly the ambiguity this task
exists to remove. The block survives in the record type's
`title_enclosure`, and the committed records carry the precise row.

### `SourceSpanRecord.sha256` stays the whole-asset digest

`SourceSpanRecord`'s doc now says so explicitly: `sha256` is the digest of the
whole asset named by `asset_id`, identical on every span of that asset, so it
pins the asset and not the range. `offset`/`length` are what locate the
bytes. No consumer re-computes a per-range digest, and
`missions/bindings/M*.json` keep the same whole-`langui.dll` digest
`357e6bb0…1faf49` / whole-program digests as before.

## The M16-A-FU1 task's parenthetical is corrected by measurement

The task's acceptance criterion asks the new test to prove the title span
"covers the bytes of `Rocky Mountains - Raid on the Rocky Express` (the row
the join actually matched)". The join did **not** match that row. Since
`missions/bindings/campaign-inventory.tsv` declares M16's title as
`Raid on the Rocky Express`, `SourceContext::confirm_title` confirms the row
whose display text equals the title verbatim, and the verbatim form wins over
a region-prefixed long name. The measured rows are:

| row id | display text | offset+length |
| --- | --- | --- |
| 3495 | `[AB14I]Raid on the Rocky Express` | 95676 + 66 — **the confirmed row** |
| 3465 | `[AB14I]Rocky Mountains - Raid on the Rocky Express` | 94576 + 102 (block 217) |

So the row the join actually matched is 3495, the short name, and
`accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes` tests against
it. The task's parenthetical names the long-name sibling 3465, which is a
different row in a different block; asserting the cited span covered *that*
row would have failed against the correct code. The test pins both facts: the
long-name row 3465 is disjoint from the cited span and is not the confirmed
row. This is a correction of the task text, not of the protected spec, and is
recorded here per AGENTS.md rule 4.

## Test inventory

New file `crates/cs_app/tests/campaign/m16_a_fu1.rs`, prefix
`accept_m16_a_fu1_`:

- `accept_m16_a_fu1_the_title_span_is_the_matched_rows_own_bytes` (retail,
  `#[ignore]`): re-measures the confirmed row's range from the decoded units,
  asserts the cited span equals it, decodes the cited bytes back to the row's
  code units and title, checks no other row of either campaign-length block
  overlaps it, checks the long-name sibling is disjoint, and pins the
  committed `M16.json`.
- `accept_m16_a_fu1_the_enclosure_is_kept_distinct_from_the_cited_span`
  (retail, `#[ignore]`): the enclosure is the confirmed row's block, the cited
  span is strictly inside it and smaller.

Both fail when the fix is reverted: with `row.span` recorded, the cited span
is the 876-byte block 219 at `95304`, which is neither the row's measured
range `95676 + 66` nor equal to the enclosure, and the decoded bytes are not
the row's.

The existing M05-A title-span assertion
(`accept_m05_a_source_derived_binding_has_no_unresolved_critical_dependencies`)
is updated to the same semantics: it asserted the old buggy equality with the
block span and now asserts `title_source` is inside `title_enclosure`, that
they differ, and that `title_enclosure` is the confirmed row's block.

The 9 other synthetic `SourceBinding` literals
(`m01_a.rs` … `m06_a.rs`, `m08_a.rs`, `m12_a.rs`, `m13_a.rs`, `m16_a.rs`)
gain `title_enclosure: None`; M16-A's own suite
`accept_m16_a_` still passes unchanged.

## Review follow-up (2026-10-02)

The rebase onto `main` brought in M07-A's binding, which landed after this
branch was cut. The production fix applies to it as well, so the review
re-pinned `missions/bindings/M07.json` (its confirmed row is `92592 + 60`
inside block 218, where the old record cited the whole `92088 + 610` block)
and gave M07-A's synthetic `authored_binding` the same `title_enclosure: None`
field. `accept_m07_a_the_committed_record_is_what_the_installation_derives`
passes with the re-pinned record, and the M16-A-FU1 evidence report is
regenerated on the reviewed tree per `docs/contracts/CLI-EVIDENCE.md`.

## What is not claimed

- Only the *span* changed. The title-to-directory join is still an inference
  (`ClaimStatus::Inferred`), `verified` is still `false`, and every unbound
  checklist entry is still recorded in each `unknowns` list. Nothing here is
  `verified_original` or a gameplay claim.
- No original bytes are committed: the spans and the whole-asset digests only.
- The row range is arithmetic over the block's decoded units; it is measured,
  not separately verified against an original disassembler or a running
  original process.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m16_a_fu1_ --include-ignored` | 0 (2 tests) |
| `cargo test --workspace --locked -- accept_m16_a --include-ignored` | 0 (11 tests) |
| `cargo test --workspace --locked -- accept_m01_a_ … accept_m13_a_ --include-ignored` | 0 (76 tests) |

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover` and
`cs_content::config::StringCatalog`; `schemas/mission-binding.schema.json`;
`crates/cs_formats/src/pe_resources.rs` (RT_STRING unit encoding);
`docs/contracts/IDENTITY-CONTENT.md`; `docs/contracts/CLI-EVIDENCE.md`;
`missions/M16.md`; `docs/findings/2026-10-02-m16-a-source-binding.md`.

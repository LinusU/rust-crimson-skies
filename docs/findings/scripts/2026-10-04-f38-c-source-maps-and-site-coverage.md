# F38-C: source maps and per-site coverage for the measured UI script programs

Date: 2026-10-04. Task: **F38-C** "Connect complete source maps and instruction
coverage reports" (`specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
section `### F38-C`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`) and ordinary build/test.
Evidence report: `private/evidence/F38-C/acceptance.json`, committed as
`docs/findings/evidence/F38-C.json`.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/source_map.rs` (new): `SiteOrigin`,
  `MappedSite`, `SourceMapError`, `line_column`, `map_sites`. The producer's
  source map: member, byte offset, length, 1-based line and column of every
  measured site, read from the program's own bytes and never retained.
- `crates/cs_formats/src/script_raw/mod.rs`: module declaration and one doc
  bullet (wiring only).
- `crates/cs_script/src/bindings/located.rs` (new): `SourceMap`,
  `LocatedBindingError`, `lower_program_located`, `SiteRow`, `SiteVerdict`,
  `LocatedSite`, `SiteAudit`, `audit_sites`. The consumer: located diagnostics
  and the per-site coverage report.
- `crates/cs_script/src/bindings/mod.rs`: module declaration and one doc
  sentence (wiring only).
- `crates/cs_script/tests/accept_f38_c_located_diagnostics.rs` (7 synthetic
  tests), `accept_f38_c_retail_source_map.rs` (1 retail) and
  `evidence_report_f38_c.rs` (harness).
- This file and `docs/findings/evidence/F38-C.json`.

**Observable failure (AC03):** before this stage a `BindingError` carried a
`CallSite` with only an optional numeric span in an anonymous program; it did not
say which archive member, line or column the bad call was in. A wrong argument
type or range now reports `member:line:column (0xoffset+len)`, and a program
whose scan disagrees with its bytes is refused by `map_sites` instead of
indexing out of range.

## What it does

- **Producer.** `map_sites(scan, bytes)` turns each `HostCallSite` span into a
  `SiteOrigin`. It fails closed (`span_outside_program`, `too_many_sites`,
  `program_too_large`) and returns no partial map.
- **Consumer, diagnostics.** `lower_program_located` is `lower_program` with each
  `BindingError` paired with the origin a `SourceMap` holds for its
  `(objective, call)`. A call with no mapped origin is reported **without** one,
  never with an invented one. The map is bounded and refuses a key mapped twice.
- **Consumer, coverage.** `audit_sites(table, rows, unjudged_heads)` judges
  **every site**: `bound`, `refused` (family measured, meaning not),
  `no_native_id`, `no_family`, `arity_mismatch`, `shape_mismatch`,
  `unknown_shape_code`, and it carries the call-shaped heads it cannot judge, so
  the report covers the whole corpus instead of the measured part of it.
  `campaign_ready` is true only for a non-empty audit with every site bound and
  nothing unjudged (AC04, per site).
- `cs_script` still depends on `cs_types` only; the producer's types cross as
  primitives (`SiteOrigin`, `SiteRow` with `ArgShape::code()` shape codes), as in
  F38-B. Nothing was added to `cs_content`, for the reason F38-B recorded.

## Retail measurement (`$CS_GAME_DIR`, read-only)

Through production readers over `GOSDATA/ASSETS/crimson.rof`:

| Count | Value |
| --- | --- |
| UI script programs mapped | **61** |
| sites located (member, offset, line, column) | **1812** |
| sites in a family the table refuses for want of a measured meaning (`refused`) | **704** |
| sites whose dispatch expression names no integer (`no_native_id`) | **114** |
| sites in a family whose own sites disagree, refused by name (`no_family`) | **994** |
| `shape_mismatch` / `arity_mismatch` against a valid family | **0 / 0** |
| sites **bound** | **0** |
| call-shaped heads outside the two forms, counted as **unjudged** | **3188** |
| `campaign_ready` | **false** |

Every origin is inside its member, in source order, and starts with the form's
head (`callback` or `mail`). The audit therefore accounts for the whole corpus:
1812 judged sites plus 3188 unjudged heads, nothing silently absent.

## What this does not establish (unresolved, not removed)

1. **No site is bound.** No original observation states what any dispatch value
   does, so nothing lowers and `SiteVerdict::Bound` is exercised by no retail
   site. `campaign_ready` is therefore never observable `true` today, in either
   direction. Resolving task: F38-D / a measured meaning.
2. **No runtime event can be traced yet**, because no measured program lowers.
   The source map is exercised through `lower_program_located` on authored
   programs; tracing a *runtime* event to its origin needs a lowering and is
   F38-D / mission work.
3. **These are UI script programs, not the mission language.** Mission programs
   are not decoded (F13-D), so no mission instruction or native call is located
   or covered here. "Instruction coverage" in this stage is coverage of the two
   measured dispatch forms plus the counted, unjudged remainder (3188, F38-B);
   the remainder is *reported* as unjudged, never folded into a covered verdict.
4. The 994 `no_family` sites are real disagreements inside the corpus (arity or
   argument shape); this stage makes them *located*, it does not resolve them.
5. The crossing from scan to `SiteRow` is written out in the tests and harness,
   as F38-B's crossing was, because no crate may depend on both producer and
   consumer; a production home for it is outside this task's owner paths.
6. The stage text asks for "teardown/retry". Neither applies to this slice and
   neither is claimed: a source map and an audit hold no resource, hold no
   handle on the installation and are pure functions of their arguments, so a
   retry is a repeated call with the same result and a teardown is a drop. The
   error propagation the stage does ask for is exercised: `map_sites` and
   `audit_sites` return errors instead of partial results, and
   `lower_program_located` returns every refusal rather than the first.

## Tests (`accept_f38_c_*`)

Synthetic (authored text in the measured dialect): source map names member,
offset, line and column; a scan that disagrees with its bytes is refused; bad
argument types and ranges report the origin (**AC03**); a call without a mapped
origin gets none; the map is bounded; the audit judges every site and refuses
`campaign_ready`; an empty audit, an unbound table and an audit with unjudged
heads are never ready; an unknown shape code and an oversized audit are refused.
Retail: all 1812 sites located and audited with the counts above, 3188 unjudged
heads reported.

## Review (bunny-alpha-2, Rally #158, fresh context, independent of the
implementer)

Findings fixed in the branch rather than handed back:

1. **The coverage report was a silent subset.** `audit_sites` judged the two
   measured dispatch forms and nothing said that 3188 measured call-shaped heads
   were outside it, so `campaign_ready` could in principle have gone true over a
   corpus it never read. `audit_sites` now *takes* the unjudged-head count (the
   corpus's `other_call_sites`), `SiteAudit` reports it and `campaign_ready`
   refuses while it is non-zero. The count is pinned in the retail test and in
   the evidence artifact's `totals`, so the limitation is machine-readable
   instead of prose-only.
2. **`map_sites` reported the wrong cause.** A program whose line or column
   number no longer fits a `u32` was reported as `span_outside_program`, which
   blames the scan/bytes disagreement it cannot have. It is now
   `program_too_large`.
3. **`SourceMap` did not say what its key means.** `(objective symbol, call
   index)` is program-scoped (`SymbolId` identifies a variable or objective
   *inside one program*), so one map reused across two programs would report
   another program's text. The type now says so, because nothing can detect it.
4. **`LocatedError` had no `code()`** while every other error type in this
   crate family does; added and asserted in the tests.
5. **The evidence harness carried two stale F38-B references**: a comment
   describing `host-call-corpus.json` with `bound_families` /
   `coverage_complete` (this harness writes `source-map-audit.json` with `bound`
   / `campaign_ready`) and a panic message pointing at
   `evidence_report_f38_b.rs`. Both corrected.

Verified by six mutation probes (each fails at least one `accept_f38_c_` test):
`SourceMap::origin_of` returning `None`, `judge` skipping the shape comparison,
`judge` skipping the arity comparison, `map_sites` dropping its span bound,
`line_column` reporting line 1, and `audit_sites` dropping one site (the retail
test catches that one, 1811 vs 1812).

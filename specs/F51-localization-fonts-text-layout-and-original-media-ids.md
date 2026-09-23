# F51: Localization, fonts, text layout, and original media ids

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F12, F14, F17, F41.
**Owner paths:** `crates/cs_content/src/localization.rs`; `crates/cs_app/src/text/`; `crates/cs_app/tests/text/`.
**Shared contract:** [UI-NETWORK](../docs/contracts/UI-NETWORK.md).

## Deliverable and interfaces

Localization resolves original string/media ids by selected locale with an explicit fallback chain. Text rendering supports the original font assets when their format is known and privately loaded, plus a separately licensed fallback font distributed only with verified permission.

## Non-negotiable behavior

1. Never bundle operating-system or proprietary game fonts. Font discovery/parsing is a content dependency, not a license grant.
2. Retain original control markup, line breaks and substitutions only after grammar validation. Do not interpret arbitrary resource strings as executable markup.
3. Text measurement, wrapping, clipping and focus order must handle long translations and UI scale. Missing glyphs are counted, not silently invisible.
4. Dialogue subtitles bind to exact speaker/cue ids and timing; generated replacement dialogue is outside faithful scope.
5. Changing locale cannot change save ids, mission identity, numeric parsing or network protocol values.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Long localized text fits or scrolls without covering required buttons.
**AC02:** Malformed markup and absent glyphs produce visible diagnostics.
**AC03:** Switch locale and reopen the same save without losing unlocks.
**AC04:** Audit all strings and media for each declared supported original locale.

## Bounded implementation slices

### F51-A: Define locale fallback, markup and font provenance

Dependencies: F12-A, F14-A, F17-A, F41-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f51_a_`. Minimum scenario: Long localized text fits or scrolls without covering required buttons.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F51-B: Implement resource decoding and text layout

Dependencies: F51-A, F12-C, F14-C, F17-C, F41-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f51_b_`. Minimum scenario: Malformed markup and absent glyphs produce visible diagnostics.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F51-C: Integrate menus, HUD, subtitles and original font loading

Dependencies: F51-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f51_c_`. Minimum scenario: Switch locale and reopen the same save without losing unlocks.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F51-D: Run locale/glyph/overflow coverage and license review

Dependencies: F51-C. Required capabilities: gpu, retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f51_d_`. Minimum scenario: Audit all strings and media for each declared supported original locale.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S02](../docs/research/SOURCES.md); [S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

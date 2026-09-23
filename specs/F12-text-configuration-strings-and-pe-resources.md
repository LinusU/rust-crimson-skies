# F12: Text configuration, strings, and PE resources

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F03, F04, F06.
**Owner paths:** `crates/cs_formats/src/text/`; `crates/cs_formats/src/pe_resources.rs`; `crates/cs_content/src/config.rs`; `tools/cs_inspect/src/config.rs`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Inventory original text/configuration dialects and localizable resources. Parse strings.dll and other PE resource containers as inert data; never load or execute a DLL. Retain string ids, locale, encoding, source position and unknown fields.

## Non-negotiable behavior

1. Extract grammar from real samples before implementing a parser. Do not globally split on whitespace, ignore quoting, or assume INI/CSV/JSON because a filename resembles one.
2. Numeric conversion is typed and checked; units and signedness belong to the schema. Preserve raw values for unverified tuning fields.
3. Resource directory offsets require bounds and cycle checks, including language subtrees and code pages.
4. Missing text shows a diagnostic key in developer mode; release-critical UI/dialogue strings must resolve in every declared supported locale.
5. Unknown configuration keys are retained and counted; gameplay-critical unconsumed keys block parity rather than being discarded.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Quoted separators, comments, CRLF and non-ASCII names survive parsing.
**AC02:** Negative, overflow and NaN values cannot become valid tuning constants.
**AC03:** Malformed PE resource offsets never invoke platform DLL loading.
**AC04:** A localized installation preserves stable ids while changing display text.

## Bounded implementation slices

### F12-A: Inventory dialects and define lossless configuration nodes

Dependencies: F03-A, F04-A, F06-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f12_a_`. Minimum scenario: Quoted separators, comments, CRLF and non-ASCII names survive parsing.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F12-B: Implement confirmed text and PE resource readers

Dependencies: F12-A, F03-C, F04-C, F06-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f12_b_`. Minimum scenario: Negative, overflow and NaN values cannot become valid tuning constants.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F12-C: Resolve typed tuning and localized ids through the catalog

Dependencies: F12-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f12_c_`. Minimum scenario: Malformed PE resource offsets never invoke platform DLL loading.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F12-D: Account for every referenced configuration field and string

Dependencies: F12-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f12_d_`. Minimum scenario: A localized installation preserves stable ids while changing display text.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S02](../docs/research/SOURCES.md); [S04](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

# F38-B: the measured host-call families of the shipped UI script programs

Date: 2026-10-03. Task: **F38-B** "Implement the observed host-binding families
in bounded batches" (`specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
section `### F38-B`). Shared contract: `docs/contracts/SCRIPT-MISSION.md`.
Capabilities used: **`retail`** (read-only `$CS_GAME_DIR`), plus ordinary
build/test. No `gpu`, `audio`, `human_play`, `human_review` or `network_real`
was used or needed. Evidence report:
`private/evidence/F38-B/acceptance.json`, committed as
`docs/findings/evidence/F38-B.json`.

## Files and the one observable failure (listed before editing)

- `crates/cs_formats/src/script_raw/ui_host_calls.rs` (new): `DispatchForm`,
  `ArgShape`, `ArgShapeCounts`, `HostCallSite`, `OtherCallHead`, `UiScriptLimits`,
  `UiScriptError`, `UiProgramScan`, `ObservedHostCall`, `CorpusMember`,
  `HostCallCorpus`, `scan_ui_program` and `measure_host_call_corpus`. The
  bounded, fail-closed measurement of the two native dispatch forms the shipped
  UI script programs contain, and the corpus aggregation that names every
  measured family with its provenance.
- `crates/cs_formats/src/script_raw/mod.rs`: module declaration and re-exports,
  plus one doc paragraph (wiring only).
- `crates/cs_script/src/bindings/observed.rs` (new): `MeasuredShape`,
  `MeasuredForm`, `MeasuredCallRow`, `RowError`, `MeasuredCall`,
  `UnimplementedReason`, `ObservedDisposition`, `ObservedFamily`,
  `ObservedCoverage`, `CoverageError`, `ObservedBindingTable` and
  `register_measured`. The measured rows crossing into the engine, the family's
  disposition, and the coverage gate.
- `crates/cs_script/src/bindings/differential.rs` (new): `DeclaredStep`,
  `TraceStepKind`, `NormalizedStep`, `NormalizedTrace`, `ScenarioRef`,
  `TraceError`, `TraceDivergence`, `TraceComparison`, `normalize_declared`,
  `normalize_emitted` and `compare_traces`. **AC02**.
- `crates/cs_script/src/bindings/mod.rs`: the two module declarations, the
  `measured` re-export with the crossing table, one doc paragraph, and
  `PartialEq` on `HostBindingRegistry` (wiring only).
- `crates/cs_script/tests/accept_f38_b_observed_bindings.rs`,
  `crates/cs_script/tests/accept_f38_b_retail_host_call_corpus.rs` and
  `crates/cs_script/tests/evidence_report_f38_b.rs`: the 12 `accept_f38_b_*`
  tests and the evidence harness.
- `crates/cs_script/Cargo.toml` and the root `Cargo.lock`: **test-only** dev
  edges on `cs_formats` and `cs_assets` (wiring only; `cs_script` still links
  `cs_types` only).
- This file and `docs/findings/evidence/F38-B.json`.

**One observable failure:** a measured host call that no binding claims must fail
validation before flight, with its source location, and must never become a
no-op. With `classify` in `observed.rs` changed to bind a family whose measured
argument shape the engine has no value for,
`accept_f38_b_no_measured_family_is_bound_and_the_gate_refuses` fails: the
coverage's `unimplemented` reason changes from `MeaningNotMeasured` to
`ArgumentShapeHasNoDomain`, so a family that should have been refused for want
of a measurement is reported as refused for want of an argument — and with the
whole gate removed,
`accept_f38_b_a_measured_call_no_binding_claims_fails_before_flight` fails
because `lower_program` would accept the call.

## Why the UI script programs and not the mission programs

F38-B needs *measured* host calls. The mission programs F13-B locates are **not
decoded**: F13-D has not run, the mission opcode table is unmeasured, and F13-C's
retail probe stops all 1452 located programs at their first counter
(`docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md`).
Reading a mission program here would mean guessing its layout, which is what
AGENTS rule 4 forbids.

The installation does ship one program family that needs no assumption at all:
the `ASSETS/SCRIPTS/*.SCRIPT` members of `GOSDATA/ASSETS/crimson.rof`, which are
**ASCII text**. 61 members decode with `trailing_len == 0` and a declared length
equal to the decoded one. This stage measures them and **nothing else**. It does
not claim these programs are the mission language, that a dispatch value means
the same thing in a mission program, or that the two families share a VM.

## What the measurement is

`scan_ui_program` walks the bytes with string literals respected (a `\` escapes
the next byte inside `"…"`, which the corpus relies on for `assets\\scripts\\`)
and recognizes exactly two native dispatch forms:

| Form | Spelling | Measured |
| --- | --- | --- |
| `callback` | `callback($$target$$, <call>, <arg>…)` | the `$$…$$` target's **class** (its name is never retained), the dispatch value when the expression is an integer literal, then each argument's class |
| `mail` | `mail(<message>, <recipient>)` | the dispatch value when the expression is an integer literal, then each argument's class |

`ArgShape` names **shapes**, never meanings: `IntegerLiteral` records that the
expression spells an integer, never what the integer is for. `Unevaluated` is
the honest answer for every form this slice does not evaluate — a parenthesised
expression, an arithmetic combination, an indexed path — and it carries no text.
No original statement, argument value or expression text leaves the installation:
`host-call-corpus.json` holds the container digest, per-program digests, per-family
ids/arities/shape-counts/spans and the head counts, and nothing else.

Everything is bounded and fail-closed under `UiScriptLimits`: a program over
`MAX_UI_SCRIPT_BYTES` (`script_too_large`), more than `MAX_HOST_CALL_SITES`
sites (`too_many_sites`), more than `MAX_HOST_CALL_ARGS` expressions in one call
(`too_many_arguments`), an argument expression over `MAX_ARG_EXPR_BYTES`
(`expression_too_long`), an unterminated call (`unterminated_call`) and
unbalanced blocks (`unbalanced_blocks`) are all refused rather than truncated,
and a refused program is never counted as a whole one. Both per-argument bounds
apply to **every** expression, the one before the closing parenthesis included,
so a bound enforced only at the commas would not leak the last argument
(reviewer correction 3 below).

## The dialect's `;` marker: measured, not assumed

The corpus contains `;` bytes outside string literals, and **whether `;`
introduces a comment in this dialect is not established**: `crates/cs_formats/src/text/dialect.rs`
records only `LexicalFeature::ScriptBlocks` for `TextDialect::UiScript`, with
`grammar: ClaimStatus::Unknown`, and
`docs/findings/2026-09-29-t351-keyed-list-reading-rules.md` refers to "the
`;`-comment marker of the `.SCRIPT` and `.H` dialects" without measuring `.SCRIPT`.

The scanner therefore **assumes no comment rule**: text after a `;` is scanned
like any other text, and the exposure is **counted** instead —
`UiProgramScan::semicolon_bytes`, `heads_after_semicolon`,
`sites_after_semicolon` and `braces_after_semicolon`, summed by
`HostCallCorpus` and reported by `semicolon_exposure_free()`. Measured over the
installation:

| Question | Answer |
| --- | --- |
| `;` bytes outside a string literal | **264**, across **34** of the 61 programs |
| call-shaped heads after a `;` on the same line | **0** |
| measured sites after a `;` on the same line | **0** |
| braces after a `;` on the same line | **0** |
| `semicolon_exposure_free()` | **true** |

So every count in this finding holds under either reading: no site is hidden or
invented by the unmeasured marker, and the block accounting is unaffected. That
is a measured result about **this** corpus, not a claim that `;` is or is not a
comment; a corpus that spelled a call head after a `;` would report
`semicolon_exposure_free() == false` and its counts would have to be
re-measured before use.

`measure_host_call_corpus` aggregates over members into one `ObservedHostCall`
per (dispatch form, dispatch value), each carrying `ScriptEvidence` at
`structural_decode` / `observed_tool` with a `container_span` locator at the
first measured site and a note that says in words that **no meaning is claimed
for the id**. Sites whose dispatch expression is not an integer literal are
counted in `sites_without_native_id` and never folded into an id.

## The retail corpus (`$CS_GAME_DIR`, read-only)

Read through production readers: `cs_formats::rof::read_tree` / `read_member`
over `GOSDATA/ASSETS/crimson.rof`, then the production scanner.

| Count | Value |
| --- | --- |
| UI script programs measured | **61** |
| host-call sites | **1812** |
| … spelling an integer dispatch value | **1698** |
| … whose dispatch expression is another expression | **114** |
| distinct `callback` families | **384** |
| distinct `mail` families | **90** |
| families total | **474** |
| call-shaped heads outside the two forms | **3188** |
| `;` bytes outside a string literal | **264** (34 programs) |
| measured families **bound** to an engine operation | **0** |
| coverage `complete` / `campaign_ready` | **false** |

The second-to-last row is the point of this stage, not a shortfall in it. Every
number is unchanged by the reviewer corrections below: the corrected scanner
produces byte-identical counts on this corpus.

## The family table and the coverage gate

`ObservedBindingTable::measure` validates every measured row and records one
family per row with an explicit disposition:

- `Bound { spelling, lowering }` — the family lowers to an engine operation and
  is registered under its **derived** spelling (`callback#2138`, `mail#10001`).
  A derived spelling is not an original symbol: the corpus contains the form and
  an integer and no name. This spelling is built only from measured facts and is
  stable and unambiguous.
- `Unimplemented { reason, evidence }` — measured, counted and **refused before
  flight**. Never replaced by a stub that reports success (F38 non-negotiable #2).

`classify` is the only place a measured family could become a binding and it
binds nothing today, because no rule can honestly map these families onto an
engine operation: the corpus's families carry presentation, dialogue and control
semantics that neither the mission IR nor the contract's argument domains can
express, and no original observation states what any of them *does*. Two refusal
reasons, both named:

| Reason | When |
| --- | --- |
| `ArgumentShapeHasNoDomain { position, shape }` | an argument's measured shape has no engine value standing for it — a member reference, a widget-class reference, an index path or any unevaluated expression |
| `MeaningNotMeasured { detail }` | the shapes are measurable but no observation states what the dispatch value does |

`MeasuredCall::arg_domain` is deliberately conservative and its limits are the
point: an `IntegerLiteral` becomes an unrestricted `IntRange` (the corpus's
integer arguments are literals of unknown meaning, and inventing a narrower
domain would be a guess), a `StringLiteral` becomes a `Str` capped at the named
safety bound `MAX_MEASURED_STRING_BYTES`, and every other shape has **no**
domain. The two caps that do exist (`MAX_MEASURED_STRING_BYTES`,
`MAX_EVIDENCE_BYTES`) are **safety bounds, not measurements**: the corpus's
string arguments and evidence summaries have not been measured for length, and
each constant says so where it is declared.

`ObservedCoverage::campaign_ready` is AC04's rule applied to this batch: while a
single measured family is unimplemented, the corpus is not covered and nothing
downstream may call itself complete. With the retail measurement that is 0 of
474. A corpus that measured **nothing** also refuses: an empty family set has
nothing unimplemented only because nothing was looked at, and answering
`campaign_ready` for it would fail open on the input most likely to be wrong
(reviewer correction 1 below).

## The differential trace (AC02)

`differential.rs` compares two normalized traces of **the same measured
scenario**:

- the **original** trace is what a measured original program *declares* — the
  dispatch values it spells, in the order the corpus measured them;
- the **recreated** trace is what the recreated engine *emits* — the
  `cs_script::runtime::MissionEvent`s its own runtime produced for the program
  lowered out of that same measurement.

Both normalize to `NormalizedStep`s, dropping everything that cannot be compared
(program counters, byte offsets, session and tick numbers) and keeping the
dispatch value, the form and the ordering. `compare_traces` then reports
`agrees()`, `matched`, both lengths and the first `TraceDivergence`
(`Missing`, `Unexpected` or `Mismatched`) at a named index, with
`divergences()` listing every one rather than only the first.

Three properties make it a comparison and not a report:

1. **The scenario must match.** Two traces of different scenarios are refused
   (`ScenarioMismatch`) and an unnamed scenario is refused (`NoScenario`), so a
   "differential" cannot compare two unrelated programs and report agreement.
2. **An unattributable event is its own kind.** `TraceStepKind::Unattributed`
   is a distinct variant rather than a sentinel id, so an engine event that
   cannot be traced back to a measured call can never compare equal to a
   declared step — **not even one whose own dispatch value was never measured**.
   (A first draft used `-1` as the placeholder and the test caught it: the
   placeholder equalled a genuinely unmeasured declared id.)
3. **The two orders are genuinely different.** F37-D measured that execution
   order is declaration order and observation order is `EventKey` (source
   symbol), and that the two are not each other's sort. The AC02 test builds the
   same program **declared in reverse** and asserts both that the runtime still
   reports events in `EventKey` order and that the comparison still agrees. A
   normalization that had followed declaration order would not agree, which is
   what makes the check discriminating rather than self-fulfilling.

### What the AC02 test does and does not exercise

Stated plainly, because the sheet asks a reviewer to inspect runtime wiring and
shortcuts:

- **Production code on both sides.** The original side is the production scanner
  (`scan_ui_program`) over authored bytes in the measured dialect: the declared
  dispatch values, their order and their spans are measured, not typed in. The
  recreated side is the **real runtime** — `MissionState::new` / `::step` /
  `MissionProgram::validate` — and the events are the ones it emitted, in its
  own `EventKey` order.
- **The recreated program is an authored stand-in, not a lowering.** No measured
  family lowers to an engine operation (0 of 474), so no real lowering of a UI
  script program exists to compare against. The test builds one `MissionProgram`
  whose objectives each carry one `GrantReward`, one per declared step, and
  supplies the origin mapping the emitted events back to their declared step.
  The differential machinery is therefore exercised end to end; the *lowering*
  is not, and nothing in the code or docs claims it is.
- **AC02 cannot be run against the retail corpus today, and that is stated.**
  The retail side of AC02 (scan → declared steps) is production code and is
  measured, but the recreated side needs a binding per measured family, and no
  family has a measured meaning. Until one does, an AC02 comparison against
  retail would be theatre, so the retail test measures the corpus and the gate
  refuses; the differential is exercised on authored text in the measured
  dialect. The retail scenario is F38-C/F38-D work.

## Design decisions

- **`cs_script` may depend on `cs_types` only** (`docs/01-ARCHITECTURE.md`), so
  it cannot name `cs_formats`' types. The measured row crossing the boundary is
  `MeasuredCallRow`, and the shape vocabulary is a **wire code** both crates
  publish: `ArgShape::code()` / `ArgShape::from_code` on one side,
  `MeasuredShape::code()` / `MeasuredShape::from_code` on the other, with the
  full crossing table in the `cs_script::bindings::measured` doc comment. A code
  this build does not know is refused on **both** sides rather than read as a
  different shape, and a test asserts the two vocabularies agree on every code
  and label.
- **`MeasuredShape` mirrors the wire codes rather than restating them.** An
  engine-side enum is needed to name a shape in `ArgDomain`; the alternative
  would be stringly-typed domains, which would not be checked.
- **The two forms put their expressions in different orders** and the difference
  is measured, not normalized away. A site whose first expression does not fit
  its form's shape is still measured: the dispatch value is `None` and the
  expression is recorded as an argument, so a variant of the form cannot be
  silently reshaped into the common one.
- **The batch's boundary is a measurement.** Every other call-shaped head
  (`initialize`, `getmessage`, `script_run`, `mail`-adjacent helpers — 3188
  sites) is counted in `other_call_heads` rather than ignored, so "what this
  batch does not cover" is a number.
- **A shape is a shape, not a domain.** `arg_domain` refuses to invent one; this
  is why `mail#200`, whose recipient is a named reference, is refused at
  position 0 while `callback#100`, whose argument is an integer literal, is
  refused for its *meaning*.
- **Two sites that disagree are a disagreement, not a tie.** `ArgShapeCounts`
  exposes the class counts and `is_uniform()`; a consumer hands `None` for a
  position that is not uniform and the row is refused by name, rather than one
  shape being picked. `dominant()` exists for reporting and ties resolve to the
  first shape in the fixed `ArgShape::ALL` order, so the answer never depends on
  iteration order.
- **No `cs_content::script_adapter` module was added.** It is an owner path, but
  the adapter this stage measured produces engine-facing *measurements*, and
  `cs_content` may not depend on `cs_script` — a module there could not hand its
  rows to the family table. The crossing happens at the consumer, which is where
  F38-A's finding already put it.
- **`register_measured` takes the `Lowering` from its caller**, never from this
  crate. A binding whose meaning has not been measured must not be given one
  here; the function exists so the path a bound family takes is production code
  rather than a test-only shortcut, and the test exercises it.

## Test inventory (`accept_f38_b_*`)

`crates/cs_script/tests/accept_f38_b_observed_bindings.rs` (12 unignored, authored
synthetic programs in the measured dialect) and
`crates/cs_script/tests/accept_f38_b_retail_host_call_corpus.rs` (1 retail):

| Test | Covers |
| --- | --- |
| `a_measured_call_no_binding_claims_fails_before_flight` | **AC01 preserved**: the registry the measured table hands out is empty, and a program referencing `callback#100` is refused with its name, its source span and its site |
| `no_measured_family_is_bound_and_the_gate_refuses` | coverage is 2 families / 3 sites / 0 bound; `complete` and `campaign_ready` are false; every family states a named reason and keeps its provenance |
| `argument_domains_cover_only_shapes_with_a_value` | `arg_domain` is total over `IntegerLiteral` and `StringLiteral` only and `None` for the other six; the string is capped; both crates agree on every wire code and label, and refuse an unknown code |
| `a_measured_row_is_validated_or_named` | all five `RowError` variants by value, and a corpus that measures one value twice yields no table at all |
| `the_measurement_counts_forms_and_refuses_a_broken_program` | 4 sites / 3 with an id; a `callback(...)` inside a string literal is not a site; the enclosing block label, the target reference's class and each argument's class; `initialize` counted as another head; unbalanced blocks, an unterminated call and a tight site bound all refused |
| `every_scan_bound_is_enforced_and_the_semicolon_is_measured` | both per-argument bounds on every expression including the last (`too_many_arguments`, `expression_too_long`), the `;` marker counted and everything spelled after one counted with it (`semicolon_exposure_free()` false for a program whose tail spells a head and a brace, true for the corpus) |
| `the_corpus_names_each_family_and_counts_the_rest` | per-family provenance (`structural_decode` / `observed_tool` / `container_span` / a note that says no meaning is claimed), the arity split between the two forms, and `sites_without_native_id` counted not dropped |
| `disagreeing_argument_shapes_are_refused_not_resolved` | two sites of one value with different argument shapes: the counts show the disagreement, `is_uniform` is false, `dominant` ties deterministically, and the row is refused by position — while the same row with agreeing sites is valid |
| `the_differential_trace_compares_original_and_recreated_order` | **AC02**: the normalized kinds are the measured dispatch values in source order; the recreated steps carry the runtime's own `(source, sequence)` event keys; the same program declared in reverse still agrees, and the runtime's report order is asserted to differ from its declaration order |
| `every_divergence_is_reported_at_its_index` | `Mismatched`, `Missing` and `Unexpected` each at their own index with both calls named; `divergences()` lists two; `ScenarioMismatch` and both `NoScenario` refusals |
| `an_unmeasured_dispatch_value_surfaces_as_an_extra_step` | an event with no measured origin becomes `Unattributed` and never compares equal to the declared step |
| `a_measured_family_with_an_operation_registers_and_binds` | the bound path through production code: a derived spelling registers with `Observed` provenance, `lower_program` lowers the call, a wrong argument type is still refused with its span, and a family with no domain for a measured shape cannot be registered at all |
| `retail_ui_script_host_calls_are_measured_with_provenance` | **`$CS_GAME_DIR`**: the four pinned totals, both family counts, `other_call_sites`, the `;` exposure (264 markers over 34 programs, nothing after one), per-family provenance over every family, per-program spans inside the program, the wire-code agreement over every shape the corpus produced, every row either valid or refused by name (and the counts add up), and the gate refusing |

`cargo test --workspace --locked -- accept_f38_b_ --include-ignored` discovers
and executes **13** tests (12 unignored + 1 retail), all passing; each also
passes when run alone with `--exact`.

## Reviewer corrections (review claim of 2026-10-03, agent `bunny-alpha-1`)

The review ran under the **same agent identity as the implementer**, so it is
**not independent evidence** (see the recorded identities in the task's review
notes). It found and fixed:

1. **A fail-open coverage gate.** `ObservedCoverage::complete()` was
   `unimplemented_families == 0`, so an *empty* corpus — one that measured
   nothing at all — answered `campaign_ready() == true`. The gate now requires a
   non-empty family set as well, and
   `accept_f38_b_no_measured_family_is_bound_and_the_gate_refuses` pins it for
   both `ObservedBindingTable::new()` and `measure(&[])`.
2. **An argument-count bound that leaked its last argument.** `arguments()`
   checked `args.len() > max_args` only at commas, so `mail(1, 2, 3)` passed a
   bound of 2: the final expression was pushed at the closing parenthesis without
   a check. Both per-argument bounds now run through one `push_argument`, so the
   last expression is bounded like every other.
3. **A bound that was documented but never enforced.**
   `UiScriptLimits::max_expr_bytes` was read by nothing and
   `UiScriptError::ExpressionTooLong` was unreachable, while the findings and the
   type docs claimed the scanner refused an over-long expression. It now refuses,
   and `classify`'s dead over-long branch is gone because the bound is checked
   before classification.
4. **An unexamined lexical risk in the corpus.** The corpus has 264 `;` bytes
   outside string literals and the scanner walked every one of them as code. The
   marker is not established as a comment, so skipping it would have been a
   guess; the scanner now **counts** the marker and everything spelled after it
   (heads, sites, braces) and reports `semicolon_exposure_free()`. Measured over
   the installation: nothing follows a `;`, so every pinned count is unchanged
   and is now shown to be unchanged *for a reason*.
5. **An unbounded untrusted string crossing into provenance.**
   `RowError::NoEvidence`'s doc promised "or an over-long summary" and no such
   check existed. `MAX_EVIDENCE_BYTES` now enforces it, and the measured string
   domain uses a named `MAX_MEASURED_STRING_BYTES` instead of `MAX_CALL_ARGS * 8`
   with a justification ("the largest string a binding may carry") that no
   constant actually supports.
6. **Claims slightly stronger than the code.** `differential.rs` said the
   recreated trace came "for the program lowered out of that same measurement";
   no such lowering exists (0 of 474 families bound). The docs now say the caller
   supplies the lowering, the test says what its stand-in is, and the section
   "What the AC02 test does and does not exercise" states plainly that AC02
   cannot yet be run against the retail corpus.
7. **Small cleanups a reviewer owes the next reader.** The dead public
   `EmittedStep` is removed (`NormalizedStep` already carries the same fields and
   nothing used it); `CoverageError::RegistryRefused` is documented as reserved
   for the bound path rather than looking like live behaviour; a doc link to
   `docs/findings/2026-10-03-f38-b-observed-host-call-corpus.md`, which does not
   exist, points at this file; and an `is_some_and(|()| true)` is an `is_some()`.

The retail test's pinned numbers (61 / 1812 / 384 / 90 / 3188) and the new `;`
numbers (264 / 34 / 0 / 0 / 0) all pass unchanged after the corrections, and the
evidence report was regenerated on the reviewed commit with the reviewer identity
stated in it.

## Mutation probes

Each mutation was applied, the task selection run, and the file restored:

- `mail` sites no longer scanned in `ui_host_calls.rs` → 6 of the 11 synthetic
  tests fail, and `accept_f38_b_retail_ui_script_host_calls_are_measured_with_provenance`
  fails at the pinned mail-family count;
- `MeasuredCall::arg_domains` given a domain for a shape that has none →
  `accept_f38_b_no_measured_family_is_bound_and_the_gate_refuses` fails;
- `compare_traces`' scenario-mismatch check disabled →
  `accept_f38_b_every_divergence_is_reported_at_its_index` fails;
- `normalize_emitted` reversing the emitted observation order →
  `accept_f38_b_the_differential_trace_compares_original_and_recreated_order` fails.

A fifth probe — re-sorting the emitted events by `(source, sequence)` — did
**not** fail anything, which is how the test was found to be weak: with every
objective declared in symbol order, that sort is a no-op. The AC02 test now also
declares the same program in reverse order, where the sort would change the
trace, and the probe then fails the same test.

### Reviewer's probes (one per correction, each applied and restored)

| Mutation | Test that failed |
| --- | --- |
| `ObservedCoverage::complete()` back to `unimplemented_families == 0` | `no_measured_family_is_bound_and_the_gate_refuses` ("an empty measurement is never campaign-ready") |
| the argument-count check removed from `push_argument` (comma-only, as before) | `every_scan_bound_is_enforced_and_the_semicolon_is_measured` — `mail(1, 2, 3)` scanned clean at `max_args: 2` |
| `semicolon_bytes += 1` removed from the scan | the **retail** test, `left: 0, right: 264` |
| the `max_expr_bytes` check removed from `push_argument` | `every_scan_bound_is_enforced_and_the_semicolon_is_measured` — an 8-byte argument scanned clean at a bound of 4 |
| the `MAX_EVIDENCE_BYTES` check removed from `from_row` | `a_measured_row_is_validated_or_named` — a 1025-byte summary was accepted |

## Recorded unknowns (not guessed)

- **No measured dispatch value has a known meaning.** The corpus spells
  integers; the meanings live in the packed executable (`crimson.icd`'s
  `.text`/`.data` are packed at entropy ≈ 7.9). No original run was observed, so
  **0 of 474** families is bound and the coverage gate refuses. A binding that
  returned success would fabricate original behaviour (F38 non-negotiable #2).
- **No measured family has a measured argument domain.** A member reference, a
  widget-class reference, an index path and any unevaluated expression have no
  engine value standing for them, so such a family is refused at that position
  rather than having the argument dropped.
- **The mission-language host calls are not measured here and not claimed.**
  F13-D has not run: the mission opcode table is unmeasured, all 1452 located
  programs stop at their first counter, and the reader-archive programs this
  stage does not read (`objectives.zrd`, `targets.zrd`, `aiv.zrd` and their
  siblings) are still located only by name.
- **These programs are the UI script family, not the mission language.** Nothing
  asserts that a measured dispatch value means the same thing in a mission
  program, or that the two families share a VM.
- **Cancellation semantics and repeatability are unknown.** `Repeatability` is
  recorded per binding and no measured family has one yet; F38-A already recorded
  that the runtime does not enforce it.
- **The 3188 other call-shaped sites are counted, not measured.** Their forms
  (`initialize`, `getmessage`, `script_run`, …) are named but not scanned; what
  they do is unknown. `script_run` in particular carries an apparent entry-point
  argument, which may or may not be a dispatch value.
- **Whether `;` introduces a comment in this dialect is unmeasured.** 264 such
  bytes exist outside string literals and the scanner counts rather than assumes
  what follows them; on this corpus nothing does, so the measurement is the same
  either way. The rule itself stays unknown — closing it needs the original
  running or a code reading of the packed image, neither of which exists here.
- **A dispatch value's runtime behaviour is unknown.** What it does, when it
  runs, what it does to the world, needs an owner-supplied original run
  (Rally #358 `REF-OWNER-FIRST-CAPTURE`). No original run was observed.
- **The report's `unknowns` array is empty by design.** That array is where an
  evidence run's own unresolved issues go — a defect that makes the run
  untrustworthy — and this run has none: the container decoded, every program
  scanned, every site accounted for, every family classified, digests taken from
  the installation this run read. Everything this task does *not* know is stated
  in full in the report's `review.method`, in `host-call-corpus.json` (which
  carries `bound_families: 0` and `coverage_complete: false` in the artifact
  itself) and in this file. F13-C's committed report uses the same split.
- **No original run was observed and nothing here is `verified_original`.** The
  claim is `implemented`.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f38_b_ --include-ignored
mkdir -p private/evidence/F38-B
cargo test --workspace --locked -- accept_f38_b_ --include-ignored \
  2>&1 | tee private/evidence/F38-B/cargo-test.log
CS_EVIDENCE_DIR=private/evidence/F38-B \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_f38_b_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 \
  cargo test --locked -p cs_script --test evidence_report_f38_b -- --ignored
python3 tools/validate_evidence.py private/evidence/F38-B/acceptance.json \
  --artifact-root private/evidence/F38-B --require-pass
```

## Sources

`specs/F38-original-program-adapters-and-native-behavior-bindings.md`,
`specs/F13-mission-language-discovery-and-compatibility-closure.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the F38-A
finding (the empty registry and the raw-call boundary), the F13-B and F13-C
findings (the located programs, the unmeasured opcode table and the reachability
probe), the F37-A/C/D findings (the IR, the `EventKey` order and the two orders
this task's differential trace relies on), the F22-H finding (the earlier
observation that the control scripts reach native callbacks by id), and the
read-only `$CS_GAME_DIR` listing.
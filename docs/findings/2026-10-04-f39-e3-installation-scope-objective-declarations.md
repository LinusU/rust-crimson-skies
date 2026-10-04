# F39-E3: what the installation-scope readers declare, and what it does to F39-D's denominator

Date: 2026-10-04. Task: F39-E3 "Extend the objective-record census to the shared
and world-group readers" (Rally #597; the sheet
`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md` has no
`### F39-E3` section, so the task-test prefix this stage uses is
`accept_f39_e3_`, stated here as the sheet states a prefix per stage). Shared
contract: `docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail`
(read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/F39-E3/acceptance.json`, committed as
`docs/findings/evidence/F39-E3.json`.

**This finding supersedes `docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`
unknown #5** (the census's denominator), and its own unknown #5 now carries a
pointer here.

## The question inherited

F39-D's census (`cs_app::objectives::survey_retail_objective_records`) walks
**mission-scoped** reader archives only — `zbd/<group>/<mission>/zrdr.zbd`, F13-B's
own `mission_scope` rule — and measured 53 archives, 1338 numbered `OBJECTIVE<N>`
blocks, 55 keys, 5477 occurrences. It recorded as unknown #5:

> The census's denominator. Mission-scoped archives only. The shared reader
> (`ZBD/zrdr.zbd`, 220 members) and the world-group readers are outside it, so a
> mission may inherit objective declarations this census does not see.

Every F39-D number, and every number derived from it since (F39-E1's dormant
count, F39-E2's precedence reading, F39-E4/E5's surfaces), is bounded by that.
This stage measures the complement and says which of those numbers the
complement changes.

## The short answer, in two parts

The installation-scope readers — the install-wide reader `ZBD/zrdr.zbd` and the
eight world-group readers `ZBD/<group>/zrdr.zbd` — were read **in full**: 9
archives, **612 declared members (381 distinct names), every one decoded**.

* **Numbered objective blocks: 0.** Not one installation-scope member declares an
  `OBJECTIVE<N>` block. F39-D's mission-scoped denominator is therefore
  **complete** for that surface: 1338 blocks is the whole installation's, not a
  share of it. This is the answer that makes the census's published figures safe
  to quote.
* **Objective target records: 5, outside the mission-scoped denominator.** The
  `c1c` **world-group** reader declares a `targets.zrd` with five objective target
  records — and `c1c` is the world group of `c1c/m01`, the **one** campaign
  mission that declares no `targets.zrd` of its own. So for the F39-E4 target
  surface the mission-scoped denominator is *not* the whole story, and the row
  F39-E4 reports as "this mission's objective kinds are unmeasured" is bounded by
  a record that lives one directory up.

The inheritance itself — whether a mission resolves a member of its world group's
or the install-wide reader — is **reader-archive precedence** (F04/F06) and was not
measured here. What is measured is that the declarations exist in one archive and
not in the other.

## Files (listed before editing)

* `crates/cs_content/src/catalog/reader_dirs.rs`:
  `classify_installation_scope` (new, public; the one definition of the two
  installation-scope rules, which `classify` now calls instead of repeating
  them), `WORLD_GROUP_MEMBERS`, `SHARED_READER_MEMBERS`, and two unit tests.
* `crates/cs_app/src/objectives.rs`: the F39-E3 section —
  `MeasuredScopeMember`, `measure_scope_member`, `ScopeObjectiveCensusError`,
  `RetailScopeObjectiveRow`, `RetailScopeObjectiveCensus`,
  `survey_retail_scope_objective_records`, `scope_label`, the private
  `collect_objective_spellings` / `scope_member_keys` readers, the
  `OBJECTIVE_SPELLING_NEEDLE` search rule, and the two bounded doc comments on
  `RetailObjectiveRow::target_kinds` /
  `RetailObjectiveCensus::missions_without_targets` below.
* `crates/cs_app/tests/campaign/f39_e3.rs` (new): 6 tests, prefix
  `accept_f39_e3_` (4 unit + 2 retail).
* `crates/cs_app/tests/campaign/f39_e3_evidence.rs` (new): the evidence
  harness, plus two synthetic tests that pin the report's measured numbers in
  their own clauses.
* `crates/cs_app/tests/campaign/main.rs`: the two module declarations and the
  doc paragraph that says why both members live in the shared `campaign` test
  binary (one fewer link on CI's disk-bound runners).
* `docs/findings/2026-10-03-f39-d-branching-optional-and-failure-validation.md`:
  unknown #5 gains a supersession pointer (one paragraph).
* Wiring only: none — every edited file is an owner path.

**The one defect this stage found.** F39-E4's census publishes
`missions_without_targets()`, and both it and the `target_kinds` field doc said
only that the *mission archive* declares no target record. That statement is true
and was not wrong, but a reader of it had no way to know that the world group of
that same mission declares five objective target records of its own — so a
measurement that read as complete was in fact bounded by an unmeasured
inheritance. Both doc comments now say so, name the scope census and name what
would have to be measured next (precedence). No number changed: the absence is
still measured, in the archive it is measured in.

## The measurement

`survey_retail_scope_objective_records` walks the **non-mission-scoped** `zrdr.zbd`
archives — the exact complement of F39-D's walk, under the same
`mission_scope` rule — opens each with the F06 two-key dispatch, classifies it
with F14-D.1's own member rules, and reads every member with the production `.zrd`
decoder.

| scope | role | declared members | distinct names | objective blocks | objective target records |
| --- | --- | --- | --- | --- | --- |
| `zbd/c1` | world group | 70 | 70 | 0 | none (`no targets.zrd`) |
| `zbd/c1b` | world group | 29 | 29 | 0 | none |
| `zbd/c1c` | world group | 28 | 28 | 0 | **5** |
| `zbd/c2` | world group | 70 | 70 | 0 | none |
| `zbd/c2b` | world group | 25 | 25 | 0 | none |
| `zbd/c3` | world group | 56 | 56 | 0 | none |
| `zbd/c4` | world group | 57 | 57 | 0 | none |
| `zbd/c5` | world group | 56 | 56 | 0 | none |
| `zbd` | **shared** | **221** | **220** | 0 | none |
| **total** | 9 archives | **612** | **381** (union) | **0** | **5** |

The per-archive distinct counts are that archive's own; the census-wide 381 is a
**union**, not their sum (611), so a member name two world groups share — `ai.zrd`
in the shared reader, `fogvol.zrd` in most groups — is one name. The shared reader
declares 221 members and 220 distinct names because it lists `player.zrd` twice —
the same fact F14-D.1's `ClassifiedReaderDir` documents, now measured again
through the census.

Neither census refuses anything: every archive classified, every member decoded,
and `zbd` + 8 groups + 53 mission archives = **62 reader archives**, which is every
`zrdr.zbd` the installation holds. The two walks together are the whole
denominator.

### The complete objective-named spelling inventory

The negative result is drawn from a **search list**, not from a hunch: every text
node of every one of the 612 members — keys *and* values, at every depth — is
searched for a spelling containing `objective` (case-insensitively). The complete
inventory:

| spelling | occurrences | where |
| --- | --- | --- |
| `OBJECTIVESLIST` | 553 | `Briefing.zrd`, `escape.zrd`, `ia_escape.zrd`, `Loading.zrd` (install-wide) — a `PRIMITIVES` entry of a `*DIALOG` record |
| `Objective` | 337 | the same four members |
| `MSG_BRF_DLG_OBJECTIVES` | 4 | the same four members, once each, as a dialog `TEXT` value |
| `objective` | 1 | `ZBD/C1C/zrdr.zbd`'s `targets.zrd` — one target record's own key |

895 occurrences, four distinct spellings. **The three dialog spellings occur in no
mission-scoped reader at all**: no mission archive carries `Briefing.zrd`,
`escape.zrd`, `ia_escape.zrd` or `Loading.zrd` (measured, one carrier each, the
install-wide reader). That is what "a mission may inherit them" means in the
files — inherited, not duplicated — and it is exactly as far as the reading goes:
whether a mission resolves an install-wide member at all is precedence.

For contrast, the same search over the 53 mission-scoped readers finds **119**
distinct objective-named spellings — the 1338 blocks' own `OBJECTIVE<n>` names
(`OBJECTIVE1` … `OBJECTIVE108`), the branching family (`WAKE_…` 412,
`NAP_…` 417, `KILL_…` 225, `WAKEUP_…` 2), `ADD_OBJECTIVE_TARGET` 76,
`REMOVE_OBJECTIVE_TARGET` 153, `OBJECTIVE_DELAY` 2, `objective` 92,
`objective_numbers` 20, `newplane_objective` 4, `pickup_objective` 19 — and
**none** of the three dialog spellings.

### The five objective target records

`ZBD/C1C/zrdr.zbd`'s `targets.zrd` (1020 bytes), read through F39-E4's own surface
(`measure_target_kinds`):

| record | `description` | `nodes` | `help_label` | `category_label` |
| --- | --- | --- | --- | --- |
| 0 | `MSG_OBJ_KLONDIKE` | `piratezep`, `rock_zeppelin` | `MSG_OBJ_DEFEND` | `MSG_OBJ_ZEPPELIN` |
| 1 | `MSG_OBJ_WVOYAGE` | `workersvoyagezep` | `MSG_OBJ_DISABLE` | `MSG_OBJ_ZEPPELIN` |
| 2 | `MSG_OBJ_WVOYAGEHOOK` | `wv_tailhook`, `peoplehook` | `MSG_OBJ_DOCK` | — |
| 3 | `MSG_OBJ_DARKANGEL` | `blackswanzep` | `" "` | `MSG_OBJ_ZEPPELIN` |
| 4 | `MSG_OBJ_KLONDIKEHOOK` | `pzhookpoint` | `MSG_OBJ_DOCK` | — |

5 records, 5 labelled, labels `MSG_OBJ_DOCK` 2, `MSG_OBJ_ZEPPELIN` 3,
`MSG_OBJ_DEFEND` 1, `MSG_OBJ_DISABLE` 1, `" "` 1, and the record keys
`description` 5, `nodes` 5, `help_label` 5, `category_label` 3, `objective` 1.

Two facts make this more than a curiosity. First, `c1c/m01` declares **no**
`targets.zrd` of its own, so the mission F39-E4 reports as unmeasured is in the
world group that declares these. Second, record 2 names the node `wv_tailhook`,
and `zbd/c1c/m01/zrdr.zbd` carries a `wv_tailhook.zrd` member — the group's record
names a node the mission places. That is what an inherited objective target would
look like; **that the original resolves it that way is an inference from the
files, not a measurement**, and no original executable was run.

One near-miss is recorded so it is not re-investigated: `zbd/c2/zrdr.zbd` carries
a member named `game_targets.zrd`, whose root is an `ANIMATION_DEFINITIONS` record
(`race_target*` / `target*_rebound` / `bullseye`). The name contains `targets` and
the spelling search finds nothing objective in it. This is why the census reads
objective target records from a member named `targets.zrd` **only**, exactly as
F39-E4's census located them: `objective_record_count` counts a root's children, so
reading any member's root as a record list would report two "objective records"
for an animation table.

### Corroboration with F39-E6 (independent, on main)

F39-E6 landed on `main` while this stage was working and walked the same
complement for a different question: `survey_excluded_objective_records` measures
every member of every archive `mission_scope` does not name, plus every
`targets.zrd` member of every reader archive. Its table
(`docs/findings/2026-10-04-f39-e6-repeated-completion-effect-key.md`) reports
**9** archives outside mission scope, **612** members decoded there, **53**
`targets.zrd` members (52 mission + the `zbd/c1c` world-group one), **332**
objective records, and — the same zero this stage is here for — *excluded members
declaring an `OBJECTIVE<N>` block: **0***.

Two independent implementations, one installation fingerprint, and the same
figures for the scope denominator, the member count and the block count. What
stays this stage's is the *decision* F39-E6 does not make: that the
mission-scoped denominator is therefore the right one for the objective-block
surface; the complete objective-named **spelling** inventory (F39-E6 scans only
the branching and order spellings, so it never sees `OBJECTIVESLIST`); the
`MSG_OBJ_*` label reading of the five `c1c` records; and the statement that the
dialog members are absent from every mission-scoped reader. F39-E6's
`targets.zrd` vocabulary reading (`category_label`, `description`, `help_label`,
`nodes`, `objective`, `other_target`) is the same six-key set this stage measured
for `c1c` minus `other_target`, which that record does not carry.

## The verdict

1. **F39-D's denominator is right, and now bounded.** For numbered objective
   blocks the mission-scoped census measures the whole installation. Its
   published figures (53 readers, 1338 blocks, 55 keys, 5477 occurrences,
   1091 branching / 1465 optionality / 24 outcome sites) need no widening, and
   the reason is measured rather than assumed.
2. **F39-E4's target surface is not bounded by its denominator.** One
   world-group reader carries five objective target records, and the one mission
   whose own archive carries none belongs to that world group. Its
   `missions_without_targets()` row is correct and is now labelled as the
   mission-archive reading it is.
3. **The precedence question is the next measurement, and this stage does not
   answer it.** Whether a mission resolves its world group's or the install-wide
   reader's member — and in which order — belongs to F04/F06's resolution order.
   Nothing here observes the original, so nothing here claims it.
4. **No original mission is played by this stage.** Nothing changed in the
   support gate: an `Installation` record stays refused by name with
   `UNMEASURED_OBJECTIVE_SEMANTICS`. The scope census is a measurement.

## Test inventory (`accept_f39_e3_*`)

`crates/cs_app/tests/campaign/f39_e3.rs` (4 unit + 2 retail)
and `crates/cs_content/src/catalog/reader_dirs.rs` (2 unit):

| Test | Covers |
| --- | --- |
| `accept_f39_e3_an_installation_scope_reader_is_decided_by_its_own_members` | the one scope predicate: both roles, the per-mission-member exclusion, `mis_anim.zbd`, undecided sets |
| `accept_f39_e3_the_scope_predicate_is_the_one_the_campaign_walk_uses` | the campaign walk and a direct caller reach the same role |
| `accept_f39_e3_a_scope_member_measures_blocks_targets_and_every_spelling` | `measure_scope_member` on hand-built documents: blocks by F39-D's reader, target records by member name only, spelling inventory whole (nested, values included), key inventory a field-name inventory |
| `accept_f39_e3_a_walk_that_stops_early_is_refused_not_published_as_empty` | a document nested past the walk bound reports `truncated_nodes` instead of a partial inventory read as complete — the precondition of the census's `WalkTruncated` refusal |
| `accept_f39_e3_a_reader_carrying_an_objective_record_is_never_a_scope_reader` | the rule that keeps the two denominators disjoint, and that neither scope role is launchable |
| `accept_f39_e3_the_census_refuses_an_installation_it_cannot_read` | the census fails instead of reporting an empty denominator |
| `accept_f39_e3_installation_scope_readers_declare_no_objective_blocks` (retail) | the whole measurement: 9 archives, 612/381 members, 0 blocks, the complete 4-spelling inventory and its 895-occurrence reconciliation, the c1c target records and labels, the dialog members' absence from mission readers, 53 + 9 = 62 |
| `accept_f39_e3_the_mission_census_denominator_is_now_bounded` (retail) | one installation, one fingerprint, and `mission blocks + scope blocks == mission blocks` |

## Measured sensitivity (mutation probes, all observed)

Each probe was applied to the production code, the affected test run, and the
change reverted. All three are unit-level probes, so they were observed on the
fast path; the retail assertions depend on the same three rules.

* **P1 — `classify_installation_scope`'s per-mission exclusion removed** (a
  scope reader would be allowed to list `objectives.zrd`):
  `accept_f39_e3_an_installation_scope_reader_is_decided_by_its_own_members`
  fails on `classify_installation_scope(&with(&["templates.zrd",
  "cam_anim.zrd"]), false)` returning `Some`, and
  `accept_f39_e3_a_reader_carrying_an_objective_record_is_never_a_scope_reader`
  fails with *"objectives.zrd made a mission reader an installation-scope
  reader"*. (Observed; the second failure is the one that matters — it is the
  rule that keeps the two denominators disjoint.)
* **P2 — `measure_scope_member`'s target-record selection widened to every
  member** (`is_target_member = true`): the same unit test fails with
  `Some(MeasuredTargetKinds { records: 4, labelled: 0, names: {} })` against
  `None` — a member read as objective records whose labels are empty, which is
  exactly the `game_targets.zrd` failure mode the name restriction prevents.
  (Observed.)
* **P3 — the spelling search narrowed from a case-insensitive substring to
  `starts_with`**: the same unit test fails with an **empty** inventory against
  `[("MSG_BRF_DLG_OBJECTIVES", 1), ("OBJECTIVESLIST", 1)]` — the dialog's
  primitive and message id are both mid-string, so a prefix rule reports nothing
  where the substring rule reports both. (Observed. A case-insensitive *prefix*
  rule would keep `objective` and still lose the other two, so the substring
  form is what the measurement rests on.)

## Unknown / deferred (not guessed)

1. **Reader-archive precedence.** Which archive a mission resolves a member from
   — its own, its world group's, or the install-wide one — is F04/F06's
   resolution order and is not measured here. Everything in this finding that
   says "a mission may inherit" is a statement about where a declaration *is*,
   never about whether the original reads it.
2. **What any of the four spellings means.** `OBJECTIVESLIST` sits in a dialog
   record's `PRIMITIVES` list beside `BACKGROUND`, `TITLE`, `LIST`, `FONT`,
   `POSITION`, `BITMAP` and `WORDWRAP`, and `MSG_BRF_DLG_OBJECTIVES` is a message
   id beside the localized `ObjListTitle` font name — so that they are **dialog
   layout and message identifiers** is an inference from the surrounding record,
   strongly supported, and it is labelled as one. That the original shows a
   mission's objectives list through them, and in what state, is unmeasured.
3. **Whether the five `c1c` target records are ever objective targets.** Their
   record shape is the F39-E4 target shape and their labels are `MSG_OBJ_*`, so
   they read as objective targets; that the original loads them as such, and for
   which missions, is unmeasured.
4. **`objectives.zrd` inside a scope reader is not merely absent today.** The
   measured rule that keeps the denominators disjoint is "a scope reader lists
   none of `map.zrd` / `aiv.zrd` / `objectives.zrd`". It is a measurement of this
   installation, not a format law: an archive that listed one would be refused as
   `Unclassified` rather than silently counted, which is the behaviour this stage
   wants.
5. **The rest of the 612 members' key vocabulary** is published whole by
   `RetailScopeObjectiveCensus::keys()` and recorded in the evidence artifact
   (`scope-objective-census.json`): **2362 distinct keys** (2059 of them in the
   install-wide reader alone). The objective-relevant part is the five-key target
   vocabulary above plus the dialog vocabulary beside it; the remaining keys are
   animation, AI-manoeuvre, weather, terrain-template and dialog-layout field
   names that name no objective. They are listed in the artifact rather than
   transcribed here, because this stage's objective claim is bounded by the
   *spelling* inventory, not by the key inventory.
6. **The compiled program behind any of this** is still undecoded (F13-B/C,
   F38). No opcode was read.

## Review 2026-10-04

The submitted tree measured everything it claims, but review found the
**committed evidence report** misstating the measurement in its `review.method`
prose, and two census completeness guards worth hardening. All three fixes are
code and report only: **no measured number changed** — the defect was in the
rendering of the numbers, never in the reading of them.

* **`review.method` attached every right number to the wrong claim.**
  `review_method` interpolated its twelve values **positionally** into a long
  template, and `rustc` accepted the transposition silently: the committed
  report read "the 612 scope archives declare 381 members (612 distinct
  names)", "53 target records in 5 of the scope archives (1)", put the
  **target-label** list where the objective-**spelling** inventory belongs and
  the spelling list inside an unrelated clause. Every number came from the
  right call, and the schema passed anyway — no validator can see that failure.
  The renderer now reads the numbers into `MethodFacts` and binds every one as
  a **named** argument, and two synthetic tests pin each count inside the
  clause that says what it counts
  (`evidence_report_f39_e3_method_prose_carries_each_measured_number_in_its_own_clause`,
  `evidence_report_f39_e3_a_blank_target_label_still_renders_as_a_label` —
  `zbd/c1c` record 3's `help_label` is a single space, which an unbracketed
  join rendered as nothing at all). `docs/findings/evidence/F39-E3.json` was
  regenerated on the fixed tree.
* **The walk bound is now measured, not assumed.** Both inventories were
  already depth-bounded — as a stack backstop — but a walk that stopped early
  would have published a partial inventory as a complete search list, the one
  failure this census cannot survive. `MeasuredScopeMember` now reports
  `truncated_nodes`, the bound moved to 65 (one step past the decoder's own
  `MAX_ZRD_DEPTH`, so no decoded document can reach it), and the census turns a
  non-zero count into `ScopeObjectiveCensusError::WalkTruncated`.
  `accept_f39_e3_a_walk_that_stops_early_is_refused_not_published_as_empty`
  pins the mechanism the refusal reads.
* **The scope label is derived, not written down.** `scope_label` returned
  `"zbd"` for every archive; it now takes the root from the archive's own key,
  so a `zrdr.zbd` under another root can never borrow the shared reader's
  name.

The acceptance selection now discovers **8** `accept_f39_e3_` tests; the two
prose-pinning tests are synthetic, unprefixed, and run in CI.

A second review pass the same day (Devin SWE-2/swe2-max-1, fresh context)
folded the two new test files into the shared `campaign` test binary as
`f39_e3.rs` / `f39_e3_evidence.rs` — the convention `evidence.rs` and
`f50_e4.rs` already set — because each standalone test target links a full
`cs_app` binary and the CI runners' disk could not afford one more link
(`docs/findings/2026-09-30-t430-rust-lld-sigbus-in-ci.md`). The selection, the
tests and the harness invocation by test name are unchanged; this file's
paths and command list were updated to match, and `docs/findings/evidence/F39-E3.json`
was regenerated on the final tree.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f39_e3_ --include-ignored
cargo test --locked --test campaign evidence_report_f39_e3 -- --ignored
python3 tools/validate_evidence.py private/evidence/F39-E3/acceptance.json \
  --artifact-root private/evidence/F39-E3 --require-pass
```

## Sources

`specs/F39-objectives-triggers-timers-spawn-groups-and-dialogue-cues.md`,
`docs/contracts/SCRIPT-MISSION.md`, `docs/contracts/CLI-EVIDENCE.md`, the F39-D
finding (its unknown #5, superseded here) and the F39-E1/E2/E4/E5 findings, the
F39-E6 finding (the independent walk of the same complement, which corroborates
the 9 archives, the 612 members and the zero blocks), the F14-D.1 reader-directory
finding (`docs/findings/2026-10-02-f14-d-1-reader-archive-directories.md`, the
member rules this stage now shares one definition of), `#463`'s `.zrd`
objective-record decoding, and the read-only `$CS_GAME_DIR` listing. No web source
was consulted and no original executable was run.
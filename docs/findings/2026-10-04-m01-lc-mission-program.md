# M01-LC-MISSION-PROGRAM: the mission control program, measured by rule

Date: 2026-10-04. Task: `M01-LC-MISSION-PROGRAM` (#630), "Measure M01's
mission-program member semantics into a runnable program". Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capability used: `retail` (read-only
`$CS_GAME_DIR`), `synthetic`. Evidence report:
`private/evidence/M01-LC-MISSION-PROGRAM/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-MISSION-PROGRAM.json`.

## What this stage changed about the question

The task was handed over with two readings attached, and neither had been
checked:

* `objectives.zrd` is M01's control program because it is an `aiv.zrd`-shaped
  `MISSION_CONTROL_MEMBER` — that is, **because of its name**;
* `wv_tailhook.zrd` (38639 bytes) is "presumed the mission's driving program by
  name shape only".

Both are name readings. This stage replaces them with a **rule** that can be
wrong, and measures what the rule finds:

> The control program of a mission-scoped reader archive is **the member whose
> decoded record declares at least one numbered `OBJECTIVE<N>` block.**

The rule needs candidates, so every member of every reader archive is decoded
with the production `.zrd` reader before one is selected. A reader with zero
qualifying members is a *measured absence* (see below) and one with two is a
refusal, never a choice.

**The name reading is measurably the wrong rule.** In M01's reader:

| Member | Bytes | Numbered objective blocks |
| --- | ---: | ---: |
| `aiv.zrd` | 9869 | 0 |
| `egen.zrd` | 1300 | 0 |
| `location.zrd` | 378 | 0 |
| `map.zrd` | 652 | 0 |
| `mis_anim.zrd` | 2173 | 0 |
| `net.zrd` | 336 | 0 |
| **`objectives.zrd`** | **24012** | **58** |
| `startanims.zrd` | 298 | 0 |
| `weather.zrd` | 3429 | 0 |
| `zeppelins.zrd` | 6766 | 0 |
| `placezeps.zrd` | 1535 | 0 |
| `wv_tailhook.zrd` | 38639 | 0 |

`wv_tailhook.zrd` is **1.61x** the length of the control member and declares no
objective block at all — it is an `ANIMATION_DEFINITIONS` record. The heuristic
the task description carried would have selected a cinematic animation library as
M01's mission program. `objectives.zrd` is the control member, and it is the
control member because of its contents rather than its spelling.

## What the control record spells

The measured directive grammar of an objective block is a **flat stream of text
keys, each with an argument list beside it if it has one**:

```text
OBJECTIVE2 = [ "DEDG", [1, 0],
               "REMOVE_OBJECTIVE_TARGET", [["piratezep", "rock_zeppelin"]],
               "ADD_OBJECTIVE_TARGET", ["workersvoyagezep"],
               …
               "INSTANTWIN" ]        <- a key with NO argument list
```

The asymmetry is load-bearing. A key followed immediately by another key carries
**no** argument: that is how the original spells `INSTANTWIN` and `INSTANTLOSS`.
Reading the pair as `key, value` would attribute the next key's name to the
previous key's argument list.

Measured over the owner's installation:

| Quantity | Value |
| --- | ---: |
| mission-scoped reader archives (`zbd/<group>/<mission>/zrdr.zbd`) | 53 |
| …declaring a control member | **40** |
| …declaring none (measured absence, see below) | 13 |
| …with two or more qualifying members | **0** |
| numbered `OBJECTIVE<N>` blocks in the measured archives | **1338** |
| directive sites | **5524** |
| distinct directive keys | **57** |
| keys reaching a mission-IR operation | **2** (`INSTANTWIN`, `INSTANTLOSS`) |
| keys refused, each with a named reason | **55** |

M01 alone: 58 blocks, 353 directive sites, 43 distinct keys, exactly 2
implemented.

The record keys outside the numbered blocks are exactly the five measured
`CONTROL_RECORD_KEY_VOCABULARY` (`MISSION_TIMER`, `PLAYER_INIT`,
`RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`) in all 40 measured
archives. **Five** further spellings appear in exactly one archive each
(`MISSION_WON_SOUND`, `MISSION_LOST_SOUND`, `PRIMARY_COMPLETE_SOUND`,
`SECONDARY_COMPLETE_SOUND`, `TERTIARY_COMPLETE_SOUND`, one site apiece) and are
carried as **unclassified** rather than absorbed into a documented field.

## The 13 readers with no control program are a measurement, not a failure

`ZBD/C1/IA1`, `ZBD/C1B/IA1`, `ZBD/C1C/IA1`, `ZBD/C2/IA1`, `ZBD/C2B/IA1`,
`ZBD/C3/IA1`, `ZBD/C4/IA1`, `ZBD/C5/IA1` (all `IA1`) and `ZBD/C1/MP3`,
`ZBD/C2B/MP3`, `ZBD/C5/MP1`, `ZBD/C5/MP2`, `ZBD/C5/MP3` declare no member
carrying numbered objective blocks. Each does declare an `objectives.zrd` — a
record-level one with no blocks (e.g. `ZBD/C1/MP3`'s is 53 bytes holding only
`MISSION_TIMER`), plus an `ia.zrd` or a scenario record beside it.

They are instant-action and multiplayer scenarios, not campaign missions, and the
census keeps them in a **separate population** (`ControlProgram::Absent`) rather
than counting them as missions with an empty program or dropping them from the
denominator. `campaign_ready()` asks about **every** row, so a reader with no
objective program is not a mission the engine can run.

## The dispositions: what the engine may do, and what it may not

A disposition is a property of **one archive's** sites, so it is counted per
record. Each distinct key of a record gets one of exactly two dispositions, and
there is no third "probably this" variant:

* **`TerminalOutcome`** — the two measured outcome keys, whose spelling the
  mission IR has an action for.
* **`Unmeasured { reason }`** with one of three named reasons:
  * `meaning_not_measured` — the spelling is measured and its **effect** is not.
    Most keys carry this.
  * `disagreeing_argument_shape` — the key's own sites spell more than one shape.
    `INACTIVE1` spells `[text]` at 35 sites, `[text,text]` at 130 and
    `[text,text,text]` at 106 **corpus-wide** (of its 271 sites; in M01 alone 2
    and 10). Both/all shapes are kept and **no majority is resolved** — a reader
    that picked the plurality would be inventing a rule the original does not
    state.
  * `argument_shape_has_no_value` — every site of that record agrees, but the
    agreed shape nests a list and `cs_script::ir::Value` has no list variant.
    Measured in M01 for `ANIM_STATE` (`[text,[text,[text],text,[text]]]`, 3 sites)
    and `COMPLETED_STOPPOINT` (`[[text,int,int]]`, 1 site), and in other archives
    for `ADD_OBJECTIVE_TARGET`, `REMOVE_OBJECTIVE_TARGET`, `ADD_OTHER_TARGET`,
    `REMOVE_OTHER_TARGET`, `SET_AI_NET`, `SET_AI_TEAM`, `SET_AI_`, `SET_HELP_LABEL`,
    `TRAVELERS`, `WAKE_ANIM` and `WARP_VEHICLE`. Flattening such a shape into a
    positional `Vec<Value>` would be a format change presented as a binding, so it
    is refused by name.

**M01's own record**, measured and pinned by
`accept_m01_lc_every_directive_m01_spells_is_measured_and_only_outcomes_run`: of
its 43 keys, **2** are implemented, **30** are refused `meaning_not_measured`,
**9** `disagreeing_argument_shape` and **2** `argument_shape_has_no_value`. A
key's disposition is **not** a corpus-wide constant — the same spelling agrees in
one record and disagrees in another — so the corpus figure is stated as "carries
this reason in at least one archive": 46 keys `meaning_not_measured`, 31
`disagreeing_argument_shape`, 13 `argument_shape_has_no_value` and 2 implemented,
which is 57 distinct keys with 2 of them in more than one bucket.

### A bare key is not an outcome key

`ZBD/C4/M01`'s `OBJECTIVE24` contains four consecutive **bare** keys spelling
ordinary English — `Change`, `to`, `mobile`, `net` — sitting between
`BEGIN_DORMANT` and `SET_AI_NET`. They read as an author's stray note left in the
directive stream.

The disposition rule is therefore a **name match against the measured outcome
vocabulary**, never "bare ⇒ terminal". All four are counted as sites (so a site's
total cannot quietly shrink) and all four are refused with
`meaning_not_measured`, exactly like any other unrecognized key.
`accept_m01_lc_a_bare_key_that_is_not_a_measured_outcome_key_is_refused` pins this.

## Why the record does not lower to a `MissionProgram` yet

`lower_program` (`cs_script::bindings`) takes a `RawProgram` whose objectives each
carry a `ContentId` and a `Condition`, and whose calls carry a flat
`Vec<cs_script::ir::Value>`. The control record spells **none** of those four
things, and `ControlLowering` accounts for each with the measured numbers behind
it:

| Requirement | Verdict | Measured basis |
| --- | --- | --- |
| `mission_identity` | **met** | The member spells no mission id; the reader is mission-scoped by path and the canonical id comes from M01-A's campaign binding record. |
| `objective_identity` | unmet | 112 `IDENTITY` sites corpus-wide spell a role spelling, a bare integer and an optional briefing label; **no** block spells a content id. |
| `objective_condition` | unmet | 1335 inactive-stage sites beside 130 completion-count thresholds and 1118 dormant markers; F39-E4 measured the stages' **names**, not the rule they state, and no block spells a predicate. |
| `call_arguments` | unmet | 57 keys over 5524 sites; keys whose agreed shape nests a list have no IR-carriable value; the widest `KILL_/WAKE_OBJECTIVE_WHEN_I_COMPLETE` site carries 12 integers. |

So the task's acceptance criterion is answered on its **second** branch: the task
records exactly which instructions and fields remain unmeasured, and
`campaign_ready()` is `false` while a single one does.

## This stage disagrees with F39-D/F39-E1 about `BEGIN_DORMANT`, and the bytes settle it

F39-D and F39-E1 measured that **1096** of the installation's 1338 blocks carry
`BEGIN_DORMANT`. This stage measures **1118**. Both numbers were taken on the
same install (`b4e780ab…`) over the same 53 readers and the same 1338 numbered
blocks, so one of the two walks is wrong, and the difference is not noise.

The cause is the flat (text, value) pairing in `cs_content::stunts::zrd_flat_fields`.
It advances two children per field, so a directive the original spells **bare** —
a text key immediately followed by another text key — is paired with the *next
key's spelling* and that next key is then stepped over and counted nowhere. There
are 49 bare directive sites corpus-wide (21 `INSTANTLOSS`, 24 `INSTANTWIN`, and
the four English words below); 28 of them are followed by another key. Measured
directly: exactly **22** of the 1118 `BEGIN_DORMANT` sites are the key immediately
after a bare directive, and 1118 − 22 = **1096**, the number the flat walk
reports. Every other key agrees between the two walks — `COMPLETED_SOUND_GROUP`
585, `DEDG` 130, `IDENTITY` 112, `INACTIVE_COMPLETION_COUNT` 130, `TRAVELERS`
75, `START_TAXI` 7, `STOP_QUEUED_SOUNDS` 49, `TICK_DEPENDS_ON_OBJ` 35 — so
`BEGIN_DORMANT` is the visible face of a systematic undercount, not a different
population.

A grammar-free cross-check agrees with the larger number: counting every text
occurrence of `BEGIN_DORMANT` inside a numbered block, with no pairing rule at
all, also gives **1118**. The same audit found **no** content-name-shaped string
read as a directive key — the only non-uppercase keys in the 57-key vocabulary are
`Change`, `to`, `mobile` and `net`, which are the stray note described above — so
the directive grammar this stage measures does not swallow an argument anywhere in
the corpus.

What this stage does **not** do is fix `zrd_flat_fields` or re-publish F39-D's,
F39-E1's, F39-E2's or F39-E4's numbers. Those belong to `cs_content::stunts` and
to those findings, and every claim downstream of the flat walk (F39-E1's 992
sentinel / 104 positive arguments in particular) has to be re-checked rather than
silently re-based. Filed as **#646** (`F39-D-COUNT`).

## The bytecode mission VM is still unlocated

F13-B/C reported **0 of 1452** located programs resolved, with every one stopping
at its first counter. That result is not contradicted by this stage, but this
stage shows the search was looking in the wrong place **for mission-scoped
readers**: their control program is a typed keyed list in one named member, and
it decodes completely through the `.zrd` grammar F09 measured.

The bytecode VM F13-B searched for is still **unlocated and unmeasured** for the
INTERP loading bodies, the animation payloads and the reader members it walked.
This stage does not claim to have found it, and a task that needs it (F13-D, the
`interp.zbd` bodies, `crimson.rof`'s UI scripts) is not advanced by this work.

## Files and the one observable failure

- `crates/cs_content/src/mission_control.rs` (new, owner path): `CONTROL_MEMBER`,
  `CONTROL_RECORD_KEY_VOCABULARY`, `ControlMemberError`, `DecodedMember`,
  `objective_blocks_of`, `control_member`, `MeasuredArg`, `DirectiveShape`,
  `MeasuredDirectiveKey`, `TerminalOutcome`, `UnmeasuredReason`,
  `DirectiveDisposition`, `ControlRecordField`, `AnimList`, `FieldSupport`,
  `BlockRefusal`, `MeasuredControlRecord`, `measure_control_record`,
  `terminal_outcome_of`, `LoweringRequirementKind`, `LoweringRequirement`,
  `ControlLowering` and `lowering_refusal`.
- `crates/cs_app/src/mission_control.rs` (new, owner path): `ControlCensusError`,
  `RetailMemberRow`, `ControlProgram`, `RetailControlRow`, `RetailControlCensus`,
  `survey_mission_control_programs`, `slice_member`, `read_control_member`.
- `crates/cs_app/tests/accept_m01_lc_mission_program.rs` (new, owner path): the
  19 `accept_m01_lc_*` tests.
- `crates/cs_app/tests/evidence_report_m01_lc.rs` (new, owner path): the evidence
  harness, deliberately **not** prefixed `accept_m01_lc_`.
- Wiring only (AGENTS rule 1): `crates/cs_content/src/lib.rs` and
  `crates/cs_app/src/lib.rs` (one module declaration and one doc paragraph each).

**One observable failure:** a reader that selects a mission's control program by
member name, length or position reports a program it never checked. The witness
is authored in
`accept_m01_lc_the_control_member_is_chosen_by_its_blocks_not_its_size`: a 64-entry
animation-definition member beside a one-block control member, so a
largest-member rule answers the animation definition and M01 would fly a
cinematic as its mission program. With `control_member` reduced to "the biggest
member", that test fails; with it reduced to "the member named
`objectives.zrd`", `accept_m01_lc_the_rule_follows_the_blocks_and_not_the_member_name`
fails on the renamed-member case.

The mirrored failure — a walk that skips a directive it cannot classify — is
`accept_m01_lc_an_unreadable_block_is_refused_with_its_block_and_child`.

## Design decisions

- **The rule is one rule and it is falsifiable.** Nothing about a member's name,
  length or archive position counts. A control member under a name the constant
  has never seen is still found; a member *named* `objectives.zrd` that stops
  carrying blocks stops being accepted.
- **Decoding every member is the point, not the cost.** A census that looked up
  `objectives.zrd` and reported success would pass on an installation where that
  member carries no blocks. The member list is also what lets a reader *see* that
  the rule chose between candidates.
- **Zero and two are different answers.** Zero is a measured absence carried as a
  row (13 real archives have it); two is a refusal, because picking one of two
  candidates would be a guess about which half drives the mission.
- **`lowering()` is derived, never cached.** A stored accounting could disagree
  with the record it was built from, and a gate reading a stale gate is the exact
  failure this accounting exists to prevent.
- **Fail-closed everywhere.** An empty record is never complete; a census with no
  rows is never campaign-ready; an unmet requirement always names its unmeasured
  fields; a block the walk cannot read is a refusal, never a shorter count.
- **Shapes are ordered and total.** `[int,float]` and `[float,int]` are different
  shapes, and a list is described by its children's shapes rather than its length,
  because `[text,text]` is a `SET_HELP_LABEL`'s node and label while
  `[[text,text]]` is an `ADD_OBJECTIVE_TARGET`'s node pair.
- **The record fields' support is one level.** `ShapeMeasured`: the value's shape
  is measured, its effect is not. No duration, unit or coordinate may be read out
  of `MISSION_TIMER`'s number or `PLAYER_INIT`'s five.

## Test inventory (`accept_m01_lc_*`, 19 tests: 14 synthetic + 5 retail)

| Test | Covers |
| --- | --- |
| `the_control_member_is_chosen_by_its_blocks_not_its_size` | **the observable failure**: an animation member beside a control member, plus the no-qualifying-member refusal |
| `two_qualifying_members_are_a_refusal_not_a_choice` | the ambiguity refusal names both candidates |
| `the_rule_follows_the_blocks_and_not_the_member_name` | a renamed control member is found; a member named `objectives.zrd` with no block is refused |
| `a_bare_directive_is_not_read_as_carrying_the_next_key` | the asymmetric grammar: bare, bare, `[text]` |
| `a_bare_key_that_is_not_a_measured_outcome_key_is_refused` | `Change`/`to`/`mobile`/`net`: counted, refused, never terminal |
| `a_nested_argument_shape_is_named_and_never_flattened` | nested lists preserved as nested; refused by name |
| `disagreeing_argument_shapes_are_both_kept_and_refused` | both `INACTIVE1` shapes kept, no majority resolved |
| `an_unreadable_block_is_refused_with_its_block_and_child` | a non-list block and a non-text key, each with its block and child index |
| `the_key_counts_reconcile_with_the_declared_site_total` | per-key sites sum to the total; a key twice in a block and once in another |
| `record_fields_are_classified_and_an_unknown_key_stays_unclassified` | the five measured fields, the `PLAYER_INIT` shape, an unknown key reported |
| `only_an_outcome_key_reaches_an_engine_operation` | `INSTANTWIN` implemented, `WAKE_ANIM` refused; the outcome vocabulary cannot drift |
| `the_lowering_accounting_names_what_each_unmet_requirement_lacks` | one row per requirement, an unmet row always names its fields, the measurement text carries the counts |
| `an_empty_record_is_not_complete` | fail-closed on a member nobody read |
| `the_measured_shapes_describe_the_production_zrd_grammar` | the tag constants and a round trip through the production decoder |
| `m01s_control_member_is_measured_and_not_assumed` *(retail)* | M01's control member, its 58 blocks, the members it was chosen from, the digest and extents |
| `the_longest_member_is_not_the_control_program` *(retail)* | the task's name reading measured wrong: the largest member is 1.61x the control member and has no block |
| `every_directive_m01_spells_is_measured_and_only_outcomes_run` *(retail)* | M01's 43 keys partitioned into implemented and three refusal reasons; bare keys are outcome keys here |
| `every_mission_is_measured_and_none_is_campaign_ready` *(retail)* | both populations, per-row reconciliation, the gate closed, the absent readers named and all IA/MP |
| `the_measured_vocabulary_is_wide_and_only_outcomes_are_implemented` *(retail)* | corpus vocabulary, site totals reconcile, implemented set is exactly the two outcome keys, mission labels are `zbd/<group>/<mission>` |

### The test prefix is shared with two other M01-LC tasks

`accept_m01_lc_` is not this task's alone. On the current `main` the selection
`cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` discovers
**32** assertions: this task's **19** (top level, from
`crates/cs_app/tests/accept_m01_lc_mission_program.rs`), **4** under `scene_ids::`
(the merged world-scene-ids task) and **9** under `import_retail::` (the world
import). All 32 pass; a prefix selection reports the neighbours beside this task's
19, and the evidence report derives the split from the recorded log rather than
asserting it, so it cannot go stale when another M01-LC task lands. The
implementer's handover note named only the scene-ids task, which was true when it
was written and stopped being true when the world-import task landed.

Every test calls production code. The retail tests re-derive every figure from
`$CS_GAME_DIR` on each run, so a stale constant fails rather than passes. All
synthetic `.zrd` bytes are authored here tag by tag; no original game data is
committed.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_lc_ --include-ignored` | 0 (19 tests: 14 synthetic, 5 retail) |

The acceptance selection was run twice. The first run exited 101 with
`could not execute process .../accept_doclib_conflict-3bad038241d20373 (never
executed)` / `No such file or directory (os error 2)` — the F54-X8 harness-missing
case documented in `docs/findings/2026-10-04-f54-x7-missing-test-harness-binary.md`,
in which cargo could not exec a harness it had already accepted as fresh, so **no**
test in that unit ran and the `accept_m01_lc_` tests in a different binary were
never reached. `ls -l` on that harness confirms the file is gone from
`target/debug/deps`. The **identical** command was rerun and is green; that rerun
is the run of record and the log committed as evidence is the rerun. The failed
first run is a failed run and is reported as one. Nothing in the tests, the
assertions or the lints was changed in response.

## Review record

Reviewed 2026-10-04 by `bunny-alpha-2/bunny-alpha-2`, the **same agent instance
that implemented the work**, so this review is **not independent** and its context
is not fresh; it is a second pass over the same author's reasoning. The
corrections it made, all of them to claims rather than to behaviour:

* the evidence report's method text claimed M01's longest member is "more than
  three times" its control member; the measured ratio is **1.61x** (38639 bytes
  beside 24012). The harness now derives the ratio, the mission and both byte
  extents from the census it renders, so the number cannot drift from the data.
* the finding claimed "1118 sites carry `-1.0`". Only the shape is measured here
  (`[float]` at all 1118); the values are F39-E1's (992 sentinel, 104 positive),
  and the finding now says so.
* the finding counted `disagreeing_argument_shape` over "26 keys", a number no
  reading of this census reproduces. A disposition belongs to one archive's sites,
  so the finding now states the basis, gives M01's measured 30/9/2/2 split and the
  corpus figure as "in at least one archive" (46/31/13/2).
* `read_control_member` returned a member row reporting **zero** objective blocks
  and `is_control: false` for the very member the rule had just selected. It now
  reports what the rule measured, and a retail test pins it.
* the census doc claimed no member can go unmeasured; it now says that the census
  sees what production discovery yields, that an archive yielding no member at all
  would arrive as `Absent { scanned: 0 }`, and that the corpus-level test fails on
  a row with no candidates rather than letting it pass.

The review also measured, independently of the production walk, that the directive
grammar does not misread the corpus: a grammar-free count of every text occurrence
inside a numbered block reproduces every key count (`BEGIN_DORMANT` 1118, `DEDG`
130, `COMPLETED_SOUND_GROUP` 585, `IDENTITY` 112, `TRAVELERS` 75,
`INACTIVE_COMPLETION_COUNT` 130, `START_TAXI` 7, `STOP_QUEUED_SOUNDS` 49,
`TICK_DEPENDS_ON_OBJ` 35), that no content-name-shaped string is read as a
directive key, that 1338 blocks over 40 archives with a block-bearing member and
none with two, and that `ZBD/C4/M01`'s `OBJECTIVE24` really does read
`BEGIN_DORMANT, Change, to, mobile, net, SET_AI_NET, NAP_OBJECTIVE_WHEN_I_COMPLETE`.
That audit is what produced the `BEGIN_DORMANT` disagreement below and filed
#646.

## Recorded unknowns (not guessed)

- **What any directive key *does* is unmeasured.** No original executable has
  been run. `INSTANTWIN` naming a win is an inference from a spelling.
- **The mission-language instruction table is still unmeasured.** F13-C's ledger
  ships empty and 0 of 1452 programs resolved. This stage located a mission's
  *control data*; it did not decode the bytecode VM the F13 census searched for,
  which remains unlocated for the INTERP bodies, the animation payloads and the
  reader members.
- **`IDENTITY`'s integer is unmeasured.** 112 sites carry one; whether it indexes
  an objective, a display slot or a label is not established, so it is not read as
  an ordinal.
- **An inactive stage's rule is unmeasured.** F39-E4 measured the names a stage
  carries (node, part, part-state) and **not** what the stage asserts; a completion
  threshold's unit is likewise unmeasured.
- **`BEGIN_DORMANT`'s number is unmeasured.** This stage measures only the
  argument's **shape** (`[float]` at every one of its 1118 sites) and no value at
  all. The values are F39-E1's measurement, not this stage's: of the 1096 blocks
  it counted, 992 carry the `-1` sentinel and 104 carry a positive argument
  (1.0 to 300.0, exactly one fractional, `13.5`). No duration, delay or sentinel
  reading is asserted here, and the 22-site difference between F39-E1's 1096 and
  this stage's 1118 is the swallowing walk described above.
- **Five record-key spellings are unclassified.** `MISSION_WON_SOUND`,
  `MISSION_LOST_SOUND`, `PRIMARY_COMPLETE_SOUND`, `SECONDARY_COMPLETE_SOUND` and
  `TERTIARY_COMPLETE_SOUND` appear once each and are reported as unclassified.
- **The four bare English keys in `ZBD/C4/M01` are unexplained.** Counted and
  refused; whether they are a stray author note or a directive form with no
  argument list is not established.
- **13 readers' control programs are elsewhere or absent.** Each declares an
  `objectives.zrd` with no blocks; which member carries an instant-action or
  multiplayer scenario's control data is **not** established here — F42-A/F42-D
  own the scenario grammar (`ia.zrd`).
- **Nothing here is `verified_original`.** The evidence claim is `implemented` and
  `campaign_ready()` is `false`.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::{discover_container, mission_scope}` and
`cs_content::stunts::decode_zrd`; `missions/M01.md`;
`docs/contracts/SCRIPT-MISSION.md`;
`docs/findings/2026-09-29-f13-b-locate-and-classify-programs.md`;
`docs/findings/2026-09-29-f13-c-signature-probes-and-reachability.md`;
`docs/findings/2026-09-29-m01-a-source-binding.md`;
`docs/findings/2026-10-04-f39-e4-count-category-producers.md`;
`crates/cs_script/src/{ir.rs, bindings/mod.rs}`.

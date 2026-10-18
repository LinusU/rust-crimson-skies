# M01-LC-DIRECTIVES: the four measured items, closed

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-13. Task: `M01-LC-DIRECTIVES` (#641), "give M01's directive keys
a measured lowering into the mission IR". Test prefix
`accept_m01_lc_directives_`. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) plus build/test — the static-image measurements this verdict
rests on were taken by the cited findings against
`crimson.decrypted.exe` (sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`). **No
original executable was run**; nothing here is `verified_original`. Evidence
report: `private/evidence/M01-LC-DIRECTIVES/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-DIRECTIVES.json` with its second production
observation `m01-directives.json` beside the recorded test log.

## What this stage is, and what it is not

#641 was filed on 2026-10-04 as the umbrella over the directive keys #630's
census had measured but not lowered: at filing, only `INSTANTWIN` and
`INSTANTLOSS` reached mission-IR operations and `campaign_ready()` was false
for all 40 measured control programs. The lowering itself landed afterwards
in stages — `M01-LC-DIRECTIVE-MEANING` (#675), `M01-LC-DIRECTIVE-{A,B,C,D,E}`
(#680–#683 and the E stage), the `Value::List` work, `M01-LC-DIRECTIVE-LOWERING`
(#717 with stages #724/#725/#726), `M01-LC-WORLD-FACTS` (#751) and
`M01-B-OBJ-4` (#1361). What #641 still owed was its own verdict: an acceptance
suite that pins each of its four items and its hard constraints through
production code, plus its evidence report. This note records that verdict; it
introduces no new lowering logic.

## The four items

**1. `IDENTITY` — the integer indexes the HUD slot, not the objective.**
Measured (`PresentationIdentity`, `DirectiveRole::ObjectiveRecord`): the
class spelling selects the completion sound channel and HUD class, and the
integer is the HUD slot ordinal the completion and save/load paths read.
Block identity is a different thing — the zero-based record index every
cross-objective directive spells — and that is what each lowered
`RawObjective`'s `SymbolId` carries, with the authored `OBJECTIVE<N>` number
naming its content id. M01 spells `IDENTITY` in two shapes
(`[text,int]` × 1, `[text,int,text]` × 4) and both stay measured.

**2. The inactive ladder is the measured evaluator.** M01 spells all
eighteen stage keys `INACTIVE1`–`INACTIVE18` (140 sites) plus
`INACTIVE_COMPLETION_COUNT`. Each `INACTIVE<n>` site lists one member by its
name chain (`[text]`, `[text,text]` or `[text,text,text]` — a node, a
node/part or a node/part/part-state) and asserts that member's in-play bit
cleared; `INACTIVE_COMPLETION_COUNT` is the threshold in units of members,
defaulting to the block's own member count. The pass-2 lowering is
`Condition::InactiveMembers` behind the block's `ObjectiveAwake` gate, and
the parser's `INACTIVE%d` loop (`d` in `1..=100`, digits spelled canonically)
is the boundary — `INACTIVE0`, `INACTIVE101` and `INACTIVE01` are not stage
keys.

**3. Nested shapes stay nested inside `Value::List`.** Seven M01 keys spell a
shape containing a list: `ANIM_STATE` (`[text,[text,[text],text,[text]]]`),
`SET_HELP_LABEL` (`[text,text]` beside `[[text,text],text]`),
`ADD_OBJECTIVE_TARGET`, `ADD_OTHER_TARGET`, `REMOVE_OBJECTIVE_TARGET`,
`SET_AI_NET` and `COMPLETED_STOPPOINT`. The emitted `RawCall`s keep every
level: operand-list operations (`ANIM_STATE`, `SET_AI_NET`, the objective
lists) carry the spelled list as their call's one `Value::List` argument, and
positional keys carry each spelled list as a `Value::List` argument of its
own. Flattening would produce a shape the key never spelled; the acceptance
walks every emitted call and checks its shape is one the census measured —
all 353 of M01's sites satisfy it.

**4. Disagreeing shapes: no plurality resolved.** Nine M01 keys spell more
than one shape — `IDENTITY`, `INACTIVE1`, the target add/remove keys,
`SET_AI_NET`, `SET_HELP_LABEL`,
`WAKE_OBJECTIVE_WHEN_I_COMPLETE` (four shapes) and
`KILL_OBJECTIVE_WHEN_I_COMPLETE` (three). Every one stays `Measured` because
a finding covers the key's semantics, and the registry registers **one
signature per measured shape** so each site binds against its own spelling.
A key whose sites disagree *and* whose effect no finding covers still
refuses `DisagreeingArgumentShape`; a key whose effect is simply unmeasured
refuses `MeaningNotMeasured`. Neither is repaired by counting heads.

## The hard constraints, with the corpus numbers

* **Counted and named, never bound to a convenient operation.** M01's 43
  keys partition as 41 `Measured` + 2 `TerminalOutcome` + **0** `Unmeasured`;
  all 353 sites bind and `MissionProgram::validate` accepts the program, so
  `is_complete()` is true for `zbd/c1c/m01`. Across the corpus the six keys
  no finding covers — `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE`, `SET_AI_`, and the
  stray English words `Change`, `to`, `mobile`, `net` — each carry their
  named reason and stay `Unmeasured`; their sites refuse `unknown host call`
  rather than bind.
* **Nothing flattened, defaulted or tolerated to launch.** A scalar beside a
  key is the `not_a_list` defect: the site emits no `RawCall`, is refused by
  name, and damages its block — the lowered objective carries
  `Condition::Unknown`, which validation refuses, rather than a guessed
  predicate. An argument the IR cannot carry (integers outside `i32`,
  non-finite floats, lists deeper than 8 or wider than 64 items) refuses the
  same way.
* **The campaign gate stays closed.** `complete_missions()` reports 14 of 40
  measured rows (M01 among them) over 53 mission-scoped readers, 13 of which
  declare no control program; `campaign_ready()` is **false** with 64
  lowering rows unmet corpus-wide — `call_arguments` on 26 missions,
  `mission_identity` and `objective_identity` on 16 each, and
  `objective_condition` on 6. No mission is marked ready while it spells a
  key the findings do not honour.

## What this does not claim

This is a **static lowering of the record into the IR**. A bound
`Action::Directive` emits the measured operation to the host; it is not
evidence that a world-side handler exists, so `Measured` stays unimplemented
in the IR's own sense and only the two outcome spellings reach an engine
operation. The residual unknowns the stage findings recorded — the in-play
bit's untraced writers, the DEDG member-field rewrites, the
danger-zone crossing test — stay named on the conditions that carry them.
No mission was played and no original ran, so nothing here is
`verified_original`; a Rally merge awards `checked`, and the human
capabilities (`human_play`, `human_review`, `network_real`) stay blocked as
before.

## Acceptance coverage

`crates/cs_app/tests/accept_m01_lc_directives.rs` — ten cases, four retail:

| Case | Retail | Item / constraint |
| --- | --- | --- |
| `..._identity_is_presentation_and_the_block_index_is_the_identity` | no | item 1 |
| `..._inactive_stages_and_their_threshold_are_the_measured_evaluator` | no | item 2 |
| `..._nested_argument_shapes_stay_nested_inside_value_list` | no | item 3 |
| `..._disagreeing_sites_bind_their_own_signature_never_a_plurality` | no | item 4 |
| `..._a_key_no_finding_covers_stays_counted_and_named` | no | counted/named |
| `..._no_field_is_flattened_defaulted_or_tolerated` | no | fail-closed |
| `..._m01s_four_items_are_all_measured` | yes | items 1–4 |
| `..._m01_lowers_completely_and_every_site_is_honoured` | yes | M01 complete |
| `..._every_emitted_call_matches_a_measured_shape_of_its_key` | yes | items 3+4 |
| `..._the_corpus_gate_names_every_unhonoured_key` | yes | campaign gate |

`crates/cs_app/tests/evidence_report_m01_lc_directives.rs` writes the report;
its `m01-directives.json` observation re-derives all of the above from the
installation — every M01 key's disposition, measured shapes, registered
signatures and emitted call shapes, the stage-key operations, the emitted
objective identity rule, and the corpus's unhonoured keys with the missions
that spell them.

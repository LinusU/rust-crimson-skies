# M01-LC-DIRECTIVE-MEANING: what M01's control directives do, joined from the stages that measured them

Date: 2026-10-06. Task: `M01-LC-DIRECTIVE-MEANING` (#675), the parent of the
split `M01-LC-DIRECTIVE-A` (#679) → `B` (#680) → `C` (#681) → `D` (#682) →
`E` (#683), and this document is the join those five stages only meet in one
tree. Capability used: `retail` (read-only `$CS_GAME_DIR`). Evidence report:
`private/evidence/M01-LC-DIRECTIVE-MEANING/acceptance.json`, committed as
`docs/findings/evidence/M01-LC-DIRECTIVE-MEANING.json`. Acceptance suite:
`crates/cs_app/tests/accept_m01_lc_directive_meaning.rs`, prefix
`accept_m01_lc_directive_meaning_` (3 tests, 2 of them retail).

**Everything about the original program below is static code evidence.** The
five stages read the owner's `crimson.decrypted.exe`; no original program was
run for any of them, so nothing here is `verified_original` runtime behaviour
(AGENTS.md rule 8), and a *measured* disposition is not a host binding (rule
4: what a directive does may be measured while the engine still cannot honour
it).

## What this document is, and what the five stage documents already are

The measurements live in the stage documents; this one does not restate them:

| Stage | Document | What it measured |
| --- | --- | --- |
| `M01-LC-MISSION-PROGRAM` (#630, the baseline) | `docs/findings/2026-10-04-m01-lc-mission-program.md` | which member carries a mission's control program, the corpus census (40 of 53 mission-scoped readers, 1338 blocks, 5524 sites, 57 distinct keys) and the argument-shape grammar |
| A (#679) | `docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md` | the `mission.cpp` objective reader in `crimson.decrypted.exe`: the key → handler map for all 43 keys M01 spells, the argument parse each handler performs, the record fields it writes, the runtime consumers |
| B (#680) | `docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md` | objective lifecycle, target flags, condition evaluators, completion effects |
| C (#681) | `docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md` | AI, world-registry and animation directives |
| D (#682) | `docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md` | sound groups, help labels, the mission timer |
| E (#683) | (no document: code) | `cs_content::mission_control`'s `DirectiveDisposition::Measured` table — `measured_directive`, `DirectiveOperation`, `MeasuredDirective`, the revised `ControlLowering` |

What this document adds is the **join**: the numbers production code reports
for M01 today, the per-key table of disposition → finding → residual unknown,
the corpus keys that stay refused, and the exact state of the two launch
surfaces `plan_mission_launch` (`mission_launch`, task #359) will read.

## Provenance

| Item | Value |
| --- | --- |
| Executable | `$CS_ENGINE_IMAGE`, PE32, image base `0x400000` |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (the same binary stages A–D read) |
| Mission data | `zbd/c1c/m01`, control member `objectives.zrd`, read through production discovery, container and `.zrd` readers |
| Tools | `r2`/`rabin2`/`objdump`, read-only; no bytes of the executable are committed anywhere |
| Original run | none — every statement about what a directive *does* is a reading of code |

## M01's reachable vocabulary, as production reads it today

Re-derived from the installation by
`cs_app::mission_control::survey_mission_control_programs` on every run and
pinned by `accept_m01_lc_directive_meaning_m01s_whole_vocabulary_is_measured_and_cites_its_findings`:

| Figure | Value |
| --- | --- |
| numbered `OBJECTIVE<N>` blocks in `objectives.zrd` | **58** |
| directive sites in those blocks | **353** |
| distinct directive keys | **43** |
| keys with `DirectiveDisposition::Measured` | **41** |
| keys with `DirectiveDisposition::TerminalOutcome` | **2** (`INSTANTWIN`, `INSTANTLOSS`) |
| keys with `DirectiveDisposition::Unmeasured` | **0** |
| findings cited by M01's measured keys | exactly three: the B, C and D documents |
| refusals (`BlockRefusal`) | 0 — every block was read |

Corpus-wide, the same production run reports **53** mission-scoped readers, of
which **40** are measured, **729** measured key occurrences beside **44**
terminal-outcome ones and **6** refused unmeasured ones, and **80** unmet
lowering rows, with **0** complete missions and `campaign_ready == false` (the
`m01-directive-meaning.json` artifact beside the report).

The three sets partition the vocabulary, and no key is left
`meaning_not_measured`: the parent task's "measure what each reachable directive
does" is discharged for `zbd/c1c/m01` — *as a static reading of the original's
code*.

## The 41 measured keys

Column "finding" is the citation the production table carries for that key;
column "residual unknown" is what the same table still records as unknown
about **that key** — carried, never dropped (`MeasuredDirective::unknowns`).

### Objective record, lifecycle and targets (finding B)

| Key | Operation | Residual unknown |
| --- | --- | --- |
| `BEGIN_DORMANT` | `DormantStart` | — |
| `TICK_DEPENDS_ON_OBJ` | `DependencyGate` | — |
| `IDENTITY` | `PresentationIdentity` | child2 (the `MSG_*` text at 4 of 5 M01 sites) is never read by the measured parse; no consumer of it was found |
| `INACTIVE1` … `INACTIVE18` | `InactiveMembers` (18 keys) | the writers of the in-play bit (`+0x24` bit 4): every consumer is measured, the spawn/despawn paths that set it are not |
| `INACTIVE_COMPLETION_COUNT` | `InactiveThreshold` | — |
| `DEDG` | `EnemyGroupDepletion` | the three member fields the evaluator rewrites on every counted member (`+0x318/+0x31c/+0x320`) feed untraced world state |
| `WAKE_OBJECTIVE_WHEN_I_COMPLETE` | `WakeObjectives` | — |
| `KILL_OBJECTIVE_WHEN_I_COMPLETE` | `KillObjectives` | — |
| `NAP_OBJECTIVE_WHEN_I_COMPLETE` | `NapObjective` | — |
| `ADD_OBJECTIVE_TARGET` | `SetTargetFlag { objective: true, set: true }` | which icon/label the target-info layer draws from the flag — presentation code, untraced |
| `REMOVE_OBJECTIVE_TARGET` | `SetTargetFlag { objective: true, set: false }` | same |
| `ADD_OTHER_TARGET` | `SetTargetFlag { objective: false, set: true }` | same |
| `COMPLETED_STOPPOINT` | `AdvanceStopPoint` | what the forwarded `{name,int,bool}` pair means to a stoppoint — its code is outside the measured bound |
| `TRAVELERS` | `Travelers` | the token for the unnamed polarity (only `APPROACHING` is a measured spelling); which of the two modes M01's site takes is a property of the world build; what group id 0 means in the counting mode |

### AI, world and animation (finding C; B where the lifecycle consumes it)

| Key | Operation | Residual unknown |
| --- | --- | --- |
| `ANIM_STATE` | `AnimationStates` | the state enum above 6 indexes past the engine's name table — unexercised by M01, unexplored |
| `SET_AI_NET` | `AssignNet` | what a re-seed copies out of the node-list entry — the setter's internals are not traced |
| `WAKEUP_ENEMIES` | `WakeEnemies` | — |
| `WAKEUP_ZEP_TURRETS` | `WakeZeppelinTurrets` | — |
| `WAKEUP_GENERATOR` | `FeedGenerator` | — |
| `WAKE_ANIM` | `WakeAnimation` | the animation call's three trailing arguments — the mission always passes `0,0,0`, other callers' defaults are unmeasured |

### Sound, help and completion UI (finding D; B where the lifecycle consumes it)

| Key | Operation | Residual unknown |
| --- | --- | --- |
| `WAKEUP_SOUND_GROUP` | `WakeSoundGroup` | what a group name resolves to (the handle is runtime state, not shipped data); which groups occupy the sound manager's four routing slots |
| `COMPLETED_SOUND_GROUP` | `CompletedSoundGroup` | same two |
| `STOP_QUEUED_SOUNDS` | `StopQueuedSounds` | no reader of the queued entry's `[+0x10]` flag was traced — the measured effect is a scheduled removal, not a measured audible stop |
| `SET_HELP_LABEL` | `SetHelpLabel` | which HUD element displays the label — assignment measured, UI consumer untraced |

Every one of those rows is falsifiable against the stage document the finding
column names; the operation, summary, citations and unknowns are read back out
of `cs_content::mission_control::measured_directive`, not out of this table.

## The two terminal spellings

`INSTANTWIN` and `INSTANTLOSS` are the only keys that reach an engine
operation (`TerminalOutcome`), and both are **readings of a spelling**: the
installation writes them bare, `+0x554` is set to 3/4, and the completion path
jumps to the mission WON/LOST setters. That `INSTANTWIN` asks for success is
an inference from its name — no original executable has been run, so
`terminal_outcome_of` says exactly this and the acceptance suite pins that it
answers for the two measured outcome keys and nothing else.

`WON` and `LOST` are **not** terminal requests: they are measured aggregation
classes (`DirectiveOperation::OutcomeClass`) — the mission resolves when every
block of a class completes.

## What still has no measured effect

**Residual unknowns on M01's own keys.** 32 of the 41 measured keys carry at
least one residual-unknown statement (36 statements in total, counted by
`accept_m01_lc_directive_meaning_m01s_whole_vocabulary_is_measured_and_cites_its_findings`);
the nine that carry none are `BEGIN_DORMANT`, `TICK_DEPENDS_ON_OBJ`,
`INACTIVE_COMPLETION_COUNT`, `KILL_/NAP_/WAKE_OBJECTIVE_WHEN_I_COMPLETE`,
`WAKEUP_ENEMIES`, `WAKEUP_ZEP_TURRETS` and `WAKEUP_GENERATOR`. *Measured* is
never *fully known*: each statement is carried on the key and named again on
the lowering row that owns it.

**Keys the corpus spells and no stage measured.** The boundary is the
findings, not the data. Over the 40 measurable archives the census spells **57**
distinct directive keys: 51 of them are `Measured` or `TerminalOutcome`, and
**six** are spelled by some mission while no finding covers them, so they keep
`Unmeasured { MeaningNotMeasured }`:

| Key | Where it is spelled |
| --- | --- |
| `SET_AI_` | `zbd/c5/m04` |
| `WAKEUP_OBJECTIVE_WHEN_I_COMPLETE` | `zbd/c1b/m03` |
| `Change`, `to`, `mobile`, `net` | `zbd/c4/m01`, `OBJECTIVE24`, between `BEGIN_DORMANT` and `SET_AI_NET` |

None of the six is reachable in `zbd/c1c/m01`, which is why M01's unmeasured
count is 0 while the corpus's is not;
`accept_m01_lc_directive_e_corpus_keys_no_finding_covers_stay_refused` and
`accept_m01_lc_directive_meaning_every_recorded_stage_finding_is_held_and_cited_within`
pin both halves.

**Parser keys no mission spells.** `OBJECTIVE_HD_a`, `OBJECTIVE_HD_b`,
`TEST_COMPLETE`, `COMPLETION_COUNT`, `WIN_ANIM`, `LOSS_ANIM` and
`DELETE_ON_SUCCESS` are in `crimson.decrypted.exe`'s directive vocabulary
(stage A's "Parser keys present but NOT spelled by M01" list; the last only as
a `TRAVELERS` argument token, consumed by a string compare rather than looked
up), but **no mission in this census spells any of the seven** — the census
never reports a disposition for them, so they are not corpus refusals. What is
measured is that our table has no entry for them: `measured_directive` answers
`None` for all seven, so a mission that spelled one would get the same
`Unmeasured { MeaningNotMeasured }` refusal. Recording them above as corpus
spellings would claim something the installation does not support; this
distinction is pinned by
`accept_m01_lc_directive_meaning_every_recorded_stage_finding_is_held_and_cited_within`
too.

**The five record-level keys** (`MISSION_TIMER`, `PLAYER_INIT`,
`RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`) sit outside the numbered
blocks and are not directives. Their **shape** is measured; their effect is
reported as `FieldSupport::ShapeMeasured`, whose refusal says no original
observation states what the value does, so no duration, unit or coordinate may
be read from it. Finding D additionally measured statically that
`MISSION_TIMER` starts on `value > 0.0f`, which means M01's `[0.0]` does not
start the timer — recorded as a code reading, still not an observation.

## The lowering accounting, and the two launch surfaces

`ControlLowering::measure` (four rows, in the order `lower_program` needs
them) for M01:

| Requirement | Met | Why |
| --- | --- | --- |
| `mission_identity` | **yes** | the id comes from the path and the campaign binding (M01-A), not from the member |
| `objective_identity` | **yes** | each block's authored `OBJECTIVE<N>` key is its identity; `IDENTITY` is measured as presentation data (class + HUD ordinal), not identity |
| `objective_condition` | **no** | the measured evaluators read live world state — member handles, registry bytes, animation states, the named counters — and several have side effects during evaluation; none is the side-effect-free `Condition` the field needs |
| `call_arguments` | **no** | no `cs_script::bindings::Lowering` variant carries the measured operations; `ANIM_STATE` nests a list the IR cannot carry and `INACTIVE1`'s own sites disagree about their shape — each named on this row rather than flattened |

So `ControlLowering::complete()` and `MeasuredControlRecord::is_complete()`
are **false** for M01, `census.complete_missions()` is empty and
`census.campaign_ready()` is false.

**About `mission_launch::plan_mission_launch`:** that function, and its
`mission_program` / `mission_objectives` surfaces by name, exist only on the
branch of task #359 (*Wire one original mission into the playable
application*), which is blocked; they are not on this branch and this task
could not touch them. The surfaces that exist here and that #359 will read
are the census row's `is_complete()` / `lowering()` (cs_app's
`RetailControlRow`), and they now report the intended thing: **Supported only
when every reachable directive is measured *and* all four lowering
requirements are met**. M01 is fully measured and still Unsupported, which is
the correct reading of the contract ("If the actual program is unavailable or
cannot be decoded, the mission remains Unsupported") — measuring the meaning
of a directive is not the same as being able to run it.

What would have to become measured before those two surfaces can report
Supported, in the order the rows ask for it:

1. a side-effect-free `Condition` for each completion evaluator (the
   inactive-members count, the danger-zone flags, the animation-state test,
   `DEDG`, `TRAVELERS`, the named counters) — including the residual unknowns
   above, or a decision that they do not matter to evaluation;
2. `cs_script::bindings::Lowering` variants for the measured operations
   (lifecycle writes, evaluator arming, ordered completion effects), with the
   argument shapes the IR can carry: the list-valued keys and `INACTIVE1`'s
   disagreeing sites need a representation, not a majority vote;
3. stable `ContentId`s for the objectives, which the numbered-block identity
   already supplies once the binding record is joined.

None of that is this task: #675 measures meaning, and meaning is now measured.

## How to re-derive every number here

```sh
cargo test --workspace --locked -- accept_m01_lc_directive_meaning_ --include-ignored   # 3 tests, 2 retail

CS_EVIDENCE_DIR=private/evidence/M01-LC-DIRECTIVE-MEANING \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_lc_directive_meaning_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=0 CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_lc_directive_meaning -- --ignored

python3 tools/validate_evidence.py private/evidence/M01-LC-DIRECTIVE-MEANING/acceptance.json \
  --artifact-root private/evidence/M01-LC-DIRECTIVE-MEANING --require-pass
```

The report's second artifact (`m01-directive-meaning.json`) is a second
production observation over the installation: corpus disposition populations,
every citation with whether this tree holds the document it names, and M01's
key-by-key dispositions, shapes, residual unknowns and lowering rows.

## Status and limits

* `implemented` only. A Rally merge awards `checked`, never
  `verified_original`; this task ran no original executable and no human has
  played the mission.
* The native-side claims (handler addresses, field offsets, what each
  operation writes) are only as good as the stage A–D readings; they are cited
  per key so a later stage can re-read the code at those addresses.
* `docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`
  (stage A's parser map) is required to be **held** by the acceptance suite
  but is not cited per key: a key's `evidence` names the finding that measured
  *that key's effect*, and A's map is consumed by B, C and D (which cite it in
  their own text). The four findings some key is measured from are pinned as
  the required citations, so a citation that drifts to a document nobody wrote
  fails the run.
* Runtime behaviour — whether a directive's effect happens as the code says in
  an actual mission — remains for VS-M01-CONTROLLED-RUNS and the owner's human
  review; nothing in this document substitutes for it.

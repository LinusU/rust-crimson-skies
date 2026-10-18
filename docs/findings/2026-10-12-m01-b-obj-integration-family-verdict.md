# M01-B-OBJ: the integrated family verdict for M01's `objectives.zrd`

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

*Task: #1355 (`M01-B-OBJ`), the integration step after #1358 (`M01-B-OBJ-1`),
#1359 (`M01-B-OBJ-2`), #1360 (`M01-B-OBJ-3`) and #1361 (`M01-B-OBJ-4`).*
*Date: 2026-10-12. Method: production census over the owner's installation
(`recover_retail_objectives` on `zbd/c1c/m01`) joined with the four stages'
findings. Nothing was run; no engine bytes are quoted here beyond what those
documents already record.*

## Question this step answers

After the four measuring stages, is every field family M01's `objectives.zrd`
uses either measured-and-recovered, or recorded with evidence while
`ObjectiveRecovery::program()` refuses naming it? And does `program()`'s
refusal state exactly that verdict — no family silently dropped, no family
blamed that is fully recovered, no blanket refusal where a computed one
belongs (AGENTS.md rules 4 and 5)?

## The verdict, per family

Production census of `zbd/c1c/m01` (58 `OBJECTIVE<N>` blocks, 358 fields: 5
record-level, 353 inside blocks), joined with each stage's finding:

| Family | M01 fields | Status | Evidence |
| --- | --- | --- | --- |
| `CompletionEffect` | 56 | measured, recovered | `2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md` |
| `OrderDependency` | 3 | measured, recovered | same (TICK_DEPENDS_ON_OBJ) |
| `Dormancy` | 52 | measured, recovered | `2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md` |
| `CompletionCount` | 9 | measured, recovered | same + `2026-10-12-m01-b-obj-3-stage-conditions-and-outcomes.md` |
| `Identity` | 5 | measured, recovered | `…-obj-2-…` |
| `SoundCue` | 41 | measured, recovered | `…-obj-2-…` |
| `InactiveStage` | 140 | measured, recovered | `…-obj-3-…` |
| `Outcome` | 2 | measured, recovered | `…-obj-3-…` |
| `ControlDirective` | 45 | measured, **not** recovered — world-side | `2026-10-12-m01-b-obj-4-mission-language-directive-keys.md` + M01-LC-DIRECTIVE B–D |
| `RecordDirective` | 5 | measured, **not** recovered — record preamble | `…-obj-4-…` |
| `Unmeasured` | **0** | — | no family M01 spells is unmeasured any more |

308 fields are recovered into the engine model (`EngineDeclaration`s,
`StageConditions`, `ObjectiveOutcome`s); 50 are unrecovered and named.
308 + 50 = 358: nothing is silently dropped. These are the production numbers
the acceptance test `accept_m01_b_obj_integration_retail_m01_every_family_is_measured_and_the_directives_are_named`
pins against `docs/findings/evidence/M01-B-OBJ.json`.

## Why the two directive families keep the refusal

M01-B-OBJ-4 measured all 19 directive keys' parse sites, argument shapes and
operations. They stay unrecovered **by measurement, not by omission**: their
effects are world-side operations — target flags, AI-net entries, enemy wakes,
generator feeds, traveler watches, group depletion, animation states — that
the `DeclaredObjectiveProgram` vocabulary has no slot for. Several of them
(DEDG, ANIM_STATE, TRAVELERS) are producers around the completion-condition
chain whose other families remain undecoded (M01-B-OBJ-3 unknown #4), so
lowering the record without them would change when objectives complete. That
is the fabricated mission AGENTS.md rule 4 forbids, so `program()` refuses
naming exactly `ControlDirective x45` and `RecordDirective x5`, each with its
measured reason.

## Why even a fully recovered record would still refuse

`program()` is now a computed refusal rather than a blanket one: it names the
unrecovered families of *this* record, and when a record leaves none (every
field a measured engine field), it still refuses — naming the schema gap
instead. Assembling a `DeclaredObjectiveProgram` from the engine model would
require mappings nothing has measured: the declared objective states carry no
slot for the engine's Dormant/Awake/Napping/Asleep lifecycle, the declared
conditions are roster count conditions rather than the engine's node-chain
`INACTIVE<n>` stages, the reveal rules and initial states of original
objectives are unmeasured, and `DeclaredPrecedence` has only the designed
`SyntheticConservative` variant. Inventing those mappings is what the refusal
prevents.

## What this does *not* change

* M01's launch is unaffected: per the owner's decision of 2026-10-10 on
  #1219, M01's objectives come from the complete control path
  (`mission_control::RetailControlRow::lowering()`), which owns the directive
  operations this record leaves unrecovered.
* Nothing here is `verified_original`: the family measurements are static
  readings of `CS_ENGINE_IMAGE` plus retail data censuses; no original
  executable was run and no human played the result.
* Rally #1219 (`VS-M01-OBJECTIVE-RECOVERY`, blocked) asked for the same goal
  and its three blockers still stand where they are outside this task's owner
  paths (schema carriers in `cs_sim`, the owner-gated support variant). This
  task supersedes its measurement half; the owner can cancel it as a
  duplicate.

## Follow-up resolution statements

* A `DeclaredObjectiveProgram` for M01 needs, before `program()` may return
  `Ok`: measured schema carriers for the engine lifecycle/stage-condition
  families (a cs_content/cs_sim design task), and the owner's approval of a
  recovered-support variant (`DeclaredSupport`, SCRIPT-MISSION.md's gate).
  Until both exist, the refusal above is the correct production behavior and
  any `Ok` would be an invention.
* The world-side directive operations are owned by the control lowering
  (M01-LC-DIRECTIVE), already complete for `zbd/c1c/m01`; they are not
  re-measured here.

# M04-B-FU1: `ANIM_STATE` reads its whole operand list — the multi-pair walk and the one-argument call

Date: 2026-10-09. Task: M04-B-FU1 "Lower M04's multi-pair ANIM_STATE sites and
bind the key through the host-call registry" (Rally #806), a follow-up of
M04-B (#268), coordinated with M02-B-FU1 (#800, landed), M06-B-FU1 (#817) and
M07-B (#277). Capabilities used: `retail` (`$CS_GAME_DIR` read-only, never
written) and `$CS_ENGINE_IMAGE` (read-only, disassembly only — the image was
never run). Implementer: **Devin SWE-2/swe2-max-1** (session of
2026-10-09T12:40Z). No reviewer yet; an implementer's own run is not
independent review and no agent review replaces the owner's human approval.

## The measured mechanism, confirmed against the image

The M01-LC directive-C finding
(`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md`,
`ANIM_STATE`) sketched the parse helper `0x4691d0`. Re-read in the owner's
decrypted image for this task, the helper's behaviour is, per objective
record:

1. **The site lookup runs once per block.** The caller invokes `0x4691d0`
   once per objective; the helper's `0x57a090` lookup takes the **first**
   `ANIM_STATE` text in the record's depth-first order — a top-level
   directive key or a text nested inside an earlier directive's operand
   list. A second `ANIM_STATE` spelling is never reached: it contributes no
   pair and it is not an error.
2. **The follower must be a list.** A selected `ANIM_STATE` text whose
   follower is not a list arms no animation evaluator at all — later
   spellings are not searched for a better match.
3. **The selected operand list is walked in order.** A tag-3 `ANIM` child
   followed by a tag-4 spec record appends one `{name, state}` pair into the
   evaluator the record carries at `+0x578`; `NAME` and `STATE` are read out
   of the spec by the flat first-match lookup `0x57a0f0`. Pairs the spec
   cannot resolve (no name text, or a state token outside the measured
   `RUNNING`/`EXECUTED`/`INVALID` vocabulary, compared case-insensitively by
   `_stricmp`) are dropped — the walk skips them rather than refusing the
   site.
4. **`COMPLETION_COUNT` is looked up recursively inside the same operand
   list** (`0x57a1b0` is called on the found list, not on the record). The
   first match consumes the lookup whether or not its follower resolves an
   integer; a resolved integer **overwrites** the header's `required`, which
   otherwise counts one per appended pair. A block-level `COMPLETION_COUNT`
   directive is never looked up — the string is referenced only inside
   `0x4691d0` — so a top-level spelling is inert for the animation condition.

Nothing in this walk bounds the pair count to one. The engine's
"the tag `ANIM` and one spec record" shape was a faithful reading of M01's
three sites — all single-pair — not a measurement of the general case.

## What the records spell

The same mechanism is spelled by three measured missions:

| mission | member | sites | operand shapes | what they spell |
| --- | --- | --- | --- | --- |
| M04 | `zbd/c1/m04` (`objectives.zrd`) | 3 | 2 operands ×1 (block 32), 18 operands ×2 (blocks 23, 37) | a `COMPLETION_COUNT [n]` plus eight `ANIM` descriptors; counts 1 and 3 |
| M06 | `zbd/c2/m01` | 8 | 2 operands ×5, 6 operands ×3 (blocks 9, 11, 41) | `COMPLETION_COUNT [1]` plus two descriptors |
| M07 | `zbd/c2/m02` | 9 | 2/4/6/8/2/6/2/10/2 operands | one to five descriptors, no `COMPLETION_COUNT` — `required` counts the pairs |

## The two refusals and the change

Before this task both halves refused:

* `cs_script::conditions::anim_state` accepted exactly two operands — one
  `ANIM` tag and one spec — so every multi-pair site refused its condition.
* `cs_app::control_lowering` registered one `BindingSpec` signature per
  measured shape, positionally; the 18-operand M04 shape and the 10-operand
  M07 shape exceed `MAX_CALL_ARGS` (8), so `HostBindingRegistry::register`
  refused the whole key and every site of it failed as an unknown host call.

The change lowers what was measured, and only that:

* `lower_block_condition` computes the block's one evaluator before the
  directive loop, from `anim_state_site` — the first depth-first
  `ANIM_STATE` text whose follower is a list. `anim_state` walks the
  operand list exactly as the helper does: every `ANIM` + spec appends a
  pair in declaration order, `NAME`/`STATE` come from the flat first-match
  lookup, an unknown state token drops its pair, and a kept name that no
  animation resolves stays (the handle lookup is a runtime property the
  facts answer — fail-closed, never invented). `completion_count` searches
  the list recursively with the same first-match-wins rule and overwrites
  `required`. The evaluator joins the block's evaluator list beside the
  other kinds; a second `ANIM_STATE` directive is silently never read.
* `control_lowering` generalizes M02-B-FU1's list-argument carrying to the
  measured operation `AnimationStates`: the condition side keeps the spelled
  operand list verbatim in `directive.args` while the call side carries the
  whole list as **one** `Value::List` argument, so an 18-operand site sees a
  one-argument signature. `MAX_CALL_ARGS` stays 8 and the signature is still
  nested and itemwise-checked (`ArgDomain::List` over the measured child
  domains). The arm that still refuses is a list wider than
  `MAX_VALUE_ITEMS`: its signature is uncarriable, so the key fails
  registration rather than truncating the spelled list.

## Result

* M04 lowers completely: all 201 calls bound, all 52 conditions lowered,
  `MissionProgram::validate` reached and accepting; the census row is
  complete and M04 joins `complete_missions`.
* M06 lowers completely through the same mechanism (all 265 calls, all 82
  conditions) — the shared class M06-B-FU1 (#817) anticipated.
* M07's `ANIM_STATE` half closes identically; its remaining gap is the
  unrelated `DANGER_ZONES_COMPLETED` condition (M07-B-FU1), unchanged here.
* A block-level `COMPLETION_COUNT` directive stays inert for the condition
  and still refuses its call as an unmeasured key — honestly unsupported.

Acceptance pins moved, never weakened: the M04-B/M06-B/M07-B gap tests were
rewritten to pin the measured behaviour and the remaining gap by name;
the `accept_m04_b_fu1_` suite and the evidence harness
`evidence_report_m04_b_fu1_*` (report `docs/findings/evidence/M04-B-FU1.json`)
record this change.

## Not claimed

No playthrough, no visual or audible evidence, no runtime observation of the
evaluator firing: `AnimationStates` evaluates against `MissionFacts`'s
animation table the measured way (an animation the facts do not carry is not
in its wanted state), but when and how the world writes animation states is
the world build's question, exactly as finding C left it. No original
executable was run; nothing here is `verified_original`.

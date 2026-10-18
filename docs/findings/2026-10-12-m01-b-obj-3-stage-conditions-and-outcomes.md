# M01-B-OBJ-3: what `INACTIVE<n>`, `INACTIVE_COMPLETION_COUNT` and `INSTANTWIN`/`INSTANTLOSS` do

> **Archive attribution note (#1568, after #1565 `M01-ARCHIVE-DECISION`).** The measurements in this document were taken on `ZBD/C1C/M01/zrdr.zbd` under the pre-decision "M01" label. That archive is campaign position `ch2-m01`, work order **M06** "The Red Menace"; M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`. The bytes measured are unchanged; read every "M01" attribution below as M06. See `docs/findings/2026-10-17-m01-archive-decision.md`. The text below is the original historical record and is not rewritten.

Date: 2026-10-12. Task: #1360 `M01-B-OBJ-3` (parent #1355 `M01-B-OBJ`). Test prefix
`accept_m01_b_obj_3_`. Capabilities used: `retail` (read of `$CS_GAME_DIR`) and
`CS_ENGINE_IMAGE` (read-only static analysis of `crimson.decrypted.exe`, 2,580,480
bytes). **No original executable was run**; nothing here is `verified_original`.
Everything below is a reading of the executable's code, not an observation of
behavior, and says so where it matters.

This note supersedes F39-E1's "what satisfying an `INACTIVE<n>` condition means"
(unknown #2 there) and F39-D's "terminal precedence between win and loss" (unknown
#2 there) for the keys named in the title. F39 could only read `objectives.zrd`,
which declares the keys; the code that consumes them is in the engine image. No
engine code, bytes or disassembly are reproduced; addresses are virtual addresses in
the image (`.text` 0x401000, `.data` 0x619000) so a reviewer can find each claim.
Sibling findings: `2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`
(branch #1358) decodes the same per-pass function and is cited for its structure.

## Where the keys are read

The mission loader (`mission.cpp`) reads each `OBJECTIVE<N>` block in the routine
around 0x468b60–0x469180 and 0x469070.

| Key | Read at | Stored as |
| --- | --- | --- |
| `INSTANTWIN` | 0x468b95 | outcome field (record +0x554) = 3, **by presence** |
| `INSTANTLOSS` | 0x468bbb | outcome = 4, by presence |
| `WON` | 0x468bd7 | outcome = 2, by presence |
| `LOST` | 0x468bf3 | outcome = 1, by presence |
| `INACTIVE1` … `INACTIVE100` | 0x469070 | a list of condition nodes (+0x564), count at +0x560 |
| `INACTIVE_COMPLETION_COUNT` | 0x46911c | threshold (+0x55c) |

All lookups are the same exact, case-sensitive, first-match key search the sibling
finding describes (0x57a090).

### Outcome keys

The loader clears the outcome to 0, then looks the four keys up **in the order
`INSTANTWIN`, `INSTANTLOSS`, `WON`, `LOST`**; each hit overwrites the previous
store. A block that spells two of them therefore ends with the later one in that
order (`LOST` over `WON` over `INSTANTLOSS` over `INSTANTWIN`), not the one it
spells last. The value after the key is never read. Retail spells only
`INSTANTWIN` (15 sites) and `INSTANTLOSS` (9) (F39-D); `WON` and `LOST` have a
reader and no retail record, so their behavior below is the code's, not a
corpus measurement.

### Stage conditions

* The loader formats `INACTIVE%d` (0x626b4c) for **1 through 100 inclusive** and
  looks each up; there is no stop at a gap. A number outside the range, or a
  spelling that is not the canonical decimal (`INACTIVE01`), is never looked up.
  The corpus numbers its stages `1..=18`; the loader's range is wider.
* Each hit is turned into a condition by the routine at 0x469180. Its argument is
  the key's list value (the lists store their entry count **plus one**, so the
  header slot is not an element). The first element names an entry in the engine's
  class-node registry (0x465e30 → 0x4d0280 with kind 7; an already-seen name is
  found in a cache list at 0x71c120). Every following element is looked up by
  0x4cc990 as a node **named that, found depth-first below the previous node; a
  node also matches its own name**.
* A chain that fails at any step makes the routine return 0 and the loader **does
  not append it**. The condition is silently absent: it is neither counted nor
  required.
* Elements are used as strings without a kind check. An integer element would be
  dereferenced as a pointer; no retail record has one, and recovery does not
  recover such a field.
* The conditions are stored as a calloc'd array of node pointers.
* `INACTIVE_COMPLETION_COUNT` is the raw first element of its list stored as a
  signed word. **When absent the threshold is the number of conditions that
  resolved**, not the number declared.

## When the conditions are met

The per-pass function (0x46a590–0x46afb0, the one the sibling finding describes)
tests an objective only if its "active" word (+0xc) is non-zero and its completed
word (+0x14) is zero, and its `TICK_DEPENDS_ON_OBJ` dependency (if any) is in
state 1. It then ORs six condition families in this order and completes the
objective on the first that holds (0x46a947): 0x469a60 (**this stage's family**),
0x469ab0, 0x4697a0, 0x46b090, 0x465910, 0x465b40. Only the first is decoded here.

0x469a60:

1. If the objective has **no** resolved condition (count 0) the result is **false**,
   whatever the threshold says.
2. Otherwise it counts the condition nodes whose node flag word (+0x24) has **bit
   value 4 clear**, and returns `count >= threshold` as a **signed** comparison.

The bit is written by 0x4cca30, whose own diagnostic names it `gwNodeSetActive`
(0x62cd28): argument 1 sets the bit, argument 0 clears it (node kinds 1, 2, 5 and 6
take this path; 9 and 10 have their own branches; 3, 4, 7 and 8 are the routine's
"unrecognized node class type" error). So the set is met when enough of its nodes
are **not marked active**, which is the **loss reading** F39-E1 judged the
consistent one: a block with count 1 over fourteen engines completes when one
engine's node is cleared, not when the mission loads. That the cleared state means
*destroyed or disabled* is the inference the names support (`INACTIVE`,
`gwNodeSetActive`, the `healthy` attribute); the measured fact is the polarity of the
count.

0x469800 (reached from a mission-language command, at 0x43eb0b) calls
`gwNodeSetActive(node, 0)` on every condition node of one objective: it **clears**
the bit for all of them.

The same function has a fall-through at 0x46a8d6–0x46a945: an objective whose
condition lists in every family are empty and whose state word (+0x5c8) is 1
completes at once, so an awake objective with no completion condition at all is
complete on its first pass. (The checks in between belong to families not decoded here.)

## Outcomes at completion

When an objective completes (0x46a947) and the completion handler of the sibling
finding has run, 0x46a9ef–0x46aa1b tests its outcome field:

* **3 (`INSTANTWIN`)** calls 0x463c10(1), which sets the mission's **won** word
  (+0xc58), and notes in a pass-local byte that the win was instant.
* **4 (`INSTANTLOSS`)** calls 0x463c20(1), which sets the mission's **lost** word
  (+0xc5c), and notes the same for the loss.
* 1 and 2 do nothing at completion. After the scan (0x46aeca–0x46af73) the pass
  tallies the objectives carrying outcome 2 and outcome 1: **if at least one
  objective has the outcome and every such objective is completed**, it sets the
  won word (outcome 2) or the lost word (outcome 1).

After the tally (0x46af76):

* If the **lost** word is set the pass starts the loss presentation with a delay
  argument of **0.1 if an `INSTANTLOSS` completed this pass, else 3.0**
  (0x3dcccccd, 0x40400000 handed to 0x463c30).
* **Only otherwise** is the won word tested, with the same 0.1/3.0 rule for
  `INSTANTWIN`.

So when both words are set the mission is **lost**; a win does not outrank a loss,
whichever was reached first. The unit of the delay is not decoded; seconds is an
inference.

## What is recovered, and what is not

`cs_content::objectives_conditions` is the pure model: `StageConditions::read`
(the loader's range, canonical spelling, first-occurrence rule and value shapes),
`StageConditions::is_satisfied` (the 0x469a60 test, with the node state supplied by
the caller as `NodeState::{Unresolved, Active, Inactive}`),
`ObjectiveOutcome::of_keys` (the loader's overwrite order) and `MissionEndFlags`
(completion, the category tallies, lost-before-won, the per-pass instant marker).

`ObjectiveRecovery::read` now recovers an `INACTIVE<n>`, `INACTIVE_COMPLETION_COUNT`,
`INSTANTWIN` or `INSTANTLOSS` field when the loader would read it: the block is one
the loader loads (`OBJECTIVE1` upward until the first absent number) and the
value is the measured shape. A field that is not stays unrecovered under its family
(`InactiveStage`, `CompletionCount`, `Outcome`) with a reason naming why.

For M01 (`zbd/c1c/m01`, 58 blocks, 358 fields): **151 fields recovered — 140 stage
conditions in 12 blocks (6, 7, 8, 9, 28, 36–39, 54, 55, 56), 9 completion
counts (blocks 6, 7, 8, 9, 36, 37, 38, 39, 55) and the 2 outcome fields (block 24
`INSTANTWIN`, block 40 `INSTANTLOSS`)**. The retail test checks the stage
conditions, paths and thresholds against F39-E1's census of the same record, which
reads it with a different parser. The remaining 207 fields are in other families.
Every subject M01 names is `piratezep` or `workersvoyagezep`; two blocks (54, 56)
declare the single element `piratezep`, a chain of length one.

`ObjectiveRecovery::program()` **still refuses**, naming the families this stage did
not touch.

## Unknown, with the evidence that would settle each

1. **Whether each M01 chain resolves.** A chain resolves only if the registry holds
   the subject and the subject's tree holds each later name; unresolved chains are
   dropped, which changes the default threshold. The registry (0x4d0320, a table
   at 0x71eb00) and the airframe nodes it exposes are not decoded, so which of the
   140 resolve is unknown. Resolution would be settled by decoding how the node
   tree of `piratezep` and `workersvoyagezep` is built from their records.
2. **What clears a node's active bit in play.** 0x4cca30 has hundreds of callers
   (damage, ownership, scripts); only 0x469800 is identified here. Whether
   destroying `reng11` clears the node named `reng11`, and what `healthy` and
   `panels` mean as node names, is undecoded. This is the producer a runtime
   consumer needs to supply `NodeState`.
3. **The initial state of the bit** when a node is created, and so whether an
   objective could complete before anything happens. Not decoded.
4. **The other five completion-condition families** in the OR chain (0x469ab0,
   0x4697a0, 0x46b090, 0x465910, 0x465b40) and the fall-through; the order of the
   chain does not matter for the result, which is an OR, but their meaning does.
5. **Other readers of the won and lost words** (0x419518, 0x46b470, 0x46b961,
   0x46ba3a, and writers 0x43ed0f, 0x43ed4b, 0x45be47, 0x47e2d0). This note
   measures only the objective pass; whether another path can end the mission
   before this one, or reset a word, is unknown.
6. **The unit of the end delay** and what the presentation does with 0.1 versus 3.0.
7. **A repeated key nested inside another value.** The engine's lookup descends into
   nested lists, so an outcome key nested in a value would count; the field walk
   sees only the top level of a block. No retail record is known to nest one.
8. **Whether the engine behaves as read when run.** Static reading only.

## Mismatches with existing models

`cs_sim::objectives::terminal` labels its tie-break `SyntheticConservative` and says
the original's rule is unmeasured; this finding measures the objective pass to
test **lost first**. The `cs_sim` crate is outside this task's owner paths, so the
rule is available as `MissionEndFlags::end` and reported as a follow-up.

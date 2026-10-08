# M01-LC-WORLD-FACTS: the world-side `MissionFacts` maps get production writers

Date: 2026-10-08. Task: `M01-LC-WORLD-FACTS` (#751), the follow-up
`M01-LC-DIRECTIVE-LOWERING` (#717) split off when its integration step found
that half of the fact map had no producer. Capabilities used: `retail`
(read-only `$CS_GAME_DIR`) plus build/test. No `gpu`, no `audio`.

## What this step is, and what it is not

`cs_script::runtime::MissionFacts` carries six maps. Two had production
writers — `actors` (cs_sim's `ActorFactTable`) and `objectives` (cs_sim's
`BlockLifecycleTable`) — and both documented that the world-side four are
somebody else's to fold in through `MissionFacts::absorb`. The other four,
`members`, `groups`, `generators` and `animations`, are read by the conditions
#717's lowering now emits (`Condition::InactiveMembers`,
`Condition::EnemyGroupDepletion`, `Condition::Travelers`,
`Condition::AnimationStates`) and were populated by nobody:
`MissionState::holds` answers `false` for a key the map does not hold, so those
blocks could evaluate but **never** complete.

This step builds the writers: `crates/cs_app/src/world_facts.rs` maps each
record operand onto the observed world and `compose_mission_facts` folds all
four maps in through `MissionFacts::absorb` before `MissionSession::advance`.
It is a second production observation over the owner's installation, not a
paraphrase: no original executable was run and no mission was played, so
nothing here is `verified_original`, and a Rally merge awards `checked`.

## What the record spells, measured

`WorldOperands::of` walks the same condition tree the evaluator walks, so the
numbers below are derived from `zbd/c1c/m01`'s own lowered program rather than
written down (test
`accept_m01_lc_world_facts_m01_operands_resolve_against_the_retail_world`):

| Map | M01's operands |
| --- | --- |
| `members` | **42 distinct chains** — 27 distinct `workersvoyagezep/...` chains across 8 `INACTIVE` blocks (18 engine/turret chains in `OBJECTIVE6/8/9/55`'s ladders, 9 in two more), 12 distinct `piratezep/...` chains across `OBJECTIVE36`–`OBJECTIVE39`, the bare `["piratezep"]` in `OBJECTIVE54` and `OBJECTIVE56`, plus the `TRAVELERS` subject `["player"]` and its anchor `["workersvoyagezep"]` |
| `groups` | **4 ids**: `1, 2, 3, 4` (the eight `DEDG` sites) |
| `generators` | **none** — no M01 `DEDG` site spells a third argument, so nothing in M01 reads pending spawns |
| `animations` | **3 names**: `wv_drop_copilot` (`RUNNING`), `wv_pickup_copilot` (`EXECUTED`), `hooked_to_klondike` (`EXECUTED`) |

The 24 world-side blocks, by zero-based record index (the id `ObjectiveAwake`
carries) and by the record's own label:

* **12 `INACTIVE` ladders** — indices 5, 6, 7, 8, 27, 35, 36, 37, 38, 53, 54,
  55 (`OBJECTIVE6/7/8/9/28/36/37/38/39/54/55/56`).
* **8 `DEDG` blocks** — indices 1, 15, 16, 19, 20, 41, 42, 43
  (`OBJECTIVE2/16/17/20/21/42/43/44`), groups `1`–`4`, `remaining` `0` except
  index 43's `1`.
* **1 `TRAVELERS` block** — index 2 (`OBJECTIVE3`): subject `["player"]`,
  anchor `["workersvoyagezep"]`, radius `700.0`, `APPROACHING`.
* **3 `ANIM_STATE` blocks** — indices 10, 14, 17 (`OBJECTIVE11/15/18`).

## How a chain resolves, and how that was measured

Finding B (`2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`)
measured the *shape*: an `INACTIVE<n>` site resolves its first string through
the original's memoized name resolver and each later string through a chained
member lookup on the previous result, so the arguments are one hierarchy — the
node/part/part-state triple F39-E4 measured — and never independent names.

`MemberResolver` implements exactly that over `ZBD/C1C/gamez.zbd`'s node
array:

1. element 0 must name **exactly one** node in the hierarchy;
2. each later element must name exactly one node **inside the subtree** of the
   node the previous element matched;
3. zero matches, or more than one, is *unresolved* — never a chosen candidate.

Step 3 is what the installation forces. Reading the container's own hierarchy
over M01's chains shows why a flat lookup is wrong: `ctur1` exists under five
different zeppelins (`blackswanzep`, `multiplayer1zep`, `piratezep`,
`workersvoyagezep`, `multiplayer2zep`), `reng11` under four, `healthy` under
176 nodes — while every chain M01 spells starts from a zeppelin name that
exists exactly once, so the hierarchy pins each of them to a single node. The
chains are **not** adjacent parent/child steps either: `["workersvoyagezep",
"reng1", "healthy"]` skips `move_zeppelin/tilt_zeppelin/rock_zeppelin/gasbag1`,
which is why step 2 is a subtree test rather than a child test.

**Result: 41 of M01's 42 chains resolve. The one that does not is `["player"]`**
— there is no node of that name anywhere in `C1C`. It is recorded
`MemberPresence::Missing`, never resolved to something that happens to share a
name. The `TRAVELERS` anchor `["workersvoyagezep"]` does resolve.

The resolver reports **existence only**. A node's world position is
per-node-type data-section content this tree does not decode, so producing one
here would be an invented number; `MemberFact::position` therefore arrives only
with the host's observation, and the acceptance suite passes the zeroed record
`MemberFact` documents rather than a fabricated transform.

## The writer, and the rules it refuses to bend

`WorldFactTable::facts(&WorldOperands)` answers in this order:

| Case | Row |
| --- | --- |
| no resolver ([`WorldFactTable::drop_resolver`]) | every spelled chain → `Missing` with `[0.0; 3]`, and **no** group, generator or animation row at all |
| chain the world reported this tick | the observed presence and position — whether or not the static hierarchy knows the name (a live object the world holds but the authored hierarchy does not is exactly this case) |
| chain the hierarchy resolves, unobserved | **no row** — the name is real, its state is unknown |
| chain nothing resolves | `Missing` with `[0.0; 3]` |
| group / generator / animation nobody reported | **no row** |

Every one of those is the same fail-closed read `MissionState::holds` already
gives a missing key, which is why an unpopulated map still completes nothing
(`accept_m01_lc_directive_lowering_m01_lowers_launches_and_steps_in_the_runtime`
still observes zero completions, unchanged).

## Which of M01's 58 blocks can now complete, with evidence

Before #751 the answer was "none of the 24": the maps had no writer, so the
answer was `false` forever regardless of the world. The writers remove that
*permanent* block. What each still needs is an observation a host has to
produce, and the acceptance suite (`accept_m01_lc_world_facts_`, 6 tests, 5 of
them retail) is the evidence for the rows that say so:

| Blocks | Can complete when | Evidence |
| --- | --- | --- |
| the **12 `INACTIVE` ladders** | the host observes at least `threshold` of the ladder's members **no longer in play** | `..._a_lowered_block_completes_on_observed_world_state` drives index 53 (`["piratezep"]`, threshold 1) to completion on observed world state; `..._the_same_block_holds_while_the_world_holds_the_member_in_play` holds it on the identical observation with the member still in play |
| the **8 `DEDG` blocks** | the host reports the group's living count (plus any pending spawns — none in M01) at or below `remaining` | `..._the_observed_world_lands_in_the_right_map` pins group ids `1`–`4` into `MissionFacts::groups` with their observed counts, and their `generators` half stays empty because M01 spells no generator |
| the **3 `ANIM_STATE` blocks** | the host reports the named animation's state byte at the wanted code (`RUNNING` 2 / `EXECUTED` 3) | the same test pins all three names into `MissionFacts::animations` with their observed bytes; the names themselves resolve against the mission's own carrier (`mis_anim.zbd` records 191, 507 and 511) |
| `OBJECTIVE3`'s **`TRAVELERS`** | the host reports the subject `["player"]` **in play with a position** inside/outside 700 of `["workersvoyagezep"]` | `..._m01_operands_resolve_against_the_retail_world` pins that `["player"]` resolves against no node, so without such a report it is recorded `Missing` and the block holds — the original's own fall-through for a subject that never resolves (finding B) |

So the count of M01's 58 blocks that can now complete **through a writer that
exists** is all 24 named in bunny-alpha-2's hand-off note; the count that can
complete **on what this engine observes today** is the 12 `INACTIVE` ladders,
and only once a host reports member presence at all — there is no production
mission host in this tree yet (`VS-M01-RUNTIME (#359)`), which is why every
retail case here drives `MissionSession` directly.

The fail-closed half is pinned too: `..._removing_the_resolver_keeps_every_answer_fail_closed`
and `..._a_chain_that_cannot_be_pinned_stays_fail_closed` show that with the
resolver removed nothing is defaulted — no presence, no position, no count —
and that a chain two nodes could answer (`reng1` under two zeppelins) is
recorded absent rather than answered for one of them.

## What is still unknown, kept unknown

1. **The in-play bit's producers** (`+0x24` bit 4) are vehicle/zeppelin spawn
   and despawn code outside the measured bound
   (`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`). This step does not touch
   that unknown: the writer *reports* presence, it never derives it.
2. **`DEDG`'s group id ↔ `SET_AI_NET` name space.** `member->+0x388` is the
   field `SET_AI_NET` writes, and M01 spells net *names* (`devastator_2` →
   `Bravo2`, `devastator_3` → `Charlie2`, `bsfury_1` → `BlackSwan`,
   `blackswanzep` → `SwanZep2`), while `DEDG` compares an *int* id. The
   name → id table the original resolves through is not traced, so this step
   does not derive group membership from `SET_AI_NET`; the group registry is a
   host observation, and a group nobody reports is absent rather than empty.
3. **A node's world position.** Not decoded from the per-node-type data
   section, so `MemberFact::position` only ever carries a host-observed value.
4. **Who observes.** Nothing in `crates/cs_app/src` runs a production mission
   tick yet, so `compose_mission_facts` is exercised by this suite through the
   production API — the same status `ActorFactTable` had before its own
   acceptance landed. `VS-M01-RUNTIME (#359)` is the task that will supply the
   launch path and the observations.

## How to re-derive every number here

```sh
cargo test --workspace --locked -- accept_m01_lc_world_facts_ --include-ignored   # 6 tests, 5 retail
cargo test --workspace --locked -- accept_m01_lc_directive_lowering_ --include-ignored   # the parent suite still observes zero completions from an empty MissionFacts
```

The evidence report for #717 is regenerated with the command in its harness
(`crates/cs_app/tests/evidence_report_m01_lc_directive_lowering.rs`), validated
with `tools/validate_evidence.py ... --require-pass`, and its copy at
`docs/findings/evidence/M01-LC-DIRECTIVE-LOWERING.json` now carries the
world-side fact **writers** in the `review.method` slot that used to carry the
limit.

## Status and limits

* `checked` only when merged: this task awarded itself no `verified_original`
  and no `release_approved`, and no original run has shown M01's objectives
  completing.
* The gate #717 recorded — no fidelity / `verified_original` /
  `release_approved` claim about M01 objective completion while #751 was open
  — is discharged by this merge for its "no production writer" form. The
  refusal itself stands: a writer that *can* complete a block is not evidence
  that the original completed it.
* Resolving task for the remaining observations: **VS-M01-RUNTIME (#359)**.

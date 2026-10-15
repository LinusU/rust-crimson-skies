# M01-PLAY-TRIGGERS-PARTS (#1481): what makes a zeppelin part state in play, and the live objects that carry it

Date: 2026-10-15. Task: `M01-PLAY-TRIGGERS-PARTS` (Rally #1481), the second
slice of `M01-PLAY-TRIGGERS` (#1439). Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) plus build/test. No `gpu`, no `audio`, no original run.
Implementer: bunny-alpha-2. Nothing here is `verified_original`; a Rally merge
awards `checked` only.

## What this slice is, and what it is not

`M01-PLAY-TRIGGERS-OBSERVE` (#1480) installed the world observation writer but
could not see M01's 39 part-state chains: `import_world_container` makes an
object only for an `object3d` record the world node owns directly, so an engine's
`healthy` state node — a grandchild of the zeppelin's own object — had no ECS
entity anywhere. No row was written for those chains, so every `INACTIVE` ladder
that counts them stayed `false` forever and the win/loss chain they feed could
not start.

This slice gives each of those chains a **live part-state object** and folds its
presence through the same observation port:

* `crates/cs_app/src/mission_session/parts.rs` — the module: the component
  [`MissionPartState`], the report, the plan reader (program operands × container
  resolver × bound actors), the designed part node and the presence reader;
* `crates/cs_app/src/mission_actors.rs` — the spawn host binds one part entity per
  planned chain and adds each part's node to its owner's damage graph;
* `crates/cs_app/src/mission_session/observation.rs` — the observer writes one
  member row per bound part, with the presence the session's damage authority
  reports;
* wiring only: the module + re-exports in `mission_session/mod.rs`, and the test
  module registration.

Nothing else changed: no condition, no lifecycle model, no record reader.

## Measurement 1 — the 39 chains, and where each one resolves

`WorldOperands::of(m01_program)` spells **42 distinct member chains**
(`docs/findings/2026-10-08-m01-lc-world-facts.md`). Reading all 42 against the
C1C hierarchy through the production `MemberResolver` on this run gives:

| Group | Chains | Resolves to |
| --- | --- | --- |
| `["player"]` | 1 | **nothing** — no node of that name exists in `C1C` (recorded `Missing`, never guessed) |
| `["piratezep"]`, `["workersvoyagezep"]` | 2 | the zeppelins' own root nodes (slots 517 and 2281) — the two **actor rows** #1480 already writes |
| `[<zeppelin>, <part>, "healthy"]` | 39 | a `healthy` state node under the named part node |

The 39 split as **27 under `workersvoyagezep`** and **12 under `piratezep`**:

* `piratezep`'s 12 are its engine nodes — `reng11`, `reng12`, `reng21`, `reng22`,
  `reng31`, `reng32`, `leng11`, `leng12`, `leng21`, `leng22`, `leng31`, `leng32`
  (resolving to slots 2468, 2480, 2633, 2645, 2800, 2812, 2444, 2456, 2609, 2621,
  2788, 2776);
* `workersvoyagezep`'s 27 are its 18 engine nodes (`reng1`…`reng9`,
  `leng1`…`leng9`) **plus nine turret chains** (`utur1`…`utur4`, `ctur1`…`ctur4`
  and `maingun9`).

Every one of the 39 resolves to exactly one node, and none is ambiguous.

### The record's own declaration, as a second route to the same set

M01's `zeppelins.zrd` records declare the engine nodes by name — the key
`engines`, read through the production carrier census
(`cs_content::pilots::survey_retail_zeppelin_carrier`):

| Zeppelin | `engines` | count |
| --- | --- | --- |
| `workersvoyagezep` | `leng1`…`leng9`, `reng1`…`reng9` | 18 |
| `piratezep` | `leng11, leng12, leng21, leng22, leng31, leng32, reng11, reng12, reng21, reng22, reng31, reng32` | 12 |

So **30 of the 39 chains name an engine node the actor's own record declares**;
the nine remaining `workersvoyagezep` chains name turret nodes (`utur*`, `ctur*`,
`maingun9`) that exist in the container — `ctur1`, for instance, exists under five
different zeppelins — but that **no** record key of M01's `workersvoyagezep` row
declares (its `left_cannons`/`right_cannons` keys are absent). What a turret node
is and which system its destruction disables is therefore not recoverable from
these records; the chains themselves are, and the ladder that counts them is
`OBJECTIVE7`/`OBJECTIVE28` (block indexes 6 and 27).

## Measurement 2 — the authored destruction state, measured from the container

Each chain's leaf is a node named `healthy`. Reading the C1C hierarchy around it
shows what that is:

```
reng9 #4310 [object3d] mesh -1
  healthy #4313 [object3d] mesh -1
    l5 #4314 [lod] -> g15 #4315 [object3d] mesh 1015
    l6 #4316 [lod] -> g16 #4317 [object3d] mesh 1016
  counterspin #4318 [object3d] mesh 1017
  spin       #4319 [object3d] mesh 1018
  propstill  #4320 [object3d] mesh 1019
  supports   #4321 [object3d] -> l8 -> o2 mesh 1020
```

So the part node owns a **`healthy` intact-state subtree** (its drawn LOD bands
under it) beside the spinning/static propeller records, and the wreck each part
becomes is a **separate root-level record**: `wvreng_destroyed.flt`,
`wvleng_destroyed.flt`, `pzreng_destroyed.flt`, `pzleng_destroyed.flt`,
`bsreng_destroyed.flt`, `bsleng_destroyed.flt`, `mp1reng_destroyed.flt`,
`mp2reng_destroyed.flt` (one per airship family, top-level nodes with no parent).
That is the same authored destruction-state shape `planes.zbd` gives an airframe,
where `healthy` and `destroyed` are siblings under the airframe root
(`crates/cs_app/src/playtest_retail.rs`'s `PLAYTEST_AIRCRAFT_INTACT_NODE_NAME`).

**The chain is therefore (actor, part, part-state) with `healthy` naming the
part's intact presentation**: the in-play state the ladder reads is about the
`healthy` node of the part, not about a separate `destroyed` node.

## Measurement 3 — the bit itself, and what is *not* recoverable

`INACTIVE<n>` counts the listed nodes whose `+0x24` bit 4 is **clear**
(`0x469a60`, measured in #1360). The bit is written by `gwNodeSetActive`
(`0x4cca30`, dispatching on the node type at `+0x34`) — and #1360 already
recorded that its producers (spawn, despawn, damage, ownership, scripts) are
untraced, which `cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED` carries.

What *does* this tree read as a producer for these parts? **Nothing**, and that is
a measurement rather than a gap in this pass:

* the `zeppelins.zrd` records state floats for their **gasbags**
  (`workersvoyagezep`: nine rows of `150.0`; `piratezep`: six of `120.0`) and the
  instant-action records carry `cannon_health` rows (`200.0` plus a
  `destroy_*` spelling and two `(0.6|0.3, 60_*|30_*)` state pairs) — but **no
  key states a value for an engine node**;
* no other member of M01's scope spells an engine's health: the objective record
  names the chains and their thresholds, nothing more.

So "an engine is destroyed" has **no measured pool in this installation**. Per
AGENTS.md rule 4 this slice does not invent one; what it does is make the parts
**addressable** — one named damage node each, which is the seam the F27/F29
damage authority already routes a hit through — and report the pool it applies.

## The implemented path

| Piece | What it does |
| --- | --- |
| `parts::plan_part_states(stage)` | reads the stage's own records — the program's `WorldOperands`, the container `MemberResolver`, the launched world-actor program's subjects — and plans one entry per chain that resolves, carries a part element and names a bound actor |
| `mission_actors::spawn_mission_actors_under` | builds each zeppelin's damage graph with one designed node per planned part (the frame node #1435 declared is unchanged) and spawns one `MissionPartState` entity per chain at the actor's own bind-time pose |
| `observation::mission_world_observation` | writes one member row per part entity: `held_in_play` when the authority says `Intact`/`Damaged`, `no_longer_in_play` when it says `Destroyed`, **no row** when the pool is unresolved or the actor is unregistered |
| `mission_actors::despawn_mission_actors` | takes the part entities and the report with the actors on restart/teardown |

The fail-closed row rules are the point of the design: an unobserved chain is
*not* a cleared member (`cs_script::runtime::MissionState::holds` counts only
`OutOfPlay` rows), so removing the provider — the plan, the spawn, the entity —
stalls every ladder exactly as it did before this slice.

### The designed value, and why there is one

A damage node whose pool is `Resolved::Unknown` **blocks every hit**
(`DamageResolver::resolve`'s `HitBlocked` arm), so a mission whose parts have no
measured pool cannot show a part leaving play at all. [`DESIGNED_PART_INTEGRITY`]
(a single unit, claim `m01-play-triggers-parts.designed-part-integrity`) is
therefore a **labelled designed** value, the same shape M01-PLAY-WEAPONS (#1437)
used for its designed loadout: it is what lets the destruction path run, it is
**not** a claim about the original's engine hit points, and every part gets the
same unit so it is explicitly not a per-part tuning table. F27-E's measured
damage table replaces it.

The part nodes also declare no `disables` system edge and use
`DamageNodeKind::InternalStructure` for every part: nine of the 39 chains name
turret nodes no record key declares, so a class or system read from the name
would be a guess for those, and the zeppelin's own motion is `Held`.

## The ladders this unblocks, measured from the lowered program

| Block index | Chains | Count | Threshold |
| --- | --- | --- | --- |
| 5 (`OBJECTIVE6`) | `workersvoyagezep` engines | 18 | 1 |
| 6 (`OBJECTIVE7`) | `workersvoyagezep` turrets | 9 | 1 |
| 7 (`OBJECTIVE8`) | `workersvoyagezep` engines | 18 | 5 |
| 8 (`OBJECTIVE9`) | `workersvoyagezep` engines | 18 | 9 |
| 27 (`OBJECTIVE28`) | `workersvoyagezep` turrets | 9 | 9 |
| 35–38 (`OBJECTIVE36`–`39`) | `piratezep` engines | 12 | 2 / 6 / 10 / 12 |
| 54 (`OBJECTIVE55`) | `workersvoyagezep` engines | 18 | 2 |
| 53, 55 (`OBJECTIVE54`, `56`) | `["piratezep"]` — the **actor**, not a part | 1 | 1 |

Eight of the twelve `INACTIVE` ladders now read live part rows. The last row is
the two actor-level ladders: they name the zeppelin's **own** in-play bit, which
is #1480's `["piratezep"]` actor row plus a world-actor destruction writer — a
different fact family, named as a residue below.

## What the acceptance suite proves

`crates/cs_app/tests/campaign/m01_play_triggers_parts.rs`, prefix
`accept_m01_play_triggers_parts_` — 6 tests (4 run in CI, 2 retail):

* `…_m01_binds_one_live_part_per_spelled_chain` (retail) — the composed run binds
  39 parts (27 + 12), every one observed in play, every one an intact node on its
  owner's graph, and the two actor-level operands bind no part;
* `…_a_destroyed_engine_completes_its_ladder_and_not_the_others` (retail) — a
  `workersvoyagezep` engine destroyed through the **session damage authority**
  takes its chain out of play, completes block 5 (threshold 1), leaves block 7
  (threshold 5) and block 35 (`piratezep`, threshold 2) incomplete, and four more
  engines then complete block 7 — the block block 5's own completion wakes;
* `…_the_synthetic_ladder_completes_on_the_live_worlds_own_state` (CI) — the
  production binder runs on a synthetic stage (one bound world actor, two chains,
  a synthetic container resolver), the ladder completes on the row the production
  observer wrote, and the part entity stays live: leaving play is a state, not a
  despawn;
* `…_a_ladder_whose_threshold_is_not_met_stalls_exactly_at_it` (CI) — one of two
  cleared members is exactly one short;
* `…_the_wrong_chain_and_a_missing_provider_never_satisfy_a_ladder` (CI) — a
  ladder over one chain is never satisfied by the other going out, and with the
  provider's entity removed the chain is unobserved and the ladder stalls;
* `…_an_unresolved_pool_writes_no_row` (CI) — the fail-closed read: an unresolved
  pool is not a statement about presence, so the chain gets **no row** and
  `part_in_play` answers `None`.

The destruction in every leg is a `HitEvent` resolved by
`DamageResolver::resolve` — the seam the F27 gun step and the F28 ordnance step
publish into — never a despawn from the test.

```sh
cargo test --workspace --locked -- accept_m01_play_triggers_parts_ --include-ignored   # 6 tests, 2 retail
```

## Evidence

The acceptance run of record (its recorded log is the first artifact):

```sh
set -o pipefail
cargo test --workspace --locked -- accept_m01_play_triggers_parts_ --include-ignored \
  2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-PARTS/cargo-test.log
```

then the harness, on the same clean commit, and the structural check:

```sh
CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-PARTS \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_parts_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_parts -- --ignored
python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS-PARTS/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-TRIGGERS-PARTS
```

`--require-pass` is deliberately **not** used: it refuses any report whose
`unknowns` array is non-empty (`tools/validate_evidence.py`), and this report
carries four unresolved issues that must stay in machine-readable evidence (the
unmeasured engine pool, the actor-level ladders, the absent geometric producer
and #1439's windowed criterion). Deleting them to turn the flag green is exactly
what the owner directive forbids. The report's `claim` is `implemented`, its
capabilities are `[retail, synthetic]` — no `gpu` capability is claimed — and the
pass/fail facts are carried by `command.exit_code` and the `tests` counts, which
the plain validation does check.

The second artifact is `part-states.json`, a second production observation on the
same tree: every bound part with its chain, actor, node, authority state and
pose; the record's own `engines` lists as the cross-check; and the `INACTIVE`
ladder census read from the lowered program.

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| the original's per-engine integrity (no key states one) | the pool every part damage node carries, i.e. how many hits a part survives | #668 (the measured damage table), F27-E |
| the in-play bit's producers (`+0x24` bit 4) | what the original does when an engine is hit, before `gwNodeSetActive` clears the node | #1360's open item; an original run (#358) |
| the two actor-level ladders (block indexes 53, 55) | `OBJECTIVE54`/`OBJECTIVE56` over `["piratezep"]` — the zeppelin's own in-play bit | a world-actor destruction rule (#1435/#668) or an original run |
| the parts have no swept-box geometry or declared allegiance | a *geometric* producer destroying a part in the composed run: the guns refuse a shot at M01's zeppelins by name (their records state no team, so nothing declares them hostile) | F30-E (allegiance), F27-B's part geometry |
| #1439's windowed firing criterion | firing the proximity trigger in the windowed mission (the start is 5 311 m from the anchor against a 700 m radius) | M01-PLAY-TRIGGERS-WINDOWED (#1486) |

## Status and limits

* `checked` only when merged: no `verified_original` and no `release_approved`
  is claimed anywhere, and no original executable ran.
* The part entities carry the owning actor's own bind-time pose: the per-node
  authored transform of a container record the world import makes no object for
  is not decoded here, so no offset is invented. M01's zeppelins declare
  `DeclaredMotion::Held`, so the pose does not drift.
* The observer reports a part the authority calls `Intact` or `Damaged` as **in
  play**. That is this reimplementation's contract for the engine's active bit
  (the entity is live and the node has not been destroyed); nothing here claims
  the original set the bit the same way, and the bit's producers stay untraced.
* The destroyable-parts mechanism is **designed**: a labelled unit pool. Every
  `INACTIVE` number this slice makes reachable is reachable *given* that pool;
  no ladder is claimed to advance in the original, whose destruction rule is
  unmeasured.

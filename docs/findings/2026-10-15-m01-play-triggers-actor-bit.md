# M01-PLAY-TRIGGERS-ACTOR-BIT (#1490): what takes a bound zeppelin out of play, and the row that reads it

Date: 2026-10-15. Task: `M01-PLAY-TRIGGERS-ACTOR-BIT` (Rally #1490), the third
slice of `M01-PLAY-TRIGGERS` (#1439), after `OBSERVE` (#1480) and `PARTS`
(#1481). Capabilities used: `retail` (read-only `$CS_GAME_DIR`) plus
build/test. No `gpu`, no `audio`, no original run. Implementer: mimo-1.
Nothing here is `verified_original`; a Rally merge awards `checked` only.

## The defect this slice repairs

Two of M01's twelve `INACTIVE` ladders name a **one-element** chain —
`["piratezep"]`, the zeppelin's own node, not a part:

| Block index | Chain | Count | Threshold |
| --- | --- | --- | --- |
| 53 (`OBJECTIVE54`) | `["piratezep"]` | 1 | 1 |
| 55 (`OBJECTIVE56`) | `["piratezep"]` | 1 | 1 |

The evaluator counts a member whose node's `+0x24` bit 4 is clear (`0x469a60`,
measured in #1360). #1480's observer wrote the actor row `held_in_play` from
the binding entity alone and **nothing could ever take it out**, so both
ladders stalled forever and the win/loss chain they feed could not start.

## Measurement 1 — what the composed run can and cannot destroy today

Three routes were measured, two of them closed:

* **The engine's bit writers are untraced.** `gwNodeSetActive` (`0x4cca30`)
  producers (spawn, despawn, damage, ownership, scripts) remain untraced
  (`cs_script::ir::IN_PLAY_BIT_WRITERS_UNTRACED`, #1360's open item). What
  the *original* does to clear a zeppelin's own node stays **unknown**.
* **The damage route into the actor is blocked by design.** #1435's zeppelin
  damage graph is a single `airframe` node whose pool is `Resolved::Unknown`,
  and a node with an unresolved pool blocks every hit
  (`DamageResolver::resolve`, the `HitBlocked` arm). No hit can produce a
  zeppelin destruction today, and this slice invents **no** integrity pool
  (#668 / F27-E still own the measured one).
* **The world-actor session's `Destroy` arm cannot fire inside a composed
  run.** The session's own lifecycle is the measured F34 surface:
  `WorldActorCommand::Destroy` → `WorldActorSet::destroy` →
  `Presence::Destroyed`, monotonic (`crates/cs_app/src/world_actors.rs`,
  `crates/cs_sim/src/world_actors/runtime.rs`). But `MissionHost::step`
  stage 2 steps the session "with no commands and no probes — this stage
  schedules nothing on its behalf", and `MissionHost` exposes no command
  port and no `&mut` session (`host.rs`: only `world_actors(&self)`).
  Measured: **in a composed run the session presence of every bound actor
  stays `Intact` for the whole mission.**

## Measurement 2 — the destruction ledger that *is* reachable

The session damage authority keeps an actor-level **five-way lifecycle
ledger**, and `crates/cs_app/src/damage.rs` calls it "the authoritative
record of an actor's destruction":

* a lethal hit records `LifecycleKind::Destroyed` into it
  (`DamageResolver::resolve`, resolver.rs);
* so does `DamageResolver::record_lifecycle`, the resolver's own documented
  port for "**a scripted destruction … that no hit produced**";
* `is_destroyed(actor)` reads it back, and the F36-C.1 publication
  (`publish_destroyed_actors` → `DamageActorDestroyed`) forwards exactly
  this ledger fact to every consumer (interaction, targeting, roster).

So the one destruction fact about a bound world actor that a composed run
can actually move — today, without inventing a pool — is this ledger.

## The implemented read

`mission_session::observation::world_actor_in_play(session, world_actor,
damage, damage_actor)` replaces the binding-only default, and
`mission_world_observation` writes each bound actor's row from it:

| World's own state | Row |
| --- | --- |
| world-actor session holds the actor `Presence::Destroyed` | **out of play** (the `INACTIVE` read) |
| damage ledger recorded the actor's `Destroyed` | **out of play** |
| session holds the actor `Presence::Intact`, ledger silent | **in play** |
| no session, or the session does not hold the actor | **no row at all** (fail-closed) |

Two rules the design keeps:

* **An unresolved pool is not a presence statement.** The actor's `airframe`
  pool (`Resolved::Unknown`) is never consulted — the row comes from the two
  measured fact families above, never from the pool and never by default.
* **Fail-closed.** A binding the world's own lifecycle state does not back
  (a bare entity, an actor id the session never registered) gets no row,
  exactly like a part whose pool nothing resolved (#1481's rule).

The entity wiring is unchanged: the observer reads the damage actor from the
`MissionZeppelinActor` the spawn host already stamps on every bound zeppelin.

## Measurement 3 — the engine model gates the two ladders behind wakes

The `INACTIVE` condition holding is not enough for a block to complete: the
engine lifecycle model arms each block. Measured from M01's recovered
declarations on this run:

| Block | `OBJECTIVE` | Starts | Woken by |
| --- | --- | --- | --- |
| 53 | `OBJECTIVE54` | **Dormant** | block 1 (`OBJECTIVE2`, the `DEDG` group-1 block, **Awake**) |
| 55 | `OBJECTIVE56` | **Dormant** | block 21 (`OBJECTIVE22`, **Dormant**), which is woken only by block 20 (`OBJECTIVE21`, **Dormant**) |
| 54 | `OBJECTIVE55` | **Awake** | — (the part ladder over `workersvoyagezep`'s engines) |

The probe behind the table, read from M01's recovered declarations on this
run (`tmp_probe_engine_states_for_actor_ladders`, removed after the
measurement; re-derivable through the same production reader):

```text
PROBE block 1:  state=Awake  wake=[40, 51, 53] …
PROBE block 20: state=Dormant  (WAKES 21)
PROBE block 21: state=Dormant  wake=[55] kill=[57]
PROBE block 53: state=Dormant
PROBE block 54: state=Awake
PROBE block 55: state=Dormant
```

So `OBJECTIVE54` can advance in the composed run once group 1's depletion
completes `OBJECTIVE2` (its measured wake), and `OBJECTIVE56` sits behind
the dormant `OBJECTIVE21` → `OBJECTIVE22` chain, which this task does not
drive and does not invent (what wakes block 20 was not measured in this
pass). The retail acceptance leg drives the first chain to completion and
pins block 55's Dormant state instead of faking its completion.

## What this advances, and what it does not claim

* The **ladder condition** of blocks 53 and 55 now holds from the live
  world's own state as soon as `piratezep`'s destruction is recorded — the
  part that stalled forever before this slice.
* **Block 53 completes** in the composed run when its measured wake
  (`OBJECTIVE2`) has armed it; block 55's *completion* stays gated by
  `OBJECTIVE22`'s wake chain (Measurement 3), recorded rather than faked.
* **Designed vs measured:** the *row* is a read of two measured fact
  families; no pool, no tuning table and no rule was invented. What the
  *original* does to clear the bit is still untraced (residue below), so
  nothing here claims the original advanced these ladders this way.
* A **gun kill of a zeppelin** still cannot happen in a composed run (the
  unknown airframe pool blocks hits; M01's records state no team, so the F27
  gun step refuses a shot at them by name). The retail acceptance leg drives
  the destruction through the ledger's scripted port — the same seam the F36
  publication reads — and says so.
* The session-presence `Destroyed` arm is read and correct but cannot fire
  inside a composed run today (Measurement 1); the drivable route is the
  ledger.

## What the acceptance suite proves

`crates/cs_app/tests/campaign/m01_play_triggers_actor_bit.rs`, prefix
`accept_m01_play_triggers_actor_bit_` — 4 tests (3 run in CI, 1 retail):

* `…_a_scripted_destruction_takes_the_actor_out_of_play` (CI) — the
  production binder binds the synthetic actor, the ladder over its
  one-element chain cannot hold while the row is in play; the moment
  `record_lifecycle(Destroyed)` lands in the production damage authority the
  observer reports the row out of play, the ladder completes, and the
  binding entity stays live (leaving play is a state, not a despawn);
* `…_the_wrong_actor_never_satisfies_the_ladder` (CI) — two bound actors;
  the decoy's destruction is reported and never satisfies the ladder over
  the other chain;
* `…_an_actor_the_world_does_not_hold_gets_no_row` (CI) — a bare binding
  whose actor id the session never registered gets **no row** and its ladder
  stalls; the reader itself answers `None` for a missing session and for an
  unregistered actor, and `Some(true)` for a held one;
* `…_m01_s_actor_level_ladders_read_the_piratezep_row` (retail) — M01's
  three zeppelins are observed in play and the measured wake gating is
  pinned (block 53 Dormant, block 55 Dormant, block 54 Awake); group 1's
  depletion completes `OBJECTIVE2` and arms block 53 while the ladder still
  cannot hold; the scripted destruction then takes exactly `piratezep`'s row
  out and completes block 53, while `workersvoyagezep` stays in play, block
  54 stays put and block 55 stays behind its own measured wake.

```sh
cargo test --workspace --locked -- accept_m01_play_triggers_actor_bit_ --include-ignored   # 4 tests, 1 retail
```

## Adjacent change: the observe suite's anchor is a launched actor

`m01_play_triggers_observe.rs`'s synthetic trigger leg used to spawn a bare
`WorldActorBinding` by hand; under the fail-closed read such a binding gets
no row, so the leg now registers the harbor anchor in the synthetic stage's
**own world-actor program** and lets the production spawn host bind it —
strictly more production path than before. Its assertions are unchanged.

## Evidence

The acceptance run of record (its recorded log is the first artifact):

```sh
set -o pipefail
cargo test --workspace --locked -- accept_m01_play_triggers_actor_bit_ --include-ignored \
  2>&1 | tee private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT/cargo-test.log
```

then the harness, on the same clean commit, and the structural check:

```sh
CS_EVIDENCE_DIR=private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT \
CS_CANDIDATE_TREE=$(git rev-parse 'HEAD^{tree}') \
CS_EVIDENCE_ARGV="cargo test --workspace --locked -- accept_m01_play_triggers_actor_bit_ --include-ignored" \
CS_EVIDENCE_EXIT_CODE=<status> CS_EVIDENCE_REVIEWER=<identity> \
  cargo test --locked -p cs_app --test evidence_report_m01_play_triggers_actor_bit -- --ignored
python3 tools/validate_evidence.py private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT/acceptance.json \
  --artifact-root private/evidence/M01-PLAY-TRIGGERS-ACTOR-BIT
```

`--require-pass` is deliberately **not** used: it refuses any report whose
`unknowns` array is non-empty, and this report carries five unresolved issues
that must stay in machine-readable evidence (the untraced bit producers, the
unreachable session `Destroy` arm, the unresolved zeppelin pool, block 55's
wake chain behind `OBJECTIVE22`, #1439's windowed criterion). The report's
`claim` is `implemented`, its capabilities
are `[retail, synthetic]`, and the pass/fail facts are carried by
`command.exit_code` and the `tests` counts.

The second artifact is `actor-bit.json`: per bound zeppelin, the world-actor
session's own lifecycle presence, the damage authority's destruction ledger
state and the observer's in-play row, beside the `INACTIVE` ladder census
read from the lowered program.

## Unknowns, affected content and resolving work

| Unknown | Affected content | Resolves in |
| --- | --- | --- |
| what the original does to clear the zeppelin's own in-play bit (`+0x24` bit 4, `gwNodeSetActive` producers untraced) | the original semantics of `OBJECTIVE54`/`OBJECTIVE56` | #1360's open item; an original run (#358) |
| the composed run has no world-actor command port, so the session's `Destroy` arm cannot fire mid-mission | any mission that should destroy a world actor from the mission side (the observer reads the arm, but nothing can drive it) | an F34 command port wired into the composition, or a measured damage rule for zeppelins |
| the zeppelin `airframe` pool stays `Resolved::Unknown` (#1435) | a live gun kill of a zeppelin: routed hits block | #668 (the measured damage table), F27-E |
| `OBJECTIVE56` is woken only by block 21 (`OBJECTIVE22`, Dormant), which is woken only by block 20 (`OBJECTIVE21`, Dormant) — what wakes block 20 was not measured in this pass | block 55's **completion** in a live M01 run (its ladder condition itself holds from this task's row) | the probe block in this finding; the M01-B/F39 wake-chain work over `OBJECTIVE21`/`OBJECTIVE22` |
| #1439's windowed firing criterion | firing the proximity trigger in the windowed mission | M01-PLAY-TRIGGERS-WINDOWED (#1486) |

## Status and limits

* `checked` only when merged: no `verified_original` and no
  `release_approved` is claimed anywhere, and no original executable ran.
* The row the observer writes is **this reimplementation's contract** for
  the engine's active bit on a bound world actor: in play while the world's
  own state holds it, out of play once its destruction is recorded, no row
  when the world says nothing. Nothing here claims the original set the bit
  the same way.
* No integrity pool, no per-actor tuning and no despawn-from-test appears
  anywhere in the change: every destruction in the suite enters through the
  production damage authority's own scripted-destruction port.

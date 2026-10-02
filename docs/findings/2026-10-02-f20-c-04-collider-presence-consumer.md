# F20-C.04: the collision-side consumer of a clip-hidden node

Date: 2026-10-02. Task: F20-C.04 "Decide and wire the collision-side consumer of
a clip-hidden node (`ColliderVerdict::NoCollider`)" (#504). Spec:
`specs/F20-object-animation-and-authored-destruction-states.md`, section
`### F20-C`, non-negotiable behavior 3, acceptance AC01. Shared contract:
`docs/contracts/FLIGHT-PHYSICS.md` (and `docs/contracts/IDENTITY-CONTENT.md`
for the generation-stamped binding the animation path already verifies).
Capabilities used: ordinary build/test only — no `CS_GAME_DIR` read, no render,
no audio, so no `private/evidence/` report is produced.

## The decision this stage records

F20-A's evaluator decides `hidden ⇒ no collider`, and F20-C.03 composed that
into `ColliderVerdict::NoCollider`, read out of
`composed_visibility_verdict`. **Nothing read it.** The engine had no
collision-enable record at all — `grep -rn CollisionEnabled crates/` was empty
— and the F20-C.03 slice deliberately did not invent one, because the original's
coupling of visibility to collision is unmeasured and writing Avian's component
from `cs_app::animation/` would have coupled a playback to a physics body on an
unmeasured semantic.

The decision, stated plainly:

> **`NodeColliderPresence` is a policy record the physics adapter owns.**
> The animation path keeps publishing the verdict and never touches it; the
> collision layer reads the composed verdict, merges it into that record, and
> projects the record onto Avian 0.7's `ColliderDisabled` marker. A damage
> removal recorded in that record is **terminal**: the merge can neither enter
> nor leave it, so no clip can re-enable a collider the damage side removed.

### The three candidates, weighed

1. **The authored `CollisionRole` on F11-C's `PartBinding`.** Rejected. It is a
   *content* record owned by the scene import
   (`cs_content::scene::CollisionRole`, `Resolved<CollisionRole>`), and it
   cannot express "removed by damage" as distinct from "not authored" — two
   different decisions that need different owners. Writing it would also
   re-decide another stage's record from an animation pass.
2. **Avian's collision component, written from the animation path.** Rejected,
   and the reason is the point of the task: the animation path stays a
   *publisher*. It writes `NodeAnimatedVisibility` and nothing else, so the
   coupling between a clip and a physics body is never made inside the
   playback, and no animation module names an Avian type.
3. **A policy record the physics adapter owns, which a consumer reads.** Chosen.
   One writer, one projection, and a merge rule that is a pure function.

A correction to the task's own wording is worth recording: **Avian 0.7 has no
`CollisionEnabled` component.** Its mechanism is the removal-side marker
`ColliderDisabled` (`avian3d-0.7.0/src/collision/collider/mod.rs:394`), whose
`On<Add, ColliderDisabled>` / `On<Remove, ColliderDisabled>` observers add and
remove the collider in the broad-phase AABB tree
(`avian3d-0.7.0/src/collider_tree/update.rs:196-204`). The record is therefore
written by inserting and removing `ColliderDisabled`; "enabled" is its absence.
Both directions are exercised and counted in the tests, so the asymmetry is
measured rather than assumed.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/physics/collider.rs` (**new**): the whole consumer —
  `NodeColliderPresence` (with `merge`, `collider_enabled`, `label`),
  `ColliderPresenceReport`, `ColliderPresenceLedger`,
  `apply_collider_presence`, `apply_collider_presence_on_fixed_tick`,
  `ColliderDecisionError`, `remove_collider_for_damage`,
  `restore_collider_after_repair`, `ColliderPresencePlugin`.
- `crates/cs_app/src/physics/mod.rs`: the module declaration, the re-exports and
  one paragraph in the module docs (wiring only).
- `crates/cs_app/tests/accept_f20_c_04_collider_presence_consumer.rs` (**new**):
  the 7 `accept_f20_c_04_*` tests.
- This file.

**One observable failure, before the change:** a declared clip that hides a node
at its authored tick leaves that node's collider in the simulation forever. A
probe flying at the node is stopped by it and the production contact reporter
reports the contact, although the node is not drawn — the invisible obstacle
F20-A's rule exists to prevent, with a verdict that existed and no reader.

## The merge rule

| the record says | composed collider verdict | merged | engine marker |
| --- | --- | --- | --- |
| `Live` | `Undecided` | `Live` | absent |
| `Live` | `NoCollider` | `HiddenByAnimation` | present |
| `HiddenByAnimation` | `NoCollider` | `HiddenByAnimation` | present |
| `HiddenByAnimation` | `Undecided` | `Live` | absent |
| `RemovedByDamage` | `Undecided` **or** `NoCollider` | `RemovedByDamage` | present |

1. **`RemovedByDamage` is terminal for the pass.** It is not a priority rule a
   later refactor could reorder: the merge has no arm that leaves it. Only
   `remove_collider_for_damage` and `restore_collider_after_repair` write it.
   That is F20 non-negotiable behavior 3, structurally.
2. **The clip's own hide is the only removal on the animation's account**, and
   the clip's own show the only thing that lifts it. The merged state comes from
   the *clip's* fact (`NodeAnimatedVisibility` through
   `composed_visibility_verdict`), never from the draw half of the verdict, so a
   node LOD culls keeps its collider.
3. **`Undecided` never removes anything.** The moment a clip stops hiding a node
   — or stops driving it at all (teardown releases the record) — the authored
   collider is back.
4. **A repair recomputes through the same rule.** `restore_collider_after_repair`
   lifts the damage removal and hands the *current* verdict to
   `NodeColliderPresence::merge`, so a repair under a still-hiding clip leaves
   the collider off. Writing `Live` unconditionally would reach the invisible
   obstacle from the other side; probe P7 removes that and the test fails.

## What this layer takes no answer from

The **collision half** of the composed verdict is computed from the clip's own
record alone, so this layer takes **no** answer from the records below. It does
read them transitively — `composed_visibility_verdict` composes all three and the
collider half discards the other two — and the distinction matters to anyone
auditing this: what is forbidden here is *deciding* from them, not *reading*
them. (Review fix: the first draft of this section said the layer "does not
read" `NodeDisabled` and `NodePresentation`, which is not true of the
composition, and would have sent a future auditor looking for a violation that
does not exist.)

- **`NodeDisabled`** (F11-C's damage marker). F11-C states that `NodeDisabled`
  decides presentation and that "collision, weapon origins and damage identity
  read their own records"; `crates/cs_app/src/scene.rs` says the same. A
  destroyed node's collider is F29's decision and arrives through
  `remove_collider_for_damage`, which this layer applies and does not make.
  Inferring collision from the marker would give the answer a second reader and
  turn the clip-vs-damage priority into a race.
- **`NodePresentation`** (F11-C's LOD record), rewritten every LOD pass:
  collision must not move with distance. The LOD acceptance test pins both
  directions — a culled node is still an obstacle, and a clip that hides a
  culled node still takes the collider away.
- **`AirframeDamageState`**: `apply_airframe_damage` is its single owner and
  projects it onto the per-entity markers; reading the resource would re-decide
  a part identity that outlives a scene load.

## The boundary: the record is opt-in, and its cost is stated

A node is managed **iff it carries `NodeColliderPresence`**. The spawner inserts
`Live` for a node whose authored `CollisionRole` is `Collider`; the pass manages
that node's marker and nothing else. That is what keeps "this layer never
re-enables a collider it did not remove" true for the rest of the engine, and it
is why the damage seam refuses an unmanaged node by name
(`ColliderDecisionError::UnmanagedNode`) rather than silently doing nothing.

Two consequences of "the record is the answer, the marker is its projection", both
added to the module docs at review because a caller has to know them:

1. **The managed entity must be the one carrying the clip's record and the
   engine's collider.** The pass reads `NodeAnimatedVisibility`, the presence
   record and Avian's `Collider` from the **same** entity, so a spawner that put a
   scene node's visibility on one entity and its Avian body on another would get
   a policy that never sees the clip. This is the constraint the spawn wiring
   below has to design against.
2. **For a managed node the record outranks the marker.** A `Live` record
   removes a `ColliderDisabled` marker another system put there, on the next
   pass — so a system that wants a managed collider out of the simulation writes
   this record, not the engine's component. Deliberate: there is no second writer
   of the marker.

`NodeColliderPresence` is deliberately **not** `Default` (review fix). `Live` is
the permissive end of the opt-in, so a derived `Default` would let an
`insert_default` adopt a node and re-enable its collider without a spawner
having chosen to.

The cost is real and is not hidden: **a clip-hidden node whose spawner never
inserted the record keeps colliding.** That is a wiring requirement on the spawn
path — F11-C's scene import, or F29's part colliders — and **neither attaches an
Avian collider to a scene node today**, so no production path can yet insert the
record. The acceptance test asserts the boundary from both sides (the pass does
not adopt an unmanaged node, and the damage seam refuses it) rather than letting
the gap pass unnoticed. It is filed as a follow-up rather than solved here,
because a scene-node collider is F11-C/F29's owner path, not to this task's.

## Where the pass runs, and the one-tick offset

`ColliderPresencePlugin` installs `apply_collider_presence_on_fixed_tick` in
`FixedPostUpdate` after `PhysicsSystems::StepSimulation` — the same fixed-tick
slot as the animation advance — and ordered **after**
`advance_animation_on_session_tick`, so the collision layer reads the verdict
the animation produced in this tick rather than the previous tick's. The engine
marker Avian honours from the **next** tick's broad phase; that offset is the
earliest the pinned engine can honour it, because the animation advance itself
runs after the step (F20-C.02's designed placement, and an unmeasured original
one). A late change is a late update, never a wrong one: the record is the
answer and the marker follows it every tick.

**The `.after(advance_animation_on_session_tick)` constraint is not pinned by any
test, and cannot be** (review finding, probe M4): deleting it leaves the whole
acceptance selection green, because Bevy happens to run the animation advance
before the presence pass in this world anyway. Losing it would make the *record*
trail the clip by one tick — a latency rather than a wrong answer, since the pass
converges on the next tick — so no acceptance criterion is unmet by its absence.
It is documented in the module as **do not remove** instead of being papered over
with a structural assertion on Bevy's schedule internals, which would be brittle
and has no precedent in this crate.

## Tests

| test | what it pins |
| --- | --- |
| `accept_f20_c_04_a_clip_hidden_node_stops_colliding_and_its_show_restores_the_collider` (minimum) | the AC01 shape on the collision channel: a shown node stops a real probe and the production contact reporter reports the contact; the clip's hide tick takes the collider out and a second probe flies through with **no** contact reported; the show tick puts it back and a third probe is stopped again. The engine marker is asserted in every phase, and the observed writes are exactly one disable and one enable across ~120 fixed ticks |
| `accept_f20_c_04_a_damage_removed_collider_is_never_restored_by_a_clip_loop_or_a_lod_pass` | non-negotiable 3: after the damage removal, four loop passes (re-shows **and** re-hides counted, so the loop really is driving the channel) leave the record `RemovedByDamage` and the marker in place; two distances, the real LOD pass and the instance teardown change nothing; a probe is not stopped while removed, and the repair puts the obstacle back |
| `accept_f20_c_04_a_repair_under_a_still_hiding_clip_leaves_the_collider_off` | the other half of the merge: a repair under a still-hiding clip lands on `HiddenByAnimation` with the marker still present, confirmed by a probe flying through; the show tick then restores it |
| `accept_f20_c_04_a_clip_teardown_returns_the_collider_a_hiding_clip_had_removed` (added at review) | the third arm of the merge — `Undecided` never removes anything — on the production release path: the instance teardown releases the clip's own record, the composed verdict goes back to `Undecided` (asserted), one more fixed step restores the record and the marker, the teardown cost **exactly one** engine write, and a probe is stopped again |
| `accept_f20_c_04_the_merge_rule_never_lets_a_clip_leave_a_damage_removal` | the whole truth table as a pure function, including that **both** verdicts leave `RemovedByDamage` alone, and that `collider_enabled`/`label` agree with each state |
| `accept_f20_c_04_applying_the_same_verdict_twice_changes_nothing_on_the_physics_side` | idempotence on three channels: the pass's own report (`is_noop`), the engine's add/remove hooks counted by Bevy observers, and the plugin's ledger totals; a repeated removal and a repeated repair are equally quiet. The engine counter is what makes it more than a value comparison |
| `accept_f20_c_04_the_damage_seam_refuses_a_node_the_spawner_never_managed` | the opt-in boundary: an unmanaged body carrying a real collider and a hidden `NodeAnimatedVisibility` (the composed verdict **is** `NoCollider` there, asserted, so the pass declined rather than saw nothing) is not adopted and never written to; the damage seam returns `UnmanagedNode` and a despawned entity returns `UnknownEntity`; the managed node beside it is unaffected |
| `accept_f20_c_04_an_lod_cull_keeps_the_collider_while_a_clip_hide_takes_it_away` | LOD is presentation only: at the far distance the real LOD pass really rewrites the record to `LodCulled` (asserted, so the rest means something) and the node still stops a probe; the clip's hide then takes the collider away **while culled**, so a render and a collision consumer cannot disagree about whether the clip hid it |

Each phase that fires a probe **retires the probe it just used** (`retire`).
`projectile`×`projectile` is a designed contact pair, so a probe left resting
against the node exchanged momentum with the next one down the same lane: the
first draft's second probe lost 40% of its speed to the leftover body and only
reached the far face with 60% of the approach margin to spare. The measurement
was sound — a slowdown cannot fake a "flew through" — but a phase whose observed
speed is set by an unrelated body is not the clean observation it reads as, so
every probe now flies into a lane holding nothing but the node.

## Mutation probes

Run locally with a rerunnable driver: each probe edited one production file,
ran `cargo test -p cs_app --locked --test accept_f20_c_04_collider_presence_consumer`,
recorded the exit code and the failing test names, and restored the file with
`git checkout --`. `grep -rn "MUTATION PROBE" crates/` returns nothing and
`git status` shows no probe edit afterwards.

| probe | edit | tests that fail (of 7) |
| --- | --- | --- |
| P1 the merge ignores the clip's verdict | `apply_collider_presence` merges `ColliderVerdict::Undecided` instead of `verdict.collider()` — the pre-task state, where the verdict had no reader | 5: the minimum, the destruction, the repair-under-hide, the idempotence and the LOD-cull tests |
| P2 a damage removal is not terminal | `merge` gains arms that let `RemovedByDamage` become `HiddenByAnimation`/`Live` | 2: the merge rule and the destruction loop |
| P3 the pass ignores the opt-in | the query becomes `With<Collider>` **and** a missing record defaults to `Live` (both halves: either alone is inert) | 1: the boundary test |
| P4 the engine marker is rewritten every pass | the "already in the requested state" test is short-circuited to false | 4 |
| P5 the policy record is rewritten every pass | `record` is hardcoded true | 2: the boundary and the idempotence tests |
| P6 the damage seam stops refusing | the `UnmanagedNode` check is short-circuited | 1: the boundary test |
| P7 a repair re-enables a hidden node | `restore_collider_after_repair` writes `Live` without re-merging | 1: the repair-under-hide test |
| P8 the composed verdict stops reporting `NoCollider` | `VisibilityVerdict::compose` answers `Undecided` for a hidden node (F20-C.03's seam, edited to show the coupling) | 6: everything except the pure merge table |

P3 is the one worth noting: the boundary is only observable when the pass would
otherwise act, so the test asserts the composed verdict *is* `NoCollider` on the
unmanaged node before asserting that nothing happened to it.

### The reviewer's own probes

Run by the reviewer (bunny-2, reviewing its own implementation) against the
submitted tree, each restored afterwards; `grep -rn "REVIEW PROBE" crates/`
returns nothing and `git status` shows no probe edit.

| probe | edit | result |
| --- | --- | --- |
| M1 the animation record leaves the collision decision | `apply_collider_presence` merges `ColliderVerdict::Undecided` instead of `verdict.collider()` (independently of P1) | selection FAILED, **5 of 7** — this is the acceptance criterion "removing the animation record from the world's collision decision fails the test" |
| M5 the record is maintained but never reaches the engine | `apply_state`'s marker branch is short-circuited to `false`, so the record updates and Avian's marker is never written | selection FAILED, **5 of 8** — the tests observe the collision channel, not this layer's own record |
| M4 the pass is no longer ordered after the animation advance | the plugin keeps `.after(StepSimulation)` and drops `.after(advance_animation_on_session_tick)` | selection **passed 7 of 7** — the constraint is unpinnable from a test; see the one-tick-offset section |
| M6 the teardown leaks the clip's visibility record | `release_instance` no longer removes `NodeAnimatedVisibility` (F20's code, probed and restored) | selection FAILED, **2 of 8** — the new teardown test has teeth, alongside the destruction test |

M5 is the probe that matters most for the stage's headline criterion: a design
that published the verdict, maintained the record and stopped there would fail
five of the tests, so the assertions are on the physics side and not on a
component this layer owns.

## Checks run

By the implementer, before handover:

- `cargo fmt --all -- --check` — exit 0.
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — exit 0 (one round caught a `////` typo in this module's docs, fixed).
- `cargo test --workspace --locked` — exit 0, 249 suites ok, 0 failed.
- `cargo test --workspace --locked -- accept_f20_c_04_ --include-ignored` —
  exit 0, **7 tests matched** in
  `crates/cs_app/tests/accept_f20_c_04_collider_presence_consumer.rs`, all
  passing, none `#[ignore]`d.
- `cargo test --workspace --locked -- accept_f20 --include-ignored` — exit 0,
  **55 tests** across the F20 selections (1 `accept_f20_a_`, 7 `accept_f20_b_`,
  8 `accept_f20_c_01_`, 9 `accept_f20_c_02_`, 8 `accept_f20_c_03_`, 7
  `accept_f20_c_04_`, plus the F20 unit tests in the library), 0 failed. No
  earlier F20 assertion changed.
- the eight mutation probes above — each probe's selection exited 101 and named
  at least one failing test; every file was restored.

By the reviewer, after its own fixes (see "Review record" below): the full four
checks again, plus `accept_f20` and the four reviewer probes.

No command needed `CS_GAME_DIR`, and `CS_CAPABILITIES` was not exercised: this
stage reads no original data.

## Review record

Reviewer: **bunny-2** — the same agent name that implemented the branch, in a new
session, so the review context was fresh but it was **not** an independent
reviewer**.** Recorded here and in the handover because the policy asks for the
actual identities; nothing in this stage carries an original-fidelity claim, so
the independent-review requirement (evidence machinery, format and mission
semantics, fidelity claims) is not engaged, but the overlap is on the record.

What the review checked, and what it changed. The decision itself, the merge
rule, the terminal damage state, the opt-in boundary, the refusal of an
unmanaged node and the honesty of the unknowns all held up: the record is a
policy the physics layer owns, the animation path publishes only its verdict, no
guessed constant or invented gameplay rule appeared, `crates/cs_app/src/animation/`
was untouched, and no protected path is in the diff. The implementation is
recorded as **checked**; it cannot be more, and nothing here should be read as
`verified_original`.

Fixes made during the review, all in the branch's own owner paths:

1. **Probe cross-talk in three tests** (`retire`, see the Tests section): a
   spent probe was left in the world and exchanged momentum with the next probe
   in the same lane, because `projectile`×`projectile` is a designed contact pair.
2. **A missing test for a documented claim**: the merge's third arm (`Undecided`
   never removes) was only covered by the pure truth table, while the module
   promised that a clip **teardown** returns the collider a hiding clip had
   removed. Added `accept_f20_c_04_a_clip_teardown_returns_the_collider_a_hiding_clip_had_removed`,
   driven through the real `stop_animation` and the real fixed loop, and shown to
   have teeth by probe M6.
3. **An overstated doc claim**: the module said it "does not read"
   `NodeDisabled`/`NodePresentation`, which is false of the composition and would
   have misled an auditor. Reworded to "takes no answer from", and the two
   consequences of the record outranking the marker were added.
4. **A `Default` footgun**: `NodeColliderPresence` derived `Default = Live`, the
   permissive end of the opt-in, so an `insert_default` could adopt a node and
   re-enable its collider. The derive is gone and the reason is documented.
5. **Naming clarity on the damage seam**: "removed" now says in the function docs
   that the Avian collider is left in place and disabled, not despawned, so a
   repair re-enables the same collider.

One thing the review could **not** close, stated rather than hidden: probe M4
shows the plugin's `.after(advance_animation_on_session_tick)` is not pinned by
any test, so the one-tick record latency it prevents is not currently guarded.
It is documented as a do-not-remove instead of asserted structurally, and the
spawn wiring's same-entity constraint is documented for the follow-up. Neither is
an unmet acceptance criterion, and both are the kind of thing a future task that
edits these files has to know.

## Unknowns

- **Whether the original couples node visibility to collision at all is
  unmeasured** and stays recorded as unknown (F20-A's finding; the original
  animation container layouts are undecoded, F13). F20-A's `hidden ⇒ no
  collider` is this engine's **designed** rule, adopted unchanged, and nothing
  here claims an original behaviour. **Affected content: a hidden node's
  surface** — a door that has swung open, a gear that has retracted, a panel a
  breakable clip hides — i.e. every node whose original visibility swap this
  rule would remove an obstacle for. F20-D measures it or nothing may claim it,
  and it is part of the F20-D original-family gate.
- Whether an original visibility swap hid the node's whole **subtree** (the way
  `NodeDisabled` and a culled band both propagate) is unmeasured. This stage
  folds no ancestors for the animation half, because the clip names one node; a
  subtree rule would be a guess.
- **What the original did with a destroyed part's collider** is F29's, and this
  stage does not decide it: `remove_collider_for_damage` is the seam F29's
  damage zones call once they have decided, and the seam itself carries no
  gameplay rule.
- The **one-tick offset** between a clip's hide and the engine honouring it, and
  the original's order of marker firing relative to physics, stay F20-C.02's
  unmeasured placement.
- The `projectile`-layer probe in the tests is a **test instrument**, not a
  gameplay claim: it is a slow solid body whose only job is to be stopped or not
  by the node's collider.

## Follow-ups

- **The spawn wiring** — the scene/airframe spawn path must insert
  `NodeColliderPresence::Live` for every node whose authored
  `CollisionRole::Collider`, and must attach an Avian collider to that node. No
  production path does either today, which is why the boundary is opt-in and
  why a clip-hidden node still collides without it. Filed as a follow-up task
  (#510); it belongs to F11-C's scene import or F29's part colliders, not to
  this stage. **Constraint the wiring must honour** (added at review): the
  presence record, the Avian `Collider` and the clip's `NodeAnimatedVisibility`
  all have to live on the **same** entity, because the pass reads all three from
  one entity. Spawning a scene node's visibility somewhere other than its Avian
  body would produce a policy that never sees the clip. If the engine has to keep
  them apart, this pass needs a node→body linkage that does not exist yet, and
  that is a design change for the spawn stage to make explicitly.
- F29's damage zones must call `remove_collider_for_damage` /
  `restore_collider_after_repair`. Until they do, a destroyed part's collider
  state is the spawner's and the clip's, and this layer correctly does not infer
  it from `NodeDisabled`.
- The F17 render-side draw consumer of `NodePresentation` is still absent from
  the crate (filed separately by F20-C.03 as #503); it must read
  `composed_visibility_verdict().drawn()` rather than re-derive the priority.
- Networked authority over a collider state (F57) is a separate concern and is
  explicitly **not** in this task.

## Evidence

Synthetic fixtures and designed contracts only. No original-data, visual,
audible or ordinary-play claim; this stage can award at most **checked**.

## Sources

- `specs/F20-object-animation-and-authored-destruction-states.md` (`### F20-C`,
  behavior 3, AC01), `docs/contracts/FLIGHT-PHYSICS.md`.
- `docs/findings/2026-10-02-f20-c-03-visibility-lod-damage-ownership.md` (the
  composed verdict this stage reads, and the recorded gap this task closes),
- `docs/findings/2026-09-30-f20-a-animation-channels-and-event-markers.md` (the
  designed `hidden ⇒ no collider` rule and its unknown original coupling),
- `docs/findings/2026-10-02-f20-c-02-fixed-tick-instances-and-teardown.md` (the
  fixed-tick slot and the teardown that releases the clip's record),
- `crates/cs_app/src/animation/visibility.rs` (the seam; read-only here),
  `crates/cs_app/src/scene.rs` (`select_lod_presentation`, `NodePresentation`,
  `NodeDisabled`, `LodDistance` — read-only here),
  `crates/cs_app/src/physics/{adapter,body,contacts,session,fixture}.rs` (the
  adapter slot, `spawn_body`, the contact reporter, the session composition
  seam),
- `avian3d 0.7.0` `src/collision/collider/mod.rs:394` (`ColliderDisabled`) and
  `src/collider_tree/update.rs:124,196-204` (the add/remove observers), read
  from the pinned source under `~/.cargo/registry`.

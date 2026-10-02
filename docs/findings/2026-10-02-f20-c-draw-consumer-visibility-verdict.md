# The draw consumer of the composed animation visibility verdict (F20-C draw consumer)

Task: `#503` (`F20-C-visibility-draw-consumer`), the follow-up
`docs/findings/2026-10-02-f20-c-03-visibility-lod-damage-ownership.md` filed.
Owner paths: `crates/cs_app/src/render/`, `crates/cs_app/tests/render/`,
`docs/findings/`. `crates/cs_app/src/animation/` is read-only here (F20's), and
`crates/cs_app/src/scene.rs` is read-only (F11's).

Task-test prefix: `accept_f20_c_draw_`. Ordinary build/test only: no
`CS_GAME_DIR`, no `retail` capability, no evidence report.

## The state this task starts from

`cs_app::animation::visibility` composes **one** draw verdict
(`composed_visibility_verdict(world, entity) -> VisibilityVerdict`, whose
`DrawVerdict` is `Drawn | HiddenByAnimation | LodCulled | Disabled`) out of two
records: F11-C's `NodePresentation` and F20-C.03's `NodeAnimatedVisibility`.
That composition had **no reader**.

`grep -rn "NodePresentation" crates/` matched only `crates/cs_app/src/scene.rs`
(the LOD pass that writes it) and doc comments. Nothing in
`crates/cs_app/src/render/` read it — and the batcher, which is the only thing
that currently keeps a part off the screen, decides from a *different and older*
record: `InstanceVisual::destroyed`, a copy of the F11-C
`AirframeDamageState` snapshot taken when the frame was built
(`crates/cs_app/src/render/batch.rs`, `withheld_codes::DESTROYED_PART`).

## The one observable failure

With a live imported scene whose presentation records are being written by the
real `select_lod_presentation`, the real `apply_airframe_damage` and a playing
clip that hides a node, `sync_frame` places a draw for **every** row the frame
still holds. Concretely, at the near band of an LOD group whose second band is
selected at the far band:

* the **culled** band is still drawn — on top of the band that was selected, so
  two LOD levels of one part are on screen at once. That is exactly what F11-B's
  `select_lod_presentation` exists to prevent ("two LOD levels of one part are
  never reported drawn at the same time"); the renderer simply never asked.
* a node a **clip hides** at its authored tick is still drawn, because the
  batcher has no idea a visibility channel exists.
* a node **damage** disabled is only withheld if the caller happened to hand the
  frame a damage snapshot; nothing reads the marker the damage pass itself wrote.

So the composed verdict existed, was tested, and changed nothing on screen.

## Files and functions this slice edits (listed before editing)

* `crates/cs_app/src/render/visibility.rs` (**new**) — the consumer seam:
  `presentation_entity(world, part)`, `row_draw(world, part) -> RowDraw`,
  `RowDraw` and `VisibilityReport`. It resolves a row's part through the live
  scene's ownership record and reads the composed verdict; it decides nothing
  about priority itself.
* `crates/cs_app/src/render/sync.rs` — `sync_frame` asks `row_draw` for every
  row before it places anything, places only the drawn rows, and counts the rest
  in a new `FrameSync::visibility`. `place_rows` takes the drawn rows.
* `crates/cs_app/src/render/mod.rs` — **wiring only**: the module declaration
  and one doc paragraph.
* `crates/cs_app/tests/render/visibility_consumer.rs` (**new**) and the one
  module line in `crates/cs_app/tests/render/main.rs`.
* this finding.

Nothing in the vertex/pipeline path changes: the same batches, meshes, images,
materials and placements are produced for every row the composed verdict draws.

## What was built

### The consumer seam

`crates/cs_app/src/render/visibility.rs` (new):

* `presentation_entity(world, part)` — the entity whose presentation records
  decide a row: resolved through [`LiveAirframeScene`], F11-C's ownership
  record, exactly as F11-C's own `damage_plan` resolves a part identity.
* `row_draw(world, part) -> RowDraw` — reads
  [`composed_visibility_verdict`] for that entity and nothing else.
* `RowDraw` — a decision plus **the verdict it came from** (`Option<DrawVerdict>`);
  `drawn()` and `reason()` are the only two things a caller may ask it.
* `VisibilityReport` — what the frame's rows were decided to be, per verdict.

### The one observable failure, fixed

`sync_frame` asked nothing about visibility, so a **culled LOD band was drawn on
top of the band the distance selected**: the two LOD levels of one part were on
screen together, which is precisely what `select_lod_presentation` exists to
prevent. A node a clip hides at its authored tick was drawn too, and a
disabled node was withheld only when the caller happened to hand the frame a
damage *snapshot* — the marker the damage pass itself wrote reached nobody.

Now, for every batch:

1. **before** any entity of that batch is written, each row's `RowDraw` is read
   from the world and counted (`VisibilityReport`);
2. a row that is not drawn gets **no placement** — no entity, no mesh in the
   store, no material; a placement a previous frame made for it is despawned by
   the existing reconciliation, so nothing stale stays on screen;
3. a batch whose every row is withheld is **not spawned at all**, and the entity
   a previous frame placed for it is released through the one release path, so
   `FrameSync::released` and the live entity map cannot disagree;
4. `FrameSync::visibility` reports all of it.

Nothing else moved: `report.spawned`/`reused`/`released`/`placed`/`painted`/
`withheld`/`presentation` keep their meanings, and the vertex, upload, material
and paint paths are untouched.

### Why the composition is read per sync and not cached

`composed_visibility_verdict` is pure and reads the records as they are at the
moment of the read, so there is no stored verdict to go stale and no schedule
constraint to maintain: a distance pass, a damage pass and a clip pass all reach
the screen through the same answer whatever order they ran in. The consumer
stores nothing, which is also why the frame it is syncing may be built from a
damage snapshot taken earlier without the two disagreeing.

### "Absence of a record is not a cull"

A row whose part identity no live entity carries — no `LiveAirframeScene` at
all, an unresolved part identity, or a part the live scene does not contain —
is **placed**, and counted in `VisibilityReport::no_record`. The alternative
would let a missing or half-loaded scene silently delete every draw, and F17
non-negotiable 4 forbids a visual decision that removes gameplay geometry. It
is the same rule the composition already states about a missing
`NodePresentation`, applied one level up, and it is reported rather than looking
like a decision.

### The recorded duplication with the batcher

F17-C's batcher already withholds a part whose **damage snapshot** named it
(`withheld_codes::DESTROYED_PART`), so a destroyed part is normally absent from
the frame before the sync runs. That behaviour is **not** replaced: it is F17-C's
contract and its tests, and the batcher has no world to read the marker through.
When a caller builds a frame without that snapshot the same part reaches the
sync and is withheld there from the marker's own record. The two agree, and both
are reported (the frame's `withheld` list; `FrameSync::visibility`) rather than
folded into one count.

### What the interpolation may not do (F20 non-negotiable behavior 1)

`interpolated_pose` returns a `PoseSample` and has no signature that can carry a
visibility, so there is no path by which a fractional-alpha frame could reach
this consumer. The test proves the observable half: a real blend between two
committed poses changes the report and the placements not at all, while the LOD
pass — a fixed-tick fact — does move a row between `Drawn` and `LodCulled`.

## Tests (`accept_f20_c_draw_`, 8 tests, all driving production code)

Fixture: a node array (`plane` root with `hatch`, an `wing_lod0`/`wing_lod1` LOD
group and `tail`) converted by `SceneGraph::build`, loaded through F11-C's own
`AirframeSceneRequest::load` → `process_airframe_scene_request`, damaged through
`apply_airframe_damage`, presented through the real `select_lod_presentation`,
animated by the declared F20-C.03 clip through `play_animation` and the wired
`advance_animation_on_session_tick`, and drawn by `batch_frame` +
`sync_frame`. The container key is chosen so the animated node's **derived** id
is exactly `SYNTHETIC_BREAKABLE_NODE`, so the ids the frame addresses are ids
the canonical conversion produced. Four items share one geometry, one state and
one paint, so the batcher merges them into **one** batch with four rows and only
the verdict can keep one off screen.

1. `..._the_sync_places_exactly_what_the_composed_verdict_draws` (minimum) — one
   batch, four rows, `VisibilityReport { drawn: 1, hidden_by_animation: 1,
   lod_culled: 1, disabled: 1, no_record: 0 }`, `placed == 1`, and the placed
   placement entity is `a.wing_near`. The four verdicts are asserted first, so
   the placement assertion cannot pass on a fixture that came out the same either
   way.
2. `..._the_clip_showing_a_node_again_places_it_again` — the same frame synced
   around the clip's show tick: the hatch is placed again and the culled and
   disabled rows stay off screen. **This is what fails if the consumer stops
   reading the animation record.**
3. `..._a_wholly_withheld_batch_is_released_not_left_drawn` — damage names every
   part, so no row is drawn: `spawned == 0`, `placed == 0`, the previous frame's
   batch entity is released, and no placement survives.
4. `..._interpolation_between_ticks_never_re_decides_a_draw` — a real blended
   pose changes nothing; the LOD pass at the far distance moves the drawn row to
   the far band, never both.
5. `..._without_a_live_scene_every_row_is_placed_and_counted` — no live scene:
   `no_record == 4`, four placements; and with a live scene, a part the scene
   does not contain is the same case and is placed.
6. `..._a_profile_switch_never_reaches_the_visibility_rule` — switching to a
   shadow-mapping profile changes the report and the placements not at all (F17
   non-negotiable 5).
7. `..._a_withheld_row_reports_the_composed_verdicts_own_reason` — the reason
   codes are `DrawVerdict::label()`'s, a drawn row has none, and a part with no
   record is `decide(None)` rather than a withheld draw.
8. `..._a_superseded_binding_never_decides_a_row` — a leftover node entity of a
   superseded generation, spawned before the load and reporting itself `Drawn`
   for the far band's own id, does not decide that row.

### Mutation probes (each fails ≥ 1 test; every file restored afterwards)

| probe | mutation | result as written | reviewer re-run |
| --- | --- | --- | --- |
| P1 | the draw decision composes from the LOD/damage record only, ignoring the clip's | 4 of 8 fail | 4 of 8 fail |
| P2 | the composition drops LOD and damage, keeping the clip's record | 6 of 8 fail | 7 of 8 fail |
| P3 | `RowDraw::drawn()` returns `true` for every verdict (default to a draw) | 6 of 8 fail | 6 of 8 fail |
| P4 | no record defaults to withheld | 15 fail across the whole render suite, the existing `accept_f17_c_*` ones included | 11 fail in `--test render` |
| P5 | a wholly withheld batch is spawned anyway | 1 of 8 fails (test 3) | 1 of 8 fails (test 3) |
| P6 | a row resolves through *any* live `SceneNodeBinding` instead of the ownership record | 1 of 8 fails (test 8, added for this) | 1 of 8 fails (test 8) |

P1 and P3 were re-run against the final base (after F20-C.03's review fix added
the `NodeDisabled` read to the composition) with the same outcome.
`grep -rn "MUTATION PROBE" crates/` is empty.

The **review** column is the #503 reviewer's own re-run of all six probes at
base `1505dbd`, each mutated file restored afterwards and the tree confirmed
clean. P1 is the acceptance criterion "removing the animation's record from
the world's draw decision fails the test", and it holds: dropping
`NodeAnimatedVisibility` from the consumer's composition fails 4 of the 8
tests, two of them on the placement itself rather than on a reason code.

**Identities.** Implementer and reviewer are the same agent instance
(`bunny-alpha-2`); the review is therefore **not independent evidence**. The
review context was fresh in the sense that it re-derived the behaviour from the
tree and re-ran every check and every probe rather than trusting this
document, but a same-agent review is not a second opinion and no agent review
replaces the owner's human approval.

## Checks (all exit 0, on the reviewed tree, base `573eca9`)

* `cargo fmt --all -- --check`
* `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
* `cargo test --workspace --locked` — **2531 passed, 0 failed, 261 ignored**
  (the ignored ones are the `#[ignore = "requires CS_GAME_DIR"]` retail tests)
* `cargo test --workspace --locked -- accept_f20_c_draw_ --include-ignored` —
  **8 matched, 8 passed**, none `#[ignore]`d
* `cargo test --test render` — 69 passed, 0 failed (the whole F17 render suite,
  every existing `accept_f17_c_*` assertion unchanged and still green)
* `cargo test -p cs_app --locked --test accept_f20_c_03_visibility_lod_ownership`
  — 8 passed: the consumer changed nothing upstream.
* `cargo doc -p cs_app --no-deps --locked` — no new warning from any file this
  slice touches (the unresolved `composed_visibility_verdict` link the first
  pass left in `render/mod.rs` is fixed; the remaining warnings are older than
  this slice).
* The branch was rebased onto `573eca9` (the F20-C wired-session integration,
  which added the animation path's producers) after those checks: the rebase
  applied without a conflict, and the commit it brought in touches no file this
  branch changes and no `Cargo.toml`/`Cargo.lock`. Two statements above were
  corrected for it — the animation producers now exist, so the "nothing writes
  `CommittedSessionTick`" claim is gone.

No evidence report: ordinary build/test only, no `CS_GAME_DIR`, no render, no
audio.

## Review fixes applied by the reviewer

* `BatchDraw::instances`/`row` are documented as the batch's **per-instance
  state** rows rather than the set on screen: a row the composed verdict
  withholds keeps its state there and has no placement. Left the data alone —
  the field is F17-C's per-instance record (non-negotiable 4) and the row's
  absence from the placements is what the draw consumer changed.
* The inline comment above the `place_rows` call said placements are "rebuilt
  when their count no longer matches", which is not what `place_rows` does: it
  reconciles by draw-item index. Corrected to say that.
* `crates/cs_app/tests/render/visibility_consumer.rs`: dropped a dead
  `live_generation(world);` call at the top of `node_entity`.
* This finding: probe table re-measured, checks updated, the one live scene and
  the store-growth consequences below recorded.

## One measured consequence: a released batch's assets stay in the stores

`release_entity` despawns the batch entity but never removes the
`Assets<Mesh>` entry or the `Assets<StandardMaterial>` entry `add_material`
created, so every release/respawn cycle of a batch leaks one of each. Measured
on this base with a temporary probe through the production `sync_frame` (one
batch of four synthetic rows, alternating between "every row withheld" and
"every row drawn"):

| after | released | spawned | `Assets<Mesh>::len()` | `Assets<StandardMaterial>::len()` |
| --- | --- | --- | --- | --- |
| first withheld sync | 0 | 0 | 0 | 0 |
| first drawn sync | 0 | 1 | 1 | 1 |
| second withheld sync | 1 | 0 | 1 | 1 |
| second drawn sync | 0 | 1 | 2 | 2 |
| third withheld sync | 1 | 0 | 2 | 2 |
| third drawn sync | 0 | 1 | 3 | 3 |

Growth is monotone and exactly one mesh + one material per cycle. The leak
itself predates this slice (the reload path reached it), but the release
condition is new: a batch is now released whenever the composed verdict
withholds **all** of its rows, so a clip that hides a whole batch, or a
damage/repair cycle over every row of one batch, reaches it on a gameplay-rate
path. Filed as **#512** (`F17-C-release-returns-store-assets`) rather than fixed
here: it is the release path's asset lifetime, not this consumer's rule.

A batch that keeps **one** drawn row is reused and leaks nothing — an LOD band
crossing despawns and respawns the one placement and adds nothing to a store.

## Unknowns, recorded and not guessed

* **Which record the original renderer consulted** for a culled, hidden or
  destroyed node. Unmeasured; the original animation containers
  (`mis_anim.zbd`, `cam_anim.zbd`) are undecoded (F13). This is a designed rule,
  so at most **checked** is claimed — never `verified_original`.
* **Whether an original visibility swap hid a whole subtree.** F20-C.03's
  recorded unknown. The animation half folds no ancestors here either: the clip
  names one node, and the damage/LOD fold is F11-C's and already in the record
  the composition reads.
* **Whether the original couples visibility to collision.** Unmeasured; the
  collision half of the verdict has its own task (#504) and nothing in this
  slice writes a collider.
* **Whether a frame is presented before or after the animation pass.** The
  verdict is read when the frame is synced, so the answer is always the last
  committed tick's fact. The animation side of that clock now has its producer
  (`animation::bind_animated_node` and `animation::AnimationPlugin`'s
  `commit_session_tick`, merged after this slice was first written), so a
  committed tick does exist in a running session; what is still undecided is
  where the **sync** sits relative to it, because `sync_frame` has no Bevy
  caller yet. A frame presented before that frame's animation advance therefore
  answers from the previous committed tick, which is F11-C's "late update"
  shape rather than a wrong answer — but it is undetermined, not designed.
* **What the original drew for a part that is both LOD-culled and clip-hidden.**
  The composed verdict reports LOD's reason, which is a presentation-reporting
  choice, not a measurement of the original's.
* **Only one live airframe scene exists.** `LiveAirframeScene` is F11-C's
  ownership record for **the** live scene, not a registry of scenes, and a row
  resolves its part identity through it exactly as F11-C's own `damage_plan`
  does. So a frame carrying parts of a second airframe resolves none of them:
  every such row is counted in `VisibilityReport::no_record` and **drawn**, and
  LOD, damage and clip hiding are not enforced for it. That fails in the safe
  direction (an extra band on screen rather than missing geometry) and the count
  makes it visible, but the enforcement gap is real. Widening the record to
  several live scenes is F11-C's decision and has not been asked for; until it
  is, a frame must not be read as evidence that culling works for every
  airframe in it.

## Unmet criteria, gaps and follow-ups

* **The collision side is not this task** (#504): nothing here writes or removes a
  collider, and `FrameSync` says nothing about collision (F17 non-negotiable 4).
* **The runtime caller is still unwired.** `sync_frame` is called from tests and
  from no Bevy system yet, exactly as before this slice; this change makes what
  it does correct, not reachable. Wiring the render schedule into the session is
  F17's and the physics/session stages'.
* **Pre-existing, filed separately:** the batch entity itself carries a `Mesh3d`
  and a material with no `Transform`, so Bevy also draws one copy of every batch
  at the world origin, in addition to its placed rows. That predates this slice
  and is untouched by it; a withheld batch now has no batch entity at all, so the
  extra copy disappears with the batch, but a batch with at least one drawn row
  still has it. Filed as **#506** (`F17-C-batch-entity-extra-draw`) rather than
  fixed here.
* **Pre-existing leak, measured above and filed as #512**
  (`F17-C-release-returns-store-assets`): a released batch's mesh and material
  stay in the asset stores. This slice adds the release condition that makes it
  reachable on a gameplay-rate path and does not fix the leak itself.
* The animation path's producers now exist on `main`
  (`animation::bind_animated_node` writes the generation-stamped
  `AnimatedNodeBinding`, `animation::AnimationPlugin` commits the session tick
  and installs the fixed-tick advance), so the clip record this consumer reads
  has a real writer; that landed after this slice was first written and changed
  nothing here. What is still absent is the **render** caller (`sync_frame` from
  no Bevy system, above) and the gameplay-marker consumer of `AnimationLog`,
  which is F37/F39's (a note on #75 carries it).

## Sources

`specs/F20-object-animation-and-authored-destruction-states.md` (`### F20-C`,
non-negotiable behavior 3), `specs/F17-rendering-material-fidelity-and-
scalable-presentation.md` (`### F17-C`, non-negotiable 4 and 5),
`docs/contracts/IDENTITY-CONTENT.md`, F20-C.03's finding (the ownership decision
this consumer reads), and read-only `crates/cs_app/src/scene.rs`
(`LiveAirframeScene`, `NodePresentation`, `NodeDisabled`, `AirframeDamageState`,
`select_lod_presentation`, `apply_airframe_damage`) and
`crates/cs_content/src/scene.rs` (`SceneGraph::build`, `SceneNodeId`).
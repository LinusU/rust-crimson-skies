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
* `crates/cs_app/src/render/mod.rs` — **wiring only**: the module declaration,
  re-exports and one doc paragraph.
* `crates/cs_app/tests/render/visibility_consumer.rs` (**new**) and the one
  module line in `crates/cs_app/tests/render/main.rs`.
* this finding.

Nothing in the vertex/pipeline path changes: the same batches, meshes, images,
materials and placements are produced for every row the composed verdict draws.

(Everything below is written after the code and its checks.)
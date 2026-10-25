# F11-E: the airframe scene request producer

Task #398. Follow-up of F11-C
(`2026-09-30-f11-c-part-socket-and-damage-visual-wiring.md`).

## What now exists

* `cs_app::scene::start_airframe_scene(world, visual, graph, collision)` is
  the session-start / mission-retry entry. It inserts the one-shot collision
  table and `AirframeSceneRequest::load`, serves it with
  `process_airframe_scene_request` and reads the outcome back from
  `AirframeSceneLog`: `Ok(generation)` or `AirframeSessionError::Refused`
  carrying the import error verbatim. A refusal leaves the running scene
  and its generation untouched; the refused attempt still burns its number.
* `end_airframe_scene(world)` is the session-end entry; it releases every
  entity of the live generation and returns how many were live.
* `AirframeScenePlugin` chains `process_airframe_scene_request`,
  `apply_airframe_damage` and `select_lod_presentation` in `Update`. The LOD
  pass is gated on a `LodDistance` resource, which stays the viewer stage's
  (F21/F17) input; no distance is guessed.

## Not done (affected content: every retail airframe)

* The session-lifetime owner (mission bootstrap / `ReadyBundle` handoff) does
  not yet call these entries with a retail graph: the GameZ node-array reader
  (task #392) is missing, so only an already converted `SceneGraph` can drive a
  load. The producer is exercised by `accept_f11_e_*` with a fixture graph.
* No code outside tests calls `start_airframe_scene` yet; wiring it into
  `mission_session` / `run.rs` is outside this task's owner paths and waits
  for #392.

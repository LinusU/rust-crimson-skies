# M01 shows no terrain or sky: the world loaded but was never drawn

Task: Rally #1433 (`M01-PLAY-WORLD-VISIBLE`), 2026-10-14.
Implementer: bunny-alpha-2. Branch: `rally/1433-m01-shows-no-terrain-or-sky-draw-m01-s-w`.
Base: `0eba62ed` (`main`, 2026-10-14).

Owner playtest, 2026-10-14, on `main` `b87e013e`:

```sh
CS_CAPABILITIES=retail,gpu,audio CS_ENGINE_IMAGE=... cs --cs-path <retail> --mission M01
```

The window showed only the player's plane against an empty background: no
terrain, no sea, no sky. `--playtest --world c1c` *did* draw an area of `c1c`,
so the meshes and the textures can render.

## What was wrong

`cs --mission M01` reached the windowed composition and
`crate::world::residency::load_world` spawned every world object — so the world
was loaded, resident and collidable — but **the world's geometry was never
drawn**.

`load_world` → `world::spawn::spawn_object` gives every object one entity
carrying the shared whole-mesh `Mesh3d` handle the mesh-derived collider is
built from, and **no material**. Bevy's PBR queue draws a mesh only through
`MeshMaterial3d`, so every world entity was extracted for its collider and
skipped by the renderer. The only material-bound geometry in the window was the
player airframe's own visual (`mission_session::player_visual`, #1216), which is
why the plane was the only thing on screen.

The retail playtest scene does not have this gap, and that is the whole
difference. `playtest_retail::spawn_playtest_content` spawns the area's records
through the same `spawn_object`, then, for every record whose mesh resolved,
spawns one **child per stored material group** carrying that group's own
`Mesh3d` + `MeshMaterial3d`, built by the production
`playtest_textures::TextureBinder` over the group's own texture archive
(`crates/cs_app/src/playtest_retail.rs`, the loop after `binder.finish()`). The
windowed mission composition never ran that half, so the world it loaded drew
nothing. The camera, the near/far planes (0.5 m / 40 km), the light and the
spawn pose were all fine; nothing was wrong with the world's bounds or its
streaming (every declared sector is loaded).

## What was built

`crates/cs_app/src/mission_session/world_draw.rs` is the missing half, and it is
the playtest's own production path rather than a second material binding:

* `WorldDrawSource` carries the world group's stored material table and texture
  archive — read through the playtest's own `read_world_sources`
  (`playtest_retail`, added by this task beside `read_aircraft_sources`) — plus
  the stored mesh-array slot of every definition mesh reference
  (`RetailWorldContainer::mesh_slot`, new; the definition names the catalog id,
  the binder names a stored slot, and `MissionContent::world_mesh_slots` is the
  one place the two are joined).
* `build_world_draw` binds every loaded mesh's stored material groups into
  `PlaytestPart`s with one `TextureBinder::new(archive, true, true)` — the same
  `textured` / `decal_offset` choices `PlaytestConfig::documented()` makes — and
  the same designed neutral colour (`[0.74, 0.70, 0.62, 1.0]`) the playtest
  falls back to.
* `attach_world_parts` (in `MissionWorldDrawPlugin`) parents those parts under
  each object's node as it is spawned (`Added<WorldVisual>`), so the first load,
  a sector load and the composed `R` restart all draw the world with no second
  call site. The whole-mesh node keeps its material-less `Mesh3d` — the collider
  is derived from it — exactly as the playtest keeps its own, and the
  material-bound children are the only drawn geometry.
* The node is given `Visibility::Inherited` when it gains drawn children: a
  role-`None` or cuboid record whose mesh reference still resolved presents no
  `Mesh3d` of its own and therefore had no `Visibility`, and a material-bound
  child under a parent without one is Bevy's B0004. The first capture produced
  42 such warnings; the fix removes all of them.

A **designed horizon cue** (`mission_session/sky.rs`) lands beside it, until the
original sky (F19) does: a `+Y`-up UV dome of radius 20 km parented to the chase
camera, drawn unlit with a per-vertex vertical gradient — `SKY_HORIZON_COLOR`
(`0.80, 0.86, 0.93`) at the eye's own height, deepening to `SKY_ZENITH_COLOR`
(`0.24, 0.44, 0.78`) at the zenith and darkening to `SKY_NEUTRAL_COLOR`
(`0.30, 0.33, 0.35`) below it — plus the zenith as the clear colour. It is
labelled: `DESIGNED_SKY_CLAIM = "m01-play.designed-sky-horizon-cue"`, printed by
`run_windowed` as `mission sky: designed horizon cue … verified_original=false
human_review=false`. Nothing about it is a reading of the original's sky,
atmosphere or horizon.

## The capture check

`mission_session/evidence.rs`'s capture now takes a **world probe** before the
player shot: for a few frames it hides the designed sky dome and the player's
aircraft, so the frame's background is the flat clear colour and every pixel off
it is geometry the world load drew. The count is taken against the window's
**declared clear colour** (`ClearColor`, read in the render pass, counted in
both its 8-bit encodings so the surface's channel order and encoding cannot
flip it — `Bgra8UnormSrgb` on the pinned Metal backend), in RGB order after
undoing the surface's channel order, and the run is refused
(`CaptureRefusal::Incomplete`) when fewer than `MIN_WORLD_COVERAGE_PERMILLE`
(20 per mille) of the frame are world pixels — the exact pre-#1433 frame is one
flat colour and counts zero. The clear colour must itself still be visible over
`MIN_CLEAR_COVERAGE_PERMILLE` (20 per mille), which is what proves the probe
hid the sky: its gradient is geometry, and a frame it filled would otherwise be
counted as terrain. The trace carries a `world` record with `world_pixels`,
`total_pixels`, `covered_permille`, `clear_pixels`, `clear_permille`,
`background` (the modal colour, a diagnostic), `clear_colour` and the two
thresholds.

Measured on the owner's installation with this build (Apple M3 Pro, Metal):

```
mission composition: M01 — … world world/c1c
mission sky: designed horizon cue m01-play.designed-sky-horizon-cue verified_original=false human_review=false
mission world draw: 309 meshes bound, 330 drawn parts (288 textured), material binding is the designed playtest reading, verified_original=false
{"kind":"world",…,"world_pixels":2339902,"total_pixels":3686400,"covered_permille":634,"background":[61,112,199],"min_covered_permille":20}
```

63.4 % of the spawn frame is the world's own pixels, over 2560×1440. The private
artifacts (never committed, `private/`) are:

* `private/m01-play-world-visible/frame_00032_world.png` — the probe frame: the
  cliff and valley terrain and the sea-coloured far geometry around the spawn,
  with the sky and the player hidden.
* `private/m01-play-world-visible/frame_00180_player.png` — the Devastator over
  the terrain with the designed sky gradient behind it.
* `private/m01-play-world-visible/trace.jsonl`, `playback.json`.

## Review amendment (bunny-alpha-1, 2026-10-15)

Two probe weaknesses were found in review and fixed in the same branch:

* **A frame the sky was not hidden from could pass.** The first version
  counted the probe frame's pixels against the frame's own *modal* colour.
  The designed sky's gradient is geometry: had the probe failed to hide the
  dome (a renamed or dropped `DesignedSkyDome` marker), its gradient would
  have filled the frame, become the "off-background" pixels and passed a
  window with no terrain at all. The count is now taken against the window's
  declared clear colour in both of its 8-bit encodings (the stricter
  off-colour count is the world's, so the surface's encoding cannot flip it),
  and the run is refused unless that clear colour is itself still visible
  over `MIN_CLEAR_COVERAGE_PERMILLE`. The modal colour stays in the trace as
  a diagnostic.
* **A capture could hang instead of failing.** The probe's screenshot was
  requested on one exact frame and only when the player had already spawned;
  a mission whose player never spawned never reached its exit condition and
  the window never closed. The probe is now skipped by name when the player
  is not there, every capture is bounded by the grace frame, and `finish`
  refuses with the reason the probe did not run.

Both are covered by `accept_m01_play_world_` unit members in
`mission_session/evidence.rs` (the count, the judgement, the encodings) that
run in CI, and the windowed and evidence members now assert the clear-colour
floor beside the world floor.

## What this does *not* claim

* **The material binding is designed, not measured.** It is the #666 name
  reading (`playtest_textures::NAME_READING`) and the declared provisional
  presentation (`PLAYTEST_TEXTURE_PRESENTATION_IS_PROVISIONAL`); 288 of the 330
  drawn parts bound a resolved original texture. It is evidence that the world's
  own textures resolve and draw on the loaded geometry, never that the original
  renderer looked like this.
* **Which stored node the original draws is still unmeasured** — LOD selection
  and variant choice are the playtest's own open question. This stage draws the
  definition's own records, every one.
* **The horizon cue is designed** (`DESIGNED_SKY_CLAIM`), a temporary
  orientation aid until F19's sky lands.
* **No original run, no human judgment.** This is an automated capture of our
  own build; it is not `human_play`, not a comparison with an original
  screenshot, and nothing here is `verified_original` (claim: `implemented`).
* **The sea or water plane.** The world's water, if it is a world object, is
  drawn like any other record; this task adds no water surface of its own and
  did not isolate one.

## Evidence

* `cargo test --workspace --locked` — green (CI's three checks).
* `cargo test --workspace --locked -- accept_m01_play_world_ --include-ignored`
  — the task selection: the synthetic attach member, the retail headless member
  (M01's stage binds 300+ meshes and the loaded nodes carry material-bound
  children) and the windowed capture member.
* `cargo fmt --all -- --check`,
  `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
  — green.
* `docs/findings/evidence/1433.json` (a copy of
  `private/evidence/1433/acceptance.json`, validated with
  `tools/validate_evidence.py --require-pass`).

# M01 player propeller blur disc (#1667 `PARITY-M01-PLAYER-VIEW-B`)

* **Target:** the owner's original capture with prop blur (+70 s, "OWNER IMAGES"
  note on #1644, kept in the gitignored `private/ref-drydock/m01/`; never committed).
* **Status:** the mesh, its radius and its texture are **measured** from
  `ZBD/planes.zbd`; the show rule is a **declared development choice**.
  Nothing here is `verified_original`.

## Measured

* Node slot **211** `prop1` under `dontmove` (207) in the `player` root, mesh
  #108, material `rotorblur.tif` (see
  `2026-10-10-vs-m01-rt-player-airframe-visual.md` §3). The player pins it by
  slot and name (`MISSION_PLAYER_BLUR_NODE_*`).
* Disc radius from the mesh's own triangles (`measure_propeller_hub`):
  **1.666 m** (the static `staticprop1` disc is 1.253 m).
* Opacity is the texture's own coverage plane drawn alpha-blended; no opacity
  constant exists in the code.

## Unknown (recorded, not guessed)

* **RPM / spool threshold** at which the original shows the blur: unmeasured.
  The rule used is the engine's `running` flag
  (`propeller_blur_visible`); the capture is the target.
* **`prop1b`** (212, mesh #109, also `rotorblur.tif`), `prop2`/`prop2b` and
  `nitroprop1`: which is shown when stays unmeasured; only `prop1` is drawn.
* Whether the original hides `staticprop1` while the blur shows: unmeasured, both draw.
* Cockpit/pilot visibility from behind belongs to the chase-camera slice
  (#1666); it does not depend on this slice's assets.

## Evidence

`accept_parity_m01_player_view_b_*` in `crates/cs_app/tests/campaign/m01_player_prop_blur.rs`
(the retail one is `#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`).
Owner-command smoke: `frame_00361_guns.png` and `frame_00300_hud.png` under
`private/evidence/PARITY-M01-PLAYER-VIEW-B/` (not committed).

# Cockpit view 3/5: the `cockpit1` interior is drawn (Rally #1811)

Task `PARITY3-COCKPIT-VIEW.03`. Capabilities used: `retail`, `gpu` (one
throwaway smoke run, below). Nothing here is `verified_original`.

## What is drawn

`crates/cs_app/src/mission_session/cockpit_interior.rs` builds the mesh-bearing
nodes of `player` → `player_pfighter` → `cockpit1` (slot 49) in `zbd/planes.zbd`
through the production path (`aircraft_graph`, `stored_render_mesh`, F17-B
upload, `TextureBinder`), spawns them under a `CockpitInteriorRoot` child of the
player body, and shows that root only while `MissionViewMode::at_pilot_eye()`
(and not during the launch cinematic). The body itself is hidden in those views
(#1810); the root sets `Visibility` explicitly so it overrides the hidden
parent.

Drawn (8 bindings): `canopy2`, `g715` (under `structure`: the black riveted
struts), `inside` (panel body: `steelplate`, `cockpit7/10/14`, burl-wood
`burloak`/`moulding`), `outside` (`dev_wing`, `gun_barrel`, `redrivet`,
`dev_winglogo`), `l_aileron3`, `r_aileron3`, `r_elevator2`, `l_elevator2`.

## Findings

- **Red wing tips live in `cockpit1`**, not the exterior meshes: `lailer3` /
  `railer3` → `l_aileron3` / `r_aileron3` (`dev_wing`, `redrivet`,
  `dev_winglogo`), plus the same textures on `outside`.
- **Gunsight glass** is the two-triangle quad of `inside` bound to texture
  `cockpit6` (128×128; 15 570 of 16 384 texels have partial coverage). It is
  drawn `AlphaMode::Blend`, opacity from the texture's own coverage plane; no
  opacity constant. Picking it by node + texture name is a **designed** choice:
  the stored records carry no glass flag. The ring and crosshair are in that
  texture. There is no separate stalk node; the stalk geometry is in the
  `inside`/`g715` meshes (not separated).
- Other keyed textures (`redrivet`, `dev_winglogo`) keep the binder's existing
  `Mask(0.5)`; whether the original blends them is unmeasured.

## Not drawn, with reason

- Everything under `gauges` (slot 56, every binding under it): the live dials, task
  PARITY3-COCKPIT-VIEW.04 (#1812).
- `pcdp4`, `pcdp6` (`damage1`) and the 18 `bul*` quads under `bullet1..5`
  (`cpit_bullethole1/2`): damage decals. Hidden by a **designed** name rule;
  the damage state that shows them is unmeasured.
- `cockpit_camera` is not under `cockpit1` (it is the eye, #1810).

## Unknown

- Draw order / depth treatment of the interior against the world (no clear of
  depth is done; it is an ordinary 3-D child), near-plane clipping, which LOD
  or draw-state flags the original applies: unmeasured.
- Scale/axes: same designed one-unit-is-one-metre reading as the exterior.

## Evidence

A throwaway local smoke run with the view forced to `Cockpit` (the forced view
was not committed) rendered struts, panel, red wing tips and the translucent
sight ring in `frame_*_hud.png` (private, not committed). The shipped smoke has
no cockpit-view shot; one belongs to a later task. Acceptance tests:
`accept_parity3_cockpit_view_03_*` in `crates/cs_app/tests/`.

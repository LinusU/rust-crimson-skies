# #1651 PARITY-M01-ATMO-D: M01's original cloud geometry is drawn

Date: 2026-10-19. Archive `ZBD/C3/gamez.zbd` (M01), read through the production
GameZ readers and the world draw. The original executable was not run: nothing
here is `verified_original`. Implementer `sonnet-1`; no independent review yet.

## What was wrong

The world import already owned the `horizon` subtree (`zone2` 3235 empty, `zone1`
3236 with `sun` 3237, `g1155` 3238, `h_zone2scroll` 3239) and the world draw
already bound their meshes. Two gaps kept them off the screen:

1. **They hung at the world origin.** M01's world spans x, z in [-16 384, 0] and the
   sky records are centred on the horizon node's origin, so the cumulus ring was
   kilometres from the player. #541 measured that `CameraSetHorizon horizon` drags
   the node to the camera's translation (all three axes, no rotation) every render
   pass; nothing in the mission composition did that.
2. **They were lit and fogged like terrain.** The distance fog (far 4 500 m) hid
   the 4.4-4.9 km ring and the neutral lit material replaced the stored vertex
   colours.

## What is drawn now

`mission_session::world_draw` anchors every record of the `horizon` subtree to the
camera each frame (`HorizonRest` keeps each record's stored offset from the
`horizon` record, `sun` keeps its (1462.7, 1420.7, -1471.4) offset). The subtree's
materials are unlit, unfogged copies (`HORIZON_SUBTREE_LABEL`, **designed**).
`g1155` draws the `cumulus1`/`cumulus2` ring (16 triangles), the `Sky1` cap and the
flat vertex-coloured skirt; `h_zone2scroll` draws the `Sky1` wall; `sun` draws `Sun`.

## `cloud1.tif` and `cloud2.tif` are absent

The two 22 m quads of `g1155` (stored materials 270 and 271) name `cloud1.tif` and
`cloud2.tif` (texture table slots 266, 267). **No `ZBD/C3` texture archive
(`rtexture2/4/6/8/12.zbd`, `texture.zbd`) holds either name**; only `cumulus1/2`. The
binder reports `texture_not_resolved`. These two quads are therefore not drawn
(a white square would be invented art). Whether the original finds them elsewhere
or draws nothing is **unknown**; they sit at Y -245 below the horizon origin.

## Still unknown (affected content: M01 sky and clouds)

- `Sky1` / cumulus scroll rates and whether `h_zone2scroll` scrolls at all (consumer
  not traced, see ATMO-A §5).
- The **zone gating**: the stored zone ids (`zone1` 1, `zone2` 2, the rest 255) and the
  render test `0x56c430` are measured, but who fills the allowed-zone list from the
  camera zone is not (ATMO-A §4). Nothing in M01 reaches `ZONE2` (above about 10.5 km),
  so every frame draws the `ZONE1` subtree; no gate is modelled.
- How the original lights and fogs the sky records. The frame is now greyer than
  `private/ref-drydock/m01/orig25.png`, whose sky is blue above the ring; the
  colour of the `Sky1` wall and cap (vertex colours times texture) is likely
  re-tinted by something unmeasured, e.g. the zone fog colour.
- The mission start cloud positions in `orig70.png` were not compared beyond the
  ring's presence.

## Evidence

`accept_parity_m01_clouds_g1155_is_drawn_and_follows_the_camera` (`cs_app`,
`#[ignore = "requires CS_GAME_DIR"]`): the subtree's slots, the cumulus/sky images, the
absence of an untextured stand-in for the missing cloud quads, unlit unfogged
materials, and the whole subtree at camera + stored offset. Owner-command smoke
(`cs --cs-path <retail> --mission M01 --smoke-seconds 45`): 13 checks, 0 failed;
`frame_00300_hud.png` shows the cumulus ring above the island, where
`orig25.png` has two cumulus clouds.

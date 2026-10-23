# PARITY2-DMG-FX-D: the damaged airframe beside the owner's frame

Task #1808. Capabilities used: `retail`, `gpu`, `audio`. Provenance: a scripted
run, never `human_play`, nothing here is `verified_original`.

## The run

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY2-DMG-FX-D` on the reviewed commit: 3988 frames, 69
checks, failures: none (framebuffers not committed). The player is damaged by
the smoke's `DamageTrail` phase through the real path: `HitEvent`s resolved by
the session `SessionDamageResolver` on the player's `leftwingdamage` (to 0.40
remaining), `nosedamage` (0.65) and `taildamage` (0.15) nodes, producer 998,
listed as test hits in `report.json`. No state is written directly; the
airframe, trail and gauge react to the resolver's own regions.
`damage_trail_drawn` passed: 155 smoke and 621 fire puffs over 186 burning
passes. Frame: `frame_03166_damage_trail.png` (2560x1440), compared with the
owner's `private/ref-drydock/m01/orig70.png` (1024x768, +70 s; comparison
material only, never committed). The two are not the same moment: ours is
the phase's chase view a few frames after the hits, the owner's is the
original's flight at +70 s.

## Compared

| Element | Original (`orig70`) | Ours | Verdict |
| --- | --- | --- | --- |
| Port wing fire | A dense orange/yellow flame plume streaming ~230 px behind the tip, bright and opaque | One small orange quad at the left tip; the 621 fire puffs are not legible as a plume | **Differs** |
| Dark smoke | Large opaque black-grey puffs below and beside the airframe | None visible | **Differs** |
| Propeller | Dark disc with pinkish ring arcs and offset ghost copies | Faint translucent disc (see `2026-10-21-parity2-dmg-fx-c-propeller-blur-profile.md`) | **Differs** |
| Damage gauge (lower-left dial) | Silhouette in red/yellow/green zones | Hatched grey silhouette at the dial's size; the zone tint is not legible in the frame | **Differs / unclear** |
| Whole frame | Bright sky, full contrast | Darker, low-contrast frame (a dim grade over the world in this phase's frame; not explained here) | **Differs** |
| Scene | Sky, banking view, chase of a damaged plane in a fight | Level over water, no fight | Not comparable |

## Remaining differences and what resolves them

All of these are unresolved; none was tuned to match.

1. **Trail legibility.** The stored sizes (0.1-0.8 m) and the designed gate
   (any wing region Damaged) put the puffs close behind the prop1 anchor,
   where the chase camera sees them end-on; the original's plume is long and
   bright. Which `injure_anims` threshold plays which trail, the side, and
   whether the anims move nodes are unmeasured
   (`2026-10-23-parity-m01-player-damage-trail.md`).
2. **Smoke volume.** The original's large black puffs are not reproduced; the
   `fuel_trail`/`pdpanel*` fires and `GROWTH_FACTOR`/`FADE_RANGE` are not applied.
3. **Prop disc.** Opacity and ghost copies unexplained
   (`prop1b` undrawn).
4. **Gauge.** The mapping is driven (`...-gauge-drive.md`), but its zone colours
   do not read in this frame; the gauge-level comparison needs a cockpit-sized
   crop and the owner's eye.
5. **Frame grade.** The darkening in the damage frame needs its own
   investigation; a task is filed.

Needed: owner review (`human_review`) of the pair; the original's exact
per-level trail selection.

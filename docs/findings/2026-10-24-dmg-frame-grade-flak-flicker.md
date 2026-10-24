# The dim damage-phase frame is a flak flicker, not a grade

Task #1966. Capabilities used: `retail`, `gpu`, `audio`. A scripted run, never
`human_play`; nothing here is `verified_original`.

`frame_*_damage_trail.png` of the owner-command smoke was darker and lower in
contrast than its neighbours (see
`2026-10-23-parity2-dmg-fx-d-damage-frame-beside-the-original.md`). No dim
overlay or grade belongs to the damage phase. The cause is the flak phase
before it: its test-woken `aagun99` stays live, keeps firing `wep_27` at the
player and every close burst starts the `frame_buffer_effects1` flicker, a
full-window overlay of six `FBFX_COLOR_FROM_TO` steps (black or grey, alpha
0.2-0.4, 1.5 s each). Overlapping flickers of later bursts were still running
at the damage shot (instrumented: two to three live overlays on ticks
6080-6247, the shot at 6247). The critical-hit frame, taken after the last
flicker ended, was bright.

The smoke now stands the turret down when the damage phase begins and judges
`damage_frame_ungraded` (no flicker overlay alive on a damage-phase shot
frame). The re-run's damage frame has the same sky and sea contrast as the
critical frame. The flak effect itself is unchanged.

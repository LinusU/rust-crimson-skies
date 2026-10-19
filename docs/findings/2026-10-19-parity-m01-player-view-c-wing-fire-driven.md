# PARITY-M01-PLAYER-VIEW-C: the wing fire driven by the damage model, end to end

Date: 2026-10-19. Task: #1668 (slice C of #1644, over its merged halves C1
#1682 and C2 #1683). Visual targets: the owner captures `orig25.png`
(intact, +25 s) and `orig70.png` (port wingtip fire, dark smoke, red/yellow
per-region gauge, +70 s) in the gitignored `private/ref-drydock/m01/`;
never committed. Capabilities used: `retail`, `gpu`, `audio`.

## What this slice owns

C is the parent acceptance over its two merged halves:

- C1's hook: `apply_region_damage_state` copies the session
  `DamageResolver`'s `PartState` verbatim onto each bound region entity —
  no parallel damage state — and `release_region_damage` drops the records
  for repair / reload / scene release.
- C2's measured `wing_flare1`/`wing_flare2` quads: single-quad
  `oil_liteflare.tif` meshes at the wingtips (slots 208/209, port at −x,
  starboard at +x), gated by `update_wing_flares` on the published record
  (`Damaged`/`Destroyed` show, `Intact`/`Unknown`/unpublished stay hidden).
- The scheduled driver `drive_airframe_damage_visuals` (Update): declare
  each flare's `RegionDamageBinding` once the session's `MissionWeapons`
  marker exists, publish `RegionDamagePresentation` from the session
  resolver, gate each quad on its own record — all convergent, so a quiet
  frame changes nothing.

The C tests (`accept_parity_m01_player_view_c_*`) drive the whole chain:
a resolver hit on `leftwingdamage` shows only the **port** quad — each wing
answers to its own damage node, never a session-wide flag — a hit on
`rightwingdamage` then draws the starboard one; releasing the actor's
records re-hides the quads; and a retail test composes real M01 and asserts
the player body carries exactly the two measured quads, bound by the
scheduled driver to the player actor's `leftwingdamage`/`rightwingdamage`
nodes.

## The damage gauge — owned by the HUD, and where the gap is

The HUD owns a damage gauge: `cockpit_hud.rs` `damage_tint` over
`damage_fraction` of `airframe_zones`, which reads the session
`DamageResolver` — the same authority, not a parallel state. A destroyed
zone turns the silhouette red (covered by
`accept_parity_m01_hud_damage_gauge_turns_red_when_hit`).

The original's rule is different and measured in the C2 finding: the
`damageindicator` cockpit meshes draw **per-region texture cycles**
(`greenhilite` [green,yellow,orange,red], `grn_hatchptrn`, `greenindicator`)
selected through `pdpanel*` gauge anims that the vehicle record's
`destroyable_parts` `<part>_damage_<green|yellow|red>` fields name.
`orig70.png` shows that per-region presentation — red wings and tail, a
yellow fuselage bar — not one tinted silhouette. The designed tint is a
placeholder, not the original rule; the original presentation is follow-up
**#1701 PARITY-M01-PLAYER-DAMAGE-GAUGE** (filed with this slice's sibling
notes). Today the gauge cannot reach either presentation in play: the M01
player actor registers no damage graph (**#1699
PARITY-M01-PLAYER-DAMAGE-GRAPH**), so `airframe_zones` returns `None`, the
fraction stays 0 and the gauge shows intact green — matching `orig25.png`.

## Owner-command smoke — what it showed

`cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-PLAYER-VIEW-C` on this branch's head: the run
played the launch cinematic first (`launch_cinematic_plays`: 4818 of 4818
ticks, handed over, 0.000 m drift), then ran all scripted checks — 19
checks, 18 pass, `world_sea_visible` skipped (the world's object records
declare no known water surface), 0 failures (`passed: true`); 19
framebuffer PNGs plus `report.json` sit under the capture dir, and the
evidence report is `docs/findings/evidence/PARITY-M01-PLAYER-VIEW-C.json`.

The scripted smoke never damages the player: with no player damage graph
the regions publish `Unknown`, both `wing_flare` quads stay hidden and the
gauge stays green — the intact presentation `orig25.png` shows. The damaged
presentation the gate draws is exercised by the `accept_` tests against the
real `DamageResolver`, not by this smoke; reaching it in a windowed run
waits on #1699.

## Unknowns carried — recorded, not guessed (`orig70.png` is the target)

- The damage levels at which the original starts fire and smoke per part:
  the resolver's `Damaged` is "integrity absorbed"; the original's level
  keys live in the undecoded `<part>_damage_<green|yellow|red>` records
  (#1700, #1701).
- The trail's emitter parameters and anchors: `fire_lt`'s scene node was
  never recovered, the trail records bind `wing_flare2` while `orig70.png`'s
  fire streams from the **port** tip — #1700 PARITY-M01-PLAYER-DAMAGE-TRAIL.
- The gauge's cycle-frame index per damage level and the
  nose/tail/rightwing `pdpanel*` mappings — #1701
  PARITY-M01-PLAYER-DAMAGE-GAUGE.
- The M01 player's damage graph itself — per-part integrity values are
  unmeasured — #1699 PARITY-M01-PLAYER-DAMAGE-GRAPH.

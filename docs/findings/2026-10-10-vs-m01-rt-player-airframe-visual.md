# VS-M01-RT-PLAYER-AIRFRAME-VISUAL: where M01's player airframe visual lives, measured

Date: 2026-10-10. Task: #1216 `VS-M01-RT-PLAYER-AIRFRAME-VISUAL`. Capability used:
**`retail`** (the owner's installation read-only). No original run happened;
nothing here is `verified_original`, and no slot was guessed (AGENTS.md rule 4).

## Sources and method

* `zbd/planes.zbd`, read through the production readers exactly as the playtest
  reads it: `read_playtest_sources(<install>, "c1c").aircraft()` →
  `aircraft_graph(..)` (a `SceneGraph` over the container's stored node array).
* The probe is the checked-in test
  `crates/cs_app/tests/campaign/vs_m01_rt_player_visual.rs::probe_dump_player_airframe_subtrees`
  (`#[ignore = "requires CS_GAME_DIR and CS_ENGINE_IMAGE"]`); its full output is
  what the tables below transcribe.
* Prior measurements this builds on: #648/#710 (the bloodhawk's intact
  selection in the same container), #709 (the stored airframe nose axis, −Z,
  container-wide), `docs/findings/2026-10-06-m01-lc-player-airframe-source.md`
  (AIRFRAME_TABLE row 5: Devastator / `player_pfighter` / `piratefighter`).

## 1. The container's roots

`planes.zbd` stores 29 roots. The eleven **player-variant** airframes (the
scene roots `support\planes.gw` creates per `AIRFRAME_TABLE`) are all present —
ten under their own `player_*` name and the Devastator nested one level deeper:

| root slot | name | note |
| --- | --- | --- |
| 1 | `player_bhawk` | player Bloodhawk |
| 10 | `player_peacemaker` | |
| 14 | `player_kestrel` | |
| 19 | `player_autogyro` | |
| 23 | `player_avenger` | |
| 25 | `player_balmoral` | |
| 38 | `bloodhawk_gear` | |
| 39 | `player_fury` | |
| 41 | `player_fbrand` | |
| 241 | `rope_ladder` | |
| 631 | `cpilot` | |
| 632 | `player_brigand` | |
| 635 | `player_warhawk` | |
| **1418** | **`player`** | **its only child is `player_pfighter` (slot 44) — the player Devastator** |
| 1776 | `anim_bloodhawk` | |
| 2269 | `pickup_cpilot` | |
| 2288 | `chuteman` | |
| 2308 | `brigand` | generic model roots begin here |
| 2324 | `piratefighter` | generic (NPC-class) Devastator build |
| 2329 | `fury` | |
| 2363 | `bloodhawk` | the playtest's airframe root (#648) |
| 2364 | `warhawk` | |
| 2369 | `autogyro` | |
| 2371 | `balmoral` | |
| 2377 | `kestrel` | |
| 2388 | `firebrand` | |
| 2397 | `avenger` | |
| 2764 | `peacemaker` | |

`player_pfighter` is **not** a root of `planes.zbd`; it is a node (count of
the name in the container: 1) hanging under root `player` (1418). The generic
`piratefighter` root (2324) is the same model without the player gear: no
cockpit interior, no `pirate_hook`. M01's player airframe is the `player` →
`player_pfighter` subtree: the authored name matches the scene root
`AIRFRAME_TABLE` row 5 creates, and only this subtree carries the
player-side parts.

## 2. The player Devastator subtree (`player` 1418 → `player_pfighter` 44, 196 nodes)

```
player_pfighter (44)
├─ geometry (48)
│  ├─ markers (214): cockpit_camera, target, ground_level, pylon1..8,
│  │                 firepoint1..8, cockpit_light (234, mesh#111), map,
│  │                 exhaust1/2, ladder_pos, cf_light — no drawable bodies
│  ├─ healthy (149)                                 ← the intact subtree
│  │  ├─ nearest   Lod(0-150)    (150)
│  │  │  ├─ winglight1 (151, m#67), winglight2 (152, m#68)
│  │  │  ├─ polys (153, m#69)                       ← the fuselage/wing skin
│  │  │  ├─ pilot_pos (154) → pilot (155) → body (156, m#71), head (157, m#70)
│  │  │  ├─ canopy (158, m#72)
│  │  │  ├─ leftwing (159):  l_rudder→l_rudder1 (161, m#73),
│  │  │  │                     lft_elev→l_elevator1 (163, m#74),
│  │  │  │                     lailer1→l_aileron1 (165, m#75),
│  │  │  │                     lailer2→l_aileron2 (167, m#76), g789 (168, m#77)
│  │  │  ├─ rightwing (169): r_rudder→r_rudder1 (171, m#78),
│  │  │  │                     rt_elev→r_elevator1 (173, m#79),
│  │  │  │                     railer1→r_aileron1 (175, m#80),
│  │  │  │                     railer2→r_aileron2 (177, m#81), g790 (178, m#82)
│  │  │  ├─ nose (179, m#83), tail (180, m#84)
│  │  │  └─ player_damage_on (184): pdp3_h (185, m#87), pdp2_h (186, m#88),
│  │  │                            pdp1..pdp8 (187..194, m#89..96)
│  │  ├─ l8        Lod(150-650)  (195) → g429 (196, m#97)
│  │  ├─ l9        Lod(650-1500) (197) → g453 (198, m#98)
│  │  ├─ l7        Lod(1500-2500)(199) → g352 (200, m#99)
│  │  └─ pirate_hook (2333): r_arm3 (2360, m#1322), l_arm3 (2336, m#1681),
│  │                         door_db (3005, m#1682), r_door (2511, m#1684),
│  │                         l_door (2330, m#1685)   — outside every band
│  ├─ shadow (201, m#100)
│  ├─ destroyed (202): piece1..4 (203..206, m#101..104)
│  └─ dontmove (207): wing_flare1 (208, m#105), wing_flare2 (209, m#106),
│                     staticprop1 (210, m#107), prop1 (211, m#108),
│                     prop1b (212, m#109), nitroprop1 (213, m#110)
└─ cockpit1 (49): the cockpit interior — pass_st (50) → structure (51)
   (canopy2 m#33, g715 m#34), inside (m#35), outside (m#36), gauges (56)
   (horiz/pfhorizon m#37/#0, comp m#1/#2, altimeter m#3..6, speedometer
   m#7..9, damageindicator m#38..42, missilegauge m#10..21, gungauge
   m#22..29, nitrogauge m#30..32), l_aileron3 (m#43), r_aileron3 (m#44),
   pcdp6 (m#45), pcdp4 (m#46), bullet1..5 chase cams (m#47..64),
   r_elevator2 (m#65), l_elevator2 (m#66)
```

Node-name counts in the whole container: `player_pfighter` ×1,
`piratefighter` ×1, `player` ×1, `healthy` ×23, `staticprop1` ×22,
`player_damage_on` ×12, `player_damage_off` ×13 — the pinned slots below are
therefore **required**, names alone are ambiguous.

All probed transforms (`player` 1418, `player_pfighter` 44, `piratefighter`
2324, `polys` 153, `nose` 179) store identity translations and identity
linear parts — the airframe's parts sit at authored positions inside the
subtree, so the intact subtree's own world transform composes to identity.

## 3. The measured intact selection (the values the spawn pins)

Applying the #648 rule — intact subtree pinned by slot+name, one `Lod` band
chosen at the designed viewer distance, one propeller node pinned by
slot+name — to this airframe:

| selection | slot | name | measured |
| --- | --- | --- | --- |
| airframe root | 1418 | `player` | the `player_pfighter` subtree's owning root in `planes.zbd` |
| intact node | **149** | `healthy` | child of `geometry` (48), four `Lod` children + `pirate_hook` |
| LOD bands of 149 | 150 `nearest` 0–150, 195 `l8` 150–650, 197 `l9` 650–1500, 199 `l7` 1500–2500 | chosen: `nearest` at the designed 20 m (same designed distance as the playtest; the original's LOD rule is unmeasured) |
| propeller node | **210** | `staticprop1` | under `dontmove` (207), mesh#107, `mat72:rotorblade.tif` ×4 |
| propeller siblings | 208/209 `wing_flare1/2` (`oil_liteflare.tif`), 211 `prop1`/`prop1b` (`rotorblur.tif`), 213 `nitroprop1` (`nitroprop.tif`) | undrawn — which state the original shows when is unmeasured; the static name read is the documented development choice, same as the playtest's |

Under the chosen `nearest` band the drawn set is every mesh binding listed
under §2's `nearest` block plus `pirate_hook`'s five (inside `healthy`,
outside every band): the `dev_*`-textured skin (`polys` m#69 →
`dev_fusalagetop.tif`, `dev_fusalage.tif`, `dev_wingflap.tif`, `dev_cock.tif`,
`dev_wingedge.tif`, `pir_cowling.tif`; `nose` m#83 → `dev_fusalage.tif`,
`dev_noselogo.tif`; `tail` m#84 → `dev_engine.tif`, `dev_enginefront.tif`,
`dev_engineback.tif`, `dev_spinner.tif`), the wing lights, canopy, pilot
body/head, all eight control surfaces, the two `g*` filler meshes — and the
ten `pdp*` meshes under `player_damage_on` (184) (`damage1.tif`,
`damage2.tif`, plus `dev_wing.tif`/`dev_fusalage.tif`/`sparbead1.tif` on
`pdp*_h`).

Undrawn under the intact rule (all reported, never silently dropped):
`markers` (no meshes), `shadow`, the four `destroyed` pieces, the propeller
siblings, and the whole `cockpit1` interior — the cockpit gauges' own damage
indicator meshes live there, not in the exterior selection.

## 4. The generic `piratefighter` root (2324) — the rejected candidate

Measured for contrast: `markers` (2467), `healthy` (2403) → `nearest`
(2404, 0–150) + `l8` (2448, 150–650) + `l9` (2450, 650–1500) + `l7`
(2452, 1500–2500), `shadow` (2454, m#1402), `destroyed` (2455, m#1403–1406),
`dontmove` (2460) → `staticprop1` (2463, m#1409) + `prop1`/`prop1b`/
`nitroprop1`/`wing_flare1/2`. Its `nearest` carries `player_damage_off`
(2435 → pdp2i m#1387, pdp3i m#1388) where the player tree carries
`player_damage_on`. It has **no** cockpit interior and **no** `pirate_hook`;
nothing names it the player's build. If a later stage ever needs the NPC
Devastator, this row is its measurement; M01's player is §2.

## 5. Open questions (recorded, not guessed)

* **`player_damage_on` under `nearest`.** The intact rule draws its ten
  damage-patch meshes (`damage1/2.tif`). Whether the original draws them on a
  pristine airframe or toggles the node by damage state is unmeasured — the
  same structure sits in `player_bhawk` (`player_damage_on` 367 under its
  `nearest` 344) and the playtest's intact rule has always drawn what the
  chosen band holds. This task draws them and says so; a damage-state visual
  rule is future measured work, not a guess here.
* **Propeller state.** `staticprop1` is pinned by name exactly as #648 did;
  `prop1`/`prop1b`/`nitroprop1`/wing flares are undrawn until an original run
  measures which state shows when.
* **Nose axis.** The visual lands on the body's `BODY_FORWARD` (−Z) through
  `nose_mapping(STORED_AIRCRAFT_NOSE_AXIS)` — the container-wide −Z nose
  measured in #709, i.e. the identity mapping. Which body axis the original
  airframe node occupies in a mission is unmeasured (already recorded in
  `docs/findings/2026-10-10-vs-m01-rt-window-composition.md`).
* **LOD distance.** 20 m is the playtest's designed viewer distance, reused:
  the original's LOD rule is unmeasured. It selects `nearest`, the
  highest-detail band this airframe authors.

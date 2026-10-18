# F49-D.03-FU-FACTION: faction and survivability of the Instant Action actors

Date: 2026-10-18. Task: #1582. Capability: **`retail`** (read-only
`$CS_GAME_DIR`) and the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
read-only, static reading only). Nothing was run, rendered or played; the claim
stays `implemented`.

## Result

**Unmeasured, with evidence of why.** The executable's Instant Action setup
supplies no faction (allegiance) and no mortality for any actor, so neither can
be bound into the catalog from the descriptor path. The claims
`f49.d.03.unmeasured.faction` and `f49.d.03.unmeasured.survivability` stay, and
their reasons now carry this evidence. All 30 presets still refuse in
`lower_preset` on the faction first.

## What the executable does (static, image VAs)

| what | VA | finding |
| --- | --- | --- |
| descriptor reader | `0x459390` (with the group reader `0x458e00`, group initialiser `0x458d00`, record initialiser `0x458ff0`) | the keys it asks for are `mission_type`, `player_plane`, `num_wingmen`, `wingman_plane`, `ace_*`, `group%d` and per group `num_enemies`, `enemy_name`, `enemy_plane`, `enemy_accentID`; **no** faction, side, team, allegiance, mortality or invulnerability key |
| spawn routine | `0x45a390` | walks the player seat, the wingman slots (`num_wingmen`), the four groups (`num_enemies` each) and the ace; each aircraft is placed by `0x47c210(node, placement, 0, 0)` after a node name built with `"%s_ia%d"` |
| spawn call | `0x47c210` | takes a scene node, a placement record and an optional position; **no side or mortality argument**. It looks the node up (`0x4d0280`), sets generic object flags and positions it; any allegiance comes from state it derives from the node and the airframe template (e.g. the object field `+0x67c` compared with 4 for the `"player"` name at `0x47c6cb`), which was **not traced** |
| per-group value around the call | `0x45b100`..`0x45b139` | reads the old value (`0x440710`), maps the group record's last dword (`+0x5c`: 0 -> 0, 2 -> 2, else 1), sets it (`0x440700`), spawns, restores it |
| that value | `0x43ff27`..`0x43ff36`, `0x440700` | the variable is the **`GameIntensity`** console variable (`push "GameIntensity"` then `mov [0x64f720], eax`): a difficulty value, not an allegiance or mortality |
| its source | `0x458ddd` | the group initialiser sets `+0x5c` to **1**; no descriptor reader writes it, so every group spawns at GameIntensity 1 |

The F14-D.5 faction rows are paint patterns (a livery choice), not an allegiance
table, as F49-D.03 already recorded; the F33 survivability policy is the
reimplementation's own vocabulary. Neither is an original measurement.

## Why this is not guessed

The catalog does seat the player, wingmen, enemy groups and the ace on sides
(`ScenarioSide`); that is the engine's own role split (which loop spawns them),
recorded since F49-D.03, with the ace's side an inference. A `ContentId`
faction and a `DeclaredSurvivability` are different facts: binding
"enemy groups belong to faction X" or "the ace is mortal" would be a guess
until the allegiance state `0x47c210` derives is traced.

## Unknowns that remain (affected: all 30 presets, every actor)

* Where an Instant Action aircraft's allegiance is decided (downstream of
  `0x47c210`: the node/airframe template, object field `+0x67c` and the
  object-creation chain `0x475820`, `0x4d1d50`, `0x4d1a30`). Resolving this
  needs a dynamic capture or a longer static trace.
* Whether any aircraft is immortal (no flag was seen; none is passed).
* The GameIntensity value 1 per group is a **difficulty** fact; it belongs to
  the scenario-difficulty follow-up (#1583), where its binding to the
  `IDS_IA_DIFFICULTY` label is also unmeasured.

Test: `accept_f49_d_03_fu_faction_*` (`crates/cs_app/tests/accept_f49_d_03_fu_faction.rs`;
the image test re-reads the keys the reader pushes and the bytes above,
independently of the production constants).
Sources: the decrypted image at the VAs above; the F49-D.01/.03/.02b findings.

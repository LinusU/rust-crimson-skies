# F27-C-FX-ORIG-MUZZLE-MOUNTS: the player's muzzle points and a legible burst frame

Date: 2026-10-25. Task #2113, follow-up of #2106
(`2026-10-25-f27-c-fx-orig-muzzle-burst-pose.md`). Capabilities: `retail`,
`gpu`, `CS_ENGINE_IMAGE`; static reading only, nothing ran the original
executable and nothing is `verified_original`. Rows: `[[player_firepoint_fact]]`
in `data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-25`,
provenance `static`). Tests: prefix `accept_f27_c_fx_orig_muzzle_mounts_` in
`crates/cs_app/tests/campaign/f27_c_fx_orig_muzzle_mounts.rs`. No disassembly
committed.

## Measured

1. **Markers.** The player Devastator (`player` > `player_pfighter`, slot 44 in
   `planes.zbd`) carries `firepoint1..8` under its `markers` node (identity
   linear part, world = local), in the stored frame whose nose axis is -Z:

   | marker | x | y | z |
   | --- | --- | --- | --- |
   | `firepoint1` | -1.9094 | 0.6019 | -1.0957 |
   | `firepoint2` | 1.9095 | 0.6017 | -1.0957 |
   | `firepoint3` | -0.9981 | 0.4402 | -1.3397 |
   | `firepoint4` | 0.9982 | 0.4400 | -1.3397 |
   | `firepoint5` | -2.1176 | -0.8252 | -0.7028 |
   | `firepoint6` | 2.1176 | -0.8252 | -0.7028 |
   | `firepoint7` | -1.1650 | -0.6206 | -1.1995 |
   | `firepoint8` | 1.2049 | -0.5847 | -1.1995 |

2. **Slot order.** The airframe set-up at `0x47695e` (kind word 0 or 4) formats
   `firepoint%d` with `N = i - j + 7` for `j = 0, 2, 4, 6`, `i = 0, 1` and stores
   the nodes in table slots `k = 0..7`: **7, 8, 5, 6, 3, 4, 1, 2**.
3. **Which slot fires.** The muzzle chooser at `0x4b6c08` takes slot
   `k = 2 * group + side` from the selected fire group `[esi+0x604]` (0..3, set by
   the group keys) and a side toggle `[esi+0x4c4]` that flips after each use.
   Both start at 0. So a fresh session fires group 0 from `firepoint7` (left) and
   `firepoint8` (right) alternately; group 1 is 5/6, group 2 is 3/4, group 3 is 1/2.
4. **Production change.** The mission's left and right wing gun mounts
   (`gun_mount_2`, `gun_mount_3`) now sit at `firepoint7` and `firepoint8`
   (`WING_LEFT_OFFSET_M`, `WING_RIGHT_OFFSET_M` in `mission_session/weapons.rs`).
5. **Smoke frame.** The gun hold now enters the held-point view (`F7`) and
   `smoke_muzzle_camera` keeps its point at body `(-4.5, -0.3, -5.0)`, ahead of
   and abeam the left wing gun, looking at the aircraft; two `F8` presses
   return to the chase view. The burst is neither scaled nor recoloured.
   Owner-command smoke on this commit: 73 checks, 0 failed;
   `private/evidence/F27-C-FX-ORIG-MUZZLE-MOUNTS/frame_02882_gun_burst.png`
   (not committed) shows the burst as an orange flame under the left wing,
   about 4 m from the camera. It is partly behind the left HUD gauge.

## Still unknown

- **Nose gun.** Group 0 has no nose firepoint. The mission's designed nose cannon
  (`gun_mount_1`, 0, -0.4, -4.0) keeps its designed placement. Which gun
  slot/group M01's player really carries, and how many guns fire per press (the
  original fires one firepoint per shot, alternating; this mission's bank fires
  all three mounts at once), is unmeasured.
- The enable byte `[slot+0x20]` per firepoint and what clears it.
- AI aircraft use the same mounts (`attach_gun_rig`); their own airframes'
  firepoints are not read.
- The gun group keys' mapping to groups beyond the reset to 0 is read, not traced
  to a loadout.

## Not claimed

Checked, not `verified_original`. The comparison of the frame with the original
stays `human_review` (Linus).

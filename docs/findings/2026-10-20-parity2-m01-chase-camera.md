# PARITY2-M01-CHASE-CAMERA (#1731): chase view of the player plane

Task test prefix: `accept_parity2_m01_chase_camera_` (`crates/cs_app/tests/campaign/m01_chase_view_parity.rs`).
Visual target: the owner capture `orig25.png` (gitignored, `private/ref-drydock/m01/`).
Our frames: `private/evidence/PARITY2-M01-CHASE-CAMERA/{before,after}/` (no-teleport owner-command smoke, not committed).

## Checked, not redone
The #1666 camera fit (10.1 m astern, 2.1 m up, designed, fixture 55° vertical FOV) still holds against
`orig25.png` in fractions of the image height: lower wing 0.89 (capture 0.918), spinner centre 0.69 (0.70).
The original's distance, height and FOV remain **unknown** (no VA recovered; `2026-10-18-parity-m01-player-chase-camera.md`).
The wing spans 0.50 of the width in our 16:9 window against 0.69 in the 4:3 capture: framing is `PreserveVertical`,
so this is the aspect, not the distance. The original's horizontal framing at 16:9 is unmeasured and not changed here.

## Changed
* **Static blades.** `staticprop1` (opaque `rotorblade.tif`) was drawn beside the `prop1` blur disc. It now carries
  `StaticPropellerBlades` and hides while the engine runs (the blur's own rule), shows when stopped. The capture shows
  a blur and no blades while flying; that the original swaps them is the capture's reading, not a measured node flag.
* **Wires.** The wires were the `pirate_hook` meshes (`r_arm3`, `l_arm3` with `spar.tif`/`oildock1.tif`, plus
  `door_db`, `r_door`, `l_door`; slots 2360, 2336, 3005, 2511, 2330), drawn because they sit under `healthy` outside every
  LOD band. They now spawn hidden; the boarding state that shows them belongs to F36.

## Unmet / unknown
* The plane's lower wing tips still reach the bottom gauge row in 16:9 (`PreserveVertical`); the capture is 4:3.
* The spinner is yellow (`dev_spinner.tif` of ours) where the capture's is white; the blur is fainter than the capture's.
* Horizon-level camera-up (#541) is unchanged.

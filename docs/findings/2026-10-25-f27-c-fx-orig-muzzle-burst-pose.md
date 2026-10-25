# F27-C-FX-ORIG-DRAW-A: the pose weapon fire gives the muzzle burst node

Date: 2026-10-25. Task #2104, follow-up of #2060
(`2026-10-25-f27-c-fx-orig-engine-fire-animation.md`). Capabilities: `retail` +
`CS_ENGINE_IMAGE`; static reading only, nothing ran the original executable and
nothing is `verified_original`. Rows: `[[muzzle_burst_pose_fact]]` in
`data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-25`,
provenance `static`). Tests: prefix `accept_f27_c_fx_orig_draw_a_` in
`crates/cs_app/tests/campaign/f27_c_fx_orig_draw_a.rs`. No disassembly committed.

## Measured

1. **Consumer.** Weapon fire `0x5af4dd` starts the FIRE>ANIMATION handle with
   `0x4edc10(handle, 0, x, y, z, pitch, yaw, roll, 0, 0, 0)` (forwarded to
   `0x4ed730`), unattached case. `(x, y, z)` is the round's position, copied from
   the muzzle position. The parent argument is 0.
2. **Pose.** `0x4ed730` sets the animation's root node to that position and, unless
   animation flag `0x200` is set, calls `SetRotation(node, pitch, yaw, roll)`
   (`0x4d1a30`, the heading routine's SetRotation).
   - `yaw = atan2(-dir.x, -dir.z)`, `pitch = asin(dir.y)` (`0x5f704a` is the
     MSVCRT `_CIasin` import) with `dir` the unit muzzle direction;
   - `roll` = 0, or with FIRE block flag mask `0x1` a fresh uniform value in
     `[-pi/2, +pi/2)` per shot (`(rand()/32768 - 0.5) * pi`).
3. **Axis mapping.** The rotation builder `0x53bf40` (row-vector convention,
   `0x53cc60`) makes the image of local +Z `(sin yaw cos pitch, -sin pitch,
   cos yaw cos pitch)`. With the pose above this is exactly **`-dir`**: the burst
   node's **local -Z is the fire direction**, +Z points back at the shooter. Local
   +Y is "up" for level fire (`(sin a sin b, cos a, sin a cos b)`), local +X
   `(cos b, 0, -sin b)` at roll 0; the frame is right-handed. This agrees with the
   data: the shared `muzzlepuffer` has `LOCAL_VELOCITY 0 0 -20` (forward).
   So `mb1` / `mb_spinflame` geometry (z from -0.195 up to +0.4) extends mostly
   backward of the node origin.
4. **AT_NODE offset.** `muzzleburst_effects` calls `AT_NODE <burst> 0 -0.2 -1`.
   The offset is a point in the node's **local frame** (`0x4cf490` transforms it by
   the node's world matrix with `0x53cc60`; the attached branch stores node and
   offset on the new animation). By item 3 it is one unit **ahead of the muzzle
   along the fire direction** and 0.2 below the node's local +Y (`-0.2 * +Y`).
   Handedness: right-handed local frame, as in item 3 (test: ahead by 1.0 for four
   directions).
5. **EVENT_OFFSET is seconds.** Event kind 3 is compared with the event clock,
   advanced by the frame delta `[0x9ad744]`; that delta is the millisecond timer
   difference times 0.001 times a time scale of 1.0 (`0x59c0c0`), capped at 0.125 s
   (rows of task #1981). So `START_TIME EVENT_OFFSET 0.001` is 1 ms of that clock:
   the burst node is switched off on the next step, a one-frame flash.

## Still unknown

- Which of the executor's two modes (`0x4eb441` node-attached, `0x4eb45b`
  position snapshot via `0x4cf490`) the `AT_NODE` keyword selects: the mode word
  `[record+0xc]` is not traced to its writer. Either mode applies the offset in the
  node's local frame, so the placement above does not depend on it; only whether
  the sub-effects follow the node afterwards does (one-frame flash: no visible
  difference). Affects every `CALL_ANIMATION AT_NODE` use.
- The FIRE-block flag masks `0x1` (random roll) and `0x2` (attached start,
  `0x4edf50`) are read from code; which weapon rows set them was not measured
  (resolving task: draw task #2059 / its siblings must read the row flags).
- Blend/transparency of material flags 17 (unchanged from #1684).

## Not claimed

Checked, not `verified_original`; no screenshot comparison; no production change.

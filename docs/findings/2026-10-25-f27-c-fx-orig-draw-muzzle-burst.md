# F27-C-FX-ORIG-DRAW-C: the measured muzzle burst replaces the designed stand-in

Date: 2026-10-25. Task #2106, closes the chain #2060 → #2104 → #2105. Capabilities:
`retail`, `gpu`. Nothing here is `verified_original`; the burst is **checked**.

## What is drawn now

- `resolve_muzzle_drawable` (`crates/cs_app/src/mission_session/weapon_present.rs`) no
  longer returns `DesignedStandIn`. With burst art installed it returns
  `MuzzleDrawable::MeasuredBurst { weapon, burst, spin_roll_deg, .. }`; without it
  `MuzzleDrawable::NotDrawn` (no substitute cube, the record still exists).
- The art is `load_muzzle_burst_art(<install>, "c3", SELECTED_GUN_RECORD)` read at
  stage time for M01 only (`MissionStage::muzzle_burst`): `wep_50` →
  `FIRE > ANIMATION muzzle_burst_slug` → gamez nodes `mb1` (1 quad) and
  `mb_spinflame` (3 quads), textured with the decoded `slug_muzzle1/2.tif`.
- Quads are drawn at **scale 1** in the burst node's local space, unlit, alpha-blended,
  no culling. The record's `Transform` is the node pose of
  `2026-10-25-f27-c-fx-orig-muzzle-burst-pose.md`: translation = muzzle position,
  rotation = `YXZ(yaw = atan2(-d.x, -d.z), pitch = asin(d.y), 0)` so local **-Z** is the
  fire direction `d`. `d` is the firing mount's live canonical forward, carried from the
  fire step on `WeaponEffect::forward` (new) into the record's pose.
- `mb_spinflame` is turned about local z by one of the definition's measured
  `OBJECT_ROTATE_STATE` rolls, chosen per shot from the projectile serial.
- Lifetime: the burst is born on the shot's tick and released on the next
  (`MUZZLE_FLASH_LIFETIME_TICKS = 1`, from `EVENT_OFFSET 0.001` s), but never before a
  rendered frame has shown it (`MuzzleFlash::presented`), because the 120 Hz fixed loop
  can run two ticks in one frame and a burst born on the first would otherwise never be
  rendered. This reads the original's one-frame flash; it is a presentation choice, not
  a measured value.

## Claim `f27-c-fx-render.designed-muzzle-flash`

Id unchanged (ids are referenced by the smoke report); text now says: measured quads,
textures, scale, pose and one-tick lifetime from the original data and engine image,
statically; **not** `verified_original`.

## Still not drawn or provisional (recorded, unmeasured)

- Smoke puffer, lights, gunshell companions of `muzzleburst_effects`: not drawn.
- `AT_NODE 0 -0.2 -1` offset: applies to those companions (item 4 of the pose finding);
  the quads sit at the node origin, no offset applied.
- Blend of material flags 17: drawn alpha-blended and unlit, provisional.
- Random roll (`FIRE` flag mask `0x1`): which rows set it is unmeasured; roll is 0.
- Every shot of the mission uses the player's selected gun record's burst; whether other
  shooters (AI aircraft, turrets) bind a different `FIRE > ANIMATION` is not wired.

## M01 owner smoke (retail, gpu)

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir private/evidence/F27-C-FX-ORIG-DRAW-C`
passed (73 checks, failures none), `weapon_muzzle_flash_drawn`: a drawn measured burst
was live in 8 frames of the hold. The smoke script changed for this task: the gun hold
now holds the snap look to the side (`Numpad4`; the quads lie along the barrel axis, so
the chase camera behind the aircraft sees them edge-on), `smoke_flashes` runs before
`smoke_script`, and the first frame with a drawn burst is photographed
(`frame_*_gun_burst.png`).

**Visual result, stated plainly:** the `gun_burst` framebuffer shows no clearly visible
flame. The only candidate is a few-pixel orange patch at the mount (`frame_02882`), which
sits under the aft wing, not at the nose: the mission's gun mounts are designed
placements, not the original's muzzle points. Whether the burst looks like the
original's is **unverified** and needs the owner's eyes (`human_review`).

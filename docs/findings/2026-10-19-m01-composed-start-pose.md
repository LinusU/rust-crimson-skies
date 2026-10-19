# PARITY-M01-ATMO-E (#1652): the composed M01 start follows callback 10's body

Date: 2026-10-19. Implementer: `claude-2`. Built on
`docs/findings/2026-10-19-m01-launch-body-pose.md` (#1685); nothing here is
`verified_original`.

## What changed

`stage_for` now starts the M01 player at the body callback 10 leaves, not at the
stored `aiv.zrd` record: position (-1426, **250**, -1813) (stored y + 100 m,
`launch_body::composed_start_pose`), heading unchanged (40 stored degrees).
The velocity direction is the nose, as before.

## What is deliberately not changed

* **Speed.** callback 10's `-20.0` has an unmeasured unit (`velocity-integration`);
  the declared development speed stays.
* **The pose at the first control frame.** `node-detach-after-intro` and
  `cinematic-flag-clear` are still unknown, so this is the body at the end of the
  intro placement, not a measured first-control-frame pose.

## Smoke

`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir
private/evidence/PARITY-M01-ATMO-E`: 464 frames, 14 checks, no failures. The
scripted frames (e.g. `frame_00300_hud.png`) are taken after the script has
placed the player near each actor, so none is the first control frame. The
comparison with `private/ref-drydock/m01/orig25.png` is therefore **not made**;
it needs a smoke frame at tick 0 (follow-up).

# M01 hand-over speed: what the image and the data give, and what they do not (#1938)

Date: 2026-10-23. Implementer: `sonnet-1`. Evidence class: static reading of the
owner's decrypted image (`$CS_ENGINE_IMAGE`, sha-256 `43540fc9...6d75`, VA = file
offset + `0x400000`) beside the measured #1752 table. Nothing here is
`verified_original`. No code changed: no source for the measured 111.8 m/s was
found, and rule 4 forbids tuning to the table.

## Every speed the image writes to the player body around the launch

| source | value | where | reading |
| --- | --- | --- | --- |
| callback 10 / `0x47f1f0(0,0)` with `veh+0x91d` set | `(0,0,1)` rotated by the body quaternion x `-20.0` (f32 at `0x608040`), throttle 0.4 | `0x47f25f` | 20 in an unmeasured unit; not 111.8 |
| callback 951 handler `0x47e557` | `veh+0x198..0x1a0` x `-53.6448` (f32 at `0x60803c`, = 120 mph in m/s) written to `veh+0x924` | `0x47e6e9`..`0x47e72f` | 53.6 m/s along basis row 2; but nothing in the launch chain fires 951 (`2026-10-18-m01-launch-release.md`) |
| `0x489e30` (flight update while `[0x71c50c]` is set) | no integration; calls `0x489f60` (visibility/LOD classification, writes `veh+0x24c/0x24d`, no velocity) and `0x4b3050` (node write) | `0x489e53`..`0x489e87` | writes no velocity |

## What this means for the two questions in #1938

1. **Why the hand-over velocity is half the measured one.** Not found. Neither
   static value equals 111.8 m/s (20 units/s and 53.64 m/s). Note only that
   111.8 m/s is 250 mph (111.76 m/s) to the table's precision and 2.084 x 53.6448;
   neither is a derivation. The measured t=0 sample is read at +-1 s, so it may
   already include the first seconds of control, which would make it a cruise
   speed (113.6 m/s at t=10..28) rather than a spawn velocity. Which of these is
   true needs the owner's capture of `V+0x924` at the first control frame.
2. **Why the no-input flight does not sink.** Open. The hand-over throttle the
   image writes is 0.4; the measured original holds 113.6 m/s with a 5.6 m/s
   sink and a nose 2.8 degrees down, which `2026-10-21-m01-hand-over-pose.md`
   found is not our law's equilibrium at 0.4 (~80 m/s). Whether the throttle is
   raised after the cinematic (the `0x124` slew toward a command, a key, or
   the intro's engine animations) was not found in the paths read.

## Needed to proceed

* A capture of `veh+0x924` and `veh+0x124` at every frame from the moment the
  cinematic ends to t=3 s (the existing #1752 harness, a finer window).
* Or a static trace of what clears `[0x71c50c]`/`veh+0x91d` and writes
  `veh+0x124` afterwards (`node-detach-after-intro`, `cinematic-flag-clear`,
  `velocity-integration` in `launch_body::BODY_UNKNOWNS`).

The declared development speed (`RETAIL_START_SPEED_M_S`, 55 m/s) stays and the
#1851 pinned trajectory test is unchanged.

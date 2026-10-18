# F27-C-FX-RENDER: drawing the muzzle-effect record

Date: 2026-10-19. Task: #1595. Predecessors:
`docs/findings/2026-10-18-f27-c-fx-sched-scheduled-presentation.md` (#1304, which
spawns one `MuzzleEffectSpawn` per accepted shot) and
`docs/findings/2026-10-17-m01-window-fire.md` (#1536, the round tracers).
Capabilities used: build, test, `retail` + `gpu` for the owner-command smoke.

## Observable failure before

The schedule spawned a `MuzzleEffectSpawn` record per accepted shot and nothing
read it: the owner smoke's held second put 90 rounds through the session and
drew no flash.

## What changed

- `mission_session/weapon_present.rs`: `MissionWeaponPresentationPlugin` now
  runs, on the fixed loop after `drive_weapon_effect_presentation`,
  `attach_muzzle_flashes` (one mesh child per record that carries no
  `MuzzleFlash` yet — the component is the once-only guard) and
  `expire_muzzle_flashes` (despawns record and drawing once the committed tick
  ledger reaches `at + MUZZLE_FLASH_LIFETIME_TICKS` = 6 ticks, designed).
  The drawing is a child of the record, so `release_muzzle_effects` (session swap
  or removal) takes it along; the clock is the tick ledger, so a pause freezes it.
- `resolve_muzzle_drawable` maps a declared id to `MuzzleDrawable::DesignedStandIn`
  under claim `f27-c-fx-render.designed-muzzle-flash`; the declared id is kept.
- `mission_session/smoke.rs`: new check `weapon_muzzle_flash_drawn` beside
  `weapon_rounds_drawn` (`docs/PLAYTEST.md` lists it).
- Tests: `tests/campaign/f27_c_fx_render.rs`, prefix `accept_f27_c_fx_render_`
  (draw + label, no double draw, expiry, pause freeze, session removal).

## Owner-command smoke (this machine, Apple M3 Pro, Metal)

`cargo test -p cs_app --test cs_app_tests_a -- accept_m01_owner_smoke --include-ignored`
with `CS_M01_OWNER_SMOKE_OUT=private/evidence/M01-OWNER-SMOKE-1595` (private, not
committed): `passed: true`, no failures; `weapon_guns_fire` 90 rounds,
`weapon_rounds_drawn` 66/66, **`weapon_muzzle_flash_drawn` pass — a drawn flash
live in 59 frames of the hold, 0 undrawn**. `frame_00361_guns.png` shows the two
flash cubes at the gun mounts beside the tracers. Only `world_sea_visible` is
the by-name skip it always is.

## Unknown, not guessed

- **The original's muzzle-effect id → asset mapping is not recovered.** The
  archive holds a `muzzle_burst` member in `zrdr.zbd`
  (`docs/findings/2026-10-04-vs-m01-runtime-m01-launch-closure.md`), but nothing
  binds it to a declared `hardpoint_equipment` id and a name is not a binding, so
  it is not used. Affected content: every muzzle flash. The flash drawn here is
  a project-designed unlit cube with no measured size, colour, orientation or
  duration; it makes no claim about the 2000 original. Original-measurement
  task filed from this one.

## Not claimed

No fidelity to the original, no human review; at most **checked**.

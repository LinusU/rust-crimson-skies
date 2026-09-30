# F25-C: forced assignment and mission-only aircraft wired to the law

Date: 2026-10-01. Task: F25-C (`specs/F25-...md`, `### F25-C`). Capabilities: ordinary build/test only; no `CS_GAME_DIR`, so no evidence report. Awards at most **checked**; every role, profile and tuning used is a synthetic fixture.

## Files and the one observable failure

- `crates/cs_app/src/airframe_visual.rs`: `MissionAircraftSession` (`launch`, `fly`, `telemetry`, `apply_damage`, `unload`), `MissionSessionError`, `rotor_mapping_from_role`. `cs_app` is the only crate that depends on both `cs_content` (roles) and `cs_sim` (law), so the wiring lives there.
- `crates/cs_app/tests/accept_f25_c_mission_only_session.rs`: three acceptance tests.

Failure before: nothing consumed `AirframeRoles::resolve_launch`; a mission-only aircraft had no path to the control law, damage or teardown.

## Behaviour

- Launch goes through `resolve_launch`; a forced assignment wins, the owned loadout is only borrowed, and `persists_to_owned_loadout()` is false. No shop list is read.
- Refusals are named (generation mismatch, unknown airframe, non-exceptional role, profile of another airframe, bad tuning) and leave no session, so a corrected retry works.
- Damage is validated and applied to later ticks; a refused record keeps the previous one.
- `unload` stops the rotor and every later call returns `Unloaded`; a refused tick leaves the rotor untouched (F25-B transactional commit).
- The role's rotor ratio becomes a `RotorSpeedMapping`; an unknown ratio gives no mapping and no visual rate.

## Unmet / unknown

- The tuning and profile are supplied by the caller; the airframe id -> tuning lookup and the original roster are not read here (F25-D, retail).
- Dispatching the flight driver on `ModelKind` is F25-E.
- Nothing here is a measured original value.

Implementer: claude-1. Not independently reviewed yet.

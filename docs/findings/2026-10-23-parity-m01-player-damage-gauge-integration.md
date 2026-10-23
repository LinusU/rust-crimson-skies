# PARITY-M01-PLAYER-DAMAGE-GAUGE: integration check

Task #1701, closing over #1942 (mapping finding) and #1943 (drive). Provenance:
`static` (never `verified_original`).

- Production path: `hud_art.rs::gauge_frame` / `zone_texture_name` drive each
  cockpit zone from the damage resolver's `PartState` and remaining integrity
  through the measured ladder `0.72 / 0.46 / 0.2` over
  `[green, yellow, orange, red]`. No colour table is invented; see
  `2026-10-23-parity-m01-player-damage-gauge-drive.md`.
- Tests: `accept_parity_m01_player_damage_gauge_*` in
  `crates/cs_app/tests/campaign/vs_m01_rt_hud_art.rs` (one retail ignored) and
  `accept_parity_m01_player_damage_gauge_a_*` in `crates/cs_formats/tests/zbd/`
  (three retail ignored), all passing with `--include-ignored`.
- Owner-command smoke on the integrated commit: `cs --cs-path <retail> --mission
  M01 --smoke-seconds 45 --capture-dir private/evidence/PARITY-M01-PLAYER-DAMAGE-GAUGE`,
  67 checks, no failures. `frame_03166_damage_trail.png` shows nose yellow,
  left wing orange, tail red, right wing green (frames not committed).
- Unmeasured, unchanged: threshold unit and boundary, what feeds a part's 0..1
  health in the original, the `<part>_got_hit` blink, node resolution of the
  cycle event. `pdpanelN` anims are body-damage effects, not gauge data.

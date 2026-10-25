# F27-C-FX-ORIG-DRAW: integration check of the measured muzzle burst

Date: 2026-10-25. Task #2059, the integration step after #2104, #2105 and #2106.
Capabilities: `retail`, `gpu`. Nothing here is `verified_original`; the burst is **checked**.

## Acceptance, item by item

- The measured drawable replaces the stand-in: `resolve_muzzle_drawable` has no
  `DesignedStandIn` left (`grep -rn DesignedStandIn crates` is empty). It returns
  `MeasuredBurst` or `NotDrawn`.
- Read from retail at load, no assets committed: `load_muzzle_burst_art` (#2105).
- Single-frame lifetime: `accept_f27_c_fx_orig_draw_c_the_burst_lives_one_tick`.
- Orientation measured: `2026-10-25-f27-c-fx-orig-muzzle-burst-pose.md` (#2104).
- Smoke and screenshots: cited in `2026-10-25-f27-c-fx-orig-draw-muzzle-burst.md`.
- Claim `f27-c-fx-render.designed-muzzle-flash` updated there.

`cargo test --locked -p cs_app -- accept_f27_c_fx_orig_draw --include-ignored` with
`CS_GAME_DIR` set ran 11 tests, all passed.

## Still open

The `gun_burst` screenshot shows no clearly visible flame, because the mission's gun mounts
are designed placements, not the original's muzzle points. Whether the burst looks like the
original's needs `human_review`. Smoke puffer, lights, gunshell companions, the `AT_NODE`
offset and the blend mode remain undrawn or provisional, as listed in the draw finding.

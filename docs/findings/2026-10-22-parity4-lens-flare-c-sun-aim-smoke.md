# PARITY4-LENS-FLARE-C (#1860): the sun-aim smoke frames

Date: 2026-10-22. Follow-up of `PARITY4-LENS-FLARE-B` (#1859), which
recovered the M01 `sun` node's stored bulb and the four `lflare` quad
placement. Capabilities: `retail,gpu,audio` — the windowed run is this
machine's own frames of the reimplementation; no original executable
ran, so nothing here is `verified_original`. The aim, the screen
fraction and the hidden-frame comparison are designed to reproduce the
composition of the owner's reference capture (cited in
`docs/PLAYTEST.md`, comparison-only, never committed).

## The six `accept_parity4_lens_flare_c_` tests

Five unit tests in `crates/cs_app/src/mission_session/smoke.rs`
(`mission_session::smoke::tests`) exercise the production phase without
a GPU:

- `accept_parity4_lens_flare_c_the_aim_puts_the_sun_on_the_capture_fraction`
  (CI): `flare_aim` turns a nose-on sun into the rotation that lands the
  bulb on the `0.35, 0.20` viewport fraction; a sun below the horizon or
  a zero direction refuses.
- `accept_parity4_lens_flare_c_the_correction_converges_a_missed_aim`
  (CI): `flare_aim_correction` iterated against the live projection
  converges a deliberately missed aim onto the fraction.
- `accept_parity4_lens_flare_c_the_probe_hides_the_quads_alone` (CI):
  `ProbeMode::LensFlareAbsent` writes `Visibility::Hidden` onto exactly
  the `FlareQuadNode` rows and nothing else — the world, the sky, the
  player and the rest of the UI keep their drawn state, so the
  `lens_flare`/`lens_flare_hidden` pair differs only in the flare.
- `accept_parity4_lens_flare_c_the_phase_aims_pauses_photographs_and_restores`
  (CI): the scripted phase queues the `Freeze` hold (a `Menu`-reason
  hold — never `PlayerRequest`, so the Esc map stays shut), the test
  placement, the `lens_flare` screenshot, the `LensFlareAbsent` probe,
  the `lens_flare_hidden` screenshot, the restore and the resume in
  order.
- `accept_parity4_lens_flare_c_the_judge_names_every_missing_piece`
  (CI): a measurement row with no source, no placement, a missed
  fraction, an undelivered pair and a quad under the pixel floor fails
  `lens_flare_sun_in_view` and `lens_flare_quads_drawn` with named
  detail.

One ignored retail/GPU/audio test
(`crates/cs_app/tests/render/parity4_lens_flare_c.rs`) drives the
owner's own command on a built release binary:

- `accept_parity4_lens_flare_c_the_windowed_smoke_photographs_the_sun_and_its_rings`
  (`#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed
  binary"]`): `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds
  45 --capture-dir <dir>`, then `report.json` — the aimed source is
  `sun`, the placement was written, the measured sun sits on the
  designed frame fraction, both raw frames arrived, all four `lflare`
  quads drew and each added at least `MIN_FLARE_QUAD_PIXELS` over the
  hidden frame — plus the two framebuffer PNGs on disk.

## The 2026-10-22 run (swe2-max-1, Apple M3 Pro, Metal)

`cargo test --locked -p cs_app -- accept_parity4_lens_flare_c_
--include-ignored`: 6 discovered, 6 executed, 6 passed, 0 failed (the
windowed smoke itself 45 s). Windowed `report.json` `lens_flare`
object, verbatim:

```json
{"source":"sun","placed":true,"sun_px":[448.0,143.7],
 "viewport_px":[1280,720],"frames_delivered":true,
 "quads":[{"texture":0,"drawn":true,"region":[736,127,320,320],"pixels_added":56273},
          {"texture":1,"drawn":true,"region":[928,344,320,320],"pixels_added":20389},
          {"texture":2,"drawn":true,"region":[1162,597,160,160],"pixels_added":11771},
          {"texture":3,"drawn":true,"region":[1424,913,480,480],"pixels_added":72927}]}
```

Both new checks passed, and the smoke passed overall (66 checks, 0
failures, `private/evidence/PARITY4-LENS-FLARE-C/report.json`):

| check | verdict |
| --- | --- |
| `lens_flare_sun_in_view` | pass — the aimed bulb measured at logical 448×143.7 of 1280×720: exactly the `0.35, 0.20` fraction of the owner's capture |
| `lens_flare_quads_drawn` | pass — all four quads drew and each added 11 771–72 927 pixels over the hidden frame (the weakest quad ~46× the floor) |

Framebuffers (non-black, 2560×1440):
`private/evidence/PARITY4-LENS-FLARE-C/frame_03183_lens_flare.png`
(flare alive — sun glow upper left, ring chain down through the centre)
and `private/evidence/PARITY4-LENS-FLARE-C/frame_03187_lens_flare_hidden.png`
(the identical frozen frame four frames later with only the quads
hidden). The run's `cargo-test.log` and `acceptance.json` (schema
`schemas/evidence.schema.json`, validated with
`tools/validate_evidence.py --require-pass`) sit beside them; a copy is
committed as `docs/findings/evidence/PARITY4-LENS-FLARE-C.json`.

## Scope and limits

- The freeze is a scripted `PauseReason::Menu` hold, not the
  `PlayerRequest` pause a player makes: the Esc pause map gates on
  `PlayerRequest`, so the `Menu` hold freezes the clock identically
  while leaving the world view on screen. The sea probe's frames were
  clean under `PlayerRequest` only because those modes hide all UI.
- The comparison is per-quad on the raw frames at `FLARE_PIXEL_DELTA`
  = 4, not the flight-check delta of 16: the faintest `lflare` disc at
  50 % alpha changes most of its pixels by single-digit levels, so a
  16-level threshold undercounts a drawn quad (measured: 129 px vs
  ~20 000 px). Delta 4 still rejects byte-equal noise; everything
  outside the quad regions compares identical.
- The placement is the smoke's own test placement (listed in
  `report.json` under `placements`), written onto the production player
  body and corrected once against the live camera projection — it is
  not an original flight path.
- `apply_probe` runs strictly after `place_flares` in `PostUpdate`, and
  in `Restored` mode it no longer touches the player body's visibility:
  `drive_cockpit_camera` owns that row every frame outside the probes.
  Regression caught by `cockpit_eye_placement`, not by inspection.
- The owner's capture is reference-only. Nothing in this task compares
  against the original executable's pixels; the frames prove the
  reimplementation draws the flare composition the split designed.

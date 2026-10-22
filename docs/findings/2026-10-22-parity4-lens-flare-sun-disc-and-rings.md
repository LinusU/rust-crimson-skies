# PARITY4-LENS-FLARE (#1756): the split's integration step

Date: 2026-10-22. Follow-up of
`2026-10-21-parity4-lens-flare-engine-rule.md`,
`2026-10-22-parity4-lens-flare-a-sun-source-and-blend.md` and
`2026-10-22-parity4-lens-flare-c-sun-aim-smoke.md`. Capabilities:
`retail,gpu,audio` — the windowed run is this machine's own frames of the
reimplementation; no original executable ran, so nothing here is
`verified_original`. The aim, the screen fraction and the hidden-frame
comparison are designed to reproduce the composition of the owner's
reference capture (cited in `docs/PLAYTEST.md`, comparison-only, never
committed).

This is the integration step of the three-part split, not a re-run of it:
each subtask's tests (`accept_parity4_lens_flare_a_`,
`accept_parity4_lens_flare_b_`, `accept_parity4_lens_flare_c_`) stay as
they landed, and the parent adds its own `accept_parity4_lens_flare_`
prefix covering the chain end to end — the stored mesh light that flags
the horizon's `sun` node (the `unk60` flare word on mesh 491's 76-byte
light, drawn through node `sun` slot 3237), the `lflare1..4` art out of
the world group's own archive, the four-quad screen-space draw the
engine's `0x57ca80` rule lays out, and the owner-command smoke frame that
shows it.

## The four `accept_parity4_lens_flare_` tests

`crates/cs_app/tests/render/parity4_lens_flare.rs`, module of
`cs_app_tests_b` (the `render/` group beside the `_b_`/`_c_` files):

- `accept_parity4_lens_flare_a_flagged_bulb_in_view_lays_out_all_four_quads`
  (CI): the production `MissionLensFlarePlugin` over a stand-in scene
  camera and a bound `node-3237` object — `spawn_flare_nodes` makes one
  quad per `lflare` texture, `place_flares` projects the stored bulb
  through the camera and lays each quad at the centre and half-extent
  `flare_quads` computes for the stored fade pair (full at M01's depths),
  the sprite routine's 50 percent alpha on the image node.
- `accept_parity4_lens_flare_the_gates_hide_what_the_engine_drops` (CI,
  negative): the same source behind the camera or projected outside the
  viewport leaves all four quads hidden — the two record-queue drops the
  engine applies — and a `held_back`-only source set spawns no quad node
  at all: `spawn_flare_nodes` iterates `drawn`, so the two `lens_flash`
  nodes whose placement is unrecovered never become a guessed quad.
- `accept_parity4_lens_flare_retail_m01_the_stored_sun_drives_the_four_quads`
  (`#[ignore = "requires CS_GAME_DIR"]`): real M01's `ZBD/C3` through the
  production readers — `LensFlareSources::from_container` draws exactly
  the `sun` record (stored bulb `(6.18489, 18.294209, ~0)`, fade pair
  `(5, 10000)`, the two `lens_flash` nodes held back), `read_flare_images`
  resolves `lflare1..4` from the group's archive, and the plugin lays the
  four quads out for the real record.
- `accept_parity4_lens_flare_the_windowed_smoke_shows_the_sun_and_its_rings`
  (`#[ignore = "requires CS_GAME_DIR + gpu + audio and a built windowed
  binary"]`): runs the owner's own command
  `cs --cs-path $CS_GAME_DIR --mission M01 --smoke-seconds 45
  --capture-dir <dir>` — the same smoke the LENS-FLARE-C phase drives;
  this task's copy is the integration acceptance the parent still owes
  (rule 6: seen, not just tested). Checks `report.json`'s two flare
  checks applied and passed, the serialised `lens_flare` measure and the
  `lens_flare` / `lens_flare_hidden` framebuffers non-black.

## The 2026-10-22 run (swe2-max-1, Apple M3 Pro, Metal)

`cargo test --locked -p cs_app -p cs_formats --
accept_parity4_lens_flare_ --include-ignored`: 16 discovered, 16
executed, 16 passed, 0 failed — the whole prefix: the two `_a_` retail
and engine-image tests in `cs_formats`, the `_b_` unit and retail tests,
the five `_c_` unit tests and the `_c_` windowed run, and this task's
four. Windowed `report.json` `lens_flare` object, verbatim:

```json
{"source":"sun","placed":true,"sun_px":[448.0,143.7],
 "viewport_px":[1280,720],"frames_delivered":true,
 "quads":[{"texture":0,"drawn":true,"region":[736,127,320,320],"pixels_added":56273},
          {"texture":1,"drawn":true,"region":[928,344,320,320],"pixels_added":20389},
          {"texture":2,"drawn":true,"region":[1162,597,160,160],"pixels_added":11771},
          {"texture":3,"drawn":true,"region":[1424,913,480,480],"pixels_added":72927}]}
```

Both flare checks passed, and the smoke passed overall (66 checks, 0
failures, `private/evidence/PARITY4-LENS-FLARE/report.json`):

| check | verdict |
| --- | --- |
| `lens_flare_sun_in_view` | pass — the aimed bulb measured at logical 448×143.7 of 1280×720: exactly the `0.35, 0.20` fraction of the owner's capture |
| `lens_flare_quads_drawn` | pass — all four quads drew and each added 11 771–72 927 pixels over the hidden frame (the weakest quad ~46× the floor) |

Framebuffers (non-black, 2560×1440):
`private/evidence/PARITY4-LENS-FLARE/frame_03183_lens_flare.png` (flare
alive — the sun's glow upper left, the ring chain down through the frame
centre, the big mirrored disc lower right, the composition of the owner's
capture) and
`private/evidence/PARITY4-LENS-FLARE/frame_03187_lens_flare_hidden.png`
(the identical frozen frame four frames later with only the quads
hidden). Both PNGs hash byte-identical to the LENS-FLARE-C run's pair —
the scripted smoke is deterministic on the same tree. The run's
`cargo-test.log` and `acceptance.json` (schema
`schemas/evidence.schema.json`, validated with
`tools/validate_evidence.py --require-pass`) sit beside them; a copy is
committed as `docs/findings/evidence/PARITY4-LENS-FLARE.json`.

## Scope and limits

- The freeze is a scripted `PauseReason::Menu` hold, not the
  `PlayerRequest` pause a player makes, and the sun aim is a test
  placement on the production player body (listed in `report.json` under
  `placements`), corrected once against the live camera projection — not
  an original flight path.
- The blend is designed: the sprite routine's `0.5` opacity is applied as
  alpha; whether the four `lflare` texture descriptors carry the additive
  flag is untraced (the A finding §4, §7).
- The engine's per-flare terrain ray cast (mask `0x40000`) is not
  implemented: which world objects carry that bit is unrecovered; the sun
  is above any terrain in M01, so the screen-bounds and near-plane tests
  are the practical gates there.
- The two `lens_flash` nodes' placement and trigger are unrecovered —
  held back, never drawn at a guess.
- The owner's capture is reference-only. Nothing in this task compares
  against the original executable's pixels; the frames prove the
  reimplementation draws the flare composition the split designed.

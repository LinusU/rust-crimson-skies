# Cabin memento frame: owner-command smoke (#2035)

Checked on `d7fdbe699` (#2040 draws the picture, #2039 decoded the pane transform).

* Command: `CS_FRONT_END_SMOKE_SCENE=cabin target/release/cs --cs-path <retail> --front-end --smoke-seconds 14
  --capture-dir <dir>`. Report: `PassengerCabin` visited, 840 frames, 5 framebuffers, 0 unhandled events.
* `cabin.png` (1600x1200): the pilot's memento photo is drawn inside the cabin photo frame, behind the frame
  border, at about x 345-515, y 640-860. The key-colour patch no longer shows.
* Task tests: `cargo test --locked -p cs_app -- accept_parity5_cabin_memento_frame_ --include-ignored`,
  4 passed (the retail one with `CS_GAME_DIR`).

## Still unknown

* Whether the decoded position matches the original. The owner's cabin capture
  (`private/ref-drydock/cabin/original.png`) is not on the agent machines, so the 11 px / 5 px offset recorded in
  `2026-10-24-cabin-memento-pane-transform.md` is not resolved. Needs that capture or `human_review`.
* The other unknowns of that finding (layout scale override columns, x/y rotations, 0.5 px bias) stand.

This is a checked state, not `verified_original`.

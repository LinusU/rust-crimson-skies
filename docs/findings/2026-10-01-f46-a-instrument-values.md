# F46-A: instrument values and display-unit policy

Task #187. Projection in `cs_app::ui::hud`; unit policy in `cs_content::hud`;
tests are the `accept_f46_a_*` tests in `crates/cs_app/tests/hud/`.

All behavior is **designed**, synthetic and not original-verified. Awards at
most *checked*. No original gauge, unit, threshold or file was read. Every policy
value carries `Basis::Designed`; `HudPolicy::fully_verified` is false until F46-B
imports verified values.

## One observable failure

A known attitude, nose 30 degrees up, must read pitch 30 degrees and heading 0.
A projection that confused the axes (or used Euler angles with the wrong order)
fails `accept_f46_a_known_attitude_gives_expected_horizon_and_heading`.

## Designed semantics

- **Angles.** Pitch is nose elevation; roll is positive with the right wing down;
  heading is measured from canonical forward (-Z) and increases toward +X, in
  `[0, 2*pi)`. Heading is `None` with the nose vertical and roll is `None` when
  the wings axis is vertical; no angle is invented.
- **Speeds.** Air speed is `|v - wind|`; ground speed is the horizontal part of
  `v`. The policy says which one the main gauge reads.
- **Altitude.** From the policy's datum (world `y = 0`, or the terrain below, which
  needs a ground height or the projection refuses). Low-altitude warning sets
  below one threshold and clears above a higher one.
- **Binding.** A sample stamped with another session or actor is refused; binding
  resets the warning. Empty ammunition sets `WeaponGauge::empty` and never touches
  the selection.

## Unknowns (not guessed)

- The original display units, which speed the original gauge reads, the altitude
  datum and the low-altitude threshold: F46-B imports, F46-D compares.
- Which canonical axis the original compass calls north, and its dial direction.
- Whether the original shows ground speed at all.
- Damage, target and spyglass, objective and map projections: later F46 stages.
  The `AircraftSample` carries no damage or target field yet; they must be added
  with the same session/actor stamp.
- The ground height source: the caller supplies it (F18/F19 terrain query); this
  stage does not choose one.

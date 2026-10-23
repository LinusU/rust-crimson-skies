# #2005 BUG-M01-TERRAIN-CRASH-DAMAGE: a direct terrain impact is fatal on first contact

Date: 2026-10-24. Implementer: `claude-2`. Provenance: **designed calibration**, not a
measurement; nothing here is `verified_original`.

## Observation and cause

Owner playtest of the original (2026-10-23): a direct, non-glancing terrain impact is almost
always fatal on first contact, a light brush that redirects the plane is survivable. Ours let
the player hit solid terrain repeatedly. Cause: the contact does the original's damage
`max(300*s^3, 50)*2` (100..600, `crates/cs_sim/src/contact_response.rs`), but it was applied to
the designed 1200 hull pool (`DESIGNED_PLAYER_INTEGRITY`), so even `s = 1` (600) could not
kill a fresh player. The severity `s` measured by the judge was not at fault: the head-on test
dive yields `s > 0.9`, the contact normal is the manifold's, and each contact episode is
judged once.

## What was tried to recover the real number

Not recovered. The player airframe's zone armor/health totals and the split of one damage pair
between a zone's armor and health in `0x4b7f80` are still unread (finding
`2026-10-22-crash-a-fatal-contact-and-crash-camera.md`, "Unknown"). Decoding that consumer is a
separate static-analysis task; this change does not claim it.

## What changed (designed)

* `crash_hull_damage(raw, integrity) = integrity * (raw / 300)^2`
  (`DESIGNED_LETHAL_CRASH_DAMAGE = 300`, `DESIGNED_CRASH_HULL_EXPONENT = 2`) converts the
  original's raw crash damage into the hull pool's damage. The retail `[50, 300]` range and the
  twice-player factor are unchanged, and gun/ordnance damage to the same pool is untouched. The
  wing region still takes the raw damage, so wing damage and fire presentation are as before.
* `TerrainContactJudged::hull_damage` carries the pool damage; `damage` stays the raw value.

| severity `s` | raw damage | hull damage (of 1200) |
| --- | --- | --- |
| 0 .. 0.41 (floor 100) | 100 | 133 |
| 0.32 (island graze) | 100 | 133 |
| 0.60 | 133 | 236 |
| 0.71 (45 degrees off the normal) | 216 | 622 |
| >= 0.80 (about 37 degrees off the normal or less) | >= 307 | >= 1200, fatal alone |

The lethal damage 300 was chosen so a strike within roughly 37 degrees of the surface normal
kills alone, and so the sea skim and island graze of the #1752 no-input flight (four episodes,
severities 0.32, 0.41 and two near 0) cost 533 and stay survivable. The curve is project design
fitted to the owner's qualitative observation, not to a capture.

## Consequences for pinned tests

* `accept_crash_b_survivable_contact_keeps_flying` now flies an oblique pass; the straight dive
  it flew is fatal on a fresh hull (`accept_crash_b_fatal_contact_*`, `accept_crash_d_01_head_on_*`
  and the composed-M01 crash flow no longer pre-damage the hull by 700).
* The no-input M01 flight (`accept_parity4_m01_collision_no_input_trajectory_is_pinned`) now ends in
  a crash at the hard push into the stopped shore (sixth episode, `s = 0.71`, before t = 70): that
  stop is already a recorded trajectory artifact (the original does not stop there).

## Unknown

* The player airframe's zone armor/health totals and the lethal threshold of the original.
  Resolving: decode `0x4b7f80` and read the airframe `armor`/`health` blocks.
* Whether the original's 45-degree strike is fatal; the owner's observation names only direct impacts.
* Restart restoring full health is the existing behaviour (the victim is re-registered on a new
  session); not re-pinned here.

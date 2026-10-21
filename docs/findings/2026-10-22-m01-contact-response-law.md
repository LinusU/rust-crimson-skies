# #1849: the contact response law, its import and its wiring (static)

Date: 2026-10-22. Implementer: `claude-1`. Provenance: `OWNER-STATIC` (see
`docs/findings/2026-10-21-m01-collision-response.md` on the #1752 branch, not
yet on `main`). Nothing here is `verified_original`; nothing was tuned to the
owner's measured table.

## What is now in the build

* `cs_content::original_airframe::OriginalDocuments::crash` /
  `import_retail_crash` read the first `player.zrd` entry's `crash` record:
  `bounce_factor` (retail `0.6`, stored single-precision), `armor_damage_range`
  and `health_damage_range` (retail `[50, 300]` each). Missing or non-finite
  keys are errors; the image defaults (`0.8`, `200/15`) are never substituted.
* `cs_sim::contact_response`: `respond` (severity, velocity
  `v + Dv * (1 + bounce * lin)`, angular gain, the `2.25 |Dv|` split, the
  inertia in body axes) and `crash_damage` (`max(range1 * s^3, range0)`,
  doubled for the player), hand-computed unit tests.
* The retail playtest aircraft (`RetailFlight::crash`, `spawn_original_flight_body`)
  carries `Restitution(bounce_factor, Max)` and `Friction(0, Min)`.

## Limits (unresolved)

* **Avian applies the point-mass case only.** The runtime consumer of
  `bounce_factor` is Avian's restitution, i.e. `lin = 1`. `respond` is not
  driving the body: its angular share needs the vehicle's `collision` point
  list (params`+0xd8`), which is not imported (#1848 resolving task: read the
  M01 player airframe's `collision` block). Until then the contact acts at the
  body's centre.
* **Crash damage has no consumer.** `crash_damage` is a tested pure function;
  what the armor/health pair is subtracted from, and the `V+0x384` gate, are
  unknown (see #1848).
* **No sea collider** exists in M01 (unchanged); the skim and the beach impact
  are not reproduced by this task.

# Crash D.01: terrain contact event, wing-zone routing, crash smoke (#1944)

Status: implemented; the zone selection stays **unknown**.

## What the code does

* `judge_terrain_contacts` (`crates/cs_app/src/playtest/fatal_contact.rs`)
  writes one `TerrainContactJudged` message per judged contact episode, fatal
  or not: tick, aircraft, world contact point (the avian manifold point),
  outward normal, severity, damage, the wing region and the fatal flag.
* The contact damage that the hull takes is also applied to one wing region
  node (`leftwingdamage` / `rightwingdamage`) of the player's graph, in the
  same resolve call, on the side of the contact point in the aircraft's local
  x: `x < 0` is port (the handedness measured for the wingtip flares in
  `2026-10-19-parity-m01-player-view-c2-wing-fire-smoke-source.md`), otherwise
  starboard. `contact_wing_zone` (`mission_session/contact_fx.rs`) returns it
  as a `Known` under the `Provenance::designed` claim
  `m01-player-contact.designed-wing-zone-side`.
* A fatal contact pushes one `PuffBurst` at the crash position into
  `PendingOrdnanceBursts`; `burst_puffs.rs` draws the measured
  `large_black_smokeball` puffs from it.
* The dial: the existing designed zone swap in `hud_art.rs` maps a
  `Destroyed` zone to `redhilite` / `red_hatchptrn`; a retail test shows the
  wing the contact destroyed takes the red pair and the other wing keeps its
  authored material. The measured gauge rule is #1701.

## Unknown (claim `m01-player-contact.zone-selection-unmeasured`)

* Which damage zone the original picks for a terrain contact (routine
  `0x4b3950` is not decoded). Whether it is ever the nose or tail, whether it
  depends on the contact point or on the damage, and whether both wings can be
  hit by one contact are all unrecorded. The side rule above is a design
  stand-in; nose and tail are never chosen by it.
* The region pools (`DESIGNED_REGION_INTEGRITY`, 300) and the share of the
  contact damage a zone takes (here: the same value as the hull) are
  unmeasured. With the designed pools a head-on contact (600) destroys the
  wing and a floor-damage contact (100) only damages it.

Affected content: the dial's wing zone colour and the wing fire/smoke plume
(D.03) after a terrain contact in M01.

# #416: the world bootstrap's substep divergence is measured, not adopted away

Date: 2026-10-02. Task: #416 "Adopt the declared F23-D substep policy in every
world bootstrap", filed by the F23-D review (#92) so the world fixture's
`SubstepCount(1)` override would not survive silently. Capabilities used:
ordinary build/test only — no `CS_GAME_DIR` read, no evidence report, nothing
`verified_original`. Every number below was measured on the pinned pair
(`bevy 0.19.1` / `avian3d 0.7.0` / `parry3d 0.27.0`) in the real headless
composition — `cs_app::asset_stack::headless_app` + the real
`PhysicsAdapterPlugin` + `cs_app::world::{world_app, load_world,
spawn_swept_probe, spawn_discrete_probe}` — 120 Hz fixed rate, gravity zero.

## What the task assumed, and what is true

The task assumed adopting `DECLARED_SUBSTEP_COUNT` in `world_app()` was a safe
wiring change: the same engine, the same solver, the count already proven on
the F23-D envelope. The adoption was made and measured — and it **regresses
the world path's own collision contract**: three F18-B acceptance tests fail
(`..._a_swept_body_flies_through_the_mesh_opening_and_is_stopped_by_its_leg`,
`..._a_mesh_role_solid_stops_a_body_and_sensor_only_reports_one`,
`..._every_material_group_of_a_stored_mesh_reaches_the_collider`). The
regression is an engine-level interaction, so the task as specified cannot be
completed without weakening pinned tests; it was blocked for an owner ruling
(see below). What this branch does carry: the divergence is now *declared* —
`world_app()` passes `WORLD_FIXTURE_SUBSTEP_COUNT` to the plugin seam instead
of overwriting the `SubstepCount` resource after plugin build — and the
findings here.

## The measured defect: trimesh contacts apply zero impulse at two substeps

Probe: the production `spawn_swept_probe` body (0.5 m box, 250 kg,
`SweptCcd` + `SpeculativeMargin::ZERO` — the configuration every swept layer
carries, `crates/cs_app/src/physics/body.rs`). Wall: the harbor world's right
leg, a 1 m thick static trimesh (`objective.hangar`, derived by
`TrimeshFromMesh` on the collider-on-body layout). Speed: 30 m/s — only
0.25 m per tick, far below any tunnelling concern; a discrete overlap test
would catch this pair trivially.

| `SubstepCount` | swept probe | discrete probe |
| --- | --- | --- |
| 1 | embeds ~0.6 m into the wall, decelerates, ends at `x ≈ 0.06` | identical |
| **2** | **sails through at constant speed, ends at `x = 8.0`** | **identical** |
| 3 | embeds ~0.5 m, ends at `x ≈ 0.07` | identical |
| 4 | stopped at the face, rebounds to `x ≈ -0.82` | identical |

At two substeps the body crosses the wall with **no deceleration at all** —
the contact log records `CollisionStart`/`CollisionEnd` episodes against the
leg, so the narrow phase saw the pair; the solver simply applied no impulse.
Swept CCD is not the discriminator: the discrete probe behaves identically,
and the same body at 400 m/s is still stopped by the swept sweep at every
count (`accept_f18_b_a_tunnelling_body_is_stopped_by_the_mesh_geometry_it_flys_at`
stays green at two substeps).

A speed sweep at `SubstepCount(2)` (start `x = -12.0`, same probe):

| speed | end `x` |
| --- | --- |
| 10, 20, 25, 33, 35, 40, 50, 75, 90, 120 m/s | `≈ -0.77` to `-0.89` — stopped at the face |
| **30, 60 m/s** | **`+2.0` — fully through** |

And the failure is not unique to two substeps in kind — only in degree:

| substeps | failing speeds in the sweep |
| --- | --- |
| 1 | 30, 60 m/s end **embedded inside** the wall (`x ≈ 0.19`, `0.30`) — the F18-B `x < 1.0` bound masks it |
| 2 | 30, 60 m/s **tunnel completely** |
| 3 | 30 m/s embedded |
| 4 | 90 m/s embedded |
| 6 | 30 m/s embedded |

So hollow trimesh contact response for a zero-margin body is unreliable at
**every** count on the pinned engine; `SubstepCount(2)` is where the measured
bands stop responding entirely. Cuboid statics are unaffected at every count
(same probe, same speeds, stopped at the face at 1 and 2 substeps), and a body
that keeps Avian's *default* `SpeculativeMargin` stops at both counts — the
failure needs the pinned `SpeculativeMargin::ZERO` × trimesh × `SubstepCount(2)`
combination. That combination is the production one: the F23 swept layers pin
the margin to zero on purpose (F23-B), and the mission world imports its
statics as trimeshes (F18-B/D).

Not caused by our code: the same failure reproduces with a hand-built
`Collider::trimesh` wall — no mesh-upload path, no world spawn code involved.

## Mitigations measured and rejected

* **`CollisionMargin` on the static trimesh** (Avian's own documentation
  recommendation for hollow shells): stops the 30 m/s band at 1–25 cm margins
  but does **not** stop 60 m/s at any tested margin — it narrows the failing
  bands, never closes them.
* **`TrimeshFlags::FIX_INTERNAL_EDGES`**: worse — the same wall lets the probe
  through at *every* tested substep count (1 and 2).
* **Restoring `SpeculativeMargin` on the probe**: fixes it, and is unavailable
  — the zero margin is a measured production decision (it is what isolates
  swept CCD from speculative prediction; the metres-scale hitbox inflation a
  nonzero margin implies is the rejected alternative in
  `docs/findings/2026-09-30-t420-mesh-ccd-decision.md` and F23 non-negotiable
  behavior 3).
* **A different count for the world path**: arbitrary — 1, 3, 4 and 6 each have
  their own failing bands; there is no measured "safe" count, only counts whose
  failures happen to be embedding rather than pass-through.

## What changed in code, and what is left open

* `world_app()` now declares its substep count through
  `PhysicsAdapterPlugin::with_substeps(WORLD_FIXTURE_SUBSTEP_COUNT)` — the
  count is part of the plugin's own construction, so a later
  `insert_resource` cannot silently diverge from it. The divergence is
  documented here and pinned by
  `accept_t416_the_world_bootstrap_declares_its_substep_count`.
* `accept_t416_the_declared_count_still_tunnels_the_mesh_leg` reproduces the
  blocker on the production composition. Its failure — the probe stopped —
  is the signal that `WORLD_FIXTURE_SUBSTEP_COUNT` can flip to
  `DECLARED_SUBSTEP_COUNT`.
* The `PhysicsFixture` divergence (`with_substeps(1)` for the
  one-integration-per-tick assertions) was already recorded in the F23-D
  finding; the other `SubstepCount` users are measurement scaffolding in
  `accept_t420_mesh_ccd.rs`, `accept_t424_collider_on_body.rs` and
  `tests/physics/evidence.rs`, each setting the count its probe measures.

## The ruling this needs

The task's acceptance criteria conflict with the measurement: "the mission
world runtime uses `DECLARED_SUBSTEP_COUNT`" and "a test shows the world
path's substep count equals the declared constant" cannot both hold while the
declared count tunnels bodies through mesh world geometry. The options are:

1. **Accept the documented divergence** — the world path keeps
   `WORLD_FIXTURE_SUBSTEP_COUNT = 1` until the trimesh interaction is
   resolved; criteria 2–3 become "the mission world runtime must import
   `DECLARED_SUBSTEP_COUNT` once the blocker is gone", which this file now
   records. Note the divergence is *differently* unsafe, not safe: at one
   substep the same bands embed bodies inside solid walls.
2. **Fix the trimesh contact path first** — an engine-level investigation
   (Avian's contact constraints vs hollow trimesh manifolds under substeps,
   or a different collider strategy for world statics). Larger scope, its own
   task; F23-D's high-speed envelope stays valid meanwhile.
3. **Adopt anyway** — not viable: it regresses three pinned F18-B acceptance
   tests and ships a world in which ordinary-speed bodies pass through solid
   mesh walls.

Related gap recorded by the task history (bunny-2): `world_app()` also does
not install `PhysicsBodiesPlugin`, so the spawn preflight and F23-B's contact
reports are not in the world composition at all — same "the world bootstrap
is not the physics composition" class of defect, same eventual owner.

## What this does not claim

Nothing here was measured against the original game. The substep count, the
solver behavior and every position above are properties of the pinned engine
under synthetic fixtures; the original's tick rate, collision strategy and
geometry classes remain unmeasured (F23-D limitation 4).

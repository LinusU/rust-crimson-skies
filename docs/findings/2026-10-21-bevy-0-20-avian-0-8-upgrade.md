# Bevy 0.20 / Avian 0.8 upgrade (task #1772)

The workspace moved from `bevy 0.19` / `avian3d 0.7` to `bevy 0.20.0` /
`avian3d 0.8.0` (`parry3d 0.31.1`, `wgpu 30`). Both releases are published on
crates.io and resolve into a single graph in `Cargo.lock`.

## API changes taken in production code

- `avian3d`: `SpeculativeMargin` was renamed `SpeculativeCcd`
  (`SpeculativeCcd::ZERO`/`MAX`/`new`, field `max_distance`). The `f32` feature
  no longer exists (f32 is the default), so it is dropped from the manifest.
- `bevy::math::primitives` moved to `bevy::shape` (`Plane3d`, `Sphere`).
- `RenderAdapterInfo` keeps its `AdapterInfo` private and exposes it by `Deref`.
- Observers: `On<Add, C>` is now `On<Add<C>>` (likewise `Insert`, `Remove`).

## Behaviour that changed: swept CCD against child-node colliders (#416, #420, #424)

Measured with `accept_t420_*` and `accept_t424_*` on the production
composition: on 0.7 `solve_swept_ccd` required `Collider` on the *body* entity,
so a 400 m/s `SweptCcd` probe tunnelled through a wall whose collider sat on a
child node. Avian 0.8's `solve_continuous` iterates `RigidBodyColliders`: the
same probe is now **stopped** by a child-node collider, for trimesh and cuboid,
at 1, 2, 4 and 8 substeps. The T420 re-measure signal fired; the pinning
assertions were inverted to assert the new stop (still strict: the probe must
end before the wall's far face region, `x < 0`).

Consequences left for follow-up (not changed here): the `SweptInvisible`
declaration and `undeclared_swept_invisible_bodies` audit of the collider-on-body
rule (#424) are now conservative rather than necessary, and the T420/T424
decision records describe 0.7. Blocked task #416 (speculative margin x trimesh
contact, substeps) was not re-measured beyond the existing `accept_t420_*`
speculative-margin test, which still passes unchanged.

## Behaviour that changed: sensors and swept CCD (F18-B, F23-B)

Avian 0.8's `solve_continuous` skips sensors (`proxy.is_sensor()`). On 0.7 a
sensor was clamped like a wall, which is how a 400 m/s `SweptCcd` body got a
`CollisionStart` for a 2 cm trigger (F23-B AC02) and what the F18-B "body on a
trigger volume holds a swept body" arm pinned. Now:

- `accept_f23_b_*` thin-trigger crossings produced no report at all. A new
  pass, `report_swept_sensor_crossings` (`physics/contacts.rs`), casts a swept
  body's collider along the tick's segment and reports each sensor it meets
  through the same classifier as an engine event, deduplicated against the
  engine's own start. It never touches the body (a sensor stays a reported
  overlap, not an obstacle). The world composition already had this design in
  `world/crossings.rs`.
- `accept_f18_b_a_body_bearing_trigger_volume_no_longer_holds_a_swept_body`
  is the 0.7 arm inverted: the flight is free and the discrete log is silent.

## Behaviour that changed: contact residual (T428)

Re-measured with the resting-rule tests, `Gravity::ZERO`, 250 kg box, 30 and
100 m/s into a 0.4 m wall:

- The contact no longer resolves in one tick: the residual settles after 2-9
  ticks (4/2/1 substeps), then is constant. It also carries lateral and
  angular components a head-on hit did not have on 0.7.
- A body held at zero is handed no solver bias (~1e-11 m/s), and a constant
  force produces the same `a·dt` every tick, so the "velocity changes" release
  signal never fired. A post-step speed above the still epsilon now counts as
  driving as well.
- A body struck at 100 m/s leaves the 3 cm contact band before four touching
  ticks pass. `RESTING_CONTACT_MEMORY_TICKS` (12) lets an unchanged velocity
  within that window of the last touch be retired as the contact residual.
- The residual no longer carries the body out of contact at 30 m/s, so the
  retired body is still held (`resting == 1`).

The retired `eprintln!("DEBUG ...")` lines in `retire_contact_residual` were
removed.

## Other

- The water patch AABB is padded 2 cm per axis on 0.8.
- `cs_xtask` pins (`INTENDED_BASELINE`) and their tests moved to 0.20 / 0.8.
- Not verified here: retail assets, GPU and the owner-command M01 smoke (no
  `CS_GAME_DIR`/`CS_CAPABILITIES` in this environment). The retail-dependent
  tests are `#[ignore]`d and untouched.

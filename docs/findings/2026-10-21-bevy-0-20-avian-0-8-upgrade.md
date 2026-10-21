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

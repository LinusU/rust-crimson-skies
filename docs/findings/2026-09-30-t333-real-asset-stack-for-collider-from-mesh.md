# T333: the asset stack Avian's mesh colliders need, and the silent one

Date: 2026-09-30. Task: #333 "Re-enable Avian collider-from-mesh with a real
asset stack" (follow-up of
`docs/findings/2026-09-23-avian-collider-from-mesh-needs-bevy-asset-stack.md`).
Stage: F00-A follow-up. Capabilities used: ordinary build/test only.
Machine: macOS (Darwin), aarch64-apple-darwin, rustc/cargo 1.98.1.

## What was done

`cs_app::asset_stack` now exists and the workspace `Cargo.toml` re-enables
Avian's default `collider-from-mesh` feature. The finding above was correct
about the cause and the workaround; this records the resolution and two things
it did not predict.

1. `PhysicsPlugins::default()` is not self-contained under the pinned feature
   set, so **every** headless world in the crate needed the stack, not only the
   ones that wanted a mesh collider. `synthetic::SyntheticSceneBuilder`,
   `physics::fixture::PhysicsFixtureBuilder` and `world::fixture::
   WorldFixtureBuilder` each spelled its own plugin tuple; they now all build
   through `asset_stack::headless_app()`, which is the single place the base
   set is written down. A fourth caller spelling the tuple by hand is how the
   feature would be broken again.
2. That fourth caller turned up immediately, and it is the reason the
   structural fix above matters more than the finding below predicted. F23-C
   merged into `main` on 2026-09-30 (`38ec594`, `PhysicsSession`) with its own
   `MinimalPlugins + TransformPlugin + PhysicsPlugins::default()` tuple. After
   this branch rebased onto it, all nine `accept_f23_c_*` tests failed with
   `Encountered a panic in system` on the first update. A new `cs_app` world
   that spells the tuple out is now a build-time-of-the-test failure, not a
   review finding. `PhysicsSession::app` was switched to `headless_app()` too.
3. Removing `WorldSerializationPlugin` from the stack does not panic. Avian's
   pinned feature set includes `bevy_scene`, which makes
   `init_collider_constructor_hierarchies` take
   `If<Res<WorldInstanceSpawner>>` so it can wait for a scene instance to
   spawn. `WorldInstanceSpawner` is created by
   `bevy::world_serialization::WorldSerializationPlugin`. Without that
   resource the whole system is skipped: a `ColliderConstructorHierarchy` over
   a `Mesh3d` produces no collider, no error and no warning. This was measured
   here, not read off the source — see below.

## The stack that works

`AssetStackPlugin` adds, in order:

| plugin | what it creates that Avian reads |
|---|---|
| `bevy::asset::AssetPlugin` (`watch_for_changes_override: Some(false)`) | `AssetServer` (needs the IO task pool, which `MinimalPlugins` installs) |
| `bevy::mesh::MeshPlugin` | `Assets<Mesh>` and `Messages<AssetEvent<Mesh>>`, via `init_asset::<Mesh>()` |
| `bevy::world_serialization::WorldSerializationPlugin` | `WorldInstanceSpawner`, without which `init_collider_constructor_hierarchies` is silently skipped |

The `watch` override is a property of this crate, not of the dependency: Bevy
0.19.1's `bevy_asset` has a `watch` feature that is *not* in the graph here
(`bevy_world_serialization`, `bevy_mesh` and `bevy_render` reach
`bevy_asset` only with its default features), so `cfg!(feature = "watch")` is
already `false`. Stating it means a future feature that turns it on cannot
silently add a filesystem watcher to a deterministic headless world.

Nothing here loads an asset. Canonical content is converted in-process by
`render::bevy_mesh::upload_group` and handed to
`asset_stack::spawn_static_mesh_collider` as a `Mesh`, so the F00 `SYNTHETIC`
scene keeps its "loads no assets" property even though it now has an asset
stack. `SyntheticScene::mesh_asset_count()` is the check, and it is asserted
both before and after 60 ticks.

## The derived collider

`spawn_static_mesh_collider` uploads the mesh, spawns a `RigidBody::Static`
root carrying `ColliderConstructorHierarchy::new(TrimeshFromMesh)` plus the
caller's `Transform`/`Position`/`Rotation`, and spawns a child holding
`Mesh3d(handle)`. Measured through `accept_t333_` on a closed unit box built
through the production path (`RawMesh` → `RenderMesh::build` →
`upload_group` → asset stack):

| quantity | value |
|---|---|
| stored triangles (6 quads) | 12 |
| triangles in the derived `TriMesh` | 12 |
| distinct positions handed to the stack | 8 |
| vertices in the derived `TriMesh` | 8 |
| every derived vertex bit-equal to an uploaded position | yes |
| `RigidBodyColliders` on the body | exactly the one derived collider |

`TrimeshFromMesh` is `SharedShape::trimesh_with_flags(_, _,
TrimeshFlags::MERGE_DUPLICATE_VERTICES)`, which merges coincident positions
and reindexes; it deletes nothing. That is why a box with one face missing
keeps 10 triangles where a convex hull of the same eight corners would have 12
— the acceptance test `accept_t333_a_mesh_with_an_opening_keeps_it_in_the_
derived_collider` pins that, because F18 non-negotiable behavior 1 forbids
closing a traversable opening through simplification. Which simplification an
*original* mesh may get is still F18-B's decision, and nothing in
`asset_stack` pre-empts it.

## Sensitivity

Measured on this branch by mutating the production code and re-running
`cargo test --locked -p cs_app --test accept_t333_mesh_colliders`:

| mutation | result |
|---|---|
| `spawn_static_mesh_collider` also inserts a hard-coded `Collider::cuboid(1,1,1)` on the node | 2 of 4 fail: `as_trimesh()` is `None` |
| `headless_app()` stops adding `AssetStackPlugin` | 4 of 4 fail: first `update` aborts with "Requested resource ... does not exist" (the F00-A symptom, reproduced) |
| rebasing onto `main` (which had gained `PhysicsSession`'s own plugin tuple) without switching it to `headless_app()` | 9 of 33 `accept_f23_c_*` tests fail with `Encountered a panic in system` on the first update |
| feature list drops `collider-from-mesh` | the test file does not compile: `ColliderCachePlugin` is `#[cfg]`-gated on the feature, so the coupling is checked by the compiler |

## What this does not establish

* No original-data claim. Everything above runs on a synthetic box; nothing
  here says what an original world mesh's collision should look like.
* `SkipReason::MeshColliderDeferred` in `world::spawn.rs` is unchanged, and
  F18-B still owns world import, the per-role collision mapping and the
  simplification policy. This task supplies the mechanism
  (`ColliderConstructorHierarchy` over a `Mesh3d` in a real asset stack) and
  nothing else.
* The image half of the canonical-to-Bevy adapter is still not an asset:
  `bevy_image::ImagePlugin` does not call `init_asset::<Image>()`, and
  `bevy_render` (which does) is not in a headless world. Nothing in this task
  needed it, and adding it would be a different change.

## Commands run

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t333_ --include-ignored
cargo test --workspace --locked -- accept_f00_a_ --include-ignored
```

`Cargo.lock` is unchanged: `collider-from-mesh` forwards to
`bevy/bevy_mesh` and `bevy/bevy_mikktspace`, and both are already in the graph
through Bevy's own `3d`/`3d_api` features, as the F00-A finding predicted.

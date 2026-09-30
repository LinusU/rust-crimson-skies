# Avian `collider-from-mesh` requires Bevy's asset stack

* Date: 2026-09-23
* Feature/stage: F00-A (`specs/F00-workspace-toolchain-and-first-executable.md`)
* Status: resolved 2026-09-30 by task #333 — see
  `docs/findings/2026-09-30-t333-real-asset-stack-for-collider-from-mesh.md`.
  The decision below still describes what F00-A did; the feature is enabled
  again now that `cs_app::asset_stack` supplies the stack.

## Observation

With `avian3d = "0.7"` default features and a headless app built from
`MinimalPlugins + TransformPlugin + PhysicsPlugins::default()`, the first
`App::update()` aborts before any physics step. Two systems from the default
`collider-from-mesh` + `default-collider` combination are the cause:

1. `avian3d::collision::collider::cache::clear_unused_colliders` takes
   `MessageReader<AssetEvent<Mesh>>`. Bevy registers `Messages<AssetEvent<Mesh>>`
   only from `AssetApp::init_asset::<Mesh>()`, which runs inside the asset/render
   stack. Failure: `Parameter ... failed validation: Message not initialized`.
2. `avian3d::collision::collider::backend::init_collider_constructor_hierarchies`
   takes `#[cfg(feature = "collider-from-mesh")] meshes: Res<Assets<Mesh>>`.
   Failure: `Parameter ... failed validation: Resource does not exist`.

Both parameter groups are gated on `#[cfg(feature = "collider-from-mesh")]` in
`avian3d-0.7.0/src/collision/collider/backend.rs` (lines ~269, ~328), and
`ColliderCachePlugin` is added to `PhysicsPlugins` under
`#[cfg(all(feature = "collider-from-mesh", feature = "default-collider"))]`
(`avian3d-0.7.0/src/lib.rs` ~line 767).

Evidence: local runs of
`cargo test --locked -p cs_app --test accept_f00_a_synthetic_body` with
`RUST_BACKTRACE=1`; the `FunctionSystem<...>` frames name the two systems above.

## Decision for F00-A

`Cargo.toml` sets `default-features = false` on `avian3d` with the default
feature list reproduced verbatim minus `collider-from-mesh`. Rationale: the
F00-A synthetic scene must be asset-free (F00 non-negotiable behavior 2), so it
adds no `AssetPlugin`. Inserting a bare `Assets<Mesh>` / `AssetEvent<Mesh>`
resource would satisfy the readers without an asset system actually existing —
that hides the coupling instead of expressing it.

## Follow-up (implemented 2026-09-30, task #333)

Mesh-derived colliders (`ColliderConstructor`, `ColliderConstructorHierarchy`
from `Mesh3d`) stay unavailable until a task adds Bevy's asset stack to `cs_app`
and re-enables `collider-from-mesh` together with it. Relevant when original
game meshes are turned into Bevy `Mesh` values in the rendering tasks; the
retail content pipeline itself parses original formats in `cs_formats` and does
not depend on this feature.

Task #333 did exactly that: `cs_app::asset_stack` installs `AssetPlugin`,
`MeshPlugin` and `WorldSerializationPlugin`, and every headless world in the
crate is built through `asset_stack::headless_app()`. The prediction below
about `Cargo.lock` held. The one thing this file did not predict is that
`WorldSerializationPlugin` is needed too — without it
`init_collider_constructor_hierarchies` is skipped silently and no collider
appears. See
`docs/findings/2026-09-30-t333-real-asset-stack-for-collider-from-mesh.md`.

## Impact

* No behavioural loss for F00-A: the acceptance scenario uses a cuboid
  `Collider`, which comes from `default-collider`/`parry-f32` and stays enabled.
* `Cargo.lock` is unchanged by this edit (both `bevy_mesh` and
  `bevy_mikktspace`, which `collider-from-mesh` forwards to, remain in the graph
  through Bevy's own default `3d`/`3d_api` features).

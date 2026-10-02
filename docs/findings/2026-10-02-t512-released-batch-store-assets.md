# Task #512: a released batch returns its mesh and material to the asset stores

Date: 2026-10-02. Task #512, key `F17-C-release-returns-store-assets`,
"Return a released batch's mesh and material to the asset stores", following
#503 (`F20-C-read-the-composed-animation-visibility-verdict`). Branch:
`rally/512-return-a-released-batch-s-mesh-and-mater`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test only
— `CS_GAME_DIR` was **not** read for this change, so nothing here is
`verified_original` and no evidence report is required.

## What was actually wrong

`release_entity` in `crates/cs_app/src/render/sync.rs` despawned a batch entity
and its placements and stopped there. Two store entries the same module had added
for that batch outlived it:

* the `Assets<Mesh>` entry `sync_frame` created with
  `resource_mut::<Assets<Mesh>>().add(upload.geometry().mesh().clone())` at spawn;
* the material entry `add_material` created — `Assets<StandardMaterial>` or
  `Assets<AdditiveMaterial>`, whichever the batch's own class calls for.

The reviewer's probe on #503 measured exactly one of each per release/respawn
cycle, monotone, and the module's own comment above `add_material` already named
the class of problem ("adding it again would leave an orphan in the store on
every frame of a stable frame, which is a leak no test that only counts batches
would see") while guarding only the *reuse* side of it. Before #503 a release
needed the frame to stop naming the batch outright — a scene reload, an aircraft
leaving, a paint change — which is rare. #503's draw consumer releases a batch
whenever the composed visibility verdict withholds **all** of its rows and spawns
it again when they come back, so the leak is now on a gameplay-frequency path.

There was a **second** release path with the same defect, which the task text
did not name: `reuse_batch` releases an entity that no longer carries the draw's
components (a batch entity damaged behind the sync path's back) and spawns a
fresh one. That path orphaned one mesh and one material per repair as well. It is
now covered.

## The ownership rule

> A batch entity **owns** the two store entries its spawn added. Its
> per-instance entities **borrow** those two handles — they are the same
> `Mesh3d` and `MeshMaterial3d` handles — and the batch's despawn is recursive,
> so every borrow ends with the owner. A release despawns the entity and its
> placements first and only then hands an owned entry back to its store, and hands
> it back only when no live batch still owns it.

Three pieces of production code carry it, all in `sync.rs`:

* **`BatchAssets`** (private component) — the mesh id and the material id, recorded
  **on the entity that draws them** by the spawn that added them. Recorded on the
  entity rather than beside it because the release path reads it *there*, before
  the despawn that takes it away: an entry is looked up by the id its owner
  recorded, so a released batch hands back exactly the two entries it added and
  cannot hand the same entry back twice.
* **`BatchAssetRefs`** (private resource) — a live-owner count per `AssetId`, for
  meshes and for both material stores. A release drops one owner of each entry and
  removes an entry only when the count reaches zero. This is what makes "a
  still-referenced handle is not removed" a decision the code makes instead of an
  invariant it documents in a comment.
* **`release_entity`** — the one place that does it, reached from all three call
  sites: the stale-batch loop at the end of `sync_frame`, the repair inside
  `reuse_batch`, and `teardown`. It returns `ReclaimedAssets`, which
  `FrameSync::reclaimed` and `RenderTeardown::reclaimed` report, so "an entry was
  handed back once" is a reported fact rather than something a caller has to infer
  from a length.

The order inside `release_entity` is the part that is easy to get wrong: **the
store removal happens after the recursive despawn**, never before. Removing first
would leave the placements holding a handle that no longer resolves — live
entities drawing an asset that is gone, which is a worse failure than the leak it
replaces.

The mesh and the material are paired rather than recorded field by field
(`added_mesh.zip(added_material)`): a batch that owned half of what it added would
hand back one entry and leak the other, so an owner record exists only when both
entries exist.

Nothing about what is drawn or which rows are placed changed. The batch key, the
material selection, the reuse rule and the placement reconciliation are untouched.

## The #503 path is this path

The visibility consumer (rule 6 in the module docs, merged on `main` after this
branch was cut) does not release a batch itself. When the composed verdict
withholds every row of a batch it `continue`s past that batch, which leaves its
key in `previous`, and the tail loop at the end of `sync_frame` releases it —
the same loop a frame that no longer names the batch releases through. So the
gameplay-frequency release the reviewer's probe reached is the tail loop this
change fixes, not a fourth call site: `release_entity` still has exactly three
callers, and the tests drive two of them by name and the third one (the tail
loop) through the `accept_t512_` cycles.

## How "a still-referenced handle survives" is established

Two halves, because the module can produce the case in one form only, and the
other form is the one a future change would need.

**The form the module produces: the placements.** A batch of four rows owns one
mesh and one material; its four placed draws hold *those very handles*. That is a
live entity naming a store entry, four times over, and the entry has to be in its
store for as long as they exist. `accept_t512_a_released_batch_hands_its_mesh_and_material_back_to_the_stores`
asserts that directly while the batch is live — every placement's `Mesh3d` and
`MeshMaterial3d<StandardMaterial>` equal the owner's, and both resolve in their
stores — and then checks, after **every** step of **every** cycle, an invariant
over the whole world: no entity anywhere holds a mesh or a material handle that
does not resolve (`every_handle_resolves`). A release that removed an entry
before despawning would fail that check on the step where the placements were
still alive, not only on the step after.

**The form a spawn cannot produce: two batches naming one entry.** A spawn adds a
fresh mesh and a fresh material for every batch, so the owner count of every live
entry is 1 and the "not the last owner" branch is unreachable through the public
path today. It is still checked, on the counter the release path itself consults:
`accept_t512_a_store_entry_two_live_batches_name_is_handed_back_once` (inline, in
`sync.rs`) drives `BatchAssetRefs` with two owners of one id and requires the
first drop to report *still owned* and the second to report free, plus that the
two material stores are counted apart. The count is per id rather than per batch
for exactly this reason: if a future change hands one entry to two batches — the
mesh path sharing by geometry fingerprint, the way the image path already shares
by paint fingerprint — the first release to end must not pull the entry out from
under the other, and that will be a decision rather than a rediscovery.

## What the tests prove, and how they fail without the change

`crates/cs_app/tests/render/release_assets.rs` holds the `accept_t512_`
selection, with a fixture built through the production readers: the canonical mesh
through `cs_content`, the paint image through `cs_formats::read_bm`, the paint
itself through the F09-C `LiveryRuntime`, and every frame driven through the
production `sync_frame`. Four tests:

1. **three release/respawn cycles** on one batch of four rows — the reviewer's
   shape. Each cycle syncs a frame that withholds every row (every surface
   refused) and then the drawn frame again, and requires the mesh and material
   counts to be 1 before, 0 after the release and 1 after the respawn, with
   `reclaimed` reporting exactly `meshes: 1, materials: 1` per release, four placed
   draws before and none after, and a `teardown` that hands back the last entry
   while a second `teardown` hands back nothing.
2. **the additive class's own store**, since `Assets<AdditiveMaterial>` is a
   separate store with its own branch in the release and its own report field.
3. **the repair path** (`reuse_batch` releasing a batch entity whose `BatchDraw`
   was removed behind the sync path's back), which is the second release site and
   the one the task text did not name.
4. **a refused frame hands nothing back** — `NoSession` and `ProfileMismatch`
   leave both stores exactly as they were, because a refusal that returned an
   entry would leave a live batch drawing a removed asset.

Verified sensitivity by reverting only the store removal in
`reclaim_store_entries` (keeping the counter, the record and the report) and
re-running: with the report assertions in place the tests fail on
`reclaimed`, and — the check that matters — with **every** `reclaimed` assertion
neutralised so only the observable store lengths remain, they still fail:

| assertion | expected | with the removal reverted |
| --- | --- | --- |
| mesh count after the first release | 0 | 1 |
| mesh count after the first repair-and-respawn | 1 | 2 |

which is the reviewer's monotone growth, reproduced by the same numbers they
measured.

The reuse guarantee is asserted in the same test and is unchanged: three resyncs
of a frame that reuses every batch report `spawned: 0`, `reused: 1` and grow no
store.

## Not closed here, and why

**The `Assets<Image>` store leaks the same way.** `sync_frame` adds one image per
spawn — the paint upload dedups by fingerprint *within one call* only, and the
canonical path dedups not at all — and #512 deliberately leaves it alone: an image
handle is not owned by one batch in the way a mesh or a material is. It is shared
between batches with the same fingerprint, and it is referenced from *inside* the
material entry (`base_color_texture`), so handing it back needs the material
removed first and an owner count over both kinds of reference. That is a different
piece of design, filed as task #514
(`F17-C-followup-released-image-store`) rather than folded in here. The image
counts are deliberately not asserted in this task's tests; the fixture uses no
image at all so the mesh and material stores are the only thing under test.

**An owner dropped without a release leaks.** `teardown` releases through
`BatchEntities`, and nothing in the workspace can drop that resource while leaving
entities alive. If it ever did, the owner's entries would stay in their stores and
the owner counts in `BatchAssetRefs` would keep them; `teardown` deliberately does
not reset the counter to hide that, because a leak to report is better than a count
reset behind.

**No original claim.** The ownership rule is an engine fact about Bevy's
immediate-mode `Assets`, not something the 2000 original renderer was measured
doing. The rule follows spec F17 non-negotiable 4 ("instancing and batching retain
per-instance livery and damage state") only in the sense that per-instance state
must not be paid for with a store that grows per release — nothing here says what
the original did with mesh or material lifetimes, and it does not need to.
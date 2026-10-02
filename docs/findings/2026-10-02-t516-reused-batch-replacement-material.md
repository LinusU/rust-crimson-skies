# Task #516: the store entry a reused batch adds when its material component is gone

Date: 2026-10-02. Task #516, key
`F17-C-followup-reused-material-ownership`, "Own the store entry a reused batch
adds when its material component is gone", the gap #512's review left open.
Depends on #512 (`F17-C-release-returns-store-assets`), which introduced the
ownership rule this extends. Branch:
`rally/516-own-the-store-entry-a-reused-batch-adds`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test
only — `CS_GAME_DIR` was **not** read and no original executable ran, so nothing
here is `verified_original` and no evidence report is required.

## The gap, exactly

#512 records the two store entries a spawn adds on the entity that draws them
(`BatchAssets`), and only when the call added **both**:

```rust
if let Some(owned) = added_mesh
    .zip(added_material)
    .map(|(mesh, material)| BatchAssets { mesh, material })
```

`added_mesh` is `Some` only on the spawn branch, so the record is written only
for a freshly spawned batch. There was one path where the module added a store
entry that could never be handed back: a **reused** batch entity whose material
component went missing.

`reuse_batch` accepted an entity carrying `BatchDraw` + `Mesh3d` and did not
look at `MeshMaterial3d<StandardMaterial>` / `MeshMaterial3d<AdditiveMaterial>`.
So when a live batch entity lost its material component behind the sync path's
back:

1. `reuse_batch` returned the entity (`added_mesh = None`),
2. `stored_material` returned `None`,
3. `add_material` added a **new** material entry,
4. `zip` yielded `None`, so no owner record was written and no owner count was
   registered,
5. the new entry was installed on the entity and every placement, while the
   entity's existing `BatchAssets` still named the **previous** material.

The entry just added leaked for the session, and at release time
`reclaimed.materials` reported `1` for an entry nothing drew with any more while
the entry the batch actually drew with stayed in the store. The rule #512 states
in its own module docs was not true on that path.

Reachability is **not** by a class change: `batch_key` digests `key.state()`,
which digests the class, and `frame_fingerprint` in `batch.rs` documents that
`material_kind` is deliberately not digested because the state covers it. Under
a stable key a reused entity always holds the matching kind's component. Nothing
in the workspace removes that component today, so the only way in is a test that
removes it, the way
`accept_t512_the_repair_of_a_damaged_batch_entity_hands_back_what_the_dead_one_owned`
removes `BatchDraw`. Before #512 this path leaked the same entry *and* the
previous one, so it is an unfinished half of the rule, not a regression.

## The rule chosen, and why

The task offered three options; this change takes the third.

> A batch entity that no longer carries the material component of its own kind
> is **not usable** as this draw. `reuse_batch` releases it through the one
> `release_entity` path and the frame spawns a fresh entity, exactly as it
> already did for an entity that lost its `BatchDraw`.

It is the smallest change that makes the #512 rule structural rather than
conditional, and it reuses machinery that is already correct:

* **It keeps the pairing invariant by construction.** A `BatchAssets` record is
  written only by a spawn that added both entries and immediately draws with
  them, so a half-owner cannot exist and a live record always names the entries
  its entity draws with. Option 1 (separate `Option<OwnedMesh>` /
  `Option<OwnedMaterial>`) would allow a half-owner and still needs the old
  entry's owner dropped on a replacement, so it is more state for the same
  result.
* **It reuses the release path #512 made correct.** Option 2 (drop and rewrite
  the record's material field in place) adds a second kind of ownership
  transition — a replacement inside a reused entity — and with it the ordering
  question of when the old entry leaves the store relative to the placements
  that still hold its handle. The release-and-respawn path already has the order
  right: read the record, despawn the batch and its placements, then remove an
  unowned entry.
* **It matches the module's own repair rule.** `reuse_batch` already documents
  that "one that no longer carries the draw's components is not it: it is
  released and a fresh entity is spawned rather than repaired in place". The
  material *is* one of the draw's components; the missing-material case is the
  same damage as the missing-`BatchDraw` case, so it takes the same path.

The entry added by this module is handed back exactly once by the release path,
and a live owner record never names an entry the entity does not draw with. The
reuse guarantee is unchanged: an entity that still carries its material
component — every entity the workspace produces — is reused exactly as before.
Nothing about what is drawn, which rows are placed, or the batch key changed.

## What changed

All in `crates/cs_app/src/render/sync.rs`:

* `reuse_batch` takes the batch's `MaterialKind` and requires
  `stored_material(world, entity, kind).is_some()` in addition to `BatchDraw` +
  `Mesh3d`. The doc comment states why the material is part of "the draw's
  components".
* The module-doc rule 7 gains a sentence: an entity without the material
  component of its own kind is released and respawned, so a replacement entry is
  always added by a spawn that records it.
* The one call site passes `batch.material_kind()`.

No public type, `BatchAssets`, `BatchAssetRefs`, `release_entity`,
`reclaim_store_entries`, `FrameSync` or `SyncError` changed.

## How the tests prove it, and how they fail without the change

`crates/cs_app/tests/render/release_assets.rs` gains the
`accept_f17_c_reused_` selection, reusing the file's production-reader fixture
(the canonical quad mesh, the F09-C livery paint and every frame driven through
the production `sync_frame`). It follows the file's existing shape:

1. **`accept_f17_c_reused_a_damaged_material_component_is_released_not_orphaned`**
   — the fleet scene (one batch of four rows, a `StandardMaterial`). Each cycle
   removes `MeshMaterial3d<StandardMaterial>` from the live batch entity and
   resyncs; the report must be `spawned: 1, reused: 0, released: 1`, `reclaimed`
   exactly `{ meshes: 1, materials: 1, additive_materials: 0 }`, and both stores
   must stay at one entry. `every_handle_resolves` runs after every step. The
   replacement is asserted to be a really new entry, and a final `teardown`
   reports exactly what the last spawn owned and leaves both stores empty —
   which is only true if the record names the entry the live entity draws with.
2. **`accept_f17_c_reused_the_additive_classes_damaged_material_is_released_not_orphaned`**
   — the same cycle for `AdditiveMaterial`, whose entry lives in its own store,
   so the additive branch of `reuse_batch`'s check is exercised too.

Verified sensitivity by reverting only the new
`stored_material(world, entity, kind).is_some()` clause, keeping everything else
in place:

* both tests fail on the first cycle (`spawned` is 0 where 1 is required, and
  `reused` is 1 where 0 is required) — the pre-fix reuse;
* with the report assertions neutralised so only the observable store lengths
  remain, the standard test still fails on **the store count alone**:

  | assertion | expected | with the check reverted |
  | --- | --- | --- |
  | `standard_materials` after the first damaged frame | 1 | 2 |

which is the per-frame material growth the task describes. The store counts, not
the reported counters, carry the selection.

## Not closed here, and why

**The `Assets<Image>` store still leaks the same way**, and was already filed as
task #514 (`F17-C-followup-released-image-store`) by #512. An image handle is
not owned by one batch the way a mesh or a material is: it is shared by
fingerprint and referenced from inside the material entry, so handing it back
needs the material removed first and an owner count over both kinds of
reference. The fixture here uses no image, so it is deliberately out of scope.

**An entity whose `BatchAssets` record is itself gone leaks**, the same way it
does for the #512 paths: if nothing names the entry, no release can remove it.
That is the documented stance (`teardown` deliberately does not reset
`BatchAssetRefs`), not a second defect.

**No original claim.** The ownership rule is an engine fact about Bevy's
immediate-mode `Assets`, not something the 2000 original renderer was measured
doing. This change follows spec F17 non-negotiable 4 ("instancing and batching
retain per-instance livery and damage state") only in the sense that per-instance
state must not be paid for with a store that grows per release. Nothing here
says what the original did with mesh or material lifetimes.

## Sources used

* `specs/F17-rendering-material-fidelity-and-scalable-presentation.md`, stage
  `### F17-C`.
* `docs/contracts/IDENTITY-CONTENT.md`.
* `docs/findings/2026-10-02-t512-released-batch-store-assets.md` (the rule this
  extends and the #516 gap it names).
* `crates/cs_app/src/render/sync.rs`, `batch.rs`, `bevy_state.rs`.

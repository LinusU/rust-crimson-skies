# Task #514: a released batch returns its bound image to the image store

Date: 2026-10-07. Task #514, key `F17-C-followup-released-image-store`,
"Return a released batch's bound image to the image store", the follow-up #512
filed against itself (`docs/findings/2026-10-02-t512-released-batch-store-assets.md`,
section "Not closed here"). Branch:
`rally/514-return-a-released-batch-s-bound-image-to`. Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary build/test only
— `CS_GAME_DIR` was **not** read for this change, so nothing here is
`verified_original` and no evidence report is required.

## What was actually wrong

`sync_frame` adds an image per spawn and `release_entity` handed back the mesh
and the material and stopped there, so the `Assets<Image>` entry a spawn added
outlived the batch. Two shapes of it, both on the production `sync_frame`:

* a **paint** image: `paint_handles` deduplicates by upload fingerprint *within
  one call* only, so a batch released and respawned once per cycle adds one
  image per cycle;
* a **canonical** image: `Assets<Image>.add(source.image().clone())` per spawn,
  with no deduplication at all, so two batches of the same surface texture get
  two entries on their first frame — each of them owned, each of them leaked by
  the same release.

The F20-C draw consumer (#503) releases a batch whenever the composed visibility
verdict withholds **all** of its rows, so this is reachable on a gameplay path,
exactly like the mesh and material half #512 fixed.

## Why it was not the same change as #512

An image handle is not owned by one batch in the way a mesh or a material is,
and both differences are structural rather than cosmetic:

* it is **shared** — `paint_handles` hands one entry to every spawn with the
  same fingerprint in a frame, so two live batches can name one `AssetId<Image>`;
* it is referenced from **inside another store entry** — the batch's material
  holds it as `base_color_texture`, so the image outlives the batch's entity
  until that material entry is removed too, and Bevy's `Assets::remove` does not
  cascade.

So returning an image needs the material entry to go first and an owner count
over *both* kinds of reference. #512's `BatchAssetRefs` counted meshes and
materials only; the limitation and this task are recorded in its finding.

## The rule, as implemented

> A spawn that binds an image records it with the batch that added it. A release
> hands it back only when **no live batch and no live material entry names it** —
> the material of the released batch having already left the store in the same
> call.

Three pieces of production code carry it, all in
`crates/cs_app/src/render/sync.rs`:

* **`BatchAssets`** gained `image: Option<AssetId<Image>>` — recorded on the
  entity that draws it, by the spawn that added it, `None` for a surface that
  samples no image. It rides in the same record as the mesh and the material
  (still written only when both of those were added) because it is the same
  "looked up by the id its owner recorded" rule; what it gives the release path
  is *one owner's name* to drop plus the id to ask the material stores about.
* **`BatchAssetRefs`** gained `images: BTreeMap<AssetId<Image>, usize>`, a live
  owner count per image id, with `own` registering it and `drop_image` dropping
  one owner. Unlike meshes and materials the two-owner branch **is** reachable
  today, through `paint_handles`.
* **`reclaim_store_entries`** drops the image owner alongside the other two,
  removes the mesh, then the material, and only **then** — with the released
  batch's own material already out of the store — asks the new
  `material_binds_image` whether either material store still samples the image.
  Only when the count reached zero *and* nothing samples it does
  `Assets<Image>::remove` run, and `ReclaimedAssets` gained an `images` field so
  `FrameSync::reclaimed` and `RenderTeardown::reclaimed` report it.

The order inside the call is the part that is easy to get wrong: **the image is
removed last**, after this batch's material. Asking while the material is still
there would keep every image forever (every batch's material samples its own
image), and removing the image before the material would leave that material
sampling a handle that no longer resolves — a live entity drawing an asset that
is gone, which is a worse failure than the leak it replaces.

Nothing about what is drawn or which rows are placed changed: the batch key, the
paint binding, the reuse rule, the material selection and the placement
reconciliation are untouched. The only new behavior is in the release path.

## The two cases the tests need, and why they are two

**`accept_t514_a_released_painted_batch_returns_its_image_to_the_store`**
(`crates/cs_app/tests/render/released_image.rs`) drives the production
`sync_frame` over a fixture of four painted aircraft that batch into one draw:
one spawn binds one texture, a frame that reuses every batch three times adds
nothing, and **three release/respawn cycles** end each release with
`Assets<Image>` at 0 and each respawn at 1 — not one per cycle — with
`reclaimed.images == 1` per release and per teardown, and a second teardown
handing back nothing. The fixture is built through the production readers
(`cs_content` mesh, `cs_formats` image and `.bm`, F09-C `LiveryRuntime` paint),
and `every_handle_resolves` is extended beyond meshes and materials to the new
invariant: **no entity anywhere holds a material or a `BatchDraw` whose texture
does not resolve in `Assets<Image>`**, checked after every step of every cycle.

**`accept_t514_an_image_shared_with_another_live_batch_stays_in_the_store`** is
the other half: one aircraft with a masked wing and an opaque tail — two phases,
so two batches — carrying one committed paint under one addressing, so both bind
the one texture. Releasing only the wing (its surfaces refused, the tail's
uploaded) reports `reclaimed.images == 0` and leaves the texture in the store for
the tail; the next cycles show the count coming back down instead of climbing,
and the final release hands the last texture back once. This case cannot be
reached with a mesh or a material — a spawn adds a fresh one of those per batch —
which is precisely why it needs its own test.

**`accept_t514_an_image_a_live_material_entry_still_samples_stays_in_its_store`**
(in `sync.rs`, beside the `accept_t512_` counter test) covers the reference the
owner count cannot see: a texture with an empty owner count whose entry is still
sampled by a material another batch drew. That shape is not reachable through
`sync_frame` today — every spawn that binds an image records an owner for it, so
the count and the store agree — so it is checked on `reclaim_store_entries`
itself, with a material entry this module never added: the release reports
`images: 0` and the store keeps the texture, and once that material is gone the
same record's second pass reports `images: 1` and the store gives it back.

## Measured sensitivity

Every mutation was applied to `reclaim_store_entries`, run, and reverted; each
row is the failure the run produced.

| mutation | what it simulates | who fails, and on what |
| --- | --- | --- |
| image removal disabled | **today's `release_entity`** | all three fail; the two integration tests fail on the **store length**: `cycle 1: its texture went back with it` (1 where 0 required) and `the store is back to the tail's own texture` (2 where 1 required) |
| removal without the material scan | owner count only | the `sync.rs` test fails on the store length: `the image a live material samples is still in its store` (0 where 1 required) |
| removal unconditional | no owner count, no material check | the shared-image test fails on the store length: `releasing one of two batches keeps the shared image in its store` (0 where 1 required) |

The observable store lengths are asserted **before** the `reclaimed` report in
every case, so a report that lies while the leak stays cannot be what carries
the selection — the same lesson the #512 review recorded.

## Not closed here, and why

**A respawn in a *later* frame does not reuse an identical texture that is
already in the store.** `paint_handles` is per call, so the shared case looks
like this: two batches bind texture `T`; one is released while the other holds
it (`T` correctly stays), and the respawned one adds `T'` with the same content.
The store then holds two identical textures until that batch's next release, and
`accept_t514_an_image_shared_with_another_live_batch_stays_in_the_store` pins
that at `2` with a comment. It is **not a leak** — both entries are owned and
both come back, and the count returns to its floor after every release, which is
what the test asserts — but it is a bounded duplicate, and deduplicating a
spawn's upload against the images already in the store would remove it. Filed as
follow-up task #728 (`F17-C-followup-respawn-image-dedup`) rather than folded in
here: it is a
different mechanism (a content lookup over the store, not an owner rule) and the
task's acceptance criteria are about growth per cycle, which is now flat.

**The canonical path still adds one entry per batch per frame.** Same bounded
shape, same reason: what #514 closes is the *growth per release*, and a
cross-spawn dedup of canonical uploads (they are identical only when the
surface, the addressing and the bytes are identical, which the batch key already
digestests) is that follow-up's business.

**An owner dropped without a release leaks an image too** — unchanged from
#512's note: `teardown` releases through `BatchEntities`, and the owner counts in
`BatchAssetRefs` are deliberately not reset behind a dropped record, because a
leak to report is better than a count reset.

**No original claim.** The rule is an engine fact about Bevy's immediate-mode
`Assets` and its non-cascading `remove`, not something the 2000 original renderer
was measured doing. It follows spec F17 non-negotiable 4 ("instancing and
batching retain per-instance livery and damage state") only in the sense that
per-instance state must not be paid for with a store that grows per release.

## Checks

Run on this branch, all exit 0:

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_t514_ --include-ignored   # 3 tests, all passed
```

`accept_t512_` (4 integration tests plus the inline counter test) and
`accept_f17_c_reused_` still pass: `release_assets.rs` only gained
`images: 0` in its `ReclaimedAssets` literals, because its fixture binds no
image.

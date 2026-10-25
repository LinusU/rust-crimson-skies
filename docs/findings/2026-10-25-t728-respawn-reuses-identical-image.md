# Task #728: a respawned batch reuses an identical texture already in the image store

Date: 2026-10-25. Task #728, key `F17-C-followup-respawn-image-dedup`, the
follow-up #514 filed (`docs/findings/2026-10-07-t514-released-batch-image-store.md`,
"Not closed here"). Ordinary build/test only; `CS_GAME_DIR` not read, nothing
here is `verified_original`.

## Change

`sync_frame` used to deduplicate paint uploads per call (`paint_handles`) and
canonical uploads not at all, so a batch respawned while another live batch held
the same texture added a second identical `Assets<Image>` entry. Now a spawn
looks the content up before adding (`stored_image` in
`crates/cs_app/src/render/sync.rs`):

* key: the upload's own fingerprint, tagged paint/canonical (`ImageContent`) so
  the two digests never stand in for each other;
* the map (`ImageContents` resource) is only a lookup; ownership is unchanged —
  every spawn that binds the handle still records it in `BatchAssets` /
  `BatchAssetRefs`, so the first release leaves it for the other live batch;
* a hit is used only if the handle still resolves in the store, and the entry is
  dropped from the map when `reclaim_store_entries` removes the image.

This also dedups canonical uploads across batches and frames (the second limitation
#514 listed). Batch keys, material selection, reuse and placement are untouched.

## Tests

* New `accept_t728_a_respawned_batch_reuses_the_identical_texture_in_the_store`
  (`crates/cs_app/tests/render/released_image.rs`): four release/respawn cycles
  of a two-batch aircraft keep `Assets<Image>` at 1, the wing binds the tail's
  handle, the last release hands it back once, a later spawn adds a fresh one.
* `accept_t514_an_image_shared_with_another_live_batch_stays_in_the_store`
  deliberately changed its `2` pins (after the respawn, and after the second wing
  release, where the reclaim is now `images: 0` because the tail still owns the
  texture) to the deduplicated values.

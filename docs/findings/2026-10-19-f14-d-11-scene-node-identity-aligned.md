# F14-D.11 (#560): the catalog's `scene_node` rows take the scene module's ids

Owner ruling, 2026-10-11 (task #560, option 5): align the catalog's `scene_node`
rows with the derivation `cs_content::scene` publishes. This note records what
changed, what was re-measured on the installation, and what the ruling costs.

## What changed

* `cs_content::scene::derive_node_identities` (crate-private) is the identity
  half of `SceneGraph::build`, extracted unchanged: hierarchy validation, the
  escaped name path (`escape_scene_node_name`), the sibling rule (`-i<hex>` of
  the stored `node_index` word on **every** member of a same-named set) and the
  digest rule (`-h<32 hex>` past `MAX_CONTENT_KEY_LEN`). `SceneGraph::build`
  calls it and keeps the `DuplicateNodeId` check.
* `catalog::baseline::node_identities` calls the same function over the reconciled
  child lists (`world_hierarchy_from_gamez`, the reconciliation
  `world_scene_graph_from_gamez` applies) of every container. The raw joined path
  `scene_node_key`, `scene_node_id` and the `NodeIdentity` classification
  (path / ambiguous / unspellable / unterminated) are gone.
* A node the derivation refuses keeps its row under `<container>.record<offset>`
  and carries one `UnsupportedReason::Unknown` under claim
  `f14.d.4.baseline.node_identity_refused` quoting the refusal. A hierarchy the
  scene module refuses as a whole (a cycle, a dangling or contradictory link)
  refuses every node of that container; a derived id two nodes share
  (`DuplicateNodeId`) refuses exactly those nodes.
* `GeometryContainerReport` drops `ambiguous`, `unspellable` and `unterminated`
  and gains `disambiguated`, `digested` and `refused`; the collection gaps
  `ambiguous_name_path`, `unspellable_name_path` and `unterminated_parent_chain`
  become `refused_node_identity`. The baseline report JSON follows.

## Re-measured on the installation (retail, read access only)

`accept_f14_d_4_retail_catalog_ids_equal_the_scene_graphs_ids` builds each of the
nine containers' scene graph with `world_scene_graph_from_gamez` and compares:

* 56 620 rows; the catalog's id set equals the scene graph's id set in every
  container; **0 refused** nodes.
* The 13 723 former fallback rows (10 235 sharing a path, 3 488 unspellable)
  convert. 10 345 rows now carry a `-i<hex>` component (a same-named sibling set
  plus every descendant spelled under one), 3 460 are digest ids.
* The 1 601 names the escaping rewrites (1 599 holding one `.`, 2 holding a `-`;
  247 of them roots) agree and read back to the stored name through
  `SceneNodeId::authored_names`.

## Accepted costs (owner ruling)

* An `-i<hex>` id depends on a stored word, so another build of the data may move
  it. The word is measured data, not an enumeration order.
* Digest ids are opaque; `authored_names` stays the reversible reading half and
  answers `None` for them.
* What the original engine used to identify a repeated node is unknown; the use of
  `node_index` is a designed choice, not an original measurement. Nothing here is
  `verified_original`.

## Not done here

* `docs/contracts/IDENTITY-CONTENT.md` is a protected path and this task does not
  carry `allowProtectedChanges`, so its notes are not updated. The contract's own
  statement (stable ids derive from a semantic source key, not enumeration order)
  is unchanged and still holds.
* `docs/findings/evidence/F14-D.4.json` is the historical record of task F14-D.4
  at its reviewed tree and is not regenerated.
* `docs/findings/2026-10-03-f14-d-4-scene-and-mesh-collections.md` describes the
  record-address fallback as it was when F14-D.4 merged; this note supersedes that
  rule.

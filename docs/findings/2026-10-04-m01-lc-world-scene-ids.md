# M01-LC-WORLD-SCENE: the world containers' `scene_node` id verdicts

Date: 2026-10-04. Task: #628. Capability used: **`retail`** (read-only access to
`$CS_GAME_DIR`) and ordinary build/test. No original run happened; nothing here
claims `verified_original` or says how the original engine identified a node.

Follows `2026-10-03-f11-e1-scene-node-id-escaping.md` and
`2026-10-03-f18-world-hierarchy-authority.md`, which left two refusals for the
eight `ZBD/<group>/gamez.zbd` containers once the hierarchy reconciles.

## Files

- `crates/cs_content/src/scene.rs`: `ParsedNode::disambiguator`, the sibling
  rule (`sibling_components`), the key-bound rule (`fit_key_bound`) and the
  reading half in `unescape_scene_node_name`. `parsed_node_from_gamez` fills the
  new field. `SceneGraph::build` is the only consumer.
- `crates/cs_app/tests/world/scene_ids.rs` (new): the `accept_m01_lc_world_scene_`
  tests; `main.rs` is wiring only.
- `crates/cs_app/tests/world/hierarchy.rs`: the F18-A retail test asserted the
  refusal this task removes; it now asserts the conversion. Three helpers became
  `pub` for `scene_ids.rs`.

## Measurements (production reader, over the installation)

Duplicate name-paths, counted over each container's reconciled hierarchy, names
lowercased as `ContentId` folds them:

| container | records sharing a name-path | groups | `node_index` distinct inside every group |
| --- | --- | --- | --- |
| C1 | 1 003 | 124 | yes |
| C1B | 1 910 | 80 | yes |
| C1C | 1 146 | 40 | yes |
| C2 | 468 | 122 | yes |
| C2B | 1 266 | 39 | yes |
| C3 | 52 | 20 | yes |
| C4 | 1 928 | 106 | yes |
| C5 | 2 462 | 447 | yes |

(F11-E1's 32–2 015 per container also counted duplicate keys in a different
way; both are over the same records.) Within a group the records are usually
**different parents' children** carrying the same name, which is why a per-sibling
rule is enough: only 1–99 groups per container are siblings of one parent.

`node_index` is the record's trailing word. The reader asserts its top byte is
constant and calls the low three bytes "the node's own index in the engine's id
space". It is **not** the slot (equal to slot + 1 for 1 110 of C1's 7 064
records, and its offset from the slot changes 2 017 times along the array) and
it is **not globally unique** (245 repeated values in C1, 1 032 in C5), but it is
distinct among every set of siblings sharing a name, in all eight containers.

Over-long name-paths (`<container>.<escaped path>` past 128 bytes): C1 83,
C1B 98, C1C 292, C2 72, C2B 97, C3 72, C4 102, C5 382 nodes.

## The rule (designed engine contract, not original behaviour)

1. **Siblings that would share an id each take the stored word.** Within one
   parent's children (and among the roots), nodes whose escaped names are equal
   after case folding all take `-i<hex>` of `node_index & 0xffffff` on their
   own component. Every member takes it, so no member is privileged by position
   and reordering records moves no id. `-i` cannot be read as an escape (`i` is
   not a hex digit). The rule applies only when every member stores a word and
   the words differ; otherwise the collision is refused as
   `SceneError::DuplicateNodeId`, exactly as before. Names with no collision keep
   their F11-E1 ids. This also resolves what F11-E1 recorded as the case-folding
   loss, for records that store a word.
2. **A name-path past the key bound is spelled by a digest.** The id is
   `<container>.-h<32 hex>`, the first 128 bits of the SHA-256 of the lowercased
   escaped path. `-h` is not an escape either. The id then does not say what
   the file said; `SceneNode::path()` and `name()` do, and
   `SceneNodeId::authored_names` answers `None` for such an id instead of a
   guess. A digest clash would be refused as `DuplicateNodeId` (none measured).

Reversible record: every node keeps its stored name and authored path;
`authored_names` reads every non-digest id back to the authored path (the test
checks all of them), dropping the `-i` suffix.

## Result

All eight world containers now convert through
`world_scene_graph_from_gamez`: C1 7 064 nodes / 165 roots, C1B 5 603 / 148,
C1C 5 644 / 149, C2 4 956 / 166, C2B 4 901 / 146, C3 5 408 / 157, C4 8 289 /
163, C5 11 438 / 215. Digest-spelled ids: 83, 98, 292, 72, 97, 72, 102, 382.
Nodes whose own component carries a stored word: 124, 103, 30, 151, 34, 46, 179,
732. `accept_m01_lc_world_scene_retail_every_world_container_converts` pins them.

## Unknowns and limitations

- **Whether the original engine uses `node_index` for identity is unknown.** The
  rule uses it because it is a stored word that is distinct among the colliding
  siblings, not because it is known to be an identity. A future measurement of
  the engine could change it.
- Digest ids are opaque (affects the 83–382 deepest nodes per container, e.g. the
  `pzhookpoint…` chains). A consumer addresses them by id or `path()`.
- The disambiguated ids depend on the stored word, so another build of the data
  would move them.
- `cs_content::scene::scene_graph_from_gamez` (the non-world path) still refuses
  the world containers at `InconsistentParentage` first; unchanged.
- No original run, so no fidelity claim; review by a fresh-context agent is
  recommended because this changes a published identity contract.

## Sources used

The two findings above, `crates/cs_formats/src/gamez/nodes.rs` (`node_index`),
`docs/contracts/IDENTITY-CONTENT.md`.

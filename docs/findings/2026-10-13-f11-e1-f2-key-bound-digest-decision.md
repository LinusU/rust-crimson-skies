# F11-E1.F2: what a `scene_node` id does with a name-path past the 128-byte key bound

Date: 2026-10-13. Task: #586 "Decide what a scene_node id does with a
name-path longer than the 128-byte key bound" (feature sheet
`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, the F11-E1
stage's one standing refusal, #493). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capability used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. No original run happened,
nothing here claims `verified_original`, and this is a decision about how
*this* repository spells ids — engine contract, not a measurement of the
original engine's identity scheme.

## The decision, and why this task records rather than re-takes it

The task was filed on 2026-10-03 because F11-E1 left one refusal standing: a
name-path whose derived key exceeds `MAX_CONTENT_KEY_LEN` (128 bytes) stays
`SceneError::NodeId { source: KeyTooLong }`, which no world container can pass.
One day later, #628 (M01-LC-WORLD-SCENE, merged 2026-10-04, finding
`docs/findings/2026-10-04-m01-lc-world-scene-ids.md`) took this exact decision
for exactly these shapes and implemented it in production:

> A name-path past the key bound is spelled by a digest. The id is
> `<container>.-h<32 hex>`, the first 128 bits of the SHA-256 of the lowercased
> escaped path (`fit_key_bound` in `crates/cs_content/src/scene.rs`). `-h` is
> not an escape. The id then does not say what the file said;
> `SceneNode::path()` and `name()` do, and `SceneNodeId::authored_names`
> answers `None` for such an id instead of a guess. A digest clash would be
> refused as `DuplicateNodeId` (none measured).

So this task does **not** take the decision again; it does the parts of its own
acceptance criteria that #628 did not cover: pinning the two measured shapes
with tests under this task's own prefix, recording the decision against the
measurement this task was filed with, recording what the rule costs, and
updating the F11-E1 limitation to name the resolving task. The rule is
reversible nowhere and lossy by exactly one stated thing — the id's own words —
never a hidden trim: the stored name and the authored path stay on the record
unchanged, as F11-E1 required for the character set.

### The three options this task listed, and why the implemented one is the defensible one

- **Raising `MAX_CONTENT_KEY_LEN`** would be one line in
  `crates/cs_types/src/content.rs`, but it moves the shared identity bound of
  every namespace plus F14's catalog normalizer and the graph-local
  `DamageNodeKey` mirror (`MAX_NODE_KEY_LEN`). That file is not this task's
  owner path and the task says to `block_task` rather than edit it unasked; no
  part of this decision needs it. The 136- and 150-byte paths fit inside the
  shared bound when spelled by a digest.
- **Shortening or folding the path** (keep the root's name and the leaf's,
  hash the middle, etc.) would invent a prefix identity the store never had:
  many measured chains share long prefixes, so a folded spelling either still
  overflows or manufactures collisions, and it discards the same bytes the
  digest discards while pretending to keep some. The digest keeps the whole
  path as its input, so two paths that differ anywhere derive different ids.
- **Refusing the offending node** (partial conversion) would keep every world
  container hostage to its deepest nodes — the measured counts below say 72 to
  382 nodes per container — and F11-E1's AC already rejected partial results
  for `player_pfighter`. The digest lets the whole container convert.

## The measurement, from production readers over the installation

The shapes this task was filed with (name-paths walked up the `parent` links,
spelled here root-first, the way `SceneNode::path` and the id spell them):

| shape | authored path | bytes | where it is the longest |
| --- | --- | --- | --- |
| crane chain | `world1.piratezep.move_zeppelin.tilt_zeppelin.rock_zeppelin.underneath.pzhookup_crane.hook_base.top_seg.mid_seg.hook_seg.hoop.pzhookpoint` | **136** | six of the eight world containers (F11-E1's count; `C1C` holds it too, as a non-maximal path) |
| tailhook chain | `world1.workersvoyagezep.move_zeppelin.tilt_zeppelin.rock_zeppelin.underneath.wv_tailhook.peoplehook.fassenbeiner.cpilot.cpilot_drop.cp_rt.cp_rc.cp_rft` | **150** | `C1C`, the installation's longest |

Re-measured today through the production reader and
`cs_content::world::world_scene_graph_from_gamez` over `ZBD/C1C/gamez.zbd`
(5 644 nodes): the longest authored path is **150 bytes** and is exactly the
tailhook chain above; the crane chain is present at **136 bytes**; **292** ids
are digest-spelled, matching #628's pinned count; every one of the 5 644 keys
fits the 128-byte bound. `planes.zbd` (the F11-E1 container) is untouched by
this rule: its longest authored path is 82 bytes and its longest derived key
104, inside the bound, which is why that container converted under F11-E1
alone.

## What the rule costs, exactly

- **Namespaces:** only `scene_node`. `MAX_CONTENT_KEY_LEN` is unchanged, so
  `ContentId` for every other kind, F14's catalog normalizer and the
  graph-local `DamageNodeKey` (`MAX_NODE_KEY_LEN`) are untouched; no
  `cs_types` owner path was needed or edited.
- **Containers:** the eight world containers carry the digest-spelled ids —
  `C1` 83, `C1B` 98, `C1C` 292, `C2` 72, `C2B` 97, `C3` 72, `C4` 102, `C5` 382
  (1 198 in total, #628's measured counts, the `C1C` figure re-pinned by this
  task's retail test). `planes.zbd` carries none.
- **Stages:** F18's world import (`world_scene_graph_from_gamez`) reaches the
  canonical build for all eight containers; this rule is what removed the
  over-long-path share of the refusal F11-E1 recorded (the duplicate
  name-path share was removed by #628's sibling rule, the parentage share by
  F18-A). F11-C sockets and F11-D2's roster audit live in `planes.zbd` and are
  unaffected. Consumers of a world container address a digest-spelled node by
  its id or by `path()`; binding rules match on the authored path, which never
  changed.
- **The stated loss:** for the 1 198 deepest nodes above, the id no longer
  spells the file's own words and cannot be reversed to them from the id alone
  (`authored_names` answers `None` rather than guessing). Nothing else is lost:
  no stored name, no authored path, no position-derived identity.

## Test inventory (this task's own prefix: `accept_f11_e1_f2_`)

In `crates/cs_content/tests/scene.rs`:

| test | covers | fails when |
| --- | --- | --- |
| `accept_f11_e1_f2_the_measured_over_long_shapes_are_digest_spelled_within_the_shared_bound` | both measured shapes (136 and 150 bytes) built through `SceneGraph::build` over the canonical adapter; per node the spelled-iff-it-fits rule against `MAX_CONTENT_KEY_LEN`; the exact digest ids (pinned from an independent `shasum -a 256` over the same literal, so the hash input and width are checked, not echoed); the inclusive boundary at exactly 128 bytes and the digest one byte past it; stability across rebuilds; distinct ids for the two shapes; `authored_names` answering `None` for a digest id; the root keeping the id it always had | the digest rule is removed or replaced, the boundary moves, the hash input/width changes, the id becomes unstable, or the loss is papered over by a guess |
| `accept_f11_e1_f2_retail_c1c_digest_spells_every_over_long_path` (`#[ignore = "requires CS_GAME_DIR"]`) | the real `ZBD/C1C/gamez.zbd` through the production reader and `world_scene_graph_from_gamez`: the container converts (5 644 nodes); every key fits the bound; exactly 292 ids are digest-spelled; both measured shapes exist with their exact digest ids; the longest authored path is the measured 150 bytes and is kept whole on the node | the conversion refuses, a key overflows, a digest id moves or reads back, or the corpus shape changes |

**Sensitivity check.** With `fit_key_bound` mutated to always spell the path
(the pre-#628 refusal), both tests fail (observed, reverted). The digest
constant itself was computed with `shasum -a 256` outside production, so a
change to the hash input (e.g. including the container key) or width dies on
the synthetic test's exact-key assertion.

## Unknowns and limitations (all recorded, none guessed)

- **Whether the original engine uses anything like a digest for deep nodes is
  unknown.** The rule is engine contract chosen here because it is total,
  injective, stable across parses and position-free (F11-A's rule); a future
  measurement of the original executable could change it, and would move the
  1 198 digest-spelled ids with it.
- **The 128-byte bound itself is engine-authored**, not measured from the
  original. Raising it remains a repository-wide contract change that needs
  the owner to add `crates/cs_types/src/content.rs` to a task's owner paths;
  this task did not touch it and does not need it.
- **Digest ids are opaque to `authored_names`** by design (1 198 nodes across
  the eight world containers); a caller holding only the id cannot recover the
  path, exactly as #628 recorded.
- **Evidence class:** `ObservedTool` — the measurements came from the original
  bytes through production readers; the rule itself is `Designed`. No original
  run happened, and no agent review replaces the owner's approval.

## Sources used

- `docs/findings/2026-10-03-f11-e1-scene-node-id-escaping.md` (the refusal
  this decision resolves, its measurement of the nine archives, and the
  limitation this task updates).
- `docs/findings/2026-10-04-m01-lc-world-scene-ids.md` (#628's decision, the
  rule as implemented, and the per-container digest counts).
- `docs/contracts/IDENTITY-CONTENT.md` (the key grammar, the
  validate-at-construction rule, the position-free identity rule).
- `crates/cs_types/src/content.rs` (`MAX_CONTENT_KEY_LEN`,
  `MAX_NODE_KEY_LEN`, `normalize_grammar`) and
  `crates/cs_content/src/scene.rs` (`fit_key_bound`,
  `SceneNodeId::authored_names`, `SceneGraph::build`).
- `crates/cs_app/tests/world/scene_ids.rs` (#628's tests, which this task's
  tests complement rather than replace).

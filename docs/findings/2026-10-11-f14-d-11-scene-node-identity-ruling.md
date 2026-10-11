# Rally task #560 (F14-D.11): the ruling a repeated authored scene-node name still needs

Date: 2026-10-11. Task: #560 "Owner ruling needed: an identity scheme for a
repeated authored scene-node name" (filed 2026-10-03 from #487 / F14-D.4).
Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Spec:
`specs/F14-canonical-content-catalog-and-dependency-closure.md` (non-negotiable:
"Stable ids derive from a semantic source key plus namespace, not enumeration
order"). Capabilities used: **`retail`** (read-only access to `$CS_GAME_DIR`)
and ordinary build/test. No original run happened and none is claimed, so
nothing here says how the original engine identified a node.

## Status: blocked on the owner ruling this task exists to collect

Nothing is implemented. The task's own text says the four options each carry a
cost the agent may not choose, so this note records what an agent *can*
establish — the evidence as it stands on `main` today, and the decision points —
and the task is blocked until the owner rules. Re-keying the affected rows is a
follow-up task once the ruling exists; this branch carries this note only.

Affected content, restated so the ruling is not needed twice: 13 723 of the
56 620 `scene_node` rows of the nine GameZ containers — 10 235 whose authored
name path another record of the same container spells, 3 488 whose path forms no
valid key (a byte the grammar refuses, or a key past `MAX_CONTENT_KEY_LEN` once
the container prefix is added) — and therefore every name-based binding that
would address them (F11-B's ECS import, F11-C's socket bindings, F11-D's
airframe roster).

## What this session verified (commands, in order)

1. `git fetch origin && git checkout --no-track -B
   rally/560-owner-ruling-needed-an-identity-scheme-f origin/main` — `main` at
   `f2d25bcb`, `crates/cs_content/src/catalog/baseline.rs` unchanged since
   F14-D.4 landed.
2. `cargo test -p cs_content --test accept_f14_d_4_geometry_collections --
   --include-ignored` — **12 passed, 0 failed**, including
   `retail_every_gamez_container_yields_its_nodes_and_named_meshes` (~113 s over
   the read-only installation). The premise #560 was filed from still holds:
   **56 620** scene node rows (42 897 named, **10 235** ambiguous, **3 488**
   unspellable, 0 unterminated) and 17 139 mesh rows across the nine
   containers.
3. `cargo fmt --all -- --check` and `cargo clippy --workspace --all-targets
   --all-features --locked -- -D warnings` — both clean on this branch's tree.
   The full `cargo test --workspace --locked` was run for this note's push and
   recorded with the commit.

No production code was changed and no original bytes were written anywhere.

## What changed on `main` since #560 was filed

Two tasks have landed that move the design space #560 described:

1. **#493 (F11-E1), done.** `SceneNodeId::for_path` was **replaced** by
   `SceneNodeId::for_escaped_path`
   (`crates/cs_content/src/scene.rs:412`) over names escaped by
   `escape_scene_node_name` (`:240`; `-` → `-2d`, `.` → `-2e`, anything else the
   grammar refuses → two lowercase hex digits), with `authored_names` (`:438`)
   as the reversible reading half. **#560's implementation plan names
   `for_path`, which no longer exists.**
2. **#628 (M01-LC-WORLD-SCENE), done.** `SceneGraph::build` now gives every
   member of a colliding sibling set `-i<hex>` of its stored `node_index` word
   (`sibling_components`, `:313`) and refuses `DuplicateNodeId` when the words
   are absent or not distinct, and spells a path past `MAX_CONTENT_KEY_LEN` as
   `<container>.-h<32 hex>`, a SHA-256 prefix (`fit_key_bound`, `:364`).
   All eight world containers convert through
   `cs_content::world::world_scene_graph_from_gamez` (`:4862`).

So within one repository the `scene_node` id space now has **two derivations**:

- `cs_content::catalog::baseline::scene_node_key`
  (`crates/cs_content/src/catalog/baseline.rs:2375`) offers the **raw** joined
  authored path to `ContentId::from_source` — the pre-#493 derivation — and
  falls back to `<container>.record<offset>` (`scene_node_id`, `:2351`;
  `GAMEZ_CONTAINER_RECORD`, `:554`) for the 13 723 rows `NodeIdentity::of`
  (`:1951`) refuses.
- `cs_content::scene` escapes, disambiguates and digest-spells.

They disagree on exactly two sets of rows:

- **The 13 723 fallback rows**: a record address on one side, an escaped,
  `-i<hex>`-disambiguated or `-h`-digest key on the other.
- **The names the escaping rewrites without refusing**: 1 599 names holding one
  `.` (245 of them roots) and 2 holding a `-`, all in the eight world
  containers (F11-E1's measurement; `planes.zbd` holds none). Those rows are
  `named` in the baseline but spell a **different** key under the scene module.
  Under the raw join a `.` inside a name is also structurally ambiguous as a
  separator — `a.b` + `c` and `a` + `b.c` form one key — which escaping removes;
  whether such a pair actually occurs in the corpus is **not** measured here.

One consequence for the implementation, whichever way the ruling goes: the
sibling rule reads the **child lists** (`sibling_components` iterates
`node.children`), that is the hierarchy F18-A0 reconciles in
`world_scene_graph_from_gamez`, while the baseline walks the **parent slots**
and measured its ambiguity set that way (F14-D.4). The two ambiguity *counts*
agree — 10 235 records sharing a name-path across the eight world containers in
both measurements, and none in `planes.zbd` — but a shared rule cannot simply be
called from the baseline's existing walk.

## The decision as it now stands

The four options #560 listed, each with what this session established about its
cost, plus the one the two merged tasks created:

1. **Keep the record address** (status quo). Machine-stable and unique, but a
   consumer must look a node up by its offset, and the address moves if the
   container is rebuilt — although the row's `SourceSpan` already pins the
   record as provenance, so the key repeats addressing information the row holds
   anyway. The new fact: the scene module *can* now address these nodes by a
   name-derived key, so this option no longer reads as "unaddressable" but as
   "the catalog and the scene graph disagree about what a node is called" — the
   exact failure F14-D.4's design decision said must never happen.
2. **Parent-relative disambiguator (parent slot + sibling ordinal).** Still
   forbidden: F14's non-negotiable above, and F11-A's "ambiguity is never
   resolved by position". Note the scene module's rule is *not* this: every
   colliding sibling takes its own **stored** word, so no member is privileged
   by position and reordering records moves no id. Measured distinct inside
   every colliding sibling group in all eight world containers (M01-LC), and
   refused rather than guessed otherwise.
3. **Author a stable per-container node id table** (56 620 entries). Stable and
   addressable, but authored content: every row would have to record which part
   of its identity is measured (the name path, the stored word) and which is
   designed (the table entry), against `IDENTITY-CONTENT`'s provenance rule.
4. **Widen the id grammar or shorten the prefix.** Fixes only the character and
   length refusals — a subset of the 3 488 unspellable rows — and touches
   `cs_types::content`, which F14-A owns. It does nothing for the 10 235
   duplicates.
5. **Align the catalog with the derivation `cs_content::scene` already
   publishes** (the option the two merged tasks created). The repository has
   now chosen this scheme for the `scene_node` id space twice over, so adopting
   it in the baseline would make the row and the graph agree, and every affected
   node would keep its authored name through `authored_names` / `path()`
   (digest ids read back as `None` and are addressed by id or `path()`). Costs
   an owner must weigh: `-i<hex>` depends on a stored word, so another build of
   the data moves those ids (M01-LC's own recorded limitation); digest ids are
   opaque; and the catalog must still hold a row for a node the scene module
   *refuses* (`DuplicateNodeId` when a colliding set lacks distinct stored words
   — measured not to occur on this installation, but
   `IDENTITY-CONTENT` says a collection cannot exclude a failed entry).

## Decision points the owner must rule on

- Which of the five schemes the `scene_node` catalog rows use — in particular
  whether option 5 (the scheme already published for this id space) is ratified
  for the catalog, or the catalog keeps `record<offset>` by design.
- If option 5: whether the catalog follows the scene module's rule exactly
  (escaping, `-i<hex>`, `-h<digest>`), **which hierarchy** the disambiguation
  reads (the reconciled child lists versus the parent-slot walk the baseline
  measures today), and how a row stays present for a node the rule would refuse.
- Whether the divergence is recorded as a defect to fix regardless: the
  baseline's key for a dotted name spells a `.` the escaping writes `-2e`, and
  F14-D.4's stated rule was that a catalog row and a scene-graph node can never
  disagree about what a node is called.
- The scope boundary for option 4 (`cs_types::content`, owned by F14-A):
  whether the grammar or `MAX_CONTENT_KEY_LEN` is in play at all, given the
  digest rule already answers the over-long case for the scene graph.

## What stays unclaimed

What the original engine used to identify a repeated node is **unknown**.
`node_index`'s role in #628's rule is a designed choice made because it is a
stored word distinct among the colliding siblings, not a measurement of the
engine; a future measurement could change it, as M01-LC records. `retail` here
is file access: this session re-ran the F14-D.4 production suite over the
read-only installation and changed nothing. No agent review, and nothing in
this note, replaces the owner's human approval.

## Sources used

- `docs/findings/2026-10-03-f14-d-4-scene-and-mesh-collections.md` (the
  measured premise, the four-way classification, the "a row here and a
  scene-graph node can never disagree" rule).
- `docs/findings/2026-10-03-f11-e1-scene-node-id-escaping.md` (the escaping
  rule, its measurement, and the dotted/dashed name counts).
- `docs/findings/2026-10-04-m01-lc-world-scene-ids.md` (the sibling `-i<hex>`
  rule, the digest rule, and their recorded limitations).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (the `node_index`
  word's measured properties).
- `docs/contracts/IDENTITY-CONTENT.md` and
  `specs/F14-canonical-content-catalog-and-dependency-closure.md` (the
  constraints above).
- `crates/cs_content/src/{scene,world}.rs` and
  `crates/cs_content/src/catalog/baseline.rs` (the two derivations as they stand
  on `main` at `f2d25bcb`).

# F11-A: node hierarchy and semantic binding records

Date: 2026-09-29. Task: F11-A "Define node hierarchy and semantic binding
records" (`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`,
section `### F11-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Required capability: ordinary
build/test (the machine also has `retail`, `gpu` and `audio`; **none was
used** — this stage reads no original data, renders nothing and plays
nothing; all fixture values are newly authored).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (new): `SceneNodeId`,
  `SceneRootRef` (`new`, `container`, `root`), `CanonicalTransform`
  (`IDENTITY`, `try_new`, `linear`, `translation`, `compose`, `apply`,
  `transform_vector`, `determinant`, `mirrored`), `AuthoredTransform`
  (`IDENTITY`), `ParsedNodeKind`, `MeshBinding`, `ParsedNode` (`new`,
  `ZONE_DEFAULT`), `LodInfo`, `NodeKind`, `NodeVisibility`,
  `CollisionRole`, `PartRole` (+ `label`), `AnimationBinding`,
  `SemanticBinding`, `BindingMap` (`new`, `rules`), `SceneNode` (the
  accessors only), `SceneGraph` (`build`, `container`, `nodes`, `roots`,
  `node`, `root`, `single_root`, `len`, `is_empty`,
  `unmatched_bindings`) and `SceneError` (`EmptyScene`,
  `DuplicateIndex`, `DanglingParent`, `DanglingChild`,
  `InconsistentParentage`, `NoRoots`, `Cycle`, `AmbiguousRoots`,
  `UnknownRoot`, `NodeId`, `DuplicateNodeId`, `NodeKind`,
  `RootOutsideContainer`, `NotARootNode`, `Transform`, `LodRange`,
  `MeshKind`, `AnimationChannelKind`, `DuplicateBindingRule`).
- `crates/cs_app/src/scene.rs` (new): `SceneGeneration` (+ `next`),
  `SceneNodeBinding`, `NodeVisualTransform` (`from_canonical`, `global`,
  `mirrored`), `NodeTransformError` (`NotRepresentable`) and two
  `accept_f11_a_*` unit tests.
- `crates/cs_app/src/airframe_visual.rs` (new): `AirframeVisual`
  (`new`, `airframe`, `root`), `AirframeVisualError` (`AirframeKind`,
  `Root`) and an `accept_f11_a_*` unit test.
- `crates/cs_content/tests/scene.rs` (new): the `accept_f11_a_*`
  acceptance tests below.
- Wiring only (no logic): `crates/cs_content/src/lib.rs` gains
  `pub mod scene;` plus a doc paragraph; `crates/cs_app/src/lib.rs`
  gains `pub mod airframe_visual;` / `pub mod scene;` plus a doc
  paragraph; `crates/cs_app/Cargo.toml` moves `cs_content` from
  dev-dependencies to dependencies (the binding records carry
  `cs_content::scene` types) and `Cargo.lock` updates accordingly.

**One observable failure:** if `canonical_local` skipped the axis-map
conjugation (copying the authored matrix through unchanged) or
`SceneGraph::build` composed `local ∘ parent_world` instead of
`parent_world ∘ local`, `accept_f11_a_nested_transforms_and_negative_scale_preserve_alignment`
fails: the `wing`/`gun`/`tip` world transforms land on different
hand-computed values and the mirror flag is wrong. The LOD/binding test
independently fails if the stored `matrix` is ignored in favor of the
euler triple.

## Design decisions

- **Node identity is `scene_node/<container key>.<authored name-path>`.**
  The id is derived from the authored names folded through
  `ContentId::from_source`, so the same parse always produces the same
  id and a name that cannot form a key is refused (`NodeId`), not
  transliterated. Array slots (`index`, `parent`, `children`,
  `mesh_index`) stay on the record as provenance only. Two nodes
  deriving the same id — same-name siblings or same-named roots — are
  refused (`DuplicateNodeId`): ambiguity is never resolved by position.
- **A container is a forest.** `PLANES.ZBD` holds one root per
  airframe, so `SceneGraph::roots` is a list in stored order,
  `root(name)` looks a root up by authored name, and `single_root`
  reports `AmbiguousRoots` when the caller asked for *the* root of a
  multi-root container. `SceneRootRef` is the checked airframe handle:
  its constructor refuses roots outside the named container and nested
  nodes, which is how "references roots in `PLANES.ZBD`, not models by
  array position" is enforced at the type level.
- **The canonical transform is a matrix, not TRS.** A
  `rotation · scale` linear map plus translation survives authored
  negative scale and any composition of it; a `Transform`-style
  rotation/scale decomposition would silently lose the mirror. The
  authored transform is preserved beside the canonical local and the
  composed world transform, so nothing about the conversion is
  irreversible. `visual_transform` and `collision_transform` return the
  same composed value by construction — LOD switching may only select
  which node's mesh is drawn (F11 non-negotiable behavior 4).
- **Conversion goes through the declared `SourceAdapter` exactly once**
  (the F16-A contract): the euler triple is converted through
  `angle_to_canonical`, the source-space linear map `rotation · scale`
  is conjugated through the axis map (`A·L·Aᵀ` — correct for
  reflection-containing maps), and the translation is scaled by
  `meters_per_unit`. The stored `matrix` wins over the euler-derived
  one because the reference corpus disagrees in ~0.74 % of objects.
- **Semantic bindings are evidence, not code.** A `BindingMap` rules
  exact authored name-paths to `Resolved<PartRole>` /
  `Resolved<CollisionRole>` / `Resolved<ContentId>` animation channels,
  each rule carrying its own `Provenance`. Duplicate rules for one path
  are refused at `BindingMap::new`; a rule that matches no node is
  reported in `unmatched_bindings`; a node with no matching rule is
  unbound, not silently bound. Resolved ids are kind-checked (`Mesh`
  for `mesh_index`, `AnimationTrack` for channels).
- **Unknown stays unknown.** `flags` and `zone_id` are stored raw —
  every CS-specific flag bit is `UNK*` in the pinned reference, so
  `visibility` arrives as `Resolved::Unknown` under claim
  `f11a.node-flags-unmeasured` rather than a guessed bit, and an
  unbound node's collision role is left to the binding evidence.
  `LodInfo.level` keeps the stored boolean uninterpreted.
- **The ECS side is records only.** `SceneNodeBinding` stamps each
  entity with a `SceneGeneration` so a reload makes stale bindings
  detectable (session-generation ownership, behavior 5), and
  `NodeVisualTransform` maps the composed matrix into a
  `GlobalTransform` — a full affine, so mirroring survives — refusing
  values that overflow f32. `AirframeVisual` binds `airframe/<key>`
  elements to `SceneRootRef`s; roster selectability and forced mission
  assignments stay out of it (behavior 3). No systems run yet.

## Test inventory

| `accept_f11_a_*` test | Covers | Fails when |
| --- | --- | --- |
| `nested_transforms_and_negative_scale_preserve_alignment` (cs_content/tests/scene.rs) | AC01: 90° rotation + translation + `[-1,1,1]` scale through the declared left-handed-centimeters-degrees adapter; hand-computed canonical locals and worlds; `mirrored` propagation; shared visual/collision transform; mesh association and authored transform preserved | the conjugation, composition order, mirror tracking or id derivation is removed or wrong |
| `lod_variants_and_bindings_survive_conversion` | every LOD variant preserved with converted meters range; stored matrix precedence; name-path binding attach (role/collision/animation); unmatched rules reported; `visibility` explicit-unknown; `single_root`/`root` errors; stable ids | variant dropping, euler-only conversion, silent binding drops, guessed visibility, or ambiguous root answers |
| `rejects_cycles_dangling_parents_and_ambiguous_roots` | `EmptyScene`, `DuplicateIndex`, `DanglingParent`, `DanglingChild`, `InconsistentParentage` (both directions), `NoRoots`, detached `Cycle`, `DuplicateNodeId` (same-named roots), `NodeId` | any link validation is weakened |
| `rejects_bad_transforms_and_wrong_binding_kinds` | non-finite transform components, reversed/negative/non-finite LOD ranges, `MeshKind`, `AnimationChannelKind`, `DuplicateBindingRule`, `NodeKind`, `RootOutsideContainer`, `NotARootNode` | the boundary checks or kind checks are removed |
| `scene_binding_carries_node_id_and_generation` (cs_app/src/scene.rs) | the binding record holds its stable id and a bumpable generation | the generation stamp is removed |
| `visual_transform_preserves_mirror_and_translation` (cs_app/src/scene.rs) | the f64→f32 `GlobalTransform` conversion keeps the mirror and translation; overflow refused | the affine conversion loses the determinant sign or skips the finiteness check |
| `airframe_visual_references_roots_by_id_not_position` (cs_app/src/airframe_visual.rs) | `airframe` kind enforcement; nested-node and wrong-container root rejection | the kind or container checks are removed |

## Unknowns and limitations (all recorded, none guessed)

- **CS node flag bits** (`UNK02`–`UNK25` in `NodeBitFlagsCs`) — kept raw;
  visibility/active/collision semantics unmeasured. Resolve with
  F11-B/D evidence.
- **`zone_id` domain** — `ZONE_DEFAULT = 255` and values 1–3 are
  observed in the reference's assertions; the field is preserved raw
  and uninterpreted.
- **LOD `level` semantics** — the stored boolean is kept uninterpreted;
  which value means "fade band" vs "variant" is unmeasured.
- **`rotation · scale` composition order** — every measured CS scale is
  `1.0`, so the order is unobservable in the corpus; `rotation · scale`
  is a designed choice consistent with the reference's matrix field
  semantics.
- **Euler convention source** — `Rz·Ry·Rx` over negated angles comes
  from the pinned reference (`mech3ax-nodes/src/math.rs`),
  observed-tool evidence only; not verified against the original
  executable.
- **Semantic bindings for real containers** — no authored name-path →
  role mapping is evidenced yet, so `BindingMap` is populated by
  fixtures only; F11-B/D supply the rules with provenance.
- **Not implemented (by stage)**: the GameZ reader for the node array,
  LOD switching, aircraft part damage wiring, spawn/teardown lifetime,
  roster audits — F11-B/C/D and F50 series.
- **Fixture scope** — synthetic only; this stage makes no retail
  compatibility claim.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (F11-A
  section, deliverable, non-negotiable behaviors, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, provenance,
  explicit unknowns, ownership-cycle rule, exact lookup).
- `docs/research/2026-09-23-s12-gamez-progressive-mesh-format.md`
  (GameZ layout: `nodes_offset`, node array ownership, `mesh_index`
  association).
- `docs/research/2026-09-28-s17-aircraft-airframe-catalog.md`
  (`PLANES.ZBD` = the aircraft ZBD; `PLANES.SP` = performance table).
- Pinned reference mech3ax v0.6.0 via `docs/research/reference/`:
  `NodeCsC`/`NodeType`/`NodeBitFlagsCs` (208-byte record, `mesh_index`
  at 60), `Object3dCsC` (144 bytes: euler rotation, scale, stored
  matrix, translation; ~0.74 % matrix disagreement; identity when
  `flags == 40`), `LodCsC` (92 bytes: level, ranges, squared-range
  check), `euler_to_matrix` and `ZONE_DEFAULT` — all ObservedTool.
- `crates/cs_content/src/coordinates.rs` (the F16-A adapter this
  conversion routes through) and `crates/cs_types/src/{content,space}.rs`
  (the shared identity/provenance and spatial types).

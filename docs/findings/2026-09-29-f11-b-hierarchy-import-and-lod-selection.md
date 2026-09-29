# F11-B: hierarchy import and LOD selection

Date: 2026-09-29. Task: F11-B "Implement hierarchy import and LOD selection"
(`specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`, section
`### F11-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Required
capability: ordinary build/test only — **no** `retail`, `gpu` or `audio` was
used: this stage reads no original data, renders nothing and plays nothing;
every fixture value is newly authored.

The acceptance scenario for this stage (AC02) is *a destroyed wing and its gun
remain disabled across an LOD transition*.

## Scope decision (taken before editing)

This stage imports an **already converted** hierarchy (`SceneGraph`, F11-A) into
the ECS and implements presentation-only LOD selection over it.

The GameZ **node-array byte reader is not part of this stage**. The container
layout lives in `crates/cs_formats/` — `GameZHeader::nodes_offset` /
`node_array_size` mark where the node array starts, and `read_gamez_meshes`
deliberately stops there (`crates/cs_formats/src/gamez/mod.rs`: "What this does
not read: the node array") — and `crates/cs_formats/` is not an owner path of
*any* F11 stage (all four name `crates/cs_content/src/scene.rs`,
`crates/cs_app/src/scene.rs`, `crates/cs_app/src/airframe_visual.rs`,
`crates/cs_content/tests/scene.rs`, `docs/findings/`). Decoding the 208-byte
`NodeCsC` record from a file this task may not open, or guessing its layout,
would break the owner-path rule and rule 4 (unknown means unknown). It is
recorded under Unknowns below and filed as follow-up task **#392** ("Read the
GameZ node array into ParsedNode records") instead.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (extend): `LodCoverage` (`Covered`,
  `Overlap`, `GapFallback`), `LodChoice` (`index`, `coverage`),
  `LodSelectError` (`NoVariants`, `Distance`, `Range { index }`) and the pure
  rule `select_lod_variant(variants: &[LodInfo], distance: Meters)`.
- `crates/cs_app/src/scene.rs` (extend): `SceneImport` (`entity`, `generation`,
  `len`, `is_empty`), `SceneImportError` (`Transform`, `ForeignContainer`,
  `UnknownNode`), `import_scene`, `import_airframe`, the components
  `NodeLodVariant` (`new`, `info`), `NodeDisabled`, `NodePresentation` +
  `PresentationState` (`Drawn`, `LodCulled`, `Disabled`), the resource
  `LodDistance` (`new`, `meters`) + `LodDistanceError`, the
  `select_lod_presentation` system, and the `accept_f11_b_*` tests below.
- `crates/cs_content/tests/scene.rs` (extend): the `accept_f11_b_*` tests of
  the selection rule.
- Wiring only (no logic): a `crates/cs_app/src/lib.rs` doc paragraph naming the
  F11-B records.

**One observable failure:** if `select_lod_presentation` wrote
`PresentationState::Drawn` for every node whose LOD band covers the distance
without consulting `NodeDisabled`, then
`accept_f11_b_destroyed_wing_and_gun_remain_disabled_across_lod_transition`
fails: when the supplied distance crosses 100 m the newly selected
`wing_lod1` variant comes back `Drawn` while the wing (its parent) and its gun
are destroyed.

## Design decisions

- **Import is `SceneGraph` → ECS, not bytes → `ParsedNode`.** The typed input
  F11-A defined already exists; `import_scene` turns every node into one
  entity carrying `SceneNodeBinding` (stable `scene_node/<container>.<path>`
  id + `SceneGeneration`), `NodeVisualTransform` (the one composed canonical
  transform), an initial `NodePresentation(Drawn)` and, for a `Lod` node,
  `NodeLodVariant`. The authored parent/children links become Bevy `ChildOf` /
  `Children`, so the hierarchy is a real ECS hierarchy. All transforms are
  converted **before** the first spawn: an import either produces the whole
  hierarchy or nothing, so a rejected transform cannot leave half a scene
  behind.
- **`import_airframe` starts from a root reference, never a position.** It
  imports only the subtree under the `SceneRootRef` of an `AirframeVisual`,
  after checking the reference's container is the graph's container and that
  the root exists (`ForeignContainer`, `UnknownNode`). That is the runtime half
  of the F11 deliverable "airframe definitions reference roots in PLANES.ZBD,
  not models selected by array position".
- **LOD groups are siblings.** The `Lod` nodes a parent holds in stored order
  form one *variant group* standing for one physical part at different
  distances (the shape F11-A's own fixture uses: `wing_lod0` / `wing_lod1` as
  siblings). `select_lod_variant` picks exactly one member of such a group:
  1. bands whose `[range_min, range_max]` contains the distance *cover* it;
     exactly one ⇒ `Covered`;
  2. more than one covers it ⇒ `Overlap`, and the tightest band wins (smallest
     `range_max`, then largest `range_min`, then stored order) — adjacent bands
     share their boundary, and the edge belongs to the band that ends there;
  3. none covers it (a gap below, between or above the bands) ⇒
     `GapFallback`: the band whose range is nearest the distance wins (ties →
     lower `range_min` → stored order), so a gap in authored ranges never
     blanks a part out of existence.
  Every choice carries its `LodCoverage`, so a fallback is reported, never
  silent. This rule is **designed engine contract**: which band the original
  picks, whether the original's ranges are inclusive and what `LodInfo::level`
  means are unmeasured (see Unknowns).
- **Presentation is one component, and LOD may only write that component.**
  `NodePresentation` carries `Drawn`, `LodCulled` or `Disabled`.
  `select_lod_presentation` recomputes it from two facts only: the LOD group
  decision at the supplied `LodDistance`, and whether the node or any ancestor
  carries `NodeDisabled`. `Disabled` wins over `LodCulled` — a destroyed node
  stays disabled whichever band becomes active, and every descendant of a
  destroyed node (its gun) is disabled with it. The verdict is **subtree-wide
  on both axes**: a node hangs under a band the group did not choose, it is
  `LodCulled` with that band, so a mesh under the far band is not reported
  `Drawn` while the near band is presented — otherwise every band's mesh of
  one part would be reported drawn at the same time. The system never writes
  `SceneNodeBinding`, `NodeVisualTransform`, `NodeLodVariant` or
  `NodeDisabled`, and it never spawns or despawns: identity, transforms,
  collision and weapon origins cannot move with distance (F11 non-negotiable
  behavior 4).
- **`NodeDisabled` is a marker, damage semantics are not invented here.**
  Destroying a node is done by inserting the component; which hit disables what
  is F29/F11-C's evidence-backed work. This stage only guarantees the state
  survives LOD changes, which is what AC02 asks for.
- **The distance is supplied, not computed.** `LodDistance` is a validated
  resource (`new` rejects non-finite and negative metres; the field is private,
  so the invariant holds everywhere). Deriving a per-node camera distance needs
  the camera/viewer stage (F21/F17 wiring, F11-C); computing it here would
  invent a camera that does not exist yet.
- **Validated constructors keep the system total.** `NodeLodVariant::new`
  runs the very same `select_lod_variant` check over its own band, and
  `LodDistance::new` checks the metre value, so the only values the system can
  see are usable; it converts the rule's `Result` with an `expect` that states
  that invariant rather than swallowing an error. `LodInfo`'s ranges were
  already refused at `SceneGraph::build` (`SceneError::LodRange`).

## Test inventory

| `accept_f11_b_*` test | Covers | Fails when |
| --- | --- | --- |
| `lod_selection_rule_reports_coverage_gaps_and_overlaps` (cs_content/tests/scene.rs) | the rule over bands produced by the real `SceneGraph::build` conversion: `Covered`, the shared-edge `Overlap` tie-break, `GapFallback` above and between bands, stored-order determinism | containment, tie-breaks or the gap fallback are removed or changed |
| `lod_selection_rule_refuses_unusable_input` (cs_content/tests/scene.rs) | `NoVariants`, non-finite/negative distance, reversed and non-finite ranges are refused with their index | any boundary check is dropped, or a bad range is silently accepted |
| `import_scene_spawns_the_hierarchy_with_stable_ids` (cs_app/src/scene.rs) | one entity per node; `ChildOf`/`Children` wiring; binding id + generation; the hand-computed canonical transform in the ECS affine; `NodeLodVariant` ranges in metres; initial presentation | the import skips the hierarchy, drops ids/generations, converts the transform wrongly or forgets LOD data |
| `import_scene_refuses_a_transform_that_cannot_be_rendered` (cs_app/src/scene.rs) | pre-flight conversion: a composed transform that overflows f32 fails the import and leaves the world untouched (all-or-nothing) | conversion errors are swallowed, or a partial hierarchy is spawned before the failure |
| `import_airframe_starts_from_the_root_reference` (cs_app/src/scene.rs) | subtree-only import from an `AirframeVisual`; `ForeignContainer` and `UnknownNode` refusals | the root reference is ignored (whole container imported) or the checks are dropped |
| `lod_inputs_are_validated_at_construction` (cs_app/src/scene.rs) | `LodDistance::new` refuses non-finite and negative metres; `NodeLodVariant::new` refuses an unusable band with the rule's own error | the private-field invariants are opened up, letting unusable values reach selection |
| `destroyed_wing_and_gun_remain_disabled_across_lod_transition` (cs_app/src/scene.rs) | **AC02**: baseline selection at 50 m, destroy the wing and its gun, then cross the 100 m band edge to 150 m; the healthy `tail` group demonstrably switches bands while the destroyed wing group (both variants and the gun) stays `Disabled`; bindings, generations, entities and transforms are byte-identical before and after; re-enabling restores `Drawn`/`LodCulled`; the band meshes swap verdicts with their bands | LOD selection resurrects a disabled node, disables only the currently active variant, reports a mesh under a culled band as `Drawn`, mutates identity/transform, or does not run at all |
| `a_mesh_under_a_culled_band_is_culled_with_it` (cs_app/src/scene.rs) | the subtree half of presentation: at 50 m the near band's mesh is `Drawn` and the far band's mesh is `LodCulled`, at 150 m they swap, so only one LOD level of one wing is ever presented | `LodCulled` is written only for the non-selected band node and not for its descendants |

**Sensitivity check (run while implementing, reverted afterwards).** With the
`NodeDisabled` consultation removed from `select_lod_presentation` the AC02
test fails with `left: Drawn, right: Disabled`; with the system body skipped
it fails with `left: Drawn, right: LodCulled`; with the gap fallback replaced
by "always the first band" `accept_f11_b_lod_selection_rule_reports_coverage_gaps_and_overlaps`
fails on its `GapFallback` index. The tests are not vacuous.

## Review correction (2026-09-30, reviewer of #57)

The submitted implementation wrote `LodCulled` only for the non-selected
`Lod` node itself and left its **descendants** `Drawn`. The fixture's own
shape (`wing_mesh_near` under `wing_lod0`, `wing_mesh_far` under
`wing_lod1`) then reports both LOD levels of one wing drawn at the same
time at every distance — a renderer that reads `NodePresentation` per node
(F17) would draw the near and the far mesh together, and the record
contradicts its own parent band. Fixed in review:

- `select_lod_presentation` now walks the ancestor chain for both facts in
  one pass: `Disabled` (node or ancestor `NodeDisabled`, checked first so
  damage still wins at any depth) and `LodCulled` (node or ancestor is a
  band its group did not choose). The system still writes only
  `NodePresentation`.
- `PresentationState::LodCulled` and the system/module docs now state the
  subtree rule.
- New test `accept_f11_b_a_mesh_under_a_culled_band_is_culled_with_it`, plus
  baseline and post-repair mesh assertions inside the AC02 test. With the
  propagation removed both fail with `left: Drawn, right: LodCulled`
  (mutation applied and reverted during review), so the gap cannot silently
  return.

## Unknowns and limitations (all recorded, none guessed)

- **The GameZ node-array reader does not exist.** `crates/cs_formats/` stops at
  `GameZHeader::nodes_offset`, and no F11 stage owns that crate, so this stage
  can only import a hierarchy built from `ParsedNode` records supplied by a
  fixture or a later reader. **Affected content:** every original airframe and
  world hierarchy — nothing retail (`PLANES.ZBD`, `GAMEZ.ZBD`, `*.ZBD` node
  arrays) can be imported, and F11-D's roster audit and F18's world import
  cannot claim original-data scene hierarchy until it lands. **Resolving
  task:** #392 "Read the GameZ node array into ParsedNode records" (needs the
  owner to grant `crates/cs_formats/` owner paths). This limitation gates every
  scene-hierarchy fidelity claim; it stays in force when this task is marked
  done.
- **LOD grouping is designed, not measured.** That sibling `Lod` nodes form one
  variant group, that exactly one is presented, and the overlap/gap tie-breaks
  above come from this engine's design; the original's selection, its handling
  of overlapping or gapped ranges and the meaning of `LodInfo::level` are
  unmeasured (`level` stays the stored boolean, carried untouched).
- **Inclusive band edges.** `range_min <= d <= range_max` makes adjacent bands
  overlap on their shared edge; the tie-break resolves it deterministically.
  Whether the original's edges are inclusive is unknown.
- **Distance is one number, supplied by the caller.** No camera, no per-node
  distance, no hysteresis (band flipping at an edge is possible by design);
  all of that arrives with the camera/viewer and F11-C wiring.
- **`NodePresentation` is not a renderer state.** Nothing here writes Bevy's
  `Visibility`; mapping the verdict to what the renderer draws is F17's, and
  mapping `NodeDisabled` to weapon/impact behaviour is F11-C/F29's.
- **The mesh association rides along as data, not as a drawable.**
  `SceneNode::mesh` stays on the content record; an imported entity carries
  the node's identity and transform, not a `Handle<Mesh>` — no asset stack
  exists yet (F00-B) and mesh adapters are F17's, so nothing here can be
  rendered by itself.
- **Damage identity across variants.** The scenario disables the *parent* of the
  variant group, which is how this fixture keeps one damage identity over two
  bands. Whether the original stores damage identity on the variant, the parent
  or a `zone_id` is unmeasured (`zone_id`'s domain is still unknown, see the
  F11-A findings); real part→zone bindings are F11-C/D evidence work.
- **`SceneImport` has no teardown.** Live-entity growth across repeated loads is
  AC03 and belongs to F11-C; this stage only reports what it spawned.
- **Fixture scope** — synthetic only; this stage makes no retail compatibility
  claim.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (F11-B
  section, deliverable, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, session generations,
  provenance, explicit unknowns, cycles invalid in ownership hierarchies).
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the `ParsedNode`
  input, the `scene_node` id scheme, the shared visual/collision transform,
  the fixture's sibling-LOD shape, the recorded unknowns this stage inherits).
- `docs/findings/2026-09-29-f10-b-gamez-mesh-layout.md` and
  `crates/cs_formats/src/gamez/mod.rs` (the node array is not read; the
  header's `nodes_offset` / `node_array_size` are where a reader would start).
- `crates/cs_content/src/coordinates.rs` (the F16-A fixture adapter every
  authored distance converts through) and `crates/cs_types/src/{content,space}.rs`
  (identity/provenance records, `Meters`).
- Bevy 0.19 (`bevy_ecs::hierarchy::{ChildOf, Children}`, `Schedule`,
  `Res`/`Resource`, `Query`) — the pinned pair in `Cargo.toml`; the hierarchy
  relationship wiring is documented in `bevy_ecs-0.19.1/src/hierarchy.rs`.

# F17-A: Material classification and render test scene

Date: 2026-09-30. Task: F17-A "Build material classification and render
test scene" (`specs/F17-rendering-material-fidelity-and-scalable-
presentation.md`, stage `### F17-A`). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: ordinary
build/test only — no retail data, no GPU, so nothing here claims original
behavior; the whole design is `designed` by construction and every
fixture declaration carries that `ClaimStatus`.

## Files and the one observable failure (listed before editing)

- `crates/cs_app/src/render/mod.rs` (new): module doc and wiring.
- `crates/cs_app/src/render/material.rs` (new): `MaterialClass`,
  `RenderPhase` (`ALL`, `code`, `depth_sorted`), `Coverage`
  (`from_source`, `has_coverage`), `AddressMode`/`TextureAddress`,
  `DeclaredClass`/`DeclaredClassError`, `MaterialFacts`
  (`declared`, `for_raw_record`), `MaterialUnknown`,
  `ClassificationFailure`, `ClassifiedMaterial`,
  `Classification` (`classified`, `reasons`), `classify`.
- `crates/cs_app/src/render/plan.rs` (new): `DrawItemKey`/`KeyError`,
  `SceneView`/`ViewError`, `DrawItem`/`DrawItemError`, `PlannedDraw`,
  `SortingLimitation`, `DrawPlan` (`build`, `entries`, `limitations`,
  `fingerprint`).
- `crates/cs_app/src/render/golden.rs` (new): `golden_scene` +
  `GoldenScene` (`items`, `view`, `item`, `draw_plan`, `fingerprint`).
- Tests (`crates/cs_app/tests/render/`, selected by `accept_f17_a_`):
  `main.rs` harness, `classification.rs` (8 tests), `golden_scene.rs`
  (8 tests).

**One observable failure:** two overlapping glass panes submitted
near-first must still draw far-first — without back-to-front ordering the
nearer pane blends against whatever the framebuffer holds instead of the
pane behind it, and the scene reads wrong. Equally, a material whose
class nothing establishes — an undeclared GameZ record or one with
unmapped flag bits — silently drawn as opaque would discard its coverage
and turn glass and fence into solid walls.

## Design decisions

- **A class is declared, never derived.** The GameZ material record's
  flag bits name only `textured`/`cycled`/bookkeeping bits; nothing in
  the 40 stored bytes says "glass" or "fence" (F10-C.02 findings). So
  `MaterialFacts.declared` is a `DeclaredClass` — class plus the
  IDENTITY-CONTENT evidence `ClaimStatus` of the assertion — and
  `classify` only checks consistency between the declaration and the
  facts the pipeline established. `MaterialFacts::for_raw_record` shows
  what a stored record alone yields: always `undeclared` (and
  `unknown_flag_bits` when the record carries bits no reference names).
  `Unknown`/`Contradicted` statuses are refused at `DeclaredClass`
  construction: an assertion that asserts nothing is not input.
- **Consistency is checked, not repaired.** `Masked`/`Blended` on
  `Coverage::Opaque` is `class_without_coverage`; on
  `Coverage::Unknown` it is `coverage_unknown`. `Masked` additionally
  needs `AlphaTest::Threshold`: `Unknown` is `alpha_test_unknown` and
  `Disabled` is `masked_test_disabled` (a mask that discards nothing
  contradicts the declaration). Every applicable failure is listed; a
  `Classification::Unclassified` surface can never reach a `DrawItem`,
  because the item's constructor takes `ClassifiedMaterial` — "drawn as
  opaque by default" is a type-level impossibility.
- **Unmeasured presentation stays open.** `MaterialUnknown` mirrors the
  `PresentationUnknown` pattern of `cs_content::textures`: two-sidedness,
  texture addressing and per-corner color meaning are `None`/flagged
  until measured, and `ClassifiedMaterial::is_release_ready` is false
  while any is open. Per-corner colors ride the `DrawItem` bit-exact —
  preserved, not interpreted (`MeshPresentationUnknown::VertexColor`
  carried forward).
- **The plan is a fixed phase order plus one sort.** `RenderPhase::ALL`
  is `Opaque → Masked → Translucent → Additive`; the class→phase map is
  `Emissive|Opaque → Opaque`, `Masked → Masked`, `Blended → Translucent`,
  `Additive → Additive`. Opaque and masked keep authored submission order
  (material ordering preserved); the two depth-sorted phases stable-sort
  back-to-front by `SceneView::depth` (dot against the normalized
  forward), and equal-depth pairs keep submission order **and** produce a
  `SortingLimitation::EqualViewDepth` row — the report non-negotiable #2
  demands instead of hiding a pane. `DrawPlan::fingerprint` pins phase,
  key and depth of every entry.
- **The golden scene is scrambled on purpose.** `golden_scene` submits
  `[sprite, glass_near, percorner, fence, glass_far, ground]`; only a
  correct plan produces the pinned order `[percorner, ground, fence,
  glass_far, glass_near, sprite]`. The sprite's depth (5.6 m) sits
  between the panes (6.2/5.0 m), so the test proves phase order, not
  depth, decides across phases. Two fingerprints are pinned: the authored
  scene (`5ec2d447…`) and the planned order (`413f2b94…`).
- **`SceneView` is the smallest camera.** A position plus normalized
  forward gives the depth the sort needs; F21 owns real cameras. The
  module is Bevy-free: F17-B owns the mesh/image-to-Bevy adapters and
  `crates/cs_app/assets/shaders/` stays empty until then.

## Recorded unknowns and limitations

- **Which original surfaces were glass/masked/additive is unmeasured.**
  Nothing in the GameZ record establishes a render class; real content
  will need evidence-backed declarations (a per-material class table
  built from observed original behavior) — that evidence collection and
  the original screenshot matrix are F17-D's scope, and until it lands
  every real-material classification carries `Designed`/`Inferred`
  status, never `verified_original`.
- **The class→phase map and phase order are new-engine design.** The
  original's pass structure is unknown; the contract fixes a deterministic
  order so AC01 is testable, marked `Designed`.
- **Interpenetrating translucent surfaces are not detected.** Equal view
  depth is reported; true interpenetration has no object-level order and
  detecting it needs geometry the contract stage does not model —
  recorded as future work for the runtime stages.
- **`Emissive` draws in the opaque phase.** Whether the original treated
  emissive surfaces as a separate pass is unmeasured; emission is modeled
  as a shading property, not coverage.
- **Unknowns needing no new task:** all of the above are the declared
  scopes of F17-B/C/D (adapters, profiles, original screenshot matrix) or
  already tracked upstream (`MeshPresentationUnknown`,
  `PresentationUnknown`), so `create_tasks` was not used.

## Review follow-up (devin-1, fresh context)

- `SceneView::new` normalized `forward` with a naive `sqrt(f·f)` in
  `f32`: components near `f32::MAX` overflowed the norm to `inf`, which
  would silently store a zero forward (a direction the constructor
  promises to reject), and subnormal components underflowed to `0` and
  were refused despite having a direction. Normalization is now
  scale-first (`f/max` before the norm), which cannot overflow or
  underflow for finite inputs.
- `SceneView::depth` now accumulates in `f64` and clamps to the `f32`
  range, so two finite-but-huge coordinates saturate to a finite depth
  instead of leaking `inf`/`NaN` into the depth-sorted phases (`NaN`
  would evade both `total_cmp` ties and the `==` equal-depth report).
- New test `accept_f17_a_view_normalization_survives_extreme_scales`
  pins both behaviors.

## Mutation verification (run, then reverted)

- `phase_entries.sort_by` removed from `DrawPlan::build` →
  `accept_f17_a_translucency_sorts_back_to_front`,
  `accept_f17_a_golden_scene_orders_glass_fence_sprite_and_corner_colors`
  and `accept_f17_a_golden_plan_fingerprint_is_pinned` fail (submission
  order leaks through). Reverted.
- The `Undeclared` check in `classify` replaced by a fall-back to
  declared-opaque → `accept_f17_a_undeclared_material_is_never_opaque`
  and `accept_f17_a_raw_record_alone_cannot_classify` fail (silent opaque
  classification returns). Reverted.

## Commands run

All from the repository root on branch
`rally/69-build-material-classification-and-render`, Rust 1.98.

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_f17_a_ --include-ignored` | 0 (16 tests: 8 classification, 8 golden scene/plan) |

## Wiring edits (outside owner paths, logic-free)

- `crates/cs_app/src/lib.rs`: `pub mod render;` plus a doc paragraph.
  No `Cargo.toml` change: `render` uses only the crate's existing
  dependencies (`cs_assets` for `install::sha256`, `cs_formats` for
  `AlphaSource`/`AlphaTest`/`RawMaterialRecord`, `cs_types` for
  `ClaimStatus`/`ContentHash`).

`crates/cs_app/assets/shaders/` was not populated: shaders belong to the
F17-B Bevy adapters; this stage defines contracts only.

No protected path, original datum or binary file is involved.

## Sources

`specs/F17-rendering-material-fidelity-and-scalable-presentation.md`
(F17-A section, AC01, non-negotiables 1–2),
`docs/contracts/IDENTITY-CONTENT.md` (evidence classes, stable ids,
fingerprint kind),
`docs/findings/2026-09-29-f10-c-02-gamez-material-records.md` (the GameZ
record establishes no render class),
`crates/cs_content/src/mesh.rs` `MeshPresentationUnknown` (front-face
winding, UV origin, vertex-color meaning still open) and
`crates/cs_content/src/textures.rs` `PresentationUnknown` (the unknowns
pattern this stage mirrors), `docs/01-ARCHITECTURE.md` (cs_app is the
rendering owner crate).

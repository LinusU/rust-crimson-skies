# F09-C: Model instances and the construction preview

Date: 2026-09-29. Task: F09-C "Wire faction/custom paints into model instances
and construction preview"
(`specs/F09-bm-multilayer-liveries-and-paint-composition.md`, section
`### F09-C`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/livery.rs`: the F09-B module gains the production
  cache map F09-B deferred — public `variant_key`, `LiveryVariantStore`
  (`new`, `len`, `is_empty`, `key_for`, `contains`, `compose`, `get`,
  `remove`, `retain`, `clear`); `compose_livery` now stamps its key through
  `variant_key`.
- `crates/cs_app/src/livery.rs` (new): the consumer — `LiverySession`,
  `ModelInstanceId`, `PaintChoice` (`Faction` / `Custom`), `ModelLivery`,
  `ConstructionPreview`, `LiveryTeardown`, `LiveryError` and
  `LiveryRuntime` (`new`, `session`, `require_session`, `livery`,
  `composed`, `image`, `bind`, `preview`, `commit`, `release`,
  `evict_unreferenced`, `teardown`), plus its module doctest.
- `crates/cs_app/src/lib.rs` (wiring only): `pub mod livery;` and one
  module-doc paragraph.
- `crates/cs_content/src/lib.rs` (wiring only): the `livery` module doc now
  covers the F09-C store.
- `crates/cs_app/Cargo.toml` (wiring only): `cs_content` and `cs_formats`
  become production dependencies because `src/livery.rs` depends on them;
  `cs_content` is dropped from `[dev-dependencies]` (it was listed there only
  because the F50-A/M01-A integration tests in `tests/campaign/` use it, and
  normal dependencies are visible to test targets).
- `Cargo.lock` (wiring only): the dependency edges above.
- `crates/cs_formats/src/bm.rs`, `crates/cs_formats/tests/bm.rs`: **not
  touched**. F09-C adds no new BM layout or composition behavior, so the
  format crate and its tests are unchanged.
- This file.

**One observable failure:** two model instances that share one source image
but choose different faction paints resolve to the same stored variant, so the
second plane renders (or displays a preview of) the first plane's colors.
`accept_f09_c_two_planes_share_source_but_keep_their_faction_colors` and
`accept_f09_c_construction_preview_does_not_touch_the_committed_livery` fail.
The mutation probe below shows this directly: keying variants on the source
only (ignoring the paint colors) fails 7 of the 8 task tests.

## Sources

- `specs/F09-bm-multilayer-liveries-and-paint-composition.md`, `### F09-C`
  (wire the implemented path into its actual producer and consumer; include
  teardown/retry and error propagation; prefix `accept_f09_c_`; minimum
  scenario = AC03) and the non-negotiable list, especially #3 (faction,
  custom colors, decals and airframe determine the variant) and #5 (cache
  keys include all color, mask, decal, source and algorithm-version inputs;
  switching factions must not mutate other aircraft).
- `docs/contracts/IDENTITY-CONTENT.md`: stable content/actor ids, session
  generations, and content produced once and referenced, not copied.
- F09-B:
  `docs/findings/2026-09-28-f09-b-deterministic-layered-composition.md`. That
  stage produced `LiveryVariantKey` and `compose_livery` and explicitly
  deferred "the per-instance cache map" to F09-C.
- F09-A:
  `docs/findings/2026-09-28-f09-a-bm-layout-and-rectangular-fixture.md`
  (`BmFile`, canonical orientation, stored planes).
- `crates/cs_formats/src/bm.rs` (`BM_COMPOSITION_VERSION`, `PaintColor`,
  `BmComposite`, `BmFile::compose`) and `crates/cs_content/src/livery.rs`
  (the F09-B production path this stage consumes).
- Downstream consumer: `specs/F44-aircraft-construction-budgets-loadouts-and-paint-editor.md`
  (F44-B builds the paint editor on top of this binding).

All fixtures are newly authored synthetic bytes built inside the tests;
nothing is derived from original game data, and no palette is guessed.

## The wired path

**Producer (content layer).** `LiveryVariantStore` is a
`HashMap<LiveryVariantKey, ComposedLivery>`. `compose(file, paint, budget)`
computes the key with `variant_key`, composes once on a miss and inserts,
and returns the stored bytes without recomposing on a hit. A different paint
has a different key, so it inserts a *second* entry and never rewrites the
first. A failed composition propagates the `BmError` and stores nothing.
Entries leave only through `remove`, `retain` or `clear` — never as a side
effect of a lookup.

**Consumer (app layer).** `LiveryRuntime` owns one store plus a
`HashMap<ModelInstanceId, ModelLivery>` for one `LiverySession`:

- `bind` requires the session, composes the choice through the store and
  records `ModelLivery { instance, choice, key }`. The instance resolves to
  bytes only through `image` -> store lookup, so two instances sharing a
  source but choosing different paints resolve to their own variants and
  neither binding changes the other.
- `preview` requires the session and a bound instance, composes a *candidate*
  choice into the store (so the bytes are readable through `composed`) and
  returns a `ConstructionPreview` **without** touching the committed
  `ModelLivery`. `commit` applies a preview only if the instance is still
  bound and the preview's variant is actually stored here.
- `release` unbinds one instance; `evict_unreferenced` drops exactly the
  variants no bound instance references; `teardown` clears both maps and
  reports the counts. That is teardown/retry: a refusal leaves no partial
  state, and a retry with a fitting budget starts from the same state.

**Paint provenance.** `PaintChoice::Faction { faction, paint }` pairs a
faction catalog id (`ContentKind::Faction`) with the colors a caller supplies;
`PaintChoice::Custom(paint)` is a player paint. `PaintChoice::paint()` returns
the `LiveryPaint`, so the store is keyed on the colors alone and a custom
paint equal to a faction's colors reuses the same variant. No palette is baked
in: the authoritative faction table is F09-D original-data work
(non-negotiable #4).

## Design decisions

- **Producer/consumer split by crate.** The pure, Bevy-free cache map lives in
  `cs_content::livery`; instance identity, preview and lifecycle live in
  `cs_app::livery`. `cs_content` and `cs_formats` stay renderer-free
  (`docs/01-ARCHITECTURE.md`), and no game state is hidden in UI or renderer
  code.
- **Keys cover every distinguishing input.** `variant_key` reuses F09-B's
  `source_fingerprint` (dimensions, base, three masks and the overlay, plus
  any unsupported tail) and adds the three colors and
  `BM_COMPOSITION_VERSION`. A changed mask, overlay, color or algorithm is a
  different key; only changed variants are composed.
- **Instances reference keys, never own pixels.** `ModelLivery` stores the
  variant key, so repainting one instance cannot mutate a shared image and
  two instances can share one composed variant by design.
- **Session generation is enforced.** Every compose, preview and commit
  refuses a session that did not open the runtime (`ForeignSession`), so a
  livery bound for a finished mission is never served after a world switch.
  A same-numbered second runtime also cannot commit a preview whose variant it
  never composed (`UnknownVariant`), which keeps keys resolvable to bytes.
- **Explicit release, no silent eviction.** `release` deliberately does not
  evict; the caller decides when to `evict_unreferenced`. This matches the
  contract's "produced once and referenced" model and keeps the preview's
  candidate variant readable until the caller is done with it.
- **Wiring-only dependency change.** `cs_app` moves `cs_content` to a normal
  dependency (and adds `cs_formats`) because production code now needs them;
  no logic was added to `src/lib.rs` or `Cargo.toml`.

## Test inventory (`accept_f09_c_*`)

| Test | Covers |
| --- | --- |
| `accept_f09_c_store_keeps_two_paints_of_one_source_distinct` (cs_content) | one source, two paints -> two keys, two variants; composing the second must not change the first's bytes or key |
| `accept_f09_c_store_reuses_and_releases_variants_explicitly` (cs_content) | a repeated request is one variant with identical bytes; `retain`/`remove`/`clear` drop only the intended entries |
| `accept_f09_c_store_refuses_over_budget_without_partial_state` (cs_content) | an over-budget composition stores nothing, leaves the existing variant intact, and a fitting retry succeeds |
| `accept_f09_c_two_planes_share_source_but_keep_their_faction_colors` (cs_app) | AC03 minimum scenario: one source, two planes, two faction paints; each plane's bytes equal an independent `compose_livery` of its own paint; repainting A does not mutate B |
| `accept_f09_c_construction_preview_does_not_touch_the_committed_livery` (cs_app) | preview composes and exposes a candidate variant while the committed livery and its bytes are unchanged; `commit` applies it and keeps both variants |
| `accept_f09_c_release_and_teardown_drop_only_unreferenced_variants` (cs_app) | `release` drops a binding but not variants; `evict_unreferenced` drops only unreferenced ones; `teardown` reports and drops everything |
| `accept_f09_c_failed_composition_leaves_no_stale_state_and_retry_succeeds` (cs_app) | over-budget `bind` binds and stores nothing, retry succeeds, a foreign session is refused on `bind`/`preview`/`require_session`, an unknown instance cannot be previewed |
| `accept_f09_c_commit_refuses_a_preview_without_its_variant_or_instance` (cs_app) | a preview cannot be committed into a runtime that did not compose its variant, nor after its instance is released; refused commits bind nothing |

Eight tests total (3 in `cs_content`, 5 in `cs_app`), all calling the
production `LiveryVariantStore`/`LiveryRuntime` path. `expected_rgb` in the
app tests calls `cs_content::livery::compose_livery` independently of the
runtime, so the assertions compare the runtime against the production
composition path rather than against the runtime's own output.

## Mutation probes

Each mutation was applied on this branch, the task tests run with
`cargo test -p <crate> --lib -- accept_f09_c_ --include-ignored` (and the
workspace prefix selection) and the file restored; no probe text remains in
the tree.

| Mutation | Failing tests |
| --- | --- |
| `variant_key` ignores the paint colors (source-only key) | 7: the 3 `cs_content` store tests + `two_planes_share_source`, `construction_preview`, `release_and_teardown`, `commit_refuses` |
| `preview` applies the candidate to the instance immediately | 1: `construction_preview_does_not_touch_the_committed_livery` |
| `preview` drops the unknown-instance guard | 1: `failed_composition_leaves_no_stale_state_and_retry_succeeds` |
| `commit` drops the stored-variant check | 1: `commit_refuses_a_preview_without_its_variant_or_instance` |
| `evict_unreferenced` becomes a no-op | 1: `release_and_teardown_drop_only_unreferenced_variants` |

## Recorded unknowns

- **Retail agreement.** This stage wires the F09-B *observed tool* algorithm
  into instances; it inherits F09-B's unknowns (exact mask weights, rounding
  and overlay alpha convention). Nothing here is verified against the
  original renderer; that is F09-D, which needs `retail` and an actual
  original run.
- **Palette provenance.** `PaintChoice` carries no palette. Which colors a
  faction really uses, which combinations are valid and how decals/airframe
  select a variant are original-data questions (F09-D); a caller-supplied
  color set is not evidence of the retail palette.
- **Instance identity and selection UI.** `ModelInstanceId` is a new-engine
  runtime id; how a scene assigns ids, how the paint editor (F44-B) lists
  factions and how the renderer (F17-B) consumes the composed variant are
  later stages. This stage only guarantees the binding, preview, commit and
  release semantics.
- **No GPU consumer yet.** The runtime hands back `ComposedLivery` bytes; no
  stage uploads them to a GPU texture here, so "renders the right colors" is
  established at the composed-bytes level, not on screen.

# F14-D baseline inventory: collections and totals

This file is the **single place** that states which collections the retail
baseline inventory (`cs_content::catalog::baseline::retail_baseline`) holds and
how many rows each has. The per-stage `f14-d-*` findings notes record each
collection's provenance, layout and unknowns; they do **not** restate the
inventory's totals, because every later collection made an earlier note's totals
stale. They point here instead.

Measured with `cs-inspect catalog --cs-path "$CS_GAME_DIR"` (capability `retail`,
read-only): the `collections` object of the report `baseline_report_json`
renders, on `main` at `0911c8c5a`, 2026-10-27. The numbers below were copied from
that report, not computed by hand.

Two tests keep this file honest (`crates/cs_content/tests/accept_f14_d_notes.rs`,
prefix `accept_f14_d_notes_`):

* without original data, every `ContentKind` label must appear in exactly one of
  the two tables below, the table rows must sum to the stated total, and no other
  F14-D note may state an inventory total of its own;
* with `CS_GAME_DIR` (ignored in CI), the populated table must equal the
  `collections` object of the report measured from the installation.

When a stage adds a collection, add its row here (and nothing about totals in the
stage's own note). Re-measure and update the numbers in the same change.

## Populated collections

<!-- totals:begin -->
| collection | rows | added by | provenance note |
| --- | --- | --- | --- |
| `install_file` | 228 | F14-D | `2026-09-29-f14-d-retail-baseline-inventory.md` |
| `mission` | 24 | F14-D | `2026-09-29-f14-d-retail-baseline-inventory.md` |
| `script` | 53 | F14-D | `2026-09-29-f14-d-retail-baseline-inventory.md` |
| `ia_scenario` | 8 | F14-D.1 | `2026-10-02-f14-d-1-reader-archive-directories.md` |
| `multiplayer_scenario` | 21 | F14-D.1 | `2026-10-02-f14-d-1-reader-archive-directories.md` |
| `multiplayer_rules` | 4 | F14-D.2 | `2026-10-02-f14-d-2-multiplayer-rules-collection.md` |
| `world` | 8 | F14-D.3 | `2026-10-03-f14-d-3-world-collection.md` |
| `scene_node` | 56620 | F14-D.4 | `2026-10-03-f14-d-4-scene-and-mesh-collections.md` |
| `mesh` | 17139 | F14-D.4 | `2026-10-03-f14-d-4-scene-and-mesh-collections.md` |
| `faction` | 11 | F14-D.5 | `2026-10-03-f14-d-5-faction-and-paint-mask-collections.md` |
| `paint_mask` | 184 | F14-D.5 | `2026-10-03-f14-d-5-faction-and-paint-mask-collections.md` |
| `airframe` | 11 | F14-D.6 | `2026-10-03-f14-d-6-airframe-collection.md` |
| `sound` | 4951 | F14-D.7 | `2026-10-03-f14-d-7-sound-cue-collection.md` |
| `scrapbook_item` | 461 | F14-D.8 | `2026-10-03-f14-d-8-stunt-scrapbook-collections.md` |
| `stunt` | 45 | F14-D.8 | `2026-10-03-f14-d-8-stunt-scrapbook-collections.md` |
| `material` | 4669 | F14-D.9 | `2026-10-03-f14-d-9-material-collection.md` |
| `engine` | 38 | F14-D.10 | `2026-10-11-f14-d-10-engine-gun-collections.md` |
| `gun` | 31 | F14-D.10 | `2026-10-11-f14-d-10-engine-gun-collections.md` |
| `image` | 3985 | F14-D.10 | `2026-10-11-f14-d-10-image-collection.md` |
| `video` | 5 | F14-D video | `2026-10-12-f14-d-video-collection.md` |
<!-- totals:end -->

**Total: 88496 rows in 20 collections.**
Coverage: 53 declared roots, 159 reachable rows, 88337 unreachable rows.
The coverage denominator is the launchable content only; the rows of every collection after `script` are counted as unreachable unknowns in `coverage.unreachable_by_kind`.

## Kinds with no rows

Every other `ContentKind` holds no row in the baseline today:

<!-- empty:begin -->
* `collision_surface`
* `armor`
* `ammo`
* `hardpoint_equipment`
* `blueprint`
* `pilot`
* `voice`
* `instruction`
* `native_binding`
* `animation_track`
* `camera_track`
* `objective`
* `trigger`
* `route`
* `music`
* `dialogue`
* `font`
* `string_resource`
* `ui_resource`
* `ia_preset`
* `custom_plane`
* `loadout`
* `weapon`
<!-- empty:end -->

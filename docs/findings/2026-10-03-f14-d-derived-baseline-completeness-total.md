# The shared baseline completeness total derives its non-launchable collections

Date: 2026-10-03. Task #584, a fix to the shared completeness check of stage
`### F14-D` of `specs/F14-canonical-content-catalog-and-dependency-closure.md`.
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_f14_d_`.

## The defect

`accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
counted every catalog row that is neither an inventoried file nor a launchable
row or its program, and required that count to equal a hand-written sum over the
collections that existed when the sum was written: `multiplayer_rules`, `world`,
`airframe`, `faction`, `paint_mask`, `scene_node`, `mesh`, and then `sound`. A
collection that inserts rows therefore had to remember to edit a list in another
stage's test, and forgetting it failed a **retail** test on somebody else's
branch rather than when the collection landed. That happened twice:

| Landed | Broke | How the failure arrived |
| --- | --- | --- |
| F14-D.6 (airframes) | F14-D.4 | fixed in #487's second review pass |
| F14-D.7 (sound cues) | F14-D.4 | fixed in #487's third review pass |

Both times the cost was a review pass, and
`docs/findings/2026-10-03-f14-d-4-scene-and-mesh-collections.md` (item 14,
"The third pass") already named the root cause: "a shared completeness total
that every new collection must remember to update is what both passes tripped
over".

## The number the task description pins was stale

The task text quotes the failing equality on `main` as **5 169 against 218**.
That was true of `main` at the time #584 was written (2026-10-03T10:18Z), before
#487's third pass landed. At the start of this task `origin/main` was
`67201343` ("record the acceptance evidence on the rebased tree"), which already
contains the sound accounting, so the retail test was green before any change
here — `git log` confirms `6645a0de fix(f14-d.4): name the sound rows in the
inventory's completeness total` is an ancestor of the branch this task started
from.

The defect was therefore **structural, not red**: the check was a list to
remember, and it had just demonstrated twice what that costs. The restatement
below is the measurement taken on this installation during this task, not a
number taken from the description.

## Measured on `$CS_GAME_DIR`, 2026-10-03

| Quantity | Value | Where it comes from |
| --- | --- | --- |
| inventoried regular files | **228** | `cs_assets::install::discover` |
| declared launchable roots | **53** | 24 campaign missions + 8 IA + 21 MP |
| program rows | **53** | one per launchable row |
| catalog rows | **79 262** | `Catalog::len()` |
| non-launchable collection rows | **78 928** | derived, see below |
| coverage reachable / unreachable | 159 / 79 103 | `Baseline::coverage` |

The 78 928 breaks down as `scene_node` 56 620, `mesh` 17 139, `sound` 4 951,
`paint_mask` 184, `airframe` 11, `faction` 11, `world` 8, `multiplayer_rules`
4. `music`, `dialogue`, `material`, `image` and every other kind hold no row;
the first two are read and reported empty with a diagnostic by F14-D.7.

## The shape chosen

`cs_types::content` gains the classification the total was missing:

- `CatalogRowRole` — `InstallFile`, `Launchable`, `Program`,
  `SourceDerivedCollection`, with `ALL`, `label()` and `is_launchable()`.
- `ContentKind::baseline_row_role()` — a `match` over all 43 variants **with no
  catch-all arm**. This is the guarantee: adding a `ContentKind` for a new
  collection does not compile until it declares which role it plays, so the
  failure lands with the collection instead of on a later rebase.
  `ContentKind::is_launchable()` is now defined *as* that role
  (`matches!(self.baseline_row_role(), CatalogRowRole::Launchable)`), so the
  launchable predicate and the accounting cannot drift.
- `account_catalog_rows(rows) -> CatalogRowAccounting` — totals rows by role and
  records `rows_by_kind`, `collections()` (the source-derived breakdown in
  canonical kind order, derived from the rows' kinds and not from
  `ContentKind::ALL`, so the two cannot disagree) and a `Display` that renders
  the whole accounting, which is what the failure messages interpolate.

The retail test's block now reads: every row has exactly one role
(`is_complete`), the accounting counts every row and no other, the install-file
total is production discovery's file count, the launchable total is the declared
denominator, the program total equals it (an orphan program row is a defect),
and `catalog.len() == install_file + 2 * launchable + unaccounted`. The
`unaccounted` total **is** `source_derived`, so no list has to name it.

The measurements are recorded in the test as **floors**, not equalities:

- `install_rows == 228` and `launchable == 53` are equalities — the original
  installation is fixed data, and both were already derived from production
  discovery and the campaign walk, so this pins the derivation.
- `MEASURED_COLLECTIONS` gives each measured collection as "at least this many
  rows", and the total is `unaccounted >= 78_928`. A later collection raises
  them without failing here, which is the whole point; a collection that loses
  rows or disappears still fails, and the message names every collection present.

## What demonstrates it

`accept_f14_d_a_collection_named_by_no_list_is_still_accounted` (non-retail, so
CI runs it) builds a real baseline over the synthetic tree, inserts one row of
each of two collections the old list named (`world`, `sound`) so the old shape
starts out correct, then inserts **five rows of `music` and `image`** — kinds
nothing in this stage uses and that appear in no list in this file. On the same
catalog it then asserts:

- `hand_summed_collections(...) == 2` while `after.unaccounted() == 7`, with
  `assert_ne!` between them: the shape this task removes would fail here.
- `after.is_complete()`, the four role totals, the completeness identity, and
  `after.collections() == [("world", 1), ("image", 2), ("sound", 1),
  ("music", 3)]`, so the new rows are named in the breakdown.

Two mutations were run against the finished tree to confirm the tests are not
decorative:

1. Deleting the `InstallFile`/`Script` arms of `baseline_row_role` does not
   compile — `error[E0004] non-exhaustive patterns`, i.e. the property is
   enforced by the compiler, not by a test.
2. Replacing `CatalogRowRole::Program => accounting.program += 1` with `= 0`
   fails `accept_f14_d_catalog_rows_are_accounted_by_kind_and_not_by_a_named_list`
   (`left: 0, right: 1`) and both non-retail `cs_content` tests, with messages
   like `12 rows: 8 install_file, 1 launchable, 0 program, 2 source-derived
   (world 1, sound 1)`.

## Scope notes

- No production behaviour changed: `is_launchable()` returns what it returned
  before, so every other F14-D suite is unaffected. The full workspace suite and
  every `accept_f14_d` test, retail halves included, pass on this tree.
- This task makes no new claim about original content, so it produces no
  acceptance.json: the measurements above are a restatement of the existing F14-D
  acceptance, whose evidence record is `docs/findings/evidence/F14-D.7.json`.
- What the change does **not** catch: a collection stage that inserts rows of a
  *launchable* kind without declaring them (the launchable total stops matching
  `catalog.launchable_count()`), or one that inserts a `Script` row that no
  launchable row reads (the program total stops matching the launchable total).
  Both now fail in this test rather than silently enlarging the inventory.

## Sources used

- `specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
  `### F14-D`, non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` ("collections cannot exclude failed
  entries"; unreachable unknowns stay in the global accounting report).
- `docs/contracts/CLI-EVIDENCE.md` ("a mutation/removal of the implementation
  would make the test fail"; the task-prefix discovery rule).
- `docs/findings/2026-10-03-f14-d-4-scene-and-mesh-collections.md`, item 14 of
  "The third pass" (the root-cause statement this task exists to remove).
- `docs/findings/2026-10-03-f14-d-7-sound-cue-collection.md` (the 4 951 sound
  rows and the empty `music`/`dialogue` collections).
- `crates/cs_types/src/content.rs` (`ContentKind`, `CatalogElement`) and
  `crates/cs_content/src/catalog/{mod,baseline}.rs` (`Catalog`,
  `retail_baseline`, `Coverage`).

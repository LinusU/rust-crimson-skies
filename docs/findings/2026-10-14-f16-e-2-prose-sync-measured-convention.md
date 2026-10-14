# #719 (`F16-E-2`): the world-import and M01-LC prose now describe F16-E's measured convention

Date: 2026-10-14. Task #719, the follow-up #390 (`F16-E`) filed when it
recorded its landmarks: three prose statements outside #390's owner paths
still called the GameZ convention's axis map, handedness and angle unit
unmeasured/`declared`, and the M01-LC world-unit-roles evidence report still
described a state #716 had superseded. Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`
(`### F18-B`, `### F18-D`); shared contract `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: `retail` (re-running the acceptance suite and the evidence
harness over `$CS_GAME_DIR`) and ordinary build/test. Nothing here is
`verified_original`: the quantities remain `observed_tool` artifact
measurements whose behavior landmark needs an original run (#358,
REF-OWNER-FIRST-CAPTURE). **This task changed no assertion, constant, gap
list, claim class or test expectation** — only sentences and the generated
evidence report, plus one new non-retail test that pins the sync.

## Files

- `crates/cs_app/src/world/retail.rs` — `RetailWorldContainer::definition`'s
  doc said the retail `retail_gamez` choice has "its other quantities stay
  declared". It now says the axis map, handedness and angle unit are measured
  by static analysis of the owner-supplied decrypted image (F16-E, #390) at
  `observed_tool` at best, each keeping a gap only because no behavior
  landmark exists until an original run supplies one (#358) — no landmark is
  missing.
- `crates/cs_content/src/world.rs` — the `WORLD_UNIT_UNMEASURED` doc already
  said the axis convention is measured (that part landed with #716); it now
  also names F16-E's calibration landmarks and states *why* the gap list still
  names those quantities: each is a gap **only for want of its behavior
  landmark (#358, REF-OWNER-FIRST-CAPTURE), not because no landmark exists**.
- `crates/cs_app/tests/evidence_report_m01_lc_world_unit_roles.rs` — the
  `review.method` template is rewritten and the census it derives is extended:
  - the convention half now says the axis map, handedness and angle unit are
    measured by static analysis (F16-E #390's artifact landmarks,
    `observed_tool` at best) and that the source's gap list names them only
    for the missing behavior landmark (#358);
  - the split half now records the three-way split: the no-geometry half
    resolves `None` (`UNINDEXED_RECORD_STORES_NO_GEOMETRY`), the `{fog}`
    records carrying the measured `fvol*` prefix resolve `None` as fog volumes
    (`FOG_VOLUME_RECORD_NEVER_BLOCKS`, #716), and the remaining `{unresolved}`
    geometry-bearing records keep `UNINDEXED_ROLE_UNMEASURED`;
  - the spawn sentence now reports what the acceptance suite measures: anchors
    and fog volumes are presented and never block and are not skips, no
    substitute geometry was introduced, and c1c's spawn reports **no**
    `unknown_collision_role` at all;
  - limit (3) is no longer "which `fvol*` volumes are sensors, triggers or
    decoration is still unmeasured" — the `fvol*` volumes resolve as fog
    volumes from their measured consumer (#716); what stays unmeasured is
    whether the 2000 engine ever intersected a fog box, which of them a script
    can switch at run time, and which gameplay query the candidate walk serves
    (#771, #358);
  - limit (4) is no longer "the axis map, … are declared, not measured": they
    are measured by static analysis (#436's owner note, F16-E #390), at
    `observed_tool`, never `verified_original`; `unit_class` still reports the
    scale quantity's class only and `axis_class` reports the applied axis map's;
  - `render_census` now returns `CensusTotals` (`containers`, `fog`,
    `unresolved`, summed from the per-container production reports), so the
    `{fog}` and `{unresolved}` counts in the prose are interpolated from the
    same production run rather than written down, and the census artifact
    records `objects_unindexed_fog` per group plus `unindexed_fog` /
    `unindexed_unresolved` totals. No reported number changed meaning; the
    fog class was simply not recorded before.
- `crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs` — one new non-retail
  test, `accept_m01_lc_world_unit_roles_the_synced_prose_matches_the_measured_record`,
  reads the three sources with `include_str!` and fails when the sync is
  reverted: `retail.rs` must not say "stay declared" and must name F16-E/#390
  and #358; `world.rs` must not call the rest of the convention unmeasured and
  must name REF-OWNER-FIRST-CAPTURE; the harness template must not contain any
  of the five stale sentences and must name
  `FOG_VOLUME_RECORD_NEVER_BLOCKS`, the `axis_class` accessor, #358 and the
  `objects_unindexed_fog` census field. It exercises no production logic — it
  is the wording pin the task asked for; every other assertion in the suite is
  unchanged and still exercises the production import path.
- `docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json` — regenerated with the
  harness's own steps on the reviewed tree (below), validated with
  `tools/validate_evidence.py --require-pass`. The report's `claim` stays
  `implemented`; `unknowns` stays empty because everything the new prose names
  as open is a limit on the claim, not an unresolved measurement. The
  `review.identity` records this regeneration and that the implementer and
  reviewer claims recorded in the older report stand as written.
- This file.

## What did **not** change

Every `quantity_status`, gap entry, `unit_class`, `axis_class`, split count
and spawn expectation the suites pin is byte-identical in behavior:
`accept_m01_lc_world_unit_roles_the_scale_is_measured_and_the_convention_stays_open`
still asserts the three quantities are gaps with `quantity_status(Unknown)`,
`claim_status()` is `Unknown` and never `VerifiedOriginal`, and the per-
container split table (`SPLITS`) is untouched. The sync is about what the
sentences *say* those facts mean: a gap for a missing behavior landmark is a
different statement from "no landmark exists".

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_m01_lc_world_unit_roles --include-ignored
#   6 tests discovered under this prefix, all passing: 3 run everywhere
#   (including the new prose pin), 3 retail tests run locally under CS_GAME_DIR.
python3 tools/validate_evidence.py private/evidence/M01-LC-WORLD-UNIT-ROLES/acceptance.json \
  --artifact-root private/evidence/M01-LC-WORLD-UNIT-ROLES --require-pass
```

Results are recorded in the handover summary for task #719.

## Evidence

`docs/findings/evidence/M01-LC-WORLD-UNIT-ROLES.json` (the harness's second
artifact, `world-unit-roles-census.json`, and the recorded
`cargo-test.log` stay in `private/`). The report is a record of the run at its
own candidate tree; a reviewer regenerates it on the rebased commit and
compares.

## Limits

1. **Retail is file access.** No original executable ran. The quantities
   remain `observed_tool` at best and the report's claim stays `implemented`;
   nothing here is `verified_original` or `checked` (a merge awards `checked`).
2. **The prose pin is a pin, not a measurement.** The new test reads sources
   at compile time; it guarantees the sentences stay synced, and every
   measurement behind them stays in the tests that already re-observe them
   over the installation.
3. **The behavior landmark is still the only thing missing** for the three
   quantities, and it needs an original run (#358, REF-OWNER-FIRST-CAPTURE),
   which no agent can supply.

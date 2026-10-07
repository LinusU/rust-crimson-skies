# #716: the `fvol*` records are the engine's fog volumes, and the world axis convention is measured identity

Date: 2026-10-07. Task #716 `M01-LC-FVOL-ROLES` ("Measure the runtime role of the
`fvol*` volume records and the world axis convention"), the `world_geometry`
measurement `VS-M01-RUNTIME` (#359) was blocked on. Feature sheet:
`specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`, stages
`### F18-B` (the import) and `### F18-D` (evidence over the installation); this
is also #677's stated follow-up ("a follow-up measurement of the volume-class
records' runtime consumption"). Shared contracts:
`docs/contracts/IDENTITY-CONTENT.md` and, for the report,
`docs/contracts/CLI-EVIDENCE.md`. Capabilities used: **`retail`** (read-only
access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and `audio` were
available and **not used**: nothing is rendered or played and no original run
happened, so nothing here is `verified_original`.

## Files

- `crates/cs_content/src/world.rs` (extended):
  - [`FOG_VOLUME_RECORD_NEVER_BLOCKS`] — an `fvol*` record is a fog volume:
    presented, never blocking, never reporting a contact. The unindexed branch
    of `import_world_container` now has a name-classified arm before the
    geometry arms, so such a record resolves to `WorldCollisionRole::None`
    with its **mesh kept as a known reference** (the box is still drawn) and
    its shape left an explicit unknown under this claim id. The two classes
    task #677 measured keep their own arms and their own claim ids.
  - [`WORLD_AXIS_CONVENTION_MEASURED`] — the axis convention, measured by
    static analysis of the owner-supplied decrypted image (task #436's owner
    note of 2026-10-05): identity axis map, `+Y` up, right-handed, radians in
    GameZ binaries, at `observed_tool` and never `verified_original`.
  - `WorldImportReport` grew `objects_unindexed_fog()`,
    `partition_records_fog_volume()` (the measured overlap between the fog
    measurement and the index rule), `axis_map()`,
    `axis_map_preserves_orientation()`, `angle_unit()`, `rotation_sense()` and
    `axis_class()`. `WORLD_UNIT_UNMEASURED`'s doc now names both measurements
    instead of calling the convention unmeasured.
- `crates/cs_app/tests/world/fvol_roles.rs` (new): this task's acceptance
  tests (`accept_m01_lc_fvol_roles_`) — the synthetic half drives the
  production import over a fixture that carries all four classes (an indexed
  `fvol*`, an unindexed `fvol*`, a geometry-bearing record with no measured
  prefix, an anchor) and all three axis evidence classes; the retail half
  pins the per-container census and the axis report over all eight containers.
- `crates/cs_app/tests/world/main.rs`, `crates/cs_app/tests/world/import_retail.rs`,
  `crates/cs_app/tests/world/world_units.rs`,
  `crates/cs_app/tests/accept_m01_lc_world_unit_roles.rs`: the counts those
  suites pinned as "geometry-bearing ⇒ unknown" are now the three-way split,
  and the spawn reports no `unknown_collision_role` on c1c. The fixture record
  that keeps the unknown is deliberately spelled `volume`, not `fvol*`, so the
  discriminator survives.
- `crates/cs_app/tests/evidence_report_m01_lc_fvol_roles.rs` (new): the
  evidence harness for `M01-LC-FVOL-ROLES`, whose second artifact is a second
  production run of the import over all eight containers.
- `docs/findings/evidence/M01-LC-FVOL-ROLES.json`: the committed report.
- This file.

## One: what the image consumes an `fvol*` record for

Provenance: static analysis of the owner-supplied decrypted image
`$CS_GAME_DIR/crimson.decrypted.exe` (sha256
`43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`) — the same
image and the same owner note the F04-D finding and task #390's owner note
work from. Addresses are virtual addresses (image base `0x400000`, file offset
= VA − `0x400000` below `0x643000`). The method here was a byte search for the
prefix, a disassembly of the referencing routine (Python `capstone`) and a
parse of the PE import directory; no decompiler was used, no image bytes and
no decompiled code are committed — only addresses, two short strings and
behaviour.

**The prefix is compared exactly once in the whole image.** The bytes `fvol`
occur once (VA `0x6249f4`), in the string table immediately before
`fogvol.zrd`, and are referenced once, at VA `0x44e087`:

```
0x44e085: push 4
0x44e087: push 0x6249f4        ; "fvol"
0x44e08c: push edi             ; the node's own name
0x44e08d: call dword ptr [0xa20348]
```

The IAT slot `0xa20348` resolves through the PE import directory to
`MSVCRT.dll!strncmp` (import name table `0x620184`, entry 113), so the call is
`strncmp(name, "fvol", 4)` — the engine's rule is a **four-byte name prefix**,
and this conversion mirrors it byte for byte.

**The consumer is the fog system, and it is one routine.** The comparison sits
in a loop over a linked list of records inside the routine that starts at VA
`0x44d9d0`; the loop reads each record's first field as the string it compares
(the same layout the GameZ info slot has: name first), and a match allocates a
0x70-byte record, stores that name in it (`mov [esi], edi` at VA `0x44e0b6`)
and appends the new record to a container. The *same* routine then opens
`fogvol.zrd` (`call 0x579c60`, the open the F04-D finding documents) and reads
exactly the keys that document holds — `clutter` (with `weight`, `nodes`,
`far_fade_range`, `perp_dist_range`, `perturb_dist_range`, `scale_range`),
`distance`, `fog_zone`, `fog_fade_dist`, `interior_fog_fade_dist` and
`fog_color`. Its caller (VA `0x44e6f0`) takes a point, asks the routine for a
value and compares that value against the very globals the routine
initialises (`0x64feec`, `0x64ff04`) — a distance/fade computation, not a
contact test.

**The data side agrees, three ways.**

* `fogvol.zrd` is present in **all eight** groups' `zrdr.zbd` (352–853 bytes
  each, among 25–70 members), and its keys are the keys the routine reads.
* **No `.zrd` member anywhere in the installation spells `fvol`** — 1 293
  members scanned across every `zrdr.zbd` — so no mission, objective or
  script document names these records either. That is #677's negative
  measurement, re-taken over the whole archive set.
* The groups whose `fogvol.zrd` carries the `fog_zone`/`clutter` document
  structure are exactly the groups that hold `fvol*` records (below); the
  three groups with no `fvol*` record carry the reduced document.

### The corpus census, through the production readers

| container | `fvol*` records | of which the grid names | world-owned, unindexed ⇒ fog | unindexed geometry, no prefix ⇒ unknown | unindexed, no geometry ⇒ `None` |
| --- | ---: | ---: | ---: | ---: | ---: |
| c1  |  9 | 0 |  9 |  1 | 56 |
| c1b |  0 | 0 |  0 |  0 | 78 |
| c1c | 21 | 4 | 17 |  0 | 36 |
| c2  |  0 | 0 |  0 |  0 | 24 |
| c2b |  9 | 0 |  9 |  0 | 39 |
| c3  |  0 | 0 |  0 |  0 | 14 |
| c4  |  9 | 0 |  9 |  4 | 38 |
| c5  | 17 | 2 | 15 | 70 | 20 |
| **total** | **65** | **6** | **59** | **75** | **305** |

The three right-hand columns partition every world-owned unindexed record
(59 + 75 + 305 = 439), so the new rule replaces part of #677's unknown
population rather than re-labelling it: **#677's "134 geometry-bearing
unindexed records" are 59 `fvol*` records plus 75 records no measured prefix
names**, and the second group is what `UNINDEXED_ROLE_UNMEASURED` still covers.
c1c's blocker number is exact: its 17 unresolved records were all `fvol*`, and
they are all fog volumes now.

## Two: what was bound into `import_world_container`

1. **Fog volumes.** An unindexed record whose stored display name starts with
   `fvol` resolves to `WorldCollisionRole::None` under
   `FOG_VOLUME_RECORD_NEVER_BLOCKS`; its mesh stays a known mesh reference and
   its shape is an explicit unknown with that claim id, so a consumer can tell
   "no collider because the store states no geometry" from "no collider
   because the record is a fog volume". `spawn_world` needed no change: it
   already presents a `None` role and builds no collider, so c1c's spawn
   report goes from `unknown_collision_role: 17` to zero, and its
   `non_colliding` population goes from 36 to 53.
2. **The index rule is untouched, and its disagreement is counted.** Six
   `fvol*` records (c1c's first four, c5's first and third) *are* named by the
   partition grid, so they keep `Solid` under `INDEXED_RECORD_IS_STATIC` and
   `partition_records_fog_volume()` reports the overlap. The two measured
   statements disagree about those six records and this task has no
   measurement that settles it — see the limitation below.
3. **The axis convention.** The report states the axis map it applied
   (`"identity"`, or the spelled permutation), whether it preserves
   orientation, the source convention's angle unit and rotation sense, and
   `axis_class()`: `ObservedTool` when an installation-backed source applied
   exactly the identity map #436 measured, `Contradicted` when such a source
   applied anything else (the measurement and the applied map disagree), and
   `Unknown` for a designed or synthetic source. The measurement is #436's
   owner note of 2026-10-05 — static analysis of the same decrypted image:
   atmosphere on `position.y` and gravity on `−y` (`0x48fc40`, `0x48ff88`) for
   `+Y` up, a proper view matrix and `−Z` camera forward (`0x53afc0`,
   `0x53b9a0`) for right-handed, yaw from x/z in the Euler decomposition
   (`0x53df30`), and radians in GameZ binaries (corpus maximum exactly π).
   Class: code-derived, `observed_tool`, never `verified_original`.

## Unknowns and limitations (recorded, not guessed)

- **Whether the 2000 engine ever intersected a fog box is UNMEASURED.** The
  resolution is a designed rule over a measured consumer: the only measured
  consumer of an `fvol*` record is the fog routine, and nothing measured
  reports a contact for one. **Affected content:** collision over the 59
  world-owned fog volumes in every world container, and therefore over M01's
  world. **Resolving task:** the follow-up below.
- **How the original engine built world collision at all is UNMEASURED**, and
  the six grid-named `fvol*` records are where it shows: the fog measurement
  says "not a collider" and `INDEXED_RECORD_IS_STATIC` says `Solid`, and both
  statements are about records that exist. **Affected content:** those six
  records (c1c: 4, c5: 2), and every claim that a grid-named record is static
  collision geometry rather than streamable content. **Resolving task:** filed
  as a follow-up with this task; it needs the original's collision
  construction measured from the image or an original run (#358).
- **The convention and the `fvol` classification are code-derived static
  analysis, at `observed_tool`.** No original executable ran, so no landmark
  is a behavior landmark and nothing reaches `verified_original` until #358
  supplies a run. The `fvol` rule was measured on **this** installation: it
  keys on stored display names, so an installation whose records were renamed
  would need the census re-run.
- **What the fog system does with a volume it is inside is not measured
  here** — whether its inside test is a render-only matter or reaches
  gameplay. Nothing in the routine reads a contact or a trigger; that is as
  far as this measurement goes.
- **`fogvol.zrd`'s `clutter` section is named, not interpreted.** Its keys are
  reported as the keys the routine reads; what each value means is a separate
  question (the F19 atmosphere stage owns fog semantics).
- **Nothing derived from the original bytes is committed.** The numbers above
  are counts, dimensions and relations; the strings quoted are the class
  prefix `fvol`, the document name `fogvol.zrd` and the document's key names.
  No name list, no mesh and no screenshot is in the repository.

## Sources used

- `crates/cs_formats` GameZ node-array reader (`GameZNodes`, `RawNode::name`,
  `mesh_index`, `info.unk140`) and `WorldPartitionGrid` — the census above was
  taken through these production readers, re-run by
  `accept_m01_lc_fvol_roles_every_container_fog_split_is_measured`.
- The decrypted image at `$CS_GAME_DIR/crimson.decrypted.exe` (read-only,
  byte search + `capstone` disassembly + PE import directory) and the retail
  `zrdr.zbd` archives through `read_reader_archive` + `decode_zrd`.
- `crates/cs_content/src/world.rs` (`import_world_container`,
  `WorldImportReport`, `WorldCollisionRole`, `stored_extent`) and
  `crates/cs_app/src/world/spawn.rs` (`spawn_world`) — read, **not** modified.
- `docs/findings/2026-10-05-m01-lc-world-unit-roles.md` (#677: the split this
  task resolves), `docs/findings/2026-10-04-m01-lc-world-import.md` (#629: the
  world-owned set), `docs/findings/2026-10-05-f04-d-original-lookup-order.md`
  (the image's provenance, address convention and `open` routine), and the
  owner note of 2026-10-05 on #436 for the axis convention.

## Relationship to other tasks

- **#677's finding** called this the resolving task; its "Unknowns" bullet now
  points here.
- **#719 (`F16-E-2`)** syncs three prose statements with F16-E's measured
  convention. One of them — `WORLD_UNIT_UNMEASURED` in
  `crates/cs_content/src/world.rs` — is already updated here, because leaving
  it saying "the rest of the convention is unmeasured" would contradict this
  task's own report. The other two (`cs_app/src/world/retail.rs`'s
  `retail_gamez` doc and #677's evidence-report template) are untouched and
  still #719's to sync; the committed `M01-LC-WORLD-UNIT-ROLES.json` still
  describes #677's measurement at its own candidate tree, which is what an
  evidence report is for.
- **#359 (`VS-M01-RUNTIME`)**: the `world_geometry` verdict on its branch is
  still `Unknown` with the #677 wording; with these two measurements bound,
  that verdict's detail is stale and its owner should re-measure it.

## Commands run

```sh
cargo fmt --all -- --check                                           # exit 0 (before and after the rebase)
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings   # exit 0 (before and after the rebase)
cargo test --workspace --locked                                      # exit 0
cargo test --workspace --locked -- accept_m01_lc_fvol_roles --include-ignored   # exit 0
#   4 tests discovered under this prefix, all passing: 3 run everywhere,
#   1 retail test run locally under CS_GAME_DIR.
python3 tools/validate_evidence.py private/evidence/M01-LC-FVOL-ROLES/acceptance.json \
  --artifact-root private/evidence/M01-LC-FVOL-ROLES --require-pass   # exit 0
```

The branch was then rebased on `origin/main` (no conflict; main's commits
touch none of the files this branch changes and no `Cargo.toml`/`Cargo.lock`,
per the owner's 2026-10-01 merge-race directive), so the re-push ran the
lighter set — `fmt`, `clippy` and the prefix run, all exit 0 — and the
evidence report was regenerated on the rebased tree (`1de7feab`) with the same
prefix run.

Evidence: `docs/findings/evidence/M01-LC-FVOL-ROLES.json` (the harness's
second artifact, `world-fvol-roles-census.json`, is the production re-run of
the import over all eight containers and stays in `private/`).

Reviewed by a separate agent instance before merge; no agent review replaces
the owner's approval, and nothing here is `verified_original`.

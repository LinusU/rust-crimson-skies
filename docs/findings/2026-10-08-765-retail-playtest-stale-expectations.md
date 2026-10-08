# #765 `PLAYTEST-RETAIL-PREEXISTING-FAILURES`: two expectations two merged features invalidated

Found 2026-10-08 while reviewing #752, on this host (`CS_GAME_DIR` set,
`CS_CAPABILITIES=retail,gpu,audio`). Both tests are `#[ignore] =
"requires CS_GAME_DIR"`, so CI never runs them and `main` was red here
unnoticed. Both reproduce with **no local change at all** (branch cut from
`origin/main`, `bd5f8a38`), so neither is a regression of #752.

## Determination

**In both cases the pinned expectation is what is wrong; the production code is
right.** In both cases a merged feature changed behaviour after the expectation
was written, and the `#[ignore]`d test that pins it was not updated with it.
No production line changes in this task; no assertion was deleted or weakened.

| test | guilty side | the feature that invalidated it | fixed by |
| --- | --- | --- | --- |
| `playtest_retail::…c1c_area_and_bloodhawk_mesh_spawn_and_capture` | the pin (`mesh_records` 401, `triangles` 8 673) | #753 / `310f23fe` "Select one intact variant and LOD band per part of the playtest area" | re-pinned to the drawn set, with `stored_bindings` taking over the old 401 meaning |
| `playtest_full_aircraft::…parts_follow_the_single_flight_body` | the expectation "nothing wrote part N's local transform" | #710 / `fba05e81` "Spin the playtest's drawn propeller with the engine" | the expected local is read from the component that writes it |

## 1. `mesh_records` 295 against a pin of 401

Reproduced exactly as reported:

```text
panicked at crates/cs_app/tests/playtest_retail.rs:557:5:
assertion `left == right` failed: how many of those nodes bind a mesh the container stores geometry for
  left: 295
 right: 401
```

Evidence that 401 is the **stored** count and 295 the **drawn** count:

* The pin was written in `d981db71` ("Render one original c1c area …"), the
  commit that created the test, when #648's behaviour drew every binding of the
  subtree.
* `310f23fe` (#753) then added the selection and changed the field's own
  documented meaning, without touching this test file (`git log` on
  `crates/cs_app/tests/playtest_retail.rs`: last content change before this
  task is `fba05e81`, then `9023f28e`, which only moved the flicker tests in).
  `PlaytestAreaReport::mesh_records` now reads *"How many of them bind a mesh the
  container stores geometry for **and are drawn**: the selected intact variants.
  Each has a collider."*, and a new field `stored_bindings` holds *"How many
  mesh bindings the subtree stores before the selection."*
* `docs/findings/2026-10-07-playtest-area-flicker.md`: *"The `piratezep`
  subtree (node 517, 793 nodes, 401 mesh bindings) … The selection now draws one
  of each; 401 stored bindings split into drawn plus hidden with a reason each,
  and the collider count equals the drawn count."*
* `docs/PLAYTEST-RETAIL.md` §"The airship draws one intact variant of each part
  (#753)" and `docs/PLAYTEST.md` say the same: 401 mesh bindings are stored, and
  "colliders follow the drawn set".
* The already-green
  `accept_playtest_area_flicker_retail_the_drawn_set_holds_one_variant_per_part_with_a_collider_each`
  pins `area.stored_bindings == 401` and
  `area.mesh_records + area.undrawn.len() == area.stored_bindings` on this same
  host, which fixes `mesh_records` at 295 and the hidden list at 106.

So the assertion's own sentence — *"how many of those nodes bind a mesh the
container stores geometry for"* — describes the new `stored_bindings` field, and
`mesh_records` moved on to mean the drawn set. The test now pins **both**:

* `area.stored_bindings == 401` (the subtree's stored bindings, unchanged);
* `area.mesh_records == 295` (drawn, one intact variant per part at the
  documented 300 m design distance);
* `area.mesh_records + area.undrawn.len() == area.stored_bindings` (every stored
  binding is drawn or listed with a reason);
* `area.triangles == 8_110` for the drawn set, **rebuilt in the test** from the
  container with `stored_render_mesh` over every spawned record before the pin is
  asserted, so a report that disagreed with the reader fails on its own numbers.
  8 673 − 8 110 = 563 stored triangles live in the 106 hidden bindings.

`area.nodes == 793`, `area.refused`/`area.gaps` empty, `objects == colliders ==
mesh_records` and the `793`/`401` subtree pins all still hold; nothing about the
reader or the spawn changed.

## 2. part 2541 "drift" against a pin of an untouched local

Reproduced exactly as reported, deterministically (same two values every run):

```text
part 2541 drifted from body pose x composed local: -0.7644531 vs 0.11122215
```

Evidence that the expectation, not the pose, is what changed:

* `PLAYTEST_AIRCRAFT_PROP_NODE_SLOT == 2541` (`staticprop1`): the one part that
  drifts is exactly the drawn propeller, and no other part does. The
  `global == body × composed local` check passed for the other 15 bindings.
* `fba05e81` (#710) added `propeller::spin_propellers`, which writes **the
  propeller child's own local `Transform`** every frame while the engine runs.
  This is the documented design, not a side effect:
  `docs/PLAYTEST-RETAIL.md` — *"only the propeller child's own local
  `Transform` is written, never the flight body's pose"* — and
  `crates/cs_app/src/playtest/propeller.rs` — *"writes the **propeller child's
  own local [`Transform`]** … It never writes the flight body's pose (AGENTS
  rule 7, one pose owner)."*
* `accept_playtest_prop_spin_*` (green on this host and in CI) pins the same
  behaviour from the other side: at zero revolutions the drawn placement *is*
  the spawn placement, and the disc then turns about the measured hub while the
  hub stays put.
* The selection work of #753 (`310f23fe`) cannot produce this: a selection
  decides **which** bindings are drawn, once, before the first frame, while the
  observed failure is a rotation of one part's local after 120 frames of flight.
  The only system in the tree that writes an aircraft part's local `Transform`
  after spawn is `spin_propellers`.
* `spawn_retail_parts` (in `playtest/scene.rs`) attaches `PropellerSpin` to that
  one child with `PropellerSpin::from_hub(&spec.hub, base)`, where `base` is the
  very `part.oriented(rotation)` this test composes as `local`.

The test's blanket *"nothing wrote part {slot}'s local transform"* therefore
contradicted a shipped, documented, separately tested feature, and has been
stale since `fba05e81`.

The check is now per part:

* every part's global must still be `body_global × expected local` (the same
  2e-3 tolerance, the same finiteness check, still over input, flight, reset and
  teardown);
* for the drawn propeller the expected local is `PropellerSpin::transform()` —
  production's own composition of its measured hub and the turn drawn so far —
  and the test first asserts `spin.base() ==` the authored placement, so the
  spin may not silently replace where the part was spawned;
* for every other part the expected local is still the authored placement, and
  the assertion that nothing else wrote it is unchanged.

The test still fails if the spin system stops writing, writes something other
than its own transform, attaches to another child, or if anything at all writes
a non-propeller part's local.

## Commands and results (this host)

```sh
git checkout --no-track -B rally/765-two-retail-ignore-playtest-tests-fail-on origin/main

cargo test -p cs_app --test playtest_retail \
  accept_playtest_retail_retail_c1c_area_and_bloodhawk_mesh_spawn_and_capture -- --include-ignored
#   FAILED: mesh_records left 295, right 401 (before the fix)

cargo test -p cs_app --test playtest_full_aircraft \
  accept_playtest_full_aircraft_parts_follow_the_single_flight_body -- --include-ignored
#   FAILED: part 2541 drifted … -0.7644531 vs 0.11122215 (before the fix)

cargo test -p cs_app --test playtest_retail accept_playtest_retail_ --include-ignored
cargo test -p cs_app --test playtest_full_aircraft accept_playtest_full_aircraft_ --include-ignored
#   after the fix: green (see the task's handover summary for exit codes)
```

Not the provisioned retail data: the same installation and the same tree answer
401 stored bindings / 295 drawn / 8 110 drawn triangles consistently across the
three suites that read them (`playtest_retail`, `playtest_retail_launch`,
`area_flicker`), and 793 nodes and 401 bindings have been stable on this host
since the subtree was first measured.

## Follow-up noticed, not fixed here

`docs/PLAYTEST.md` §"What is provisional" describes the flown airship as
"`piratezep`, 401 mesh records, 8 673 triangles", which is the **stored**
subtree; since #753 the drawn set is 295 records / 8 110 triangles. The number
is not wrong as a subtree measurement and the #753 section immediately below it
explains the selection, so this is a wording ambiguity rather than a defect; it
is recorded here rather than edited in a task that only owns the two tests.

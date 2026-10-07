# F18-E: world placement decoded, vertex unit applied, routes still unstated

Date: 2026-10-07. Task: #436 (`F18-E-world-placement-and-traversal`). Feature
sheet: `specs/F18-world-geometry-terrain-water-and-traversable-interiors.md`.
Capabilities: `retail`, ordinary build/test (`gpu` ran only the existing F18-D
capture tests). No original run; nothing here is `verified_original`.

## What changed

`cs_app::world::audit` now reads each world group's node array with the
production `read_gamez_nodes` (same parse context as the mesh and material
readers) and reports `PlacementSource::Decoded { placed_objects }`, the number
of node records whose stored `mesh_index` resolves to a present mesh slot
(`NodeMeshBindings::of(&nodes, &meshes).resolved`).
`vertex_scale_to_m` is `Some(1.0)`, read from
`CoordinateSource::retail_gamez(..).convention().meters_per_unit()` (task #677's
measured unit, `observed_tool`; axis convention code-derived per the #436 owner
note of 2026-10-05, never `verified_original`).

| group | stored node records | resolved mesh placements (`placed_objects`) |
| --- | --- | --- |
| c1 | 7 064 | 3 966 |
| c1b | 5 603 | 3 485 |
| c1c | 5 644 | 3 354 |
| c2 | 4 956 | 2 558 |
| c2b | 4 901 | 3 039 |
| c3 | 5 408 | 2 868 |
| c4 | 8 289 | 4 929 |
| c5 | 11 438 | 6 003 |

Both `TraversalBlocker`s are gone from every group because their facts exist.
`accept_f18_d_retail_every_discovered_world_group_is_visited_and_compared` was
changed deliberately to assert the new verdict; `accept_f18_e_retail_...` pins
the counts above.

## What is NOT met

**Routes and stunt openings are still not located, in all eight groups.** The
audit reports one `NoRouteMeasured` gap per group and all five opening classes
(`tunnel`, `arch`, `building_opening`, `hangar`, `stunt_passage`) unlocated.
No measured rule says what such an opening is in placed geometry. Node names
(`hangerdoors`, `gate1`, `pedbridge1`, ...) exist but are authored object names,
not a statement that a passage exists or how wide it is; classifying by name or
by mesh shape would be a guess, which AGENTS rule 4 forbids.

**Affected content:** every traversal route, stunt-critical opening and
clearance in all eight world groups; F18's "preserve tunnels, arches, building
openings, hangars and stunt passages" remains unmet for original data. This
limitation gates every F18 world-geometry fidelity claim. **Resolving task:**
#732 (`F18-E.1`) — measure the original's traversal-opening and route rule,
then locate openings and measure routes in this audit (needs evidence such as
the original's own opening or flight-path data).

The retail trigger-volume survey (`survey_retail_trigger_volumes`,
`RetailTriggerVolumeSurvey`) still constructs with `vertex_scale_to_m: None`
deliberately: whether that consumer should carry the now-measured factor is a
separate decision, filed as #733 (`F18-E.2`).

Also seen: the first run of `accept_f18_d_a_gpu_capture_...` once failed with
`No such file or directory` creating `private/evidence/F18-D/` (two GPU tests
race to create it); a rerun passed. Not touched here.

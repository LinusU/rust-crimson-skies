# F18-B follow-up #423: sector loads no longer clone the WorldDefinition

## Decision

`ResidentWorld` holds `Arc<WorldDefinition>`. `load_world` makes the one copy
it needs (its argument is a `&WorldDefinition`); `load_sector` takes an
`Arc::clone` of the handle instead of copying the definition out of the
`WorldResidency` resource. Spawning needs `&mut App` while the record lives in
the world, which is why a handle is needed at all.

## Measurement

Test: `accept_f18_b_sector_reload_of_a_large_world_keeps_every_record`
(`crates/cs_app/tests/world/sector_load_cost.rs`), run with `--nocapture` in a
debug build on the owner's development host. Synthetic world: 8 sectors x 500
solid cuboid instances = 4000 objects, empty mesh source.

| quantity | cost |
| --- | --- |
| `WorldDefinition::clone()` at 4000 objects (the cost removed from every sector load) | 1.13 ms |
| `load_sector` of 500 objects (the clone included, before this change) | 11.4 ms |

The clone was about a tenth of a sector load at this size and grows linearly
with the world's object count, not with the sector's. The change is kept
because it removes that cost with a three-line edit and changes nothing a load
means. Release-build figures and a retail-sized world (F18-D) are not measured
here; the numbers are a synthetic debug-build estimate, not retail evidence.

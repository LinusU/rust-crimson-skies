# F49-D.03 re-key (#2089): presets launch the descriptor's default mission type

Follow-up of #2037 (`2026-10-24-f49-d-03-fu-rules-type-order.md`, claim
`f49.d.03.ia-type-order`, evidence class `static`).

## Decision

* `directory_presets` still emits one preset per menu row (30), each with its
  own identity (`ia_preset` id, scenario id, `original_mission_type()` = the
  menu row's type). Identities are not collapsed: the menu offers the rows.
* Every preset also carries `launched_mission_type()`: the descriptor's
  `mission_type` (`IaDirectory::default_mission_type`), the type the engine
  runs. A descriptor without a recognised `mission_type` keeps the record
  initialiser's reset value `dogfight_ace` (`0x458ff0`).
* The roster (`directory_roster`: wingman template, named ace) and the
  victory rules (`measured_ia_victory_rules`) are bound on the launched type.
* The 22 non-default rows are therefore **the same launch as the default row**
  of their directory: identical roster and rules. Only their identity differs.
* In retail no descriptor defaults to `dogfight_ace`, so the named ace is never
  seated and the wingmen are `wingman_base` for `dogfight_squadron` directories
  and `wingman` for `stunt_flying`/`zeppelin_run`.

Unmeasured, unchanged from #2037: whether the menu enables a row whose type
differs from the default, and what the player sees.

Tests: `accept_f49_d_03_*` in `crates/cs_app/tests` (retail catalog, rules end,
rules roster, intensity) re-keyed on `launched_mission_type()`; the synthetic
roster test asserts a non-default row has the default row's roster.

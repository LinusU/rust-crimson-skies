# T452-R.03-D: equipment-rule census over the sixteen catalogued records

Date: 2026-10-28. Task #2167 (parent #2161). Capability used: `retail` (read of
`weapons.zrd` only; no engine image, nothing was run). Provenance: `static`,
never `verified_original`.

## Result

`cs_content::ordnance::read_original_equipment_rule_census` reads the records of
the component table (the sixteen catalogued records) and reports every
top-level key whose name contains `REQUIRE`, `PERMIT`, `ALLOW`, `FORBID`,
`PROHIBIT`, `EXCLUDE`, `NEED` or `EQUIP`.

- Records covered: 16.
- Union of top-level keys: exactly the 41 keys of `MEASURED_ROCKET_KEYS`
  (`accept_t548_retail_blast`); the guns' additional keys (`CALIBER`, `CANNON`,
  `FIRING_HEAT`, ...) are outside the sixteen.
- Requirement/permission keys: none.

So no catalogued record states which equipment an airframe must carry or must
not carry; the empty rule set (`DeclaredEquipmentRules::default()`) is the
measured value for all sixteen, not a gap.

## Not claimed

- The key-name fragments are a search heuristic: a requirement spelled by a key
  without any of them would not be found, but the key union is pinned exactly,
  so a reviewer can read all 41 names and see none states one.
- Whether the engine applies compatibility rules from another source (the
  airframe record, the shop screens) is outside this census.
- Stack capacity, unit mass and media ids still leave
  `t452.limit.runtime_lowering` standing (T452-R.03-B / -C and later).

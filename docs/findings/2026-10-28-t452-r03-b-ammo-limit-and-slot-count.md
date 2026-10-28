# T452-R.03-B: `AMMO_LIMIT` is parsed into an unread float; the slot stack is `CLUSTER_SIZE`

Date: 2026-10-28. Task #2165 (parent #2161). Capabilities used: `retail`, static
reading of `$CS_ENGINE_IMAGE` (base 0x400000). Provenance of every row:
`static`, never `verified_original`. Nothing was run. Rows are in
`data/owner-static/engine-facts.toml` (`[[ammo_limit_fact]]`); no disassembly is
reproduced.

## Result

1. **Parser field.** The `weapons.zrd` record parser (entry `0x5ad630`, called
   from `0x4647ef`) reads `AMMO_LIMIT` at `0x5ad925` (`call 0x57a130`, which
   converts an integer or float token to a float) into the float at
   `record + 0x18` (record stride `0x214`, list `0xa1d78c`).
2. **Default.** `0x5ad805` stores `0x42480000` (50.0) there before any key is
   read. A record without `AMMO_LIMIT` keeps 50.0.
3. **Consumer.** No reader of `record + 0x18` as `AMMO_LIMIT` was found. The
   whole zweapon module (`0x5ac000..0x5af000`) only writes it; the 18 callers
   of the by-name lookup `0x5abfd0` and the 2 callers of the by-id lookup
   `0x5ac050`, scanned 200 instructions on, read `+0x18` only of other
   structures (the `record + 0x210` object's `BEEPER` time, and a different
   object at `0x4c0f06`/`0x4c107c`). This is a negative static search, not proof
   of absence: an access through a pointer copied elsewhere is not covered.
4. **The stack count is `CLUSTER_SIZE`.** The per-type callback `0x4ba6f0`
   (invoked at `0x5ae0f7`) allocates a zero-filled `0x38`-byte object into
   `record + 0x210` and stores `CLUSTER_SIZE` at its `+0xc` (`0x4baa17`). That
   is the dword `0x4bad90` returns and `0x4b2550` installs as a slot's count
   (T452-R.03-A). So the count is **per slot** (every slot holding the record
   gets the same number, not a lifetime total across slots), it is `CLUSTER_SIZE`,
   and it is **0 when `CLUSTER_SIZE` is absent**. This resolves the "which key
   fills `(record + 0x210) + 0xc`" item left open in
   `2026-10-28-t452-r03-rocket-type-map-and-purchase-totals.md` section 5:
   `AMMO_LIMIT` is not that key.

## Retail check (`cs_content::ordnance::read_original_ammo_declarations`)

- Gun records (`wep_00`..`wep_03`, `wep_30`.., `wep_130`..): `AMMO_LIMIT` equals
  `CLUSTER_SIZE` (2800, 2400, 2000, 1600, 1200), hence the HUD number matching
  `AMMO_LIMIT` found earlier; the engine path reads `CLUSTER_SIZE`.
- The eleven selectable rocket records `wep_05`..`wep_15` all declare
  `CLUSTER_SIZE` (stack 1..6); only `wep_12` also declares `AMMO_LIMIT` (100),
  and its stack is 2.
- `wep_04`, `wep_25`, `wep_26`, `wep_28` declare `AMMO_LIMIT` 100 with
  `CLUSTER_SIZE` 1; `wep_27` 9999 with 1. `wep_23` and `wep_29` declare 9999 and
  **no** `CLUSTER_SIZE`, so their slot count is 0.

The engine does not equate `AMMO_LIMIT` with `CLUSTER_SIZE`; they coincide in
data on guns only.

## Not established

- What, if anything, uses the parsed `AMMO_LIMIT` float (and so what 50.0 means
  as a default or what 100 / 9999 mean on ordnance). The negative scan above is
  the only evidence. Until a reader is found no port behaviour may be derived
  from `AMMO_LIMIT` on ordnance records.
- The runtime consumption of the count after installation (decrement, reload).
- Remaining parent items: `FIRE`/`FLYOUT`/`IMPACT`/`SOUND_NAME` media mapping
  and the equipment-rule census (T452-R.03 siblings).

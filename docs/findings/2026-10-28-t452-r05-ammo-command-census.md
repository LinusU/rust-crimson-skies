# T452-R05-AMMO-SCRIPT: no retail INTERP script uses the `ammo` command

Date: 2026-10-28. Task #2173 (follow-up of #2170, T452-R.05). Capability used:
`retail` (read of `ZBD/interp.zbd`, `ZBD/zrdr.zbd` and a signature scan of the
installation; no engine image read here, nothing was run). Provenance:
`static`, never `verified_original`. No original script text is committed; this
note carries counts and command names only.

## Question

#2170 measured that no plane-record rocket slot can hold `wep_04`, `wep_24`,
`wep_25`, `wep_26` or `wep_27`, and that the one writer able to install an
index outside that map is the script command `ammo` (two arguments: slot,
index; one argument: all eight rocket slots). Does any retail script run it?

## Result

`cs_content::ordnance_ammo::read_original_ammo_census` decodes the
installation's INTERP container through `cs_formats::interp::decode_interp`
and resolves every line whose head is `ammo` (ASCII case-insensitive).

- Scripts inspected: 98 (`ZBD/interp.zbd`). Lines inspected: 5083.
- `ammo` commands: **0**. Installed indices: none. Slots written: none.
- Therefore `wep_04`, `wep_24`, `wep_25`, `wep_26` and `wep_27` are installed
  by no retail script, in any slot.

The container is the only file of the installation carrying the INTERP
signature (`0x08971119`, version 7; a byte scan of every installed file found
it once, at offset 0 of `ZBD/interp.zbd`). The container's 85 distinct command
heads are loader and scene commands (`FindNode`, `LoadGameGen`, `NodeSetActive`,
`CycleTextureSetMap`, ...); `ammo` is not among them. The mission and
instant-action archives (`ZBD/C*/M*`, `ZBD/C*/IA1`, `ZBD/C*/MP*`) hold reader
members (`aiv.zrd`, `egen.zrd`, `ia.zrd`, ...) and none contains the bytes
`ammo` in any case. In `ZBD/zrdr.zbd` the bytes occur only as the record keys
`AMMO` and `AMMO_LIMIT`, `NO_AMMO_WARNING` and one `ammosign` name, never as a
command head. In the animation and model containers the only occurrence is the
node name `char_ammo`.

## Affected content

`wep_25` (and `wep_04`, `wep_24`, `wep_26`, `wep_27`) has no runtime
availability through a retail loadout: neither a plane record nor a retail
script can put it in a slot. The `ammo` command stays implemented in the engine
for scripts that do not ship; this crate parses it
(`resolve_ammo_line`, both arities, out-of-range slot and index reported) so a
mod script that uses it is not silently dropped.

## Not claimed

- The command table that holds `ammo` may be reachable from a source other than
  the INTERP container (a console, a network message); this census reads
  scripts only.
- How the engine's argument reader (`0x5b7980`) spells a number is not
  established; the parser accepts plain decimal digits and reports anything
  else as unreadable.
- The index-to-record spelling `wep_<NN>` (two digits minimum) is the naming of
  the `weapons.zrd` records; that the engine indexes records the same way is
  #2170's static claim.

## Evidence

`docs/findings/evidence/T452-R05-AMMO-SCRIPT.json`; tests
`accept_t452_r05ammo_*` in `crates/cs_content/tests/`.

# M01-LC-FC-B: where the Flight Check's gun and rocket lists come from (task #1670)

Date: 2026-10-19. Slice of #1661 (`PARITY-M01-LC-FLIGHTCHECK`), follow-up of
`2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`. Capability:
`retail` plus the owner-supplied decrypted image (`$CS_ENGINE_IMAGE`,
sha256 `43540fc9…6c37d75`), **static analysis only**. The original executable
was not run; nothing here is `verified_original`. Implementer: `sonnet-1`
(Sonnet 5.5), no independent review yet.

Committed under the owner exception of AGENTS rule 3: small cited fact rows in
`data/owner-static/engine-facts.toml` (`[flight_check_layout]`,
`[[flight_check_default]]`), no disassembly, no game text.

## The answer in one paragraph

The four Flight Check lists are **not option lists**. They show the *current
loadout of the selected plane*: the pilot's and the wingman's live plane record
(guns, ammunition kinds, rockets). The `$$E$$` host function answers
`callback($$E$$, 2005..2008, index, buffer)`; ids 2005/2007 (guns) and 2006/2008
(rockets) read a plane record picked by slot (`-1` pilot, `-2` wingman). On a
fresh campaign both slots hold a copy of the roster's **Devastator** record
(plane id 5, the airframe M01 already binds), so the defaults are the same for
the pilot and the wingman.

## Measured chain (virtual addresses in the image)

| step | where | what |
| --- | --- | --- |
| script side | `FLIGHTCHECK.SCRIPT` | `YP.TJ = 2005`, `ZP.TJ = 2006` (pilot), `HQ.TJ = 2007`, `IQ.TJ = 2008` (wingman); the shared list control calls `callback($$E$$, TJ, -1)` for the item count and `callback($$E$$, TJ, n, buffer)` per item (`CTL.SCRIPT`, `SHAREDITEMS.SCRIPT`) |
| host function | `0x4093a0` | switches on `id - 2000` (0..0x26) through the dword table `0x40f518`; this table is the native-id map for 2000..2038 |
| gun lines | `0x409c14` (ids 2005, 2007) | index -1 stores the count **8**; otherwise item `n` |
| rocket lines | `0x409cfa` (ids 2006, 2008) | index -1 stores the count **8**; otherwise item `n` |
| slot to plane | `0x40fac0` | slot -1/-2 reads the selection cell `0x6480bc + 4*slot`; `-3` (the roster-init value, kept by the script's `callback($$E$$, 2014, -1, -3, 0)`) falls through to the default cell `0x64b67c + 4*slot` |
| default cells | roster init `0x4113b0` | pilot 0 (`0x411477`), wingman 1 (`0x41147d`); selection cells `-3` (`0x4113b1`..) |
| live records | `0x64b78c`, stride `0xcc`, 25 records | `0x406060` allocates (`cmp ecx,0x19`) |
| roster | pointer stored at `0x41134d` = `0x619f58` | 13 roster records of `0xcc` bytes |
| fresh-campaign copy | `0x411579` / `0x4115a8` | live record 0 <- roster record 11 (`+0x8c4`), live record 1 <- roster record 12 (`+0x990`); i.e. `0x61a81c` and `0x61a8e8`, both plane id 5 |

Record fields read by the handlers: pair-mask byte `+0x84`, four gun ids
`+0x88..`, four ammunition kinds `+0x98..`, eight rocket ids `+0xa8..`; plane id
`+0x2c`.

### How a line is built

* Guns, item `n`: slot `s = n >> 1`; the line is empty when the slot's gun id is
  `5` (no gun); an odd `n` also needs bit `s` of the pair mask. A shown line is the
  item number (`n + 1`, string 519), the caliber label (string
  `3320 + gun`), a space and the ammunition label (string `3365 + kind`).
* Rockets, item `n`: slot `n`; empty when the id is `11`; otherwise the item
  number and the label `3395 + rocket`.
* The three label blocks are the ones the F27/F28 findings imported
  (`IDS_GUNSHORTNAME`, `IDS_AMMOABBRNAME`, `IDS_ROCKETSHORTNAME`) in
  `langui.dll`. The label texts are not committed.

## The defaults for M01

Both roster records (pilot `0x61a81c`, wingman `0x61a8e8`) are identical in
every loadout field:

| field | value |
| --- | --- |
| plane id | 5 (the Devastator row) |
| pair mask | `0x07` (slots 0, 1, 2 paired) |
| gun ids (slots 0..3) | 2, 1, 0, none |
| ammunition kinds | 0, 0, 0, (none slot: 4) |
| rocket ids (slots 0..7) | 1, 1, none, none, 1, 1, none, none |

Derived lists (item number: id): **guns** 1:2, 2:2, 3:1, 4:1, 5:0, 6:0, 7 and 8
empty; **rockets** 1:1, 2:1, 5:1, 6:1, the other four empty. The same for the
wingman. `FlightCheckFacts::gun_list` / `rocket_list` in
`crates/cs_app/src/engine_facts.rs` compute them from the rows.

## What stays unknown

* **Whether anything changes the live records before M01's Flight Check opens.**
  The roster-init copy is the only writer on the fresh-campaign path found. The
  profile/save load, a purchase, and the hangar and ordnance screens (natives
  2027/2028, 2034..2037) write the same records; they were not traced. The rows
  are therefore the *fresh-profile* defaults, as the player-airframe row
  already is. Affected content: M01's Flight Check on a profile that has
  changed its loadout. Resolving: tracing those writers, or an original run
  (REF-OWNER-FIRST-CAPTURE).
* **The meaning of a pair-mask bit** beyond "an odd line is shown": the handler
  shows the second line of a slot when the bit is set. Whether this is twin-linked
  fire is not read.
* **Per-mission or per-roster gating** of the lists: the handlers read only the
  record, so nothing in the list path gates on a mission; whether the mission start
  rewrites the record is the first bullet.
* **Natives 2009..2038** other than 2005..2008 are located in the same table but
  not interpreted here (2010/2014/2018/2034..2036 only read as far as the
  selection cells).
* No original executable ran; nothing is `verified_original`.

## Files and tests

* `data/owner-static/engine-facts.toml`: the two sections above.
* `crates/cs_app/src/engine_facts.rs`: `FlightCheckFacts` reader and the list
  derivation. Wiring-only exports: none (the module is already public).
* `crates/cs_app/tests/campaign/m01_lc_fc_b.rs` (registered in
  `tests/campaign/mod.rs`): `accept_m01_lc_fc_b_*`. The unignored test derives the
  lists from the committed rows through the production reader. The ignored test
  (`requires CS_ENGINE_IMAGE`) re-reads the cited bytes out of the image:
  dispatcher, jump-table entries for 2005..2008, handler constants, label bases,
  roster init and both roster records.
* Sensitivity: changing one committed gun id fails both tests.

# F27-E.2: the gun damage table is shipped in `weapons.zrd`

Date: 2026-10-27. Task: #549 "Recover the original ammunition damage table from
the protected executable, or record precisely why no static route exists".
Predecessor: `docs/findings/2026-10-03-f27-e-1-measured-binding-gate.md`.
Capabilities used: `retail` (read access) and ordinary build/test. No run of the
original executable; nothing here is `verified_original`.

## The short version

F27-E.1 concluded that the damage amounts "are in the executable and no shipped
file declares them". That was wrong, and for a reason visible in its own list of
what it read: it never opened `weapons.zrd`. The member (`ZBD/zrdr.zbd`) holds
the weapon records, and the twenty the engine's own gun loadout map selects state
`ARMOR_DAMAGE` and `HEALTH_DAMAGE` as plain numbers. No unpacking was needed, so
no `cs_inspect` subcommand is added.

## What was measured

* The engine maps (gun id `g` in `0..=4`, ammunition kind `k` in `0..=3`) to the
  record `wep_{30,40,50,60,70}[g] + k` (image VA `0x443d70`, committed as
  `[gun_loadout] gun_base_0..4` in `data/owner-static/engine-facts.toml`; see
  `2026-10-19-m01-window-fire-gun-loadout.md`). The retail test re-reads those
  five bases from the facts file.
* All twenty records exist and each states both amounts (armor / health):

| gun (caliber) | slug | dum-dum | armor-piercing | explosive (`EX`) |
| --- | --- | --- | --- | --- |
| 0 (30) | 3 / 3 | 1.5 / 4.5 | 4.5 / 1.5 | 3.5 / 2.5 |
| 1 (40) | 4.5 / 4.5 | 2.25 / 6.75 | 6.75 / 2.25 | 5 / 4 |
| 2 (50) | 6.25 / 6.25 | 3.125 / 9.375 | 9.375 / 3.125 | 6.75 / 5.75 |
| 3 (60) | 8.5 / 8.5 | 4.25 / 12.75 | 12.75 / 4.25 | 9 / 8 |
| 4 (70) | 11.75 / 11.75 | 5.875 / 17.625 | 17.625 / 5.875 | 12.25 / 11.25 |

  The kind order slug, dum-dum, armor-piercing, explosive is the record
  descriptions' (`MSG_WEAP_*_{SLUG,DUMDUM,APIERCING,MAGNESIUM}`), read from the
  data, not assumed. The table is *measured*; the regularities in it are not used
  by any code, so no multiplier table exists.
* The channel meaning is from the static facts: the record parser stores
  `ARMOR_DAMAGE` at `weapon+0x4c` and `HEALTH_DAMAGE` at `weapon+0x50`, and the hit
  path subtracts them from the victim's armor and health pools unconverted.
  `weapons.zrd` states **no unit**; the amounts stay in the engine's pool units.

## What was delivered

`cs_content::weapons::read_original_gun_damage_table` /
`original_gun_damage_table_from_document` return an `OriginalGunDamageTable`: one
`OriginalGunAmmoDamage` per cell, each amount a `Resolved::Known` whose
`Provenance` is `ObservedTool` with a `SourceSpan` over the amount's own bytes
(installation digest, `ZBD/zrdr.zbd`, member `weapons.zrd`, member digest). A
record the map names but the member lacks, or an amount that is not a number,
leaves the cell absent and is listed in `unmeasured()`; nothing is defaulted.
`limit_evidence()` yields `LimitEvidence::Measured` only for all twenty cells,
`PartlyMeasured` with the shortfall otherwise, so
`OriginalLimitReport::bound()` contains `f27.d.limit.ammo_names_damage` only on a
complete observation. The `Designed` refusal of the gate is untouched.

## What is *not* resolved

* **No type-wide ammunition profile is bound** through `bind_ammunition_damage`:
  a type's amount differs per gun (slug is 3 on the 30 and 11.75 on the 70), so
  one `MeasuredAmmunitionDamage` per type would pick a gun. The catalogue's four
  ammunition records keep `Resolved::Unknown` damage; a consumer should read the
  (gun, kind) cell. Binding the cell into a `DeclaredGunDefinition` also needs the
  gun's mount, which is still unmeasured.
* `f27.d.limit.gun_group_assignment`, `convergence`, `inheritance` and
  `interaction_rules` are unchanged and stay deferred under their F27-E.1
  re-filings. `weapons.zrd` carries no gun-group or mount key in the twenty
  records read here (`CANNON`, `CANNON_SPREAD`, `CALIBER`, `FLYOUT`, `IMPACT`,
  sound/effect names only); the per-airframe gun ids come from the roster record
  (`+0x88`), not from this member. Whether other members of `zrdr.zbd` say more
  about mounts was not searched.
* The F27-E.1 evidence report and finding are historical and are not edited; this
  finding supersedes their `ammo_names_damage` conclusion.

## Evidence

`docs/findings/evidence/F27-E.2.json` (validated with `--require-pass`,
`claim: implemented`, reviewer is the implementer so not independent).
Commands: `cargo test --locked -p cs_content -- accept_f27_e_2_
--include-ignored` (exit 0, 6 passed).

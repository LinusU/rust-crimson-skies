# F27-E.3: the original's armor positions are four, and they are not gun mounts

Date: 2026-10-27. Task: #550 "Record the original's armor and hardpoint position
vocabulary measured from ARMOR.SCRIPT and LAYOUT.CSV" (follow-up to #547 / F27-E.1).
Capabilities used: `retail` (read-only) and ordinary build/test. No run of the
original executable; nothing here says how the original *behaves*.

## What is measured

All three members are read from `GOSDATA/ASSETS/crimson.rof` through the production
ROF mount by `cs_content::damage::read_original_armor_vocabulary`:

- `ASSETS/SCRIPTS/ARMOR.SCRIPT` builds four section titles, keyed in this order
  `ar_t_nosetitle`, `ar_t_tailtitle`, `ar_t_lefttitle`, `ar_t_righttitle`; declares
  `object X[4]` and builds four `ar_d_point` dropdowns, each asking the engine
  through `callback($$E$$, 2247, ...)`; asks for the armor name and description
  through callbacks `2220` and `2225`.
- `ASSETS/LAYOUT.CSV`, section `[@Armor@]`, places each title (string macros
  `IDS_AR_NOSE_TITLE`, `..._TAIL_`, `..._LEFT_`, `..._RIGHT_TITLE`) and each
  dropdown `AR_D_POINT0..3`. Dropdown *n* sits below title *n* and above title
  *n+1*, which is how the pairing slot-to-position is measured rather than assumed.
  Section `[@Purchase@]` declares `PUR_T_ARMORLIST`, `PUR_T_ARMORWEIGHT` and
  `PUR_T_ARMORCOST` as text lists: armor is bought by name, weight and cost.
- `ASSETS/SCRIPTS/RESOURCE.H` declares the four title ids and `IDS_PX_ARMORUNITS`.

The file's own format comment for dropdown rows is stale (it lists a script pointer
the data does not carry); the reader takes Y as the field after X, as the sibling
`EN_D_ENGINE` row does.

Each fact carries an `observed_tool` provenance whose span is the member's stored
extent, plus the line number and decoded byte range of the line it was read from.
The vocabulary is queryable from production code: `OriginalArmorPosition`,
`OriginalArmorVocabulary::{positions, point_count, purchase_columns,
units_string_id, point_callback, name_callback, description_callback}`.

## What is not claimed

- `f27.d.limit.gun_group_assignment` is **not resolved** and stays deferred.
  Nothing shipped maps a declared gun group onto nose/tail/left/right, and the
  eleven wing-station groups still name a station without a side.
  `OriginalArmorVocabulary::gun_group_for` returns `Resolved::Unknown` for every
  position, naming that claim.
- `DeclaredGunMountKind` is **not widened**: this measurement fixes no gun-mount
  side. `OriginalLimitClaim::measured_lead` records where the lead now lives.
- No armor amount, weight, cost or damage zone is recorded; F29 owns that model.
  The mailbox in the script also mirrors points 2 and 3 into each other; that is
  noted here and deliberately not encoded.

## Files

`crates/cs_content/src/damage.rs`, `crates/cs_content/src/weapons.rs`,
`crates/cs_content/tests/accept_f27_e_3_armor_positions.rs` (4 fast tests, 1
`#[ignore = "requires CS_GAME_DIR"]` retail test with its own independent scan),
`crates/cs_content/tests/evidence_report_f27_e_3.rs`,
`docs/findings/evidence/F27-E.3.json`, and `tests/main.rs` wiring.

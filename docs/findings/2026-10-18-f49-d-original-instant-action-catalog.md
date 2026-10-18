# F49-D: the original Instant Action catalog, as far as the installation measures it

Date: 2026-10-18. Task: F49-D.04 (#786), integrating F49-D.01 (#783), F49-D.02
(#784) and F49-D.03 (#785) under the parent F49-D (#204). Capability:
**`retail`**, which is **read access to the installation's files only**.
Nothing was run, rendered or played, no original executable ran, and nothing in
this document is `verified_original`. The claim stays `implemented`.
`gpu` and `audio` were available and not used.

Evidence: `docs/findings/evidence/F49-D.json` (links the subtask reports
`F49-D.01.json`, `F49-D.02.json`, `F49-D.03.json`; artifacts stay in
`private/evidence/F49-D/`). Installation fingerprint recorded in the report:
`install_sha256 b4e780ab84cf31d85b8452fbfcec1478137768e32d9a75ccedc4c1847c631978`,
`content_sha256 a0223506e512b50c0e0445ba73204a0461e60197e28d58a7f7144632d262c12d`.
Acceptance run: `cargo test --locked -p cs_content -p cs_app -- accept_f49_d_
--include-ignored`, exit 0, 52 tests passed (only these two crates hold
`accept_f49_d_` tests, and AGENTS.md forbids a local workspace run; CI runs the
whole suite).

## Measured

**Preset denominator (F49-D.01).** 8 `IA<n>` directories, each with an
`ia.zrd` member in its `zrdr.zbd` reader archive (spans are in the F49-D.01
report): `c1, c1b, c1c, c2, c2b, c3, c4, c5`. One candidate preset per directory
and offered mission type: **30 presets over 8 scenarios** (6 directories offer
four types, `c1c` and `c2b` offer three).

**Per-directory roster (F49-D.03, from the `ia.zrd` descriptors).** Every
scenario seats one player; `num_wingmen` is 3 everywhere except `c2b`, which
states none. Enemy groups are `plane:skill:authored count`:

| scenario | enemy groups (1..4) | ace |
| --- | --- | --- |
| c1 | fbrand novice 6, fury veteran 5, brigand veteran 4, warhawk ace 3 | peacemaker |
| c1b | kestrel novice 4, bhawk veteran 3, fury ace 3, bhawk ace 2 | fury |
| c1c | fbrand novice 4, brigand veteran 3, fury ace 3, warhawk ace 2 | fbrand |
| c2 | autogyro novice 4, brigand veteran 4, warhawk ace 3, fury ace 2 | bhawk |
| c2b | fbrand novice 6, kestrel veteran 5, bhawk veteran 4, warhawk ace 3 | pfighter |
| c3 | kestrel novice 6, brigand veteran 5, pfighter veteran 4, peacemaker ace 3 | kestrel |
| c4 | fbrand novice 6, brigand veteran 5, fury veteran 4, pfighter ace 3 | bhawk |
| c5 | autogyro novice 6, kestrel veteran 5, brigand veteran 4, warhawk ace 3 | peacemaker |

Planes are `airframe/player_<name>` rows resolved through
`cs_app::mission_start::airframe_entry` and checked against `AIRFRAME_TABLE`.
The ace's side is an **inference** (enemy) from the key set, not a measurement.

**Option table per list (F49-D.02, counted by the consumer trace `ac04.json`).**

| list | entries | evidence |
| --- | --- | --- |
| worlds | 8 | measured (F14-D.3 world rows) |
| environments | 32 | measured (`read_mission_weather` over the campaign's missions) |
| factions | 11 | measured (F14-D.5 paint patterns) |
| airframes | 11 | measured (F14-D.6 declared roots) |
| difficulty tiers | 3 | measured (F32-D steps) |
| victory conditions | 3 | **designed** (F49-A vocabulary, not original) |
| max players | 8 | **designed** (`MAX_SCENARIO_PLAYERS`, the original's limit is unmeasured) |
| loadouts | 0 | **unmeasured** |
| pilots | 0 | **unmeasured** |
| relations | 0 | **unmeasured** |

**AC04 (F49-D.04).** Through the production path
(`CustomScenarioDraft::resolve`, `InstantActionCatalog::validate_custom` /
`require_valid_custom`, `resolve_custom` / `lower_custom`, and the flow's
Launch), `accept_f49_d_04_*` shows that an empty roster, an airframe no option
table offers, an impossible faction pair (same faction on both sides, a faction
the table does not offer, a declared friendly or neutral pair), a seat count the
roster cannot fill and an incomplete draft each return **every** problem as a
sentence carrying dimension, the selection to replace or the open claim, and a
detail; a request combining four classes returns all four in stable order. An
unknown is refused with its own claim and reason, never defaulted. A refused
selection spends no session generation, writes no record and spawns nothing:
checked for all 30 retail presets (unedited launch) and for a custom draft
edited to an unoffered airframe (`session_started` is false for every row of
`ac04.json`). Dropping a problem, truncating to the first or rendering `Debug`
fails `accept_f49_d_04_synthetic_every_invalid_class_is_reported_in_full`
(both mutations were run).

**The positive case has a stated limit.** Because loadouts, pilots and
relations are empty in the retail table, the pure retail catalog refuses *every*
custom request that names a loadout; that is asserted
(`..._every_measured_selection_is_accepted_and_the_unmeasured_loadout_is_refused`:
world, environment, factions, airframes, tier, rules, seed and seats are all
accepted and the loadout is the only refusal, once per actor). The "valid
request is accepted" control therefore runs on the retail lists plus **declared
stand-ins** for those three lists (`standin_catalog`). It proves refusal is
specific rather than unconditional; it says nothing about what the original
accepts. Follow-up #1587 replaces the stand-ins when the lists are measured.

## Still unknown (each with its resolving task)

* **No retail preset launches (AC01's launch half is unmet).** Per-actor
  faction and survivability: #1582. Victory rules per mission type
  (`dogfight_ace`, `dogfight_squadron`, `zeppelin_run`, `stunt_flying`; the
  designed `VictoryCondition` vocabulary does not cover them), per-scenario
  difficulty, which declared aircraft each type spawns, the wingmen's planes,
  and `c2b`'s `player_plane` / `num_wingmen`: #1583. Loadout identities: #1575.
* **Preset display titles** and whether the menu offers every (scenario, type)
  pair: #1584.
* **Pilots and faction relations** in the option table: the F33-D and F30-D
  measurements named in `PILOT_UNMEASURED_REASON` / `RELATION_UNMEASURED_REASON`;
  re-checked by #1587.
* **Seed policy**: the seed per preset is a designed FNV-1a of the preset id
  (`preset_seed`), explicit and unique, not an original value; whether the
  original reseeds per launch needs an original run: #1588 (owner-gated).
* **Environment binding**: resolved for all eight directories (the IA archive's
  own `weather.zrd`, F49-D.03); the *selectable* environment list (32 ids) is the
  weather reader's, and whether the original's custom screen offers the same set
  is unmeasured (original run, `human_play`).

## F14-D.1's "presets inside one IA1"

`2026-10-02-f14-d-1-reader-archive-directories.md` left "how many
player-selectable presets one IA1 directory yields" unmeasured. **Restated
after F49-D.01:** `ia.zrd` offers three or four mission types per directory
(`spawn_points` keys, minus `disallow_missions`), giving 30 candidate presets
over 8 directories. That the original's menu is exactly this product, and what
the presets are called, remain unmeasured (#1584); the count 30 is the
denominator the descriptors support, not a claim about the menu.

## What this does not claim

`retail` is file access. No preset was launched, played or compared with an
original run, so nothing here is `verified_original`, and a merged F49-D is
**checked**, not "recreated". The owner's approval and an original run are
still required for any fidelity claim.

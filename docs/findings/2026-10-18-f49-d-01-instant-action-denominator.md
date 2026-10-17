# F49-D.01: the Instant Action scenario and preset denominator, from ia.zrd

Date: 2026-10-18. Task: F49-D.01 (#783). Capability: **`retail`** (read-only
`$CS_GAME_DIR`). Nothing was run, rendered or played; the claim stays
`implemented`.

Resolves the unknown F14-D.1 recorded ("how many player-selectable presets one
IA1 directory yields is unmeasured",
`2026-10-02-f14-d-1-reader-archive-directories.md`) **as far as the data
says**, not further.

## Reader

`cs_content::instant_action::survey_retail_instant_action` runs production
discovery (`cs_assets::install::discover`), the reader-archive discovery
(`cs_formats::script_raw::discover_container`) and the `.zrd` decoder
(`stunts::decode_zrd`, `scenario_*`, `zrd_flat_fields`) over every
`ZBD/<group>/IA<n>/zrdr.zbd`. Per directory it reports the container key and
SHA-256, the `ia.zrd` member span, the offered mission types
(`spawn_points`, flat alternating: type text, list of spawn-point records), the
disallowed types (`disallow_missions`), the default `mission_type`, the roster
and the `dzones` bindings. It refuses (naming container and member) a missing
`ia.zrd`, a missing `spawn_points`, an undecodable member, a malformed
`spawn_points`/`disallow_missions`, a mission type outside the observed
vocabulary and duplicate ids.

## Measured (2026-10-18, through the production reader)

* 8 directories (c1, c1b, c1c, c2, c2b, c3, c4, c5), equal to an independent
  `std::fs` walk.
* Vocabulary = {dogfight_ace, dogfight_squadron, zeppelin_run, stunt_flying,
  ground_target}. For every directory, vocabulary minus `disallow_missions`
  equals its `spawn_points` key set. `ground_target` is offered nowhere and
  disallowed everywhere; `stunt_flying` is also disallowed in c1c and c2b.
* Spawn-point record counts per offered type: 8 for dogfight_ace,
  dogfight_squadron and stunt_flying, 4 for zeppelin_run. Each record is four
  numbers; **their meaning is not interpreted here**.
* Default `mission_type`: dogfight_squadron (c1, c3), stunt_flying (c1b, c2,
  c4, c5), zeppelin_run (c1c, c2b).
* All eight declare four `group<N>` records and an ace.

## Id scheme and denominator

`ia_scenario/<group>-<leaf>` (matches F14-D.1's scenario row key) and
`ia_preset/<group>-<leaf>-<mission_type>`: one candidate selectable slot per
directory and offered mission type. 6 directories offer four types and 2
(c1c, c2b) offer three: **30 preset ids over 8 scenario ids**, all unique.
That the original's menu is exactly this product is **not measured**; it is the
denominator the descriptors support.

## Correction to the scoping note

The scoping note says all eight declare `num_wingmen = 3`. Measured: **seven
do; `ZBD/c2b/IA1` declares no `num_wingmen`**, in addition to declaring no
`player_plane`. Both are reported as unknown, never defaulted.

## Unknowns

* Whether the original menu offers every (directory, offered type) pair, and
  the original preset titles.
* The player airframe and wingman count of c2b's scenario.
* The meaning of the four numbers per spawn-point record and the runtime effect
  of `disallow_missions`.

Evidence: `docs/findings/evidence/F49-D.01.json` (artifacts stay in
`private/`). Sources: the `ia.zrd` members of the installation's eight IA1
reader archives, read through the code above.

# M18-B-FU2 (#1255): which consumer joins M18's `WAKE_ANIM` / `ANIM_STATE` names

Date: 2026-10-18. Capability `retail` (`$CS_GAME_DIR`, read-only). Implementer
claude-2. Evidence class: **ObservedTool** (this repository's own readers over
the retail files). `$CS_ENGINE_IMAGE` was not set in this session and no
original run exists, so nothing here is `verified_original` and no static
reading of the executable was done for this note.

## Question

M18-B measured that block 25 of `ZBD/C4/M03/zrdr.zbd :: objectives.zrd` spells
`pzhomebase` (`WAKE_ANIM`) and `hooked_to_klondike` (`ANIM_STATE`), and that no
other member of M18's own reader archive declares either. Which production
consumer joins them to the record that defines them?

## Measurement (test `accept_m18_b_fu2_control_record_names_resolve_through_the_name_join_and_the_landing_table`)

| Name | Operand | Declared by (production `bind_mission_animation("zbd/c4/m03").animation_by_name`) | Carrier record |
| --- | --- | --- | --- |
| `pzhomebase` | `WAKE_ANIM` | `zbd/zrdr.zbd :: pzep_hookup.zrd` (the shared root, third archive of M18's search order) | `zbd/c4/m03/mis_anim.zbd` #214, playable, no refusal |
| `hooked_to_klondike` | `ANIM_STATE` | `zbd/zrdr.zbd :: pzep_hookup.zrd` | `zbd/c4/m03/mis_anim.zbd` #217, playable, no refusal |

So both names are not "declared elsewhere in the installation" in an
unreachable way: M18's reader scope is three archives (`zbd/c4/m03`, `zbd/c4`,
`zbd/zrdr.zbd`), and the shared root is the third. M18-B only searched the
first.

## The consumers

* **`WAKE_ANIM` (`pzhomebase`).** The mission host's `WAKE_ANIM` consumer
  (`cs_app::mission_animations`, host step 6b) resolves the spelled name through
  the scope's `AnimationNameIndex` (`animation_by_name`), which now measurably
  binds it. Same shape as M06's `pzhomebase` in
  `2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`.
* **`ANIM_STATE` (`hooked_to_klondike`).** The gate reads the animation state
  the host publishes per tick from the record player (`mission_session::host`,
  step 4: `RUNNING`/`EXECUTED` keyed by each running or finished record's
  identity). As the M06 note measured, a `WAKE_ANIM` does **not** run the gated
  record; the record is run by the **landing table**. M18's world group
  `zbd/c4/zrdr.zbd :: landings.zrd` (read by `read_landings(…, "zbd/c4")`, the
  same call `mission_session::compose` makes from the stage mount) has five
  rows with no refusal; two run `hooked_to_klondike`: `pz_manual_land`
  (manual) and `pz_auto_land` (`auto`), both anchored on `piratezep`. No row
  runs `pzhomebase`. The other three rows (`cg_initiate_hookup`,
  `bm_initiate_hookup`, `pickup_sparks`) are not block 25's.

## What stays unknown

* Whether the landing consumer's activation reports the record under the
  identity `hooked_to_klondike` that the host's `ANIM_STATE` fold keys on; this
  note pins the join and the table, not the end-to-end run. That is a runtime
  observation (M18-C, `human_play`).
* The landing-window semantics (`angle`, `speed`, `auto`) and the landing
  detection are the designed rules of `landings.rs` (claims
  `m01-landing.*-designed`), unchanged and not verified here.
* The original's own resolution order for `ANIM_STATE` names is not read from
  the executable in this task.

M18-B's tests are unchanged and pass; its assertion that the two names are
absent from M18's *own* archive is still true and does not contradict this
note.

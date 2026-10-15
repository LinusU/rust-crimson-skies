# M01-PLAY-TRIGGERS-ANIMS (#1482): the `WAKE_ANIM` events do **not** run the records M01's `ANIM_STATE` gates read

* Measured 2026-10-15, capability `retail`, implementer `mimo-1`, task #1482,
  with the production readers. Evidence class for every row below:
  **ObservedTool** — a description of the retail files read by this repository's
  own code, not of the original executable. No original run exists; nothing here
  is `verified_original`.
* Method: the three reader archives `zbd/c1c/m01/zrdr.zbd`,
  `zbd/c1c/zrdr.zbd` and `zbd/zrdr.zbd` were walked with
  `cs_formats::script_raw::discover_container` and
  `cs_app::animation::programs::{read_animation_definition_member,
  read_startup_animations}`; the two animation carriers
  (`zbd/c1c/m01/mis_anim.zbd`, `zbd/c1c/cam_anim.zbd`) were walked with
  `cs_app::animation::carrier::survey_animation_bindings`; the statement
  payloads (which the production definition reader keeps only as kinds and
  field names) were read as raw `.zrd` text through
  `cs_content::stunts::decode_zrd`. A raw-byte search (`grep -a`) over the whole
  installation at `$CS_GAME_DIR` checked that no other file names these
  animations.

## The three pairs the mission record spells

Each M01 control block pairs one `WAKE_ANIM` event with one `ANIM_STATE` gate
(census: `crates/cs_app/tests/accept_m01_lc_directive_c.rs:211-261`; the same
`pzhomebase`/`hooked_to_klondike` pairing recurs in M19 and M21,
`docs/findings/2026-10-10-m21-b-compatibility-gaps.md:146-150`):

| block | `WAKE_ANIM` event | `ANIM_STATE` gate |
| --- | --- | --- |
| `OBJECTIVE11` | `activate_dropoff_node` | `wv_drop_copilot` `RUNNING` |
| `OBJECTIVE15` | `activate_pickup_node` | `wv_pickup_copilot` `EXECUTED` |
| `OBJECTIVE18` | `pzhomebase` | `hooked_to_klondike` `EXECUTED` |
| `OBJECTIVE13` | `fadein_bszep` | — (no gate) |
| `OBJECTIVE49` | `activate_dropoff_node` | — (no gate) |

Both names of a pair resolve through the engine's one animation table
(`0x523820`, entries at `0x9fd14c`, state byte `+0xa0`:
`docs/findings/2026-10-06-m01-lc-directive-c-ai-world-and-animation-directives.md:441-536`).
They are nevertheless **different entries**.

## Measurement 1 — every name is its own definition and its own carrier record

| name | definition (member, index, `ANIMATION_NAME`) | mission-carrier record (`mis_anim.zbd`, 573 records) |
| --- | --- | --- |
| `activate_dropoff_node` | `zbd/c1c/m01/zrdr.zbd :: wv_tailhook.zrd` #3 | 498 |
| `wv_drop_copilot` | `zbd/c1c/m01/zrdr.zbd :: wv_tailhook.zrd` #16 | 511 |
| `activate_pickup_node` | `zbd/c1c/m01/zrdr.zbd :: wv_tailhook.zrd` #5 | 500 |
| `wv_pickup_copilot` | `zbd/c1c/m01/zrdr.zbd :: wv_tailhook.zrd` #12 | 507 |
| `pzhomebase` | `zbd/zrdr.zbd :: pzep_hookup.zrd` #0 | 188 |
| `hooked_to_klondike` | `zbd/zrdr.zbd :: pzep_hookup.zrd` #3 | 191 |
| `fadein_bszep` | **no `ANIMATION_NAME` anywhere**; a `NAME1` state of `zbd/zrdr.zbd :: bswan_gasbags.zrd` #0 (node `blackswanzep`) | 344 |

The camera carrier (`zbd/c1c/cam_anim.zbd`, 307 records) holds none of the
seven names.

## Measurement 2 — no call path runs from a woken animation to its block's gate

Every `CALL_ANIMATION`/`INVALIDATE_ANIMATION` statement of every decoded member
of the three archives was read (the raw `.zrd` text, so the called `NAME` is
included). The caller census for the seven names:

* `activate_dropoff_node` calls `extend_wv_hook`, `wv_hookup_lights` and sets
  `OBJECT_ACTIVE_STATE dropoff_node ACTIVE` plus
  `OBJECT_ACTIVE_STATE [GLOBAL workersvoyagezep wv_manual_land land_on] ACTIVE`.
  **No definition calls it** — its only other name-sites are the `WAKE_ANIM`
  directive arguments in `objectives.zrd`.
* `activate_pickup_node` calls `extend_wv_hook`, `setup_cp_and_fb`,
  `wv_hookup_lights` and sets `pickup_node ACTIVE` and the same
  `wv_manual_land/land_on` node. **No definition calls it.**
* `pzhomebase` calls `open_pzepcargodoors`. **No definition calls it.**
* `wv_drop_copilot` (prerequisite `REQUIRED OBJECT_ACTIVE_LIST [[wv_tailhook,
  dropoff_node]]`) is called **only** by `wv_initiate_hookup`, and
  `INVALIDATE_ANIMATION`s itself at its end.
* `wv_pickup_copilot` (prerequisite `REQUIRED OBJECT_ACTIVE_LIST [[wv_tailhook,
  pickup_node]]`) is called **only** by `wv_initiate_hookup`.
* `hooked_to_klondike` has **no `CALL_ANIMATION` in any of the three
  archives**; the only statement outside its own definition that names it is
  `INVALIDATE_ANIMATION` inside `zbd/zrdr.zbd :: pzep_destruction.zrd`
  (`killpzep`).
* `wv_initiate_hookup` is called only by `wv_tailhook.zrd`'s definition `it`
  (nothing in the three archives calls `it`) — and by the landing table below.

## Measurement 3 — the landing table is what runs the gated records

`zbd/c1c/zrdr.zbd :: landings.zrd` carries exactly three records:

| record | fields |
| --- | --- |
| 0 | `anim [wv_initiate_hookup]`, `node [wv_manual_land]`, `angle [35]`, `speed [50 220]` |
| 1 | `anim [hooked_to_klondike]`, `node [pz_manual_land]`, `angle [35]`, `speed [50 220]` |
| 2 | `anim [hooked_to_klondike]`, `node [pz_auto_land]`, `auto []` |

So the records the three gates read are started by **the player landing on a
named node** — `wv_manual_land` runs the whole `wv_initiate_hookup` cutscene
chain (which calls `wv_drop_copilot`, then `wv_pickup_copilot`, among others),
and `pz_manual_land`/`pz_auto_land` run `hooked_to_klondike`. The raw-byte
search confirms no other file in the installation names these animations, and
`landings.zrd` currently has **no consumer in this workspace** — only catalog
membership (`crates/cs_content/src/catalog/reader_dirs.rs:324`) and test
fixtures.

## Conclusion (the measurement step of task #1482)

The record's own animation join does **not** prove that a `WAKE_ANIM` event
runs the record its block's `ANIM_STATE` gate reads; it proves the opposite
shape: the woken animation **arms a landing node** (`dropoff_node` /
`pickup_node`, and the shared `wv_manual_land/land_on` node — exactly the
`OBJECT_ACTIVE_LIST` prerequisites the gated animations require), and the
gated records are run later by the landing table when the player docks. The
same-block pairing in `objectives.zrd` is a mission-authoring relation, not a
run chain.

Consequently a `WakeAnimation` consumer that started the *gated* records would
invent the mapping (AGENTS.md rule 4) and would make `OBJECTIVE11/15/18`
complete without the player ever docking — contradicting the acceptance carried
from #1439 ("the docking/drop-off beats advance from the player's actual
actions in the mission"). A consumer that started only the *named* record
(`activate_dropoff_node`, carrier record 498) is measured and honest, but the
three gates read other names, so it cannot by itself close the gap.

**Affected content:** M01's `OBJECTIVE11`, `OBJECTIVE15` and `OBJECTIVE18`
gates on the win chain, and the same `pzhomebase`/`hooked_to_klondike` pairing
in M19 and M21. **Resolving tasks:** the landing/docking path belongs to the
parent task M01-PLAY-TRIGGERS (#1439) — the landing table needs a runtime
consumer (a player-action trigger with the measured angle/speed windows and the
`auto` flag) before any of the three gates can become true through real
animation records; a `WakeAnimation` consumer for the honest, named record is
still worth having, but is not what the gates read.

## What stays unknown

* Whether the original evaluates `ACTIVATION_PREREQUISITE` before a
  `CALL_ANIMATION` (this repository carries it as
  `UnmeasuredFieldFamily::PrerequisiteEvaluation`,
  `crates/cs_app/src/animation/programs.rs:1675`), i.e. whether the armed
  `dropoff_node`/`pickup_node` states are what let the cutscene chain's calls
  succeed.
* What calls `wv_tailhook.zrd`'s definition `it`, and whether the landing table
  is the only caller of `wv_initiate_hookup` at runtime.
* The landing table's own engine semantics (`angle`, `speed` window, `auto`) —
  the fields are read verbatim above; no original run measured their meaning.
* None of the seven names is a `startanims.zrd` identity of M01 (the seven
  startup identities are pinned in
  `crates/cs_app/tests/accept_m01_lc_actor_anim_playback.rs:1081-1093`), so
  `MissionAnimationPlayer` cannot hold a row for them today without a new,
  measured join surface.

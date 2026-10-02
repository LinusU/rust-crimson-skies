# F56-A follow-up (task #475): each scenario slot's mode, decoded from its own `targets.zrd`

Date: 2026-10-03. Task: #475 "Decode MP slot programs to bind each
`ZBD/<group>/MP<n>` slot to a multiplayer mode" under stage
`### F56-A` (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`).
Capability: `retail` (the installation at `$CS_GAME_DIR`, one language, 1033).
Parent finding: `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`, whose
"Not known" item 1 this file resolves at the family level.

## Files and the observable failure

- `crates/cs_content/src/multiplayer.rs`: `ScenarioMode` (the three measured
  mode families), the `TARGET_DESCRIPTION_*` keys, and `resolve_slot_mode`,
  which locates the slot archive's `targets.zrd` through the production
  `cs_formats::script_raw::discover_container` and decodes it with the
  production `cs_content::stunts` `.zrd` reader. `ScenarioSlot::mode` is now a
  `Resolved<ScenarioMode>` carrying the member's own span and an
  `observed_tool` class.
- `crates/cs_content/tests/accept_f56_a_catalog.rs`: the synthetic suite authors
  version-one reader archives whose `targets.zrd` is written with the measured
  `.zrd` grammar, and the retail test pins all 21 bindings.
- Observable failure without the change: every slot's `mode` is an unresolved
  unknown, so a consumer cannot tell a Capture-the-Flag slot from a deathmatch
  one. With it, each slot's `mode` names the family its decoded objectives
  select, and a slot with no usable `targets.zrd` stays an explicit unknown.

## What was decoded (this installation only)

Every `ZBD/<group>/MP<n>/` slot ships a reader archive `zrdr.zbd`. Its
`targets.zrd` member holds one objective record per target in the measured
`.zrd` pair shape (`[["description", <text>], …]`, read through
`cs_content::stunts::zrd_field`). The `description` value is a localized string
key naming the objective. The measured keys are:

- `MSG_TRGT_REARM_BASE` — a rearm base; present in every family, so on its own
  it identifies nothing.
- `MSG_TRGT_FLAGBASE` / `MSG_TRGT_FLAG` — a capture-the-flag base / a carried
  flag.
- `MSG_TRGT_ZEP_ENEMY` / `MSG_TRGT_ZEP_FRIEND` — an enemy / friendly zeppelin.

The rule this yields: **a flag objective is Capture the Flag; a zeppelin
objective is Zeppelin vs. Zeppelin; neither is Deathmatch** (whose `targets.zrd`
holds at most a rearm base, or no objective at all). The catalog names four
modes; only deathmatch and capture-the-flag can hold no zeppelin objective, and
only capture-the-flag holds a flag, so the three-way rule is exhaustive over the
measured mode set.

### Why the whole-archive markers were not enough

F56-A recorded the mixed byte-level markers (`Flag base` text and `\zeps`
paths) over the whole `zrdr.zbd`. They are mixed because the members that carry
them are not the mode selector: `C2/MP3` carries the `Flag base` text while its
objectives are zeppelins, and `C3/MP1`, `C4/MP1` and `C4/MP2` carry `\zeps`
paths (in `mis_anim.zrd`) while being deathmatch or capture-the-flag. The
markers are kept on `ScenarioSlot::markers` as the weaker evidence they are; the
member-level decode removes the ambiguity.

## The measured binding (21 of 21)

| slot | `targets.zrd` objective keys | mode |
| --- | --- | --- |
| C1/MP1 | `REARM_BASE` | Deathmatch |
| C1/MP2 | `FLAGBASE`, `FLAG`, `REARM_BASE` | Capture the Flag |
| C1/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C1B/MP1 | `REARM_BASE` | Deathmatch |
| C1B/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C1C/MP1 | (one record, no description) | Deathmatch |
| C1C/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C2/MP1 | `REARM_BASE` | Deathmatch |
| C2/MP2 | `FLAGBASE`, `FLAG`, `REARM_BASE` | Capture the Flag |
| C2/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C2B/MP1 | (no objective record) | Deathmatch |
| C2B/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C3/MP1 | `REARM_BASE` | Deathmatch |
| C3/MP2 | `FLAGBASE`, `FLAG`, `REARM_BASE` | Capture the Flag |
| C3/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C4/MP1 | `REARM_BASE` | Deathmatch |
| C4/MP2 | `FLAGBASE`, `FLAG`, `REARM_BASE` | Capture the Flag |
| C4/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |
| C5/MP1 | `REARM_BASE` | Deathmatch |
| C5/MP2 | `FLAGBASE`, `FLAG`, `REARM_BASE` | Capture the Flag |
| C5/MP3 | `ZEP_ENEMY`, `ZEP_FRIEND`, `REARM_BASE` | Zeppelin vs. Zeppelin |

The retail test pins this table exactly (`accept_f56_a_retail_every_scenario_slot_binds_to_a_mode_with_evidence`).

## Evidence class and spans

Every known binding is `Resolved::Known(Known)` with `ClaimStatus::ObservedTool`
and a `SourceSpan` that names the slot archive as container
(`ZBD/<group>/MP<n>/zrdr.zbd`), `targets.zrd` as the member, the member's own
byte offset and length inside the archive, and the member's SHA-256. The retail
test asserts the class, the container, the member, a nonzero offset and length
and a present member digest for all 21 slots, so a binding cannot be produced
from another member or another slot.

## What stays unknown

1. **Which deathmatch variant.** The two string-named deathmatch modes
   (`7011` without teams, `7012` with teams) differ only in team play, and no
   member of an `MP<n>` reader archive states it. `ScenarioMode::Deathmatch`
   therefore covers both (`mode_name_ids` lists both), and F56-B must decide how
   the variant is chosen at launch. This is the residual of the parent finding's
   item 1 and is not a guess: the variant is not in the files.
2. **A slot with no usable `targets.zrd`.** If the member is absent, does not
   decode, is not a record list, or names both a flag and a zeppelin objective,
   the binding stays `Resolved::Unknown` with a reason naming the cause. The
   synthetic suite pins each of those cases.
3. **Per-mode rules.** Unchanged from the parent finding and owned by task
   #476: spawn, respawn, lives, limits, friendly fire, victory/draw, disconnect,
   late join, human scaling, component limits, and which event each printed
   briefing point value rewards.

## Method and sensitivity

The binding was measured by decoding each slot's `targets.zrd` through the
production readers, and the synthetic suite authors the `.zrd` and archive bytes
independently, so the production decoder's accepted shape is what the test
proves. Two mutations were applied and caught: removing `resolve_slot_mode`
(every slot unknown) fails the synthetic family test and the retail 21-slot
test; forcing the capture-the-flag case to `Deathmatch` fails the synthetic
family test. The synthetic `...without_a_usable_targets_member_stays_unknown`
case fails if the unknown paths are short-circuited to a family. Nothing here is
`verified_original`: no original run happened, and reading the installation's
files is not evidence of how the game behaves.

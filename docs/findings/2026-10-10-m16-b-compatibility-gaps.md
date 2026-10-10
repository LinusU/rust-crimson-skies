# M16-B: M16's control program, measured, and the two gaps that keep it unlowered

Date: 2026-10-10. Task: M16-B "Implement and regress mission-specific
compatibility gaps" (#304, `missions/M16.md`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail` (read-only
`$CS_GAME_DIR`) and the owner-supplied decrypted engine image
(`$CS_ENGINE_IMAGE`, read-only). Implementer: **swe2-max-1/swe2-max-1**
(Devin SWE-2 Max, Rally #304 implement claim, sessions of 2026-10-10).
Reviewer: recorded by the reviewer in the `complete_review` notes — this
document and the report were written by the implementer, so they are not
independent evidence and no agent review replaces the owner's human
approval.

## What changed

No production code. The census (`cs_app::mission_control`), the directive
dispositions, the record → `RawProgram` adapter and `SourceContext`’s
control-program binding are M01-LC's/M02-B's; this stage runs them over M16
and pins the result in `crates/cs_app/tests/campaign/m16_b.rs` (six retail,
one engine-image and two synthetic `accept_m16_b_*` tests) plus the
evidence harness in `evidence/m16_b.rs`. Wiring: `campaign/main.rs` and
`campaign/evidence.rs` (`mod m16_b;`).

## Measured

| Fact | Value |
| --- | --- |
| Reader archive | `ZBD/C4/M01/zrdr.zbd`, 16 members, SHA-256 `553b9b3b…fcd5d` (the program M16-A bound) |
| Control member | `objectives.zrd` (offset 10737, 9672 bytes), the only member declaring numbered blocks — not the first (`aiv.zrd`), not the longest (`sparks_pkup.zrd`, 17 668 bytes) |
| Blocks / sites / keys | 37 (numbered 1..=37, no gaps) / 133 / 28 |
| Record-level keys | the five measured fields (`MISSION_TIMER`, `PLAYER_INIT`, `RESTORE_ANIMS`, `EXECUTE_ANIMS`, `INVALIDATE_ANIMS`), each once, none unclassified |
| Terminal outcomes | `INSTANTLOSS` in OBJECTIVE19 and `INSTANTWIN` in OBJECTIVE20 — one bare site each, both nap-armed at 15 s; M12 spelled no loss latch, M16 spells both |
| Dispositions | 2 implemented (the two latches), 22 measured, 4 unmeasured, 0 refused parse rows |
| Calls | 122 of 133 bind; **11 refuse** (`unknown host call`): the 7 `STOP_QUEUED_SOUNDS` sites and the 4 bare comment words; 1 unbound key |
| Conditions | all 37 lower |
| Cross-objective edges | 41 addresses over `WAKE`/`NAP`/`KILL_OBJECTIVE_WHEN_I_COMPLETE`, all in `1..=37`; min 1, max 37 |
| Latch arming | OBJECTIVE20's only incoming edge is OBJECTIVE11's `NAP … [20, 15.0]`; OBJECTIVE19's only incoming edge is OBJECTIVE17's `NAP … [19, 15.0]`; both latches `BEGIN_DORMANT -1` |
| Start structure | OBJECTIVE29 self-wakes at 1 s (`piratezep` turrets + `snd_RM1Start`), OBJECTIVE3 at 10 s (the four `bhatbrigand_*`); 31 dormant timers off; OBJECTIVE9/10/15/16 are empty stubs and OBJECTIVE33 a disabled dormant stub — no edge reaches any of them |
| Kill lists | OBJECTIVE2 kills {31, 32}; OBJECTIVE17 kills {2, 4, 5, 6, 7, 8, 11, 31, 32} — the pickup gate among them |

The win path the record spells: OBJECTIVE13's completion (the Black Hat
depletion, `DEDG [1, 0]`) re-wakes OBJECTIVE1 — the armed train-turret
phase, `PRIMARY` `MSG_BRF_RMM1_OBJ1` — and the battle-success music
OBJECTIVE23, while OBJECTIVE1's own completion wakes the failure block
OBJECTIVE17 and naps OBJECTIVE35. The pickup gate OBJECTIVE11 (`PRIMARY`
`MSG_BRF_RMM1_OBJ3`, woken by OBJECTIVE8's completion) completes only when
the named animation `got_sparks` reports `EXECUTED`, and its completion
schedules the 15 s nap that arms `INSTANTWIN`. The failure path is
symmetric: OBJECTIVE17 completes when `train01` no longer carries the
in-play bit (`INACTIVE1 [train01, healthy]`), retires the train's target
flag, plays `snd_c4-RM-m1_Zachary_59`, schedules the same 15 s nap onto
`INSTANTLOSS`, wakes OBJECTIVE37's fourteen-name sound cleanup and kills
nine blocks — OBJECTIVE11 among them, so a train gone after the pickup
can no longer complete the win chain through the spelled record. Whether
the two naps can fire inside one window, and which outcome then wins, is
the precedence the contract requires measuring — a runtime question left
to M16-C.

Sheet priorities. The record carries **no** collision, damage, docking,
pickup, boarding, transfer or interaction directive at all — the whole
28-key vocabulary is pinned, so the actor/interaction halves of the three
priorities are not in this member. What the record spells:

- *Moving reference frame*: `train01` is the only name the target lists
  and membership lists agree on — `ADD_OBJECTIVE_TARGET` in OBJECTIVE1 and
  OBJECTIVE8, `REMOVE_OBJECTIVE_TARGET` in OBJECTIVE2, OBJECTIVE11 and
  OBJECTIVE17, `INACTIVE1` member of OBJECTIVE17, the `TRAVELERS` anchor of
  OBJECTIVE34 (`[player, APPROACHING, train01, 1500.0, 1]`, which plays
  `snd_RM1Train`), the `MSG_OBJ_DOCK` help label of OBJECTIVE8, the
  `tsega1` anim of OBJECTIVE1 and the `tcargun**` two-wildcard turret wake
  beside it. The designer note in OBJECTIVE24 ("Change to mobile net")
  sits directly above the `SET_AI_NET` site that moves the four
  `bhatbrigand_*` onto `M1Intercept`; OBJECTIVE25's `SET_AI_NET` puts the
  four `stihellhound_*` escorts on `M1Train` — the mobile-net assignment
  is the record's own spelled mechanism for putting an escort on a moving
  frame.
- *Pickup authorization*: the rescue is an animation gate, not a
  directive — OBJECTIVE11's `ANIM_STATE [ANIM, [NAME, [got_sparks]],
  [STATE, [EXECUTED]]]`. The archive carries the pickup sequence's own
  members beside the control member: `pickups.zrd`, `sparks_pkup.zrd`
  (the archive's longest) and `ladder.zrd`.
- *Post-pickup terminal state*: spelled above — two nap-armed latches,
  and the failure path kills the pickup gate itself. The objective classes
  the record spells are four `PRIMARY` blocks (1, 7, 8, 11 — OBJECTIVE8's
  identity stops after its slot ordinal `Int(4)` with no `MSG_*` text, as
  does the `SECONDARY` OBJECTIVE12's after `Int(11)`).

Mission start data, spelled at record level: `MISSION_TIMER` 0,
`PLAYER_INIT` airborne at `(-7634, 600, -1363)` with **zero** spelled
velocity (`0.8`, `180.0`) — the authored start as the record carries it,
not a walkthrough's.

## The M16-specific gaps

All 37 conditions lower and 122 of 133 calls bind, yet no program
assembles, `validation` never runs, the only unmet row is
`call_arguments` and M16 is not ready:

**`STOP_QUEUED_SOUNDS` never registers** (7 sites, spelling 1, 3, 3, 4, 7,
9 and 14 names). The measured shapes produce one signature each, and the
9- and 14-name signatures exceed `MAX_CALL_ARGS` (8); one unfit signature
makes the whole spec unfit, so the key registers nothing and all seven
sites — even the one-name site of OBJECTIVE7 — refuse `unknown host call`
(the sole `unbound_keys` entry is `` `STOP_QUEUED_SOUNDS`: binding
`STOP_QUEUED_SOUNDS`: too many arguments ``). The original's parser copies
at most ten names (`cmp esi, 0xa` at `0x4688bc`, the M01-LC-D finding), so
the fourteen-name site of OBJECTIVE37 is faithful data the original
truncates; carrying the name list as one list argument — the shape the
other list-taking keys already use — is the follow-up this gap names,
tracked as **M16-B-FU1** (#1252).

**OBJECTIVE24's four bare words are not directive keys** (`Change`, `to`,
`mobile`, `net` — a designer note labelling the `SET_AI_NET` site below
them). They are unmeasured keys, so they register nothing by design and
the four sites refuse `unknown host call`. The engine-image test reads the
directive-key string table the M01-LC-A finding measured (file offsets
`0x226040..0x2268c0`, 87 entries): every completion-effect key M16 spells
is present, `NAP_OBJECTIVE_WHEN_I_COMPLETE`'s string begins at the
extent's end `0x2268b0` as measured, and none of the four words is an
entry. `Change`, `to` and `mobile` are not standalone strings anywhere in
the image (the `Change` substrings live inside `Save Changes?`,
`Change Net Params` and C++ mangled names); `net` occurs exactly once as a
standalone string, at `0x22b75c` inside the airframe-parameter name
cluster beside `max_accel` and `cannon_fire_delay` — a different lookup
with no mission-directive meaning. Under the measured "the parser looks up
only the keys it uses" rule the original ignores them, so the record's
unmeasured disposition is the faithful one.

## Not claimed

No playthrough, difficulty, optional or presentation row is covered: the
wrong-actor, wrong-session and repeated-event halves of the three
priorities are runtime observations and need M16-C plus human play. The
precedence of the two nap-armed latches — whether a success and a failure
armed in the same window can conflict — stays a runtime question. The
census keeps M16 unready under `call_arguments` only (its conditions all
lower and its identity resolves) and the campaign gate stays closed;
nothing is `verified_original`.

## Mutation checks

Each applied, run against `accept_m16_b_`, observed and reverted:

| Mutation | Result |
| --- | --- |
| `cs_content::mission_control::terminal_outcome_of`: `INSTANTLOSS` → `None` | 3 failures (vocabulary partition's implemented pair, lowering "eleven refused" — a twelfth site refuses as `INSTANTLOSS` becomes an unknown host call, synthetic two-latch record) |
| `cs_script::bindings::MAX_CALL_ARGS`: 8 → 16 | 2 failures (synthetic poisoning — `unbound_keys` empties as the 9-name signature fits; retail lowering — the refused count drops 11 → 4 as the sound key registers) |
| `m16_b.rs`: expected block count 37 → 36 | 3 failures (vocabulary block count, block-graph numbering `1..=37`, lowering condition-verdict count) |

# M01-PLAY-AI-ARM-ORDER — the arming pass out-ran the session re-open by one schedule stage

Filed as #1518 from #1509 (`M01-PLAY-AI-RESTART`). Nothing in this note is
original-data evidence; the defect and the fix are entirely the project's
own scheduling.

## The defect

Three systems decide this story, in schedule order:

| System | Stage | What it does on a restart |
| --- | --- | --- |
| `arm_mission_ai` | `FixedPreUpdate` | registers each flown record's actor into `WeaponSession`; writes the binding's generation from `MissionWeapons` |
| `publish_mission_weapon_input` → `ensure_mission_weapons` | `FixedPostUpdate` | compares `MissionHost::served()` to `MissionWeapons::session()` and re-opens on mismatch |
| `mission_host_restart` | `PostUpdate` | consumes `MissionHostRestartRequest`, mints the generation, respawns the roster |

`mission_host_restart` runs *after* the fixed schedule, so the first fixed
tick of the update after a restart found the respawned records already
present while `MissionWeapons`/`WeaponSession` still served the dead
generation. `arm_mission_ai` registered each fresh `ActorId` against the
old session, which refused —

```
<record>: the weapon session refuses the guns: the actor belongs to session 2, but this session is 1
```

— `ensure_mission_weapons` re-opened later the same tick, and from the
second tick every record armed correctly. But `MissionAiReport::refusals`
is replaced only at spawn and never cleared per tick, so the transient
refusal stayed latched for the rest of the run: one tick of unarmed guns
plus a permanently misleading report entry. Unreachable before #1509
because no AI aircraft flew after a restart, so the pass had no candidates.

It was visible, not guessed: running
`accept_m01_play_ai_arm_order_composed_restart_arms_into_the_fresh_session`
against the unfixed code produced exactly the refusal above.

## The choice the task asked for

(a) **Re-open the session before the arming pass** — chosen. `arm_mission_ai`
now calls `ensure_mission_weapons` before it reads the session: the same
production staleness check `FixedPostUpdate` already runs, moved earlier so
the pass always arms into the session the host serves. The session the
registration sees can no longer differ from the one the director re-opens —
the guarantee is the check itself, not the two systems' relative order.

(b) *Not recording a refusal the same tick resolves* — rejected: a session
mismatch is a real signal everywhere it can still occur (a session that
cannot serve is not a scheduling artifact), and suppressing the entry would
hide it rather than keep it honest. The fix removes the cause; the refusal
path is unchanged.

What did **not** change: the refusal stays a refusal. `WeaponSession`
still refuses a foreign-generation actor; the pass simply never presents
one now, because `ensure_mission_weapons` runs first.

## Consequences

- The post-restart transient is gone entirely, not just the report entry:
  the first tick arms into the fresh session, so the guns are never unarmed.
- `ensure_mission_weapons` became `pub` and is re-exported through
  `mission_session` — a wiring-only change so both schedule stages share
  one staleness definition.
- The `#1509`/`#1436` "no `MissionAiReport` refusal" expectations are
  unchanged and stay green; the new suite adds the leg they never checked
  (refusals *after* the post-restart ticks, not only before them).

## Evidence

`accept_m01_play_ai_arm_order_` (synthetic composed leg + retail M01
roster leg) pins: refusals empty after restart + two fixed ticks, and every
flown record's `WeaponActorBinding.generation` equal to the fresh
`MissionWeapons::generation()`. The evidence harness
(`evidence_report_m01_play_ai_arm_order`) re-runs the retail restart and
records the census in `arm-order-census.json`; the report lives at
`docs/findings/evidence/M01-PLAY-AI-ARM-ORDER.json`.

# M01 DEDG groups 2 and 4: no member is needed — the original counts 0

Task #1483 (`M01-PLAY-TRIGGERS-DEPLETION`), capability `retail`, 2026-10-16.
Nothing here is `verified_original`: no original executable ran, and the
engine image was not re-read for this task — the evaluator facts below are the
already-committed static findings.

## The question

M01's eight `DEDG` sites name groups 1, 2, 3 and 4. The composed run spawned
members only for 1 and 3 (and 5), so `EnemyGroupDepletion` over 2 and 4 could
never answer. The task asked which actors carry groups 2 and 4.

## Measured

* **Records.** `aiv.zrd` group (positional field 4 → `vehicle+0x388`,
  `0x47c694`): group 2 = `{rusdevastator_3}`, group 4 = `{bsfury_2}`
  (docs/findings/2026-10-15-m01-aiv-layout.md,
  docs/findings/2026-10-15-m01-play-triggers-observation.md). No other `aiv`
  record carries 2 or 4.
* **Both records are `enabled == 0`**, which the original's spawn loop skips
  (`0x474d10`, #1453). They never become vehicles.
* **Zeppelins carry no group.** `zeppelins.zrd` is a keyed 26-key grammar with
  no group key (`ZeppelinKey::ALL`); the world's ground units have no records
  (docs/findings/2026-10-14-m01-play-actors-premise-and-census.md). So no other
  actor can carry group 2 or 4.
* **The counter is a registry count.** `0x4658d0` counts members of the vehicle
  registry (`0x71dabc`) with `+0x91d == 0` and `+0x388 == group`
  (docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md).
  `0x465910` then fires iff `living + pending <= remaining`; M01 names no
  generator, so `pending` is 0. There is no "group unknown" state in the
  original: a group with no registry member counts **0**.

## Conclusion

Giving groups 2 and 4 "real members" would be the wrong fix: it would invent
vehicles the original never spawned. In the original, blocks 16/41/42 (group 2,
`remaining 0`) and 20 (group 4, `remaining 0`) have a count of 0 from the first
tick. Block 43 (group 4, `remaining 1`) likewise. Whether each fires is then
decided by its other conditions (`ObjectiveAwake{53}`, lifecycle).

## What changed

* `MissionSkippedAircraft` (mission_actors.rs) keeps the records
  `enabled == 0` drops.
* The observer (mission_session/observation.rs) reports a group for every
  `aiv.zrd` record's group once the actor spawn ran; a skipped-only group
  counts 0. A group no record carries stays unobserved, and
  `EnemyGroupDepletion` over it still answers `false`.
* Reviewer refinement (2026-10-16, bunny-alpha-1): the skip's zero is never
  borrowed by a group an **enabled** record also carries. Such a group could
  have had a registry member, so a record of it this composition refused to
  spawn leaves the count unknown rather than 0 — the mixed-group CI leg
  `accept_m01_play_triggers_depletion_a_refused_enabled_record_keeps_its_group_unknown`
  pins it. M01 has no mixed group, so its measured counts are unchanged.

## Unmeasured

* Whether the original's *other* evaluator paths (not `0x4658d0`) treat a
  never-registered group differently. None is known.
* The `0x465850` per-member field rewrites do not apply to groups with no
  member; they remain a named residual for groups 1 and 3
  (`DEDG_MEMBER_FIELD_REWRITES`).
* The count of group 5 (the `player` record's group) is still the
  composition's four spawned aircraft; no M01 `DEDG` reads it.

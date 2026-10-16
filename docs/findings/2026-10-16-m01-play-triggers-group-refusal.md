# M01 DEDG groups: a partial spawn is an unknown count, not a smaller one

Task #1512 (`M01-PLAY-TRIGGERS-GROUP-REFUSAL`), capability `retail`,
2026-10-16. Nothing here is `verified_original`: no original executable ran.
The evaluator facts are the already-committed static findings; the new
material is the observer's accounting rule and the fail-open it closes.

## The question

`docs/findings/2026-10-15-m01-play-triggers-group-membership.md` established
that the original's group counter (`0x4658d0`) counts **vehicle-registry
members** and that every *enabled* `aiv.zrd` record enters that registry at
spawn (`0x474d10`, #1453). #1483 then made the observer report `0` for a
group whose only records are the original's `enabled == 0` skips, and left
any group an enabled record of *this* composition failed to spawn
unobserved — but only when that enabled record never produced an entity for
another reason. The gap the reviewer of #1483 filed as this task: when a
group's enabled records **partially** spawn — one produces an entity, a
sibling is refused — the observer seeded the group from the one spawn row
and reported the live count of the fraction that spawned. A `DEDG [group,
0]` gate could then complete on an undercount the original never computes.

## Measured

* **The counter is a registry count.** `0x4658d0` counts members of the
  vehicle registry (`0x71dabc`) with `+0x91d == 0` and `+0x388 == group`
  (docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md).
* **Every enabled record enters the registry at spawn.** The spawn loop at
  `0x474d10` skips `enabled == 0` records and registers the rest
  (docs/findings/2026-10-15-m01-aiv-layout.md). In the original there is
  therefore no state in which an enabled record names a group but is not a
  registry member — the registry count and the enabled-record count are the
  same number.
* **Our composition is weaker than the original's spawn.** The composition
  may refuse an enabled record (no stored pose, an unbuildable dispatch, a
  body-spec or allocator refusal — `MissionActorSpawnReport::refusals`).
  Once that happens the true registry count is unknowable: the original
  would have counted the refused record, this world does not hold it.
* **The only honest answers.** For a group some enabled roster record
  carries: all of them spawned → the live entity count is the registry
  count; any refused → **unobserved**, and `EnemyGroupDepletion` answers
  `false` ("an unrecorded group is unknown, not an empty one"). For a group
  whose only records are `enabled == 0` skips: **0**, #1483's measured
  answer, kept. For a group no record carries: unobserved, unchanged.

## What changed

* `mission_session/observation.rs` step 3 now matches every enabled roster
  record against the spawn report's rows as a **multiset** — one spawn row
  stands for exactly one roster record, so two same-named records cannot
  share a single spawn — and seeds a group's row only when the admitted
  count equals the expected count. A refused sibling leaves the group with
  no row at all rather than an undercount.
* The skipped-only zero and the no-record unobserved arms are unchanged.
* `accept_m01_play_triggers_group_refusal_a_partial_spawn_stays_unobserved_and_never_completes`
  (CI): a synthetic stage with groups 5 (spawned + refused), 6 (skip-only)
  and 7 (two spawned) observes `{6: 0, 7: 2}`, never reports 5, and never
  completes group 5's `DEDG` gate — while 6's completes on 0 and 7's
  completes only after both members leave the world.
* `accept_m01_play_triggers_group_refusal_retail_m01s_groups_are_all_observed`
  (`retail`): M01's enabled records all spawn, so every group is observed
  exactly as before — `{1: 4, 2: 0, 3: 1, 4: 0, 5: 4}` (5 is the `player`
  record's group, counted on the composition's four spawned aircraft; no M01
  `DEDG` reads it).

## Unmeasured

* Whether the original has any group-evaluation path other than `0x4658d0`
  that could observe a refused record's absence differently; none is known.
* The duplicate-name multiset rule is a composition-level accounting choice,
  not a measured original behavior: M01 has no two same-named records, and
  the refusal case it guards is one the original cannot reach.

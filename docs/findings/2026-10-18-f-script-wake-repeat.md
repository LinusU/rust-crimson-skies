# F-SCRIPT-WAKE-REPEAT (#1524): wake-role effects repeat on every wake

Date: 2026-10-18. Capabilities: `retail` (read-only `$CS_GAME_DIR`) plus
build/test. No original run; nothing here is `verified_original`.
Implementer: `sonnet-2`, continued by `Devin SWE-2/swe2-max-1`; reviewed by
`Devin SWE-2/swe2-max-1` in a fresh session (same model — not independent
evidence).

## Measurement

The wake call `0x469af0` runs the per-record wake effect list on every wake,
from dormant, nap or done; only `RESET_TIMER` and `HIDE_OBJ` are dormant-only
(`2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`,
*`BEGIN_DORMANT`*). So `WAKEUP_ENEMIES`, `WAKEUP_TURRETS`,
`WAKEUP_ZEP_TURRETS`, `WAKEUP_GENERATOR`, `WAKE_ANIM`, `WAKEUP_SOUND_GROUP`
and `COUNTER ON_WAKEUP` repeat. The runtime emitted each at most once per
session because `consumed` keys on (`session`, `source`, `sequence`).

## Affected content (M01, retail `$CS_GAME_DIR`, label `M06`)

Read from the composed stage (`stage.host.control` and the recovered
`EngineObjectives`): 24 of M01's 58 blocks carry a wake-role site, and **all
24 are loaded Dormant**. A block repeats a wake only if the record's effect
graph can wake it twice. The ones whose graph names more than one source:

* block 3 (`OBJECTIVE4`, `WAKEUP_SOUND_GROUP`): `NAP`ped by block 2 (20 s) and
  block 47 (20 s) — a second nap re-wake runs the sound-group wake again;
* block 26 (`FEED_GENERATOR`): woken by blocks 2 and 47;
* block 48 (`WAKE_ANIM`): woken by blocks 9 and 16.

Every other wake-site block has one waking/napping source. The win chain's own
`NAP` re-wakes (blocks 12–14 among them) are each the block's *first* wake, so
they were already right. Whether blocks 2 and 47 (or 9 and 16) both complete in
one real run is not measured here; M01's headless route
(`2026-10-16-m01-play-triggers-route.md`) does not need a repeat. The
resolving measurement stays the one the task names: the wake call's own call
site inside a stepped game loop, with an observable effect (a second
`WAKE_ANIM`).

## Decision

`MissionState` keeps a per-block wake ledger. The first wake of a block takes
the key the completion path uses (`source`, action index + 1), so a
completion after it still emits nothing twice. The n-th later wake takes the
key `SEQS_PER_ITEM * (SEQS_PER_ITEM * n + action index + 1)` — a multiple of
65, which no pending-item sequence can be (`(ordinal + 1) * 65 + (action +
1)` keeps a nonzero residue) and which is always past the completion path's
`0..=64` — so each wake is admitted once and a retry or restore of the same
wake is still suppressed by `consumed`. A site whose packed slot does not
fit a `u32` is not emitted (fail closed) rather than aliased. `consumed` is
not cleared, so the exactly-once guarantee for every other effect family is
unchanged.

The ledger (`awake`, `wakes`) is in the save record, which is now
`SNAPSHOT_VERSION` 6; version 5 records are refused. A restored session
therefore neither repeats the wake it was on nor loses the next one. Both
ledger fields stay inside the program: a facts row naming a block the
program does not carry is tolerated by the emit pass but never recorded, so
the record's `awake ⊆ wakes` check is exactly the live invariant. The
dormant-only pair stays right: those sites only ever follow a first wake in
M01 (a block loaded Dormant), and the host's engine model owns the
"prior state was 0" guard.

## Limits

* Consumers keep their own per-key `applied` sets, so a repeated `WAKE_ANIM`
  now reaches `MissionAnimationPlayer::activate` a second time; what the
  player does with an already-running record is the animation slice's
  question, not measured here.
* The tick boundary of the wake emission is still the designed reading
  (`2026-10-16-m01-play-triggers-route.md` §2).

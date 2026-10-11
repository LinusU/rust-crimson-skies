# F28-AE1: the declared area effect now reaches stable recipients

## What changed

`OrdnanceRuntime::open_area` (a triggered item) and `open_detonation_area` (a
guidance-loss `Detonate`) apply an item's declared `AreaEffect` once, on the
tick the item ends:

- Recipients are the supplied `TargetPath` actors (the same input the proximity
  fuse reads) whose current position is strictly inside the radius measured
  from the item's position. Each recipient is a stable `ActorId`, listed once.
- The area is an `ActiveArea` in an `AreaLedger` with id `area/<session>#<n>`,
  expiring at `opened_at + lifetime_ticks` with the status ledger's boundary
  rule, reported once by `advance_areas`.
- A second application for the same item is `AreaAlreadyOpened`.
- The ledger is per session generation; a restarted session inherits nothing.
- The area holds no pose, no Avian body and no particle. It is separate from
  damage routing and status effects.

`DECLARED_FIELDS_WITHOUT_CONSUMER` is now empty.

## Limitations (not claimed)

- Radius-from-detonation is the shape **measured** for the guidance-loss blast
  (`2026-10-11-f28-c-2-lost-target-blast-measured.md`). For a fuse-triggered
  item the original's area semantics are **unmeasured**: this is the same shape
  applied by analogy, not an observed behavior.
- Recipient membership is fixed at the application pass. Whether the original
  keeps affecting actors that enter a lingering area afterwards is unmeasured.
- What the area does to a recipient beyond membership (per-tick damage, choke,
  markers) is still delivered only by the item's declared channels and status
  effects; no per-tick area rule is invented.
- Nothing here is `verified_original`.

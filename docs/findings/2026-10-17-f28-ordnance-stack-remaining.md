# Ordnance stack remaining count (task #758)

F46-B found no authoritative per-launcher ordnance count. `OrdnanceSession`
now keeps one: `stack_remaining(shooter, ordnance)` returns the declared
`StackLoad` capacity and the units left.

## Designed semantics (not measured)

- One accepted launch spends one unit of the declared capacity. Whether the
  original spends more than one per launch (salvos) is unmeasured.
- A launch against a spent stack is refused by name
  (`OrdnanceOrderRefusal::StackExhausted`) before the launcher pose is read; it
  spends nothing, emits no effect and no network event.
- Boosters have no stack (`None`); nitro capacity stays the runtime's.
- `close` clears the counts with the loadout; a new session starts full.
- `HudFrame::ordnance` rows carry `capacity_units` and `remaining_units`; a
  spent launcher stays a row (zero is a gauge state).

## Unknown

Which `mgindicator` shows which row, and whether the original refills or
reloads a stack mid-mission, remain unmeasured (see the F46-B finding).

Tests: `accept_f28_c_stack_*` in `crates/cs_app/tests/accept_f28_c_ordnance_session_step.rs`;
the HUD row assertion is in `accept_f46_b_ordnance_rows_are_the_sessions_launchable_components`.

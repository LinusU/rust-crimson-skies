# F33-C2: one owner for the ends-action lifecycle split

`cs_sim::damage::LifecycleKind::ends_action` is now the single statement of
the designed rule: `Destroyed`, `Despawned` and `MissionRemoved` end an
actor's ability to act; `PilotBailout` and `OwnershipCaptured` do not.

Delegating readers (behavior unchanged):

- `cs_sim::allies::AllyStatus::ends_action` (maps each terminal status to its
  `LifecycleKind`);
- `cs_sim::targeting`'s private `ends_targeting`;
- `cs_app::damage`'s private `lifecycle_ends_action`.

Tests: `accept_f33_c2_` in `crates/cs_sim/tests/accept_f33_c2_ends_action_owner.rs`
assert the exact set, that capture and bailout do not end action, and that
`AllyStatus` agrees with the predicate.

The split is designed, not measured; nothing here claims original behavior.
Evidence: synthetic only, at most **checked**. Follows
`2026-10-12-f33-c1-grounded-actor-across-damage-consumer.md`.

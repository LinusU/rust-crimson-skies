# F27: Guns, ammunition, hardpoints, and ballistic hits

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F23, F29, F30.
**Owner paths:** `crates/cs_sim/src/weapons/guns.rs`; `crates/cs_content/src/weapons.rs`; `crates/cs_app/src/weapons.rs`; `tests/`.
**Shared contract:** [FLIGHT-PHYSICS](../docs/contracts/FLIGHT-PHYSICS.md).

## Deliverable and interfaces

WeaponDefinition separates mount, caliber, ammunition type, rate, muzzle velocity, lifetime, spread, damage channels, effects and sound. WeaponState tracks selected banks, cooldown in ticks, ammunition and disabled mounts. Fire intents are resolved once by authoritative simulation.

## Non-negotiable behavior

1. Enumerate all actual ammunition ids from original data. Slug, armor-piercing, dum-dum and explosive are discovery leads; no unverified multiplier table is hardcoded.
2. Mount transforms come from the live aircraft hierarchy/damage state, not a fixed center-screen origin. Convergence and inherited velocity are explicit verified rules.
3. Sweep projectiles between previous and current position against relative target motion as needed; one projectile applies a hit at most once.
4. Define friendly fire, self-hit exclusion, penetration, ricochet and ammo switching by evidence or mark unknown. Do not invent simulator features unsupported by game content.
5. Consumption, sound and muzzle effects derive from accepted fire events; no ammo drain from a denied input or duplicate network packet.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** At high velocity a projectile crosses a thin target and hits once.
**AC02:** A disabled wing gun emits neither projectile nor sound nor ammo decrement.
**AC03:** Switch gun bank during cooldown without duplicating fire or refilling ammo.
**AC04:** Original ammo/loadout audit maps every type to its behavior and damage consumer.

## Bounded implementation slices

### F27-A: Define weapon/ammo schemas and fire-event tests

Dependencies: F14-A, F23-A, F29-A, F30-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f27_a_`. Minimum scenario: At high velocity a projectile crosses a thin target and hits once.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F27-B: Implement gun cadence, mounts and swept ballistics

Dependencies: F27-A, F14-C, F23-C, F29-C, F30-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f27_b_`. Minimum scenario: A disabled wing gun emits neither projectile nor sound nor ammo decrement.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F27-C: Wire damage, effects, selection and authoritative ownership

Dependencies: F27-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f27_c_`. Minimum scenario: Switch gun bank during cooldown without duplicating fire or refilling ammo.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F27-D: Verify every original gun/ammunition combination and convergence rule

Dependencies: F27-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f27_d_`. Minimum scenario: Original ammo/loadout audit maps every type to its behavior and damage consumer.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

Exact ammunition multipliers and ballistic parameters are not established by the public manual. Discover and validate them; web summaries are not authoritative tuning data.

## References

[S13](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

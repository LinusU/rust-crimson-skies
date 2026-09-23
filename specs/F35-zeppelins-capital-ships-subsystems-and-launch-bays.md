# F35: Zeppelins, capital ships, subsystems, and launch bays

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F11, F20, F28, F29, F31, F34.
**Owner paths:** `crates/cs_sim/src/capital/`; `crates/cs_content/src/capital.rs`; `crates/cs_app/src/capital.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

CapitalShipDefinition contains trajectory, engines, gas/structural sections where supported, turrets, weapon bays, hangar/launch sockets, docking anchors, cargo and ownership state. Overall health alone is insufficient for mission interactions that target subsystems.

## Non-negotiable behavior

1. Subsystem destruction changes the appropriate behavior: movement, weapon access, spawning, vulnerability or mission condition. Each rule has source evidence.
2. Broadside/weapon bay openings are explicit time-varying weakpoint states. A closed bay is not equivalent to an always-hittable invisible health bar.
3. Aircraft spawn/release from verified bay transforms, inherit carrier motion and gain dynamic authority once. Destroying a launch bay affects later spawns according to the script.
4. Capture is a staged ownership transaction; guns, targeting, docking eligibility and AI relation switch coherently.
5. A cinematic crash or sinking phase can remain dangerous while its actor is dying if evidenced; destruction and despawn are separate.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Disable engines and measure motion response without destroying the hull automatically.
**AC02:** Hit a weakpoint before, during and after its exposure window.
**AC03:** Destroy a bay with a pending launch and prove no duplicate aircraft appears.
**AC04:** Capture a moving ship while projectiles are in flight; relation and hit policies stay consistent.

## Bounded implementation slices

### F35-A: Define capital-ship subsystem and bay contracts

Dependencies: F11-A, F20-A, F28-A, F29-A, F31-A, F34-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f35_a_`. Minimum scenario: Disable engines and measure motion response without destroying the hull automatically.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F35-B: Implement movement, weakpoints and turrets

Dependencies: F35-A, F11-C, F20-C, F28-C, F29-C, F31-C, F34-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f35_b_`. Minimum scenario: Hit a weakpoint before, during and after its exposure window.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F35-C: Wire launch, capture, cargo and staged destruction

Dependencies: F35-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f35_c_`. Minimum scenario: Destroy a bay with a pending launch and prove no duplicate aircraft appears.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F35-D: Validate all original capital-ship mission interactions

Dependencies: F35-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f35_d_`. Minimum scenario: Capture a moving ship while projectiles are in flight; relation and hit policies stay consistent.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

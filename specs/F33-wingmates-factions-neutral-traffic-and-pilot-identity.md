# F33: Wingmates, factions, neutral traffic, and pilot identity

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F30, F31, F32.
**Owner paths:** `crates/cs_sim/src/allies.rs`; `crates/cs_content/src/pilots.rs`; `crates/cs_app/src/roster.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

PilotId, FactionId and ActorId are distinct. Wingmate assignments, aircraft/loadouts, voices, allegiance and survivability policies are sourced per mission. Neutral aircraft/traffic only appear where content or an explicitly enabled modern sandbox requests them.

## Non-negotiable behavior

1. Faction relations are versioned state, supporting scripted hostility, alliance, capture and neutral protection. Never bake enemy status into mesh names or paint colors.
2. Do not assume in-flight wingmate commands existed. Preserve verified original autonomous wingmates; optional command UI is a designed extension and not a core substitute.
3. Pilot identity survives an aircraft swap where authored; aircraft damage does not automatically transfer to a different vehicle.
4. Ally deaths and protected-neutral losses report specific events to mission logic; not every ally is an immortal escort.
5. Voice selection and subtitles resolve through catalog ids; a random line cannot replace a missing mission dialogue.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** A captured vehicle changes faction without changing its geometry id.
**AC02:** Wingmate loadout matches briefing selection and resets appropriately on retry.
**AC03:** An ally killed during a cutscene cannot later fire from a stale actor.
**AC04:** Neutral traffic omitted by the authored mission is not spawned by a global population system.

## Bounded implementation slices

### F33-A: Define pilot/aircraft/faction separation

Dependencies: F14-A, F30-A, F31-A, F32-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f33_a_`. Minimum scenario: A captured vehicle changes faction without changing its geometry id.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F33-B: Implement allegiance and wingmate assignment rules

Dependencies: F33-A, F14-C, F30-C, F31-C, F32-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f33_b_`. Minimum scenario: Wingmate loadout matches briefing selection and resets appropriately on retry.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F33-C: Connect AI roles, dialogue voices and mission callbacks

Dependencies: F33-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f33_c_`. Minimum scenario: An ally killed during a cutscene cannot later fire from a stale actor.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F33-D: Verify original roster, relation changes and allied behavior

Dependencies: F33-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f33_d_`. Minimum scenario: Neutral traffic omitted by the authored mission is not spawned by a global population system.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md); [S14](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

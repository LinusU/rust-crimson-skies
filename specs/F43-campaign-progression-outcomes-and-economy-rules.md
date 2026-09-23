# F43: Campaign, progression, outcomes, and economy rules

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F14, F29, F37, F39, F48.
**Owner paths:** `crates/cs_sim/src/campaign/`; `crates/cs_content/src/campaign.rs`; `crates/cs_app/src/campaign.rs`; `tests/`.
**Shared contract:** [STATE-TRANSACTIONS](../docs/contracts/STATE-TRANSACTIONS.md).

## Deliverable and interfaces

CampaignDefinition is an ordered/conditional graph of original missions, briefings, interludes, rewards, roster availability and ending states. CampaignState stores current progression, owned aircraft/resources, best/latest records, difficulty and profile identity. MissionOutcome is a single immutable transaction input.

## Non-negotiable behavior

1. Only completed authorized outcomes can change progression. Debug/synthetic/modded evidence runs use separate profiles or explicitly mark modified progression.
2. Rewards, purchases and unlocks are idempotent. A repeated result packet after restart cannot grant cash twice.
3. Mission replay and progression resume are distinct. Replaying an earlier mission does not overwrite the selected next mission or erase later unlocks.
4. Failure/retry/skip rules and economy amounts must be extracted/observed. Do not invent a three-failure skip rule without verifying the target edition.
5. Authored chapter-start changes are explicit transitions, not accidental carryover of damage, plane or capital ship state.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Apply the same success outcome twice; progression and currency change once.
**AC02:** Replay an old mission with a worse result; preserve best while recording latest as appropriate.
**AC03:** Crash during reward save; recovery applies either the old or the new complete transaction.
**AC04:** Complete final mission and verify ending/unlock state without manufacturing an extra mission.

## Bounded implementation slices

### F43-A: Define campaign graph and transactional outcome schema

Dependencies: F14-A, F29-A, F37-A, F39-A, F48-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f43_a_`. Minimum scenario: Apply the same success outcome twice; progression and currency change once.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F43-B: Implement progression, replay and reward transactions

Dependencies: F43-A, F14-C, F29-C, F37-C, F39-C, F48-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f43_b_`. Minimum scenario: Replay an old mission with a worse result; preserve best while recording latest as appropriate.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F43-C: Wire briefing/loadout/results/save transitions

Dependencies: F43-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f43_c_`. Minimum scenario: Crash during reward save; recovery applies either the old or the new complete transaction.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F43-D: Play through the complete original campaign and verify every progression edge

Dependencies: F43-C. Required capabilities: retail.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f43_d_`. Minimum scenario: Complete final mission and verify ending/unlock state without manufacturing an extra mission.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

[S13](../docs/research/SOURCES.md); [S14](../docs/research/SOURCES.md); [S15](../docs/research/SOURCES.md)

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

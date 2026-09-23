# F53: Mod mounts, custom content, and compatibility signatures

**Target:** 2000 PC original-data reimplementation. **Initial status:** not implemented; not original-verified.
**Prerequisite features:** F04, F14, F37, F44, F48.
**Owner paths:** `crates/cs_content/src/mods/`; `crates/cs_assets/src/mods.rs`; `crates/cs_app/src/ui/mods.rs`; `tests/`.
**Shared contract:** [IDENTITY-CONTENT](../docs/contracts/IDENTITY-CONTENT.md).

## Deliverable and interfaces

Mods are opt-in mounts with manifest, stable id, version, dependencies, content overrides and compatibility hash. Prefer declarative custom data and privately resolved original assets. A mod can replace art or tuning without editing the retail installation; source formats and new canonical formats are distinct.

## Non-negotiable behavior

1. Validate dependencies, cycles, collisions, declared engine range and budgets before enabling. Deterministic mount order is visible.
2. No native DLL/plugin execution from original or mod archives. Sandboxed mission IR uses the same bounded validator as original adapters.
3. Modified gameplay marks sessions/saves/replays/network handshakes. Cosmetic-only classification requires a hash-policy definition, not an author assertion alone.
4. Export custom blueprints/manifests without copying retail textures/models. Tools explain unresolved original dependencies.
5. Hot reload only in developer/paused safe boundaries; no mid-tick weapon or collision mutation in verified sessions.

## Acceptance tests

These are minimum discriminating tests. Add regression cases for every discovered variant. Tests call production code, not parallel test-only implementations. They must be selected with a nonempty task-specific test prefix and must fail when the relevant behavior is removed.

**AC01:** Two conflicting mods produce a deterministic explicit precedence report.
**AC02:** A malicious relative path or cyclic dependency is rejected.
**AC03:** A tuning mod changes compatibility hash and prevents joining a mismatched stock lobby.
**AC04:** Disable a mod and reopen a dependent save without destructive fallback.

## Bounded implementation slices

### F53-A: Define mod manifest and override validation

Dependencies: F04-A, F14-A, F37-A, F44-A, F48-A. Required capabilities: ordinary build/test.

Work only on this stage. Define typed inputs/outputs and a minimal synthetic fixture first; do not jump ahead to a whole runtime.

Required task-test prefix: `accept_f53_a_`. Minimum scenario: Two conflicting mods produce a deterministic explicit precedence report.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F53-B: Implement safe mounts and compatibility signatures

Dependencies: F53-A, F04-C, F14-C, F37-C, F44-C, F48-C. Required capabilities: ordinary build/test.

Work only on this stage. Implement the smallest production path that exercises the declared behavior; keep unrelated systems unchanged.

Required task-test prefix: `accept_f53_b_`. Minimum scenario: A malicious relative path or cyclic dependency is rejected.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F53-C: Add selection, diagnostics and private export tooling

Dependencies: F53-B. Required capabilities: ordinary build/test.

Work only on this stage. Wire the implemented path into its actual producer and consumer; include teardown/retry and error propagation.

Required task-test prefix: `accept_f53_c_`. Minimum scenario: A tuning mod changes compatibility hash and prevents joining a mismatched stock lobby.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.


### F53-D: Verify source protection, mod isolation and reproducible load order

Dependencies: F53-C. Required capabilities: ordinary build/test.

Work only on this stage. Collect actual integration/reference evidence; repair discovered regressions without weakening the specification. Missing capabilities or original data block this stage.

Required task-test prefix: `accept_f53_d_`. Minimum scenario: Disable a mod and reopen a dependent save without destructive fallback.

Before editing, list the specific functions/files and one observable failure. If the slice exceeds one format variant, one focused system behavior or one bounded UI path, follow TASK-SPLITTING.md rather than producing a giant change. Preserve all parent acceptance criteria.

## Evidence and completion

Record the actual input fingerprint and consumer trace. Synthetic fixtures alone cannot certify original-data behavior.

A code/test pass awards at most **checked**. The reviewer must inspect runtime wiring, error paths, stale-state handling and test sensitivity. Data-dependent claims require source span, installation hash and an independent reference/probe. Visual, audio and ordinary-play claims require their respective capabilities. Developer placeholders never satisfy a retail acceptance case.

## Research boundary

No unrecorded compatibility assumptions are allowed.

## References

Project-designed engineering contract; no original behavior is asserted by the design alone.

The requirements and tests are independently authored engineering designs. Source labels distinguish observed leads from measured original semantics; consult the evidence ledger before changing gameplay values.

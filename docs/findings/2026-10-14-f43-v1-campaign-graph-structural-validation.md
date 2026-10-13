# F43-V1: campaign graph structural validation — cycles, reachability, endings

**Task:** #625, branch `rally/625-validate-campaign-graph-structure-cycles`,
prefix `accept_f43_v1_`. Validation stage of
`specs/F43-campaign-progression-outcomes-and-economy-rules.md`; shared
contract `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only — **no `retail`**, so nothing here is a claim about the
original game.

## What this stage decided

F43-A's `CampaignDefinition::try_new` and `CampaignGraph::try_new` validated
only local shape: unique ids, a declared entry, no dangling targets, one edge
per condition, no edges out of an ending, no edge-less non-endings. Three
*structural* defects could still be declared, and none of them is a runtime
decision. This stage makes the declaration boundary refuse all three, and
records — per the task — which rules are measured and which are designed.

**Every rule added here is designed, not derived from original evidence.** No
original campaign data was read: the real campaign cannot yet be declared
through this schema at all (its import is a later task), and the original's
tolerance for cyclic or unreachable campaign content is unmeasured. These are
engine policies chosen so a bad import fails loudly at the boundary instead
of hanging or dead-ending a run later. If F43-D measures original campaign
shapes these rules would refuse, they must be revisited.

### Rule 1: every node must be reachable from the entry — error, not warning

`try_new` computes the reach set from the entry over edges of **any**
`EdgeCondition` and refuses the first node outside it with
`CampaignError::UnreachableNode`, which names the stranded node.

- *Why an error:* a node no edge path reaches can never be selected, never
  report an outcome, never open a roster gate — it is dead weight, and dead
  weight in declared content is almost always an importer or authoring
  defect (a missing edge, a stale node). The schema has no warning channel
  weaker than a refusal, and silence is exactly how a wrong offset survives
  an import.
- *Why all conditions count:* `apply_outcome` moves `current` along whichever
  edge matches the reported outcome, so a node held only by a `Defeat` or
  `Abort` edge is still reachable campaign content. Pinned by
  `accept_f43_v1_a_defeat_only_branch_still_counts_as_reachable`.
- The runtime layer mirrors this: `GraphError::UnreachableNode` refuses the
  same defect in `CampaignGraph::try_new`, because unreachable content is a
  defect at any layer, not only at the declared one.

### Rule 2: the victory path must reach an `Ending` from every node

Every non-`Ending` node — missions *and* interludes — must declare a
`Victory` edge, and the directed graph formed by those edges alone must be
acyclic. `try_new` walks the single victory edge out of each node; a walk
that stops at a node with no victory edge is `CampaignError::Unfinishable`
naming that node, and a walk that re-enters a node it already crossed is
`CampaignError::VictoryCycle` naming the node the path came back to —
matching the naming contract `InterludeLoop` already keeps.

- *Why victory edges only:* following `Victory` is what a winning run does.
  If that path can loop or stop short of an `Ending`, a campaign that can
  never be finished was declared — the AC04 shape ("complete final mission
  and verify ending state") made impossible by construction. Refusing at
  declaration is strictly better than `advance_interludes` discovering the
  same defect mid-run, which is the hang F43-B measured and bounded.
- *Why not all cycles:* `Defeat`/`Abort` edges are deliberately outside the
  check. A cycle through them is traversed once per player-driven outcome —
  retry-in-place and authored replay loops are exactly that shape, and the
  task's own analysis flagged blanket cycle rejection as unsafe because
  mission replay loops may be authored content.
  `accept_f43_v1_a_defeat_self_loop_is_a_replay_not_a_cycle` pins a defeat
  self-loop as legal; `accept_f43_v1_a_victory_self_loop_is_still_a_cycle`
  pins that a *victory* self-loop is not (the victory path cannot leave it).
- This does **not** guess the original's cycle tolerance: it is the
  conservative direction. An importer emitting `Victory → already-visited`
  is far more likely buggy than deliberate, and anything the rule wrongly
  refuses is a shape no run could ever win past anyway.

### Rule 3 (deliberate non-rule): the runtime graph stays permissive

`CampaignGraph::try_new` was *not* given the finishability checks. The
declared/runtime invariant sets are now deliberately asymmetric:

- The declared schema is the authoring/import boundary — the one place every
  importer must pass — so structural finishability belongs there.
- `CampaignGraph` is also the vocabulary `CampaignState::advance_interludes`
  is specified to refuse *dynamically*: `InterludeLoop` on a beat cycle,
  `InterludeDeadEnd` on a beat with no onward edge. Those refusals are
  tested production behavior; a runtime layer that could not hold the shapes
  would leave nothing for the guards to refuse, and nothing for the tests to
  exercise. `accept_f43_v1_a_runtime_graph_still_holds_traversal_shapes`
  pins that a cyclic runtime graph and a victory-free runtime beat still
  construct.
- Reachability is the exception: mirrored at both layers (Rule 1), because a
  node outside the reach set is a defect everywhere, and refusing it at
  runtime costs the dynamic refusals nothing — a dead-end or cycling chain
  is still reachable content.

`lower_campaign` needs no new code: a declared draft that passes `try_new`
already satisfies every runtime invariant, and the two F43-B fixtures that
needed the shapes the declared layer now refuses were moved to direct
`RuntimeNode`/`RuntimeEdge` construction so the dynamic refusals stay
covered end to end.

## What this does not settle

1. **The original campaign's actual tolerance is unmeasured.** Whether the
   retail campaign contains replay loops, unreachable content, or
   victory-free nodes is unknown until its layout is imported (F43-D and the
   campaign-import task). All three rules above are conservative engine
   policy; evidence of a conflicting original shape would reopen them.
2. **`Unfinishable` and `DeadEnd` overlap by design.** `DeadEnd` (no edges
   at all) still fires first for an edge-less node; `Unfinishable` covers
   the subtler case of a node that has edges but none a winner can take.
3. **A victory cycle is reported at the first re-crossed node**, not the
   whole cycle — the named node is on the cycle and identifies it, which is
   what the error contract requires; enumerating the ring adds nothing a
   consumer could act on.
4. **`advance_interludes` is unchanged.** Its `InterludeLoop` /
   `InterludeDeadEnd` refusals and node-count bound remain the runtime
   answer for graphs that bypass the declared layer (hand-built runtime
   graphs now; any future runtime-only source). Declared campaigns simply
   can no longer produce those shapes.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f43_v1_ --include-ignored
```

Workspace results are in the handover summary.

## Sources

`specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
`docs/contracts/STATE-TRANSACTIONS.md`, the F43-A and F43-B findings
(`docs/findings/2026-10-01-f43-a-campaign-graph-and-outcome-schema.md`,
`docs/findings/2026-10-04-f43-b-progression-replay-and-purchase-transactions.md`),
and the code in `crates/cs_content/src/campaign.rs`,
`crates/cs_sim/src/campaign/` and `crates/cs_app/src/campaign.rs`. No
original data was read and no original executable was run; no web source was
consulted.

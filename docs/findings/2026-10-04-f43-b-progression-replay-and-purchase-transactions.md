# F43-B: progression, replay and the purchase transaction

**Task:** #175, branch `rally/175-implement-progression-replay-and-reward`,
prefix `accept_f43_b_`. Stage B of
`specs/F43-campaign-progression-outcomes-and-economy-rules.md`; shared
contract `docs/contracts/STATE-TRANSACTIONS.md`. Capabilities used: ordinary
build/test only — **no `retail`**, so nothing here is a claim about the original.

## What F43-B was, and what it changed

F43-A built the campaign graph, the outcome identity and an in-memory outcome
transaction, and its finding named exactly two things it did **not** do and
handed to this stage:

1. *"nothing in this stage advances `current` through an interlude chain or an
   interlude entry node"* — the interlude traversal.
2. *"Purchases/sell drafts and the expected-revision check are F43-B's separate
   transaction family and are **not** in this slice."*

Both were real gaps in production code, not missing conveniences. This stage
implements them.

### The interlude dead end (the progression transaction)

`RuntimeNodeKind::Interlude` exists, the declared schema declares interlude
nodes with their own edges (`NodeKind::Interlude`, "transitions
unconditionally"), and `lower_campaign` lowers them faithfully. **But
`state.rs` never referenced `Interlude` at all.** The consequence is a dead end:
winning a mission whose `Victory` edge targets a beat sets `current` to that
beat, and from there nothing can move the run — `apply_outcome` refuses a
non-mission node with `NotAMission`, and no other production path writes
`current`. Any campaign whose path crosses a briefing or a cutscene was
unfinishable.

`CampaignState::advance_interludes` is that missing path. It walks forward from
the selected node while it is an interlude, stopping on the first node that can
report an outcome of its own, and pays each beat's own grant exactly once.

It is deliberately **not** folded into `apply_outcome`, for a reason worth
recording: a beat edge can carry its own `RewardSpec`, and paying it inside the
mission's transaction would merge two revisions into one and leave the beat with
no entry in the run's own record. Making the walk its own transaction keeps one
revision per progression change and makes the beat observable.

Three properties are deliberate:

* **Idempotent by construction.** The walk moves `current` off each beat, so a
  second call starts on a non-beat, traverses nothing, pays nothing and does
  **not** bump the revision. That is spec F43 behavior 2 (rewards and unlocks
  are idempotent) without a second ledger: the progression itself is the receipt.
* **A dead-end beat is refused, not silently stopped.** A beat with no `Victory`
  edge cannot be walked. `CampaignError::InterludeDeadEnd` names it, and the
  whole walk is computed before anything is written, so a refusal leaves the run
  exactly where it was rather than half-way down a chain it cannot finish. This
  is what lets F43-D tell a bad import from a rule this stage has not
  implemented.
* **A non-beat is not a walk.** `advance_interludes` on a mission returns an
  empty advance and writes nothing, which is asserted so the call cannot be
  mistaken for a progression move.

### The purchase draft (the economy transaction)

`CampaignState::purchase` implements the contract's purchase rule: *"Validate
current availability, money and weight before writing. Conflicting revisions
fail and refresh the view; they do not overwrite unrelated progression."*

`PurchaseDraft { item, price, expected_revision }` is the draft the caller
built from a view; `expected_revision` is the optimistic-concurrency token.
Validation order is availability → ownership → money → expected revision, and
every refusal returns before the single mutation point, so the profile is
bit-identical afterwards.

Two ordering decisions are load-bearing and are pinned by tests:

* **Revision last, deliberately.** Availability, ownership and money are
  *semantic* facts about what the player is trying to do; staleness is a fact
  about the caller's view. Reporting "you cannot afford it" for a stale draft
  would send the caller to refresh a view and be told the same thing forever.
* **Availability first.** A stale draft naming an item this run does not offer
  must not read as a conflict: refreshing would re-show a menu entry that cannot
  be bought. So the availability fault is reported, and a stale draft naming an
  *available* item is then reported as stale even when it is also unaffordable.

**Idempotence** falls out of the same place the interlude's does: a purchase
adds the item to the owned set, so a draft replayed after a crash is refused
with `AlreadyOwned` instead of charging twice (spec F43 behavior 2).

## The minimum scenario (AC02)

`accept_f43_b_a_worse_replay_keeps_best_and_records_latest`. The declared fixture
is `m01 → interlude → m02 → ending`, with `m01`'s victory granting 500 and
`plane_a`, the beat granting 25, and `m02`'s victory granting 900 and
`plane_b`; roster gates open `plane_b` on `m01` and `plane_c` on `m02`.

The replay is run with the campaign **finished**, which is the hardest case
spec F43 behavior 3 names ("Replaying an earlier mission does not overwrite the
selected next mission or erase later unlocks"): the selected node is the ending
and `plane_b` came from the *last* mission, so a regression in either is plainly
visible. `m01` is replayed in a new session (a different transaction, not a
duplicate packet) with a much worse score, and the test pins that `best_score`
stays 900, `latest` becomes `(Succeeded, 120)`, the victory tally reaches 2, the
run stays finished on the ending, `plane_b` survives, `m02`'s own record is
undisturbed, and the currency does not move.

The failure cases are their own test: a replay of a **never-flown** node is
refused with `IneligibleNode` and writes nothing, and the same-identity packet
twice is still the ledger's `AlreadyApplied` rather than a second grant — the
F43-A AC01 behavior this stage must not regress.

## Test inventory (`accept_f43_b_*`)

All ten are in `crates/cs_app/tests/accept_f43_b_progression_and_economy.rs`
and all run in CI (no `retail` needed, no `#[ignore]`):

| Test | Covers |
| --- | --- |
| `accept_f43_b_a_worse_replay_keeps_best_and_records_latest` | **AC02**, the minimum scenario: worse replay at a finished campaign |
| `accept_f43_b_a_replay_of_an_unflown_node_is_refused_without_writing` | AC02's failure cases: `IneligibleNode`, and AC01's `AlreadyApplied` not regressing |
| `accept_f43_b_progression_walks_a_declared_interlude` | the walk, the beat's own grant, and reaching the ending through the declared path |
| `accept_f43_b_walking_a_beat_twice_pays_it_once` | idempotence, a non-beat writing nothing, a defeat replay on an old node |
| `accept_f43_b_a_beat_with_no_onward_edge_is_refused_not_silently_stopped` | `InterludeDeadEnd` and the refusal leaving the run untouched |
| `accept_f43_b_a_purchase_is_validated_then_written_once` | the purchase rule and its idempotence |
| `accept_f43_b_every_purchase_refusal_leaves_the_profile_untouched` | `ItemUnavailable` / `InsufficientFunds` / `StaleRevision` and the bit-identical invariant |
| `accept_f43_b_a_stale_draft_is_reported_as_stale_before_its_other_faults` | the refusal ordering the contract's "fail and refresh the view" depends on |
| `accept_f43_b_a_replay_invalidates_a_draft_taken_before_it` | the shared revision counter: a paying-nothing replay still invalidates a draft |
| `accept_f43_b_the_declared_beat_lowers_to_an_interlude_and_a_roster_gate_holds` | the walk and the classification agree on what a beat is |

### Measured sensitivity

Both mutations were applied to production code, run, and reverted:

* **stubbing `advance_interludes` to an empty advance** — 7 of 10 fail.
* **dropping the `expected_revision` check** — the same 7 fail.

A third probe worth recording: the AC02 test is the only one that catches a
regression which *lowers* `best_score`, because it is the only test that
replays with a worse score against a completed node.

## Recorded unknowns (not guessed — spec F43 behavior 4)

1. **Every economy amount here is synthetic.** The fixture's 500 / 25 / 900
   grants and the 400 purchase price exist only to make the transaction
   observable. The original's reward and price amounts are **unmeasured**; F43-D
   must observe them from the campaign data or the original's rules before any
   economy claim.
2. **Whether the original's campaign has interludes at all, and how it walks
   them.** `advance_interludes` implements the declared schema's semantics
   ("transitions unconditionally", a `Victory` edge). Whether the original
   campaign routes through narrative beats this way is unmeasured; if it does
   not, this path is simply never taken.
3. **Whether a campaign may legally *begin* on an interlude.** Still open from
   F43-A and deliberately left open: `CampaignState::begin` selects the entry
   node and does not walk, so a campaign whose entry is a beat needs one
   `advance_interludes` call to become playable. Making `begin` walk silently
   would decide the question, and nothing measured decides it.
4. **Sell is not implemented.** The contract says "Purchase/sell"; this stage
   implements the purchase side only. A sale needs a purchase ledger to refund
   the price actually paid (the declared schema has no price field at all), and
   inventing one would be a guess. Filed as a follow-up task.
5. **Loadout weight is not modelled.** The contract's purchase rule names
   "money **and weight**". `CampaignState` has no weight or capacity concept,
   and inventing one is a guess about an economy this stage cannot observe. The
   price is validated; the weight is not, and that omission is a known gap in
   the contract's rule rather than a claim that it is satisfied.
6. **Refund/cancellation, and what a purchase does to `best`/records** — no
   original rule is known for any of it.
7. **Difficulty still opaque** (carried from F43-A): `DifficultyId` is a
   selector; which levels the original offers, and whether a purchase price
   varies by them, is unmeasured.

## Commands

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f43_b_ --include-ignored
```

Workspace results are in the handover summary.

## Sources

`specs/F43-campaign-progression-outcomes-and-economy-rules.md`,
`docs/contracts/STATE-TRANSACTIONS.md`, the F43-A finding
(`docs/findings/2026-10-01-f43-a-campaign-graph-and-outcome-schema.md`), and the
code it left in `crates/cs_sim/src/campaign/`. No original data was read and no
original executable was run; no web source was consulted.
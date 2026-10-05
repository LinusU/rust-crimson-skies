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
* **A cycling beat chain is refused too.** A declared chain that comes back to a
  beat it already crossed is a cycle in the *declared* campaign, and nothing
  upstream rejects one. `CampaignError::InterludeLoop` names the beat, and the
  walk is additionally bounded by the graph's node count, which is the longest a
  legitimate chain can be. Both guards exist: the refusal makes the answer
  right, the bound keeps a lost guard from hanging the caller.
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

* **Revision last, deliberately.** Availability and ownership are *structural*
  facts — a roster gate the run has not opened, an item the owned set already
  holds — and no refresh can change either, so they are reported even when the
  draft is stale. The balance can move under any other committed transaction, so
  its verdict drawn from a stale view is the least reliable thing the call could
  say; the caller refreshes and re-reads it. Putting the revision check first
  would report the conflict for drafts whose real fault survives the refresh.
* **Availability before staleness.** The same argument for the item itself: a
  stale draft naming an item this run does not offer must not read as a
  conflict, because refreshing cannot make the item appear.

**Idempotence** falls out of the same place the interlude's does: a purchase
adds the item to the owned set, so a draft replayed after a crash is refused
with `AlreadyOwned` instead of charging twice (spec F43 behavior 2).

Two limits of this transaction are design consequences, not measured rules, and
neither is checked anywhere in the engine:

* **The price is whatever the draft says.** The declared schema has no price
  field — `RosterEntry` is `{ item, available_from, provenance }` — so the
  price a draft names is the only price the engine ever sees. `purchase`
  validates that the balance covers the price it was *given*, never that the
  price is what the item costs; a caller that passes `1` for anything it can
  afford buys it. Closing this needs a declared price (a content-schema change,
  and `cs_sim` may not depend on `cs_content` anyway — `docs/01-ARCHITECTURE.md`
  allows `cs_types` and `cs_script` only), so it is a later stage's decision,
  not a check this one could add.
* **A granted item and a bought item are the same thing afterwards.** Both write
  `CampaignState::unlocks`, so a reward-granted item reads as `AlreadyOwned` and
  cannot be bought, and a bought item is indistinguishable from a granted one
  (no ledger of what was paid — see the sell follow-up #621). That single
  ownership set is what makes a replayed draft refuse itself without a second
  ledger, so it is deliberate; it is still a modelling choice of this stage.

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

All fourteen are in
`crates/cs_app/tests/accept_f43_b_progression_and_economy.rs` and all run in CI
(no `retail` needed, no `#[ignore]`):

| Test | Covers |
| --- | --- |
| `accept_f43_b_a_worse_replay_keeps_best_and_records_latest` | **AC02**, the minimum scenario: worse replay at a finished campaign |
| `accept_f43_b_a_replay_of_an_unflown_node_is_refused_without_writing` | AC02's failure cases: `IneligibleNode`, and AC01's `AlreadyApplied` not regressing |
| `accept_f43_b_progression_walks_a_declared_interlude` | the walk, the beat's own grant, and reaching the ending through the declared path |
| `accept_f43_b_walking_a_beat_twice_pays_it_once` | idempotence, a non-beat writing nothing, a defeat replay on an old node |
| `accept_f43_b_a_beat_with_no_onward_edge_is_refused_not_silently_stopped` | `InterludeDeadEnd` and the refusal leaving the run untouched |
| `accept_f43_b_a_declared_beat_cycle_is_refused_not_walked_forever` | `InterludeLoop` for a self-looped beat and for a two-beat cycle (review fix 1) |
| `accept_f43_b_a_beat_chain_granting_more_than_i64_max_pays_exactly` | the currency accumulator's width (review fix 2) |
| `accept_f43_b_a_beat_grant_that_would_overflow_the_balance_is_refused` | `CurrencyOverflow` from a beat grant, and nothing written |
| `accept_f43_b_an_overflowing_later_beat_names_the_running_balance_not_the_walks` | the refusal names the balance the failing grant actually met (review fix 4) |
| `accept_f43_b_a_purchase_is_validated_then_written_once` | the purchase rule and its idempotence |
| `accept_f43_b_every_purchase_refusal_leaves_the_profile_untouched` | `ItemUnavailable` / `InsufficientFunds` / `StaleRevision` and the bit-identical invariant |
| `accept_f43_b_purchase_refusals_report_the_structural_faults_before_staleness` | the refusal ordering the contract's "fail and refresh the view" depends on |
| `accept_f43_b_a_replay_invalidates_a_draft_taken_before_it` | the shared revision counter: a paying-nothing replay still invalidates a draft |
| `accept_f43_b_the_declared_beat_lowers_to_an_interlude_and_a_roster_gate_holds` | the walk and the classification agree on what a beat is |

### Measured sensitivity

All mutations were applied to production code, run, and reverted:

* **stubbing `advance_interludes` to an empty advance** — 7 of the original 10
  fail.
* **dropping the `expected_revision` check** — the same 7 fail.

A third probe worth recording: the AC02 test is the only one that catches a
regression which *lowers* `best_score`, because it is the only test that replays
with a worse score against a completed node.

### Review (bunny-2, reviewing its own implementation)

**This review is not independent evidence.** The implementer and the reviewer are
the same agent instance (`bunny-2/bunny-2`) continuing in a second session, so
this section records a self-review with the same measurements available to any
reviewer, not a second opinion. AGENTS.md asks for a different instance or model
for format/mission semantics; the owner should treat the F43-B claims below as
reviewed-but-not-independently-reviewed.

Three things were found and fixed; the first two were defects in the code this
stage added.

**1. A declared beat cycle never terminated the walk (defect, fixed).** Neither
`cs_content::campaign::try_new` nor `CampaignGraph::try_new` rejects a cycle —
both only require a non-ending node to declare *an* edge — and the walk was a
bare `while` over `Victory` edges with no memory of where it had been. Measured
before the fix, on a declared `beat --Victory--> beat`: the walk produced no
result within 100 ms, and with the guard removed under test it was still running
after 30 minutes and had to be killed. A cycle therefore turned a progression
transaction into a hang. Fixed by refusing the repeat as
`CampaignError::InterludeLoop { node }` and bounding the walk by the graph's node
count — the longest a legitimate chain can be — so that losing the guard fails
the caller instead of spinning it. Measured after the fix, with **only** the
refusal neutered and the bound kept: the test fails in 0.00 s and shows the
damage concretely (`traversed: [beat, beat, beat]`, `currency_delta: 30` — the
same beat crossed and paid three times).

**2. A beat chain granting more than `i64::MAX` minor units panicked (defect,
fixed).** The chain's grants were summed in an `i64` accumulator and cast back to
`u64`, so a legitimate chain of three `2^62` grants — above `i64::MAX`, inside
`u64` — overflowed. Measured before the fix: `attempt to add with overflow` at
`state.rs:481`, i.e. a debug-build panic on a syntactically valid declaration, and
a wrapped (wrong) sum in a release build, which can silently pay the wrong
balance. The walk now accumulates in the currency's own `u64` with
`checked_add`, so it either pays the exact sum or refuses with
`CurrencyOverflow`; `InterludeAdvance::currency_delta` is `u64` for the same
reason, since a walk only ever grants. Measured after the fix: the three-beat
chain pays `3 * 2^62` exactly, and a grant that would carry the balance past
`u64::MAX` is refused with nothing written.

**3. A test name and a doc comment stated the opposite of the pinned behaviour
(correctness of the record, fixed).** `purchase` checks the expected revision
**last**, and
`accept_f43_b_a_stale_draft_is_reported_as_stale_before_its_other_faults` asserted
exactly that — availability first, price before staleness — under a name claiming
the opposite, while `purchase`'s own doc comment said the revision "is *always*
reported as stale". Neither the name nor the comment was wrong about the code;
they were wrong about each other. The test is now
`accept_f43_b_purchase_refusals_report_the_structural_faults_before_staleness`,
and both the comment and the section above give the honest reason for the order:
structural facts are reported even for a stale draft, the balance is not.

What the review checked and found sound, unchanged: the exactly-once ledger and
its `AlreadyApplied` answer (F43-A's AC01, still pinned here), replay never
moving `current` or re-paying a grant, the single mutation point in both
transactions, every refusal leaving the profile bit-identical, and the AC02 test
being the only one that catches a lowered `best_score`. `advance_interludes`
being unwired is unchanged and is still F43-C's job; nothing on this branch
depends on the walk happening implicitly.

**4. A multi-beat chain's overflow refusal named the wrong balance (defect,
fixed, second review pass).** `advance_interludes` reported
`CurrencyOverflow { before: self.currency }` — the balance the *walk* started
from — so the refused grant was blamed against a balance it might have fitted
into easily. Measured on a chain `m01 -> beat_a -> beat_b -> m02` where
`beat_a` grants `u64::MAX - 5` and `beat_b` grants `10`: the refusal read
`before: 0, delta: 10`, i.e. "grant of 10 would overflow the balance 0", which
is false and tells an importer nothing about what tripped the walk. The field
now carries the **running** balance, the one the refused grant would have been
added to, which also matches the variant's own doc ("the currency before").
Measured after the fix: the same chain reports `before: u64::MAX - 5,
delta: 10`, nothing is written, and the run stays on `beat_a`.

**Second review pass, and its limits.** That pass re-read the whole diff, the
sheet, the contract and the neighbouring modules with fresh context, and
confirmed the CI failure independently rather than taking the first pass's word
for it (same `ld` SIGBUS on the `Doc-tests cs_app` link, every earlier suite
green, main green at the same time). It found nothing further in the code. It
did confirm two things about the surrounding design, recorded above and in the
follow-ups rather than changed here: `cs_sim` may depend only on `cs_types` and
`cs_script` (`docs/01-ARCHITECTURE.md`), so the campaign economy's bare `u64`
minor units cannot become `cs_content::construction::MoneyMinor` without a
layering decision, and the sell follow-up (#621) and weight follow-up (#622)
both need #175 on `main` before they can start.

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
   not, this path is simply never taken. The *refusals* on top of that walk are
   engine policy, not measured original rules: what the original does with a
   beat that has no onward edge, with a chain that cycles, and with a grant it
   cannot represent is **unknown**. F43-D must observe it; until then the
   runtime's refusal is a designed answer to a declaration nothing upstream
   validates.
3. **Whether a campaign may legally *begin* on an interlude.** Resolved by
   F43-B.3 (#623) as a **designed** rule, **not measured**: no original
   campaign data was read, so whether the original opens on a narrative beat
   is still unknown. A campaign's entry must be a node that can report an
   outcome. `CampaignDefinition::try_new` refuses a beat entry with
   `CampaignError::InterludeEntry` and `CampaignGraph::try_new` with
   `GraphError::InterludeEntry`, so a run never needs a manual
   `advance_interludes` to start and `begin` stays a pure selection with no
   grant or commit. `advance_interludes` is still needed mid-campaign. If
   F43-D measures an original entry beat, this refusal must be revisited.
   Tests: `accept_f43_b_3_*`.
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
# F43-V2: an `Ending` node is refused as the campaign entry

**Task:** #1387, prefix `accept_f43_v2_`. Capabilities used: ordinary
build/test only — **no `retail`**.

## Decision

`CampaignDefinition::try_new` refuses an `Ending` entry with
`CampaignError::EndingEntry`, and `CampaignGraph::try_new` mirrors it with
`GraphError::EndingEntry`. Both sit beside the existing `InterludeEntry`
check, so the two layers keep the same entry invariants.

## Measured or designed?

**Designed, not measured.** No original campaign data was read, and the
original's tolerance for a campaign that opens on an ending is unmeasured.
The rule extends the rationale of `InterludeEntry`: a run begins on a node
that can report an outcome. An `Ending` reports none, so
`CampaignState::begin` would park a fresh run where `is_finished` is true at
tick 0.

## Is an all-Endings draft a legal empty campaign?

No. An importer mid-population should not produce a draft that validates but
cannot be played; `Empty` already refuses a draft with no nodes, and an
all-Endings draft is the same "nothing to play" shape. Refusing it loudly at
the boundary is the conservative direction, and a one-node `{ending}` draft
is pinned as refused by
`accept_f43_v2_declared_all_endings_draft_is_not_a_legal_empty_campaign`.

If F43-D measures an original campaign that does open on an ending, this rule
must be revisited.

## Tests

`crates/cs_app/tests/accept_f43_v2_ending_entry.rs` covers the declared
layer, the one-node draft and the runtime layer, each with a mission entry
as the accepted control.

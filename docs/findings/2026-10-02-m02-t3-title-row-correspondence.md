# M02-T3: the title-to-campaign join corroborated by the two localized row blocks

Date: 2026-10-02. Task: M02-T3 "Corroborate the title-to-campaign join with the
two localized row blocks, not only the region grouping" (Rally #450), found
while reviewing M02-A (#261). Branch
`rally/450-corroborate-the-title-to-campaign-join-w`. Shared contracts:
`docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`.
Capabilities used: `retail` (`$CS_GAME_DIR` read-only, never written),
`synthetic`. Owner paths: `crates/cs_content/src/campaign_bindings.rs`,
`crates/cs_app/tests/campaign/`, `docs/findings/`.

M02-A's `classify_join` guards the work-order → retail-mission join with the
*shape* of the region-prefixed long names' grouping (`[5, 5, 5, 5, 4]`), whose
first four entries are identical and so discriminate only the last chapter.
This task adds the strongest structure the installation actually offers: the
same 24 missions appear **twice**, once as region-prefixed long names and once
as bare short names, and the two blocks name the same position in the same
order. That correspondence is region-agnostic and much harder to satisfy by
coincidence. It is now a third corroboration, checked before any campaign
position is derived from a title.

**The join stays an inference.** No region name is bound to a chapter, no
original run was observed, and nothing here is `verified_original` (AGENTS
rule 8). This merge is `checked` at most.

## The rule

`blocks_correspond(left, right)` (`crates/cs_content/src/campaign_bindings.rs`)
is a pure function over two slices of display text. It is true exactly when:

1. the two blocks are non-empty and equally long;
2. every row of one shares **at least one content token** with its own row of
   the other; and
3. that own pairing is a **strict argmax** of shared content tokens — strictly
   more shared tokens than any other row of the opposite block — checked from
   both sides, so neither a row nor a column may tie.

A content token is a lowercase alphanumeric run with an in-word apostrophe kept
and the articles `a`, `an`, `the` dropped (`content_tokens`). The rule compares
no whole string, so it is not tuned to any one spelling, and it drops only what
the retail display convention varies on its own. A tie, an empty diagonal, a
length difference or an empty block is `false`: a permutation that ties carries
no order information, and the function guards a refusal, so it errs toward
refusing.

Three more pure functions expose the rest of the decision so every arm is
reachable without an installation, following M02-A's pattern:

- `classify_correspondence(&[bool])` — no measured pair: `Unavailable`; every
  pair corresponding: `Agreed`; any pair not corresponding: `Disagreed`.
- `merge_corroboration(region_groups, correspondence)` — a `Disagreed` from
  either side decides; otherwise an available `Agreed`; both `Unavailable`
  stays `Unavailable`.
- `join_state(layout_chapters, grouped, correspondences)` — the exact
  composition `join_agreement` now uses.

`SourceContext::join_agreement` builds one boolean per unordered pair of
campaign-length blocks (`block_correspondences`, over `block_displays`) and
folds `join_state` into `JoinAgreement::state`. `JoinCorroboration` keeps its
three honest values; a non-corresponding block pair is a `Disagreed`, exactly
like a contradicting grouping, and refuses the join. No field was added to
`JoinAgreement` (it is built by struct literal in many test modules) and no
existing public type changed meaning. `CONTRADICTED_JOIN_REFUSAL` now names
both causes; its existing `"contradicts"` wording is preserved.

## Evidence measured on this installation

`$CS_GAME_DIR` was opened read-only. Nothing original is committed. All of the
following is re-derived by the tests, except the offline sampling noted.

| Fact | Value |
| --- | --- |
| Campaign length | 24 (`chapter_sizes()` = `[5, 5, 5, 5, 4]`) |
| Campaign-length row blocks | exactly two: `3450…3473` long names, `3480…3503` short names |
| `blocks_correspond(long, short)` | `true` |
| Every non-zero rotation of the short block | refused (all 23) |
| `join_agreement().state` | `Agreed` (region grouping **and** correspondence agree) |

The plain rule the task suggested was **rejected by measurement**, and the
tests pin why:

- Case- and article-insensitive **normalized equality** of the long name's
  tail against the short name fails on **16 of 24 rows**:
  `0, 1, 2, 3, 5, 6, 7, 8, 9, 10, 12, 13, 16, 19, 21, 22` — the long names
  carry a subject (`Nathan Zachary &`) and extra words.
- **Token containment** and **word subsequence** each fail on exactly one row,
  index 7: long `Northwest - Nathan Zachary & The Petrol Pit` against short
  `The Petrol Plot`. Those rules would have to be special-cased to pass, which
  is why they were rejected. The strict-argmax token rule decides row 7
  correctly through the one token the two spellings share (`petrol`), which no
  other row of either block carries.
- **Offline sampling** (not production code, not in the suite): generating
  two unrelated 24-row blocks from the union vocabulary of these 48 rows, the
  rule accepted 0 of 20 000 random pairs; shuffling the real short block, it
  accepted 0 of 2 000 permutations. This is a probe of the rule's selectivity,
  not evidence of anything about the game.

## What the rule decides, and the rows it cannot decide

On this installation the rule decides **all 24 rows**: each diagonal pairing is
the unique strict best in its row and column, including row 7. It does not, and
cannot, decide:

- **Which chapter a region is.** Only the correspondence of two ordered blocks
  is used; the region prefix is never interpreted. The M02-A standing unknown
  ("chapter ↔ region naming is not established") is unchanged. The size-only
  ordering still admits only "chapter 5 = Manhattan" and says nothing about
  chapters 1–4.
- **A row whose own pairing ties or is empty.** The rule has no answer there
  and returns `false`, so the whole pair is refused. On this installation no
  tie occurs; two *identical* blocks with a duplicated token set would be
  refused even though they obviously correspond. That is the conservative
  direction (refuse rather than accept) and is asserted in the synthetic test.
- **That the two texts are the same string.** Because the rule is an overlap
  strict-argmax, a row can add or drop a word and still correspond as long as
  its diagonal stays the strict best. The rule establishes the **order** of the
  two blocks, which is what the join needs; it is not an equality test.
- **Anything at all when fewer than two campaign-length blocks exist.** Then
  `classify_correspondence(&[])` is `Unavailable` and the join keeps the
  inference, exactly as before.

## Files

- `crates/cs_content/src/campaign_bindings.rs` (owner path) adds
  `blocks_correspond`, `classify_correspondence`, `merge_corroboration`,
  `join_state`, the private `content_tokens`, and the `SourceContext` methods
  `block_displays` and `block_correspondences`; it changes `join_agreement` to
  fold the correspondence in, and broadens `CONTRADICTED_JOIN_REFUSAL` to name
  both causes.
- `crates/cs_app/tests/campaign/m02_t3.rs` (owner path): the six
  `accept_m02_b_*` tests (three retail, three synthetic).
- `crates/cs_app/tests/campaign/evidence.rs` (owner path): the M02-T3 test
  lists and the `evidence_report_m02_t3_writes_the_acceptance_report` harness
  plus its `m02-t3-correspondence.json` artifact.
- `docs/findings/evidence/M02-T3.json` (owner path): the committed report copy.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod m02_t3;`).
- No `Cargo.toml` or `Cargo.lock` change.

## Test inventory (`accept_m02_b_*`, 6 tests)

| Test | What it pins |
| --- | --- |
| `accept_m02_b_the_two_campaign_length_blocks_correspond_row_to_row` (retail) | exactly two campaign-length blocks, each as long as the campaign; `blocks_correspond(long, short)` and its symmetry; every one of the 23 non-zero rotations refused |
| `accept_m02_b_a_plain_normalized_comparison_would_not_correspond` (retail) | the plain normalized equality rejects exactly the 16 measured rows; row 7's `The Petrol Pit` / `The Petrol Plot`; and the production rule still accepts the pair |
| `accept_m02_b_the_join_agreement_folds_in_the_correspondence` (retail) | `join_agreement().state` is `Agreed` and `establishes()`; the measured correspondence is `Agreed`; the state is exactly `join_state(layout, grouped, [correspondence])` |
| `accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows` (synthetic) | acceptance and symmetry; a moved row; a row with no shared token; no shared tokens at all; a length difference; two empty blocks; and a tie |
| `accept_m02_b_the_rule_ignores_case_and_articles` (synthetic) | case and article tolerance, and that the same rows reordered do not correspond |
| `accept_m02_b_a_corroboration_disagreement_is_a_refusal` (synthetic) | every arm of `classify_correspondence`, of `merge_corroboration` and of `join_state` (including the agreeing-grouping / contradicting-correspondence arm no installation produces), and `campaign_position_for`'s refusal on `Disagreed` |

The retail tests read the installation through production code and are
`#[ignore = "requires CS_GAME_DIR"]`; the synthetic tests run in CI.

## Mutation probes

The implementer applied five mutations to `campaign_bindings.rs` and reverted
each; the tree carries none of them.

| Mutation | Observed result |
| --- | --- |
| `join_state` ignores the correspondence (returns `classify_join` alone) | 1 fails: `accept_m02_b_a_corroboration_disagreement_is_a_refusal` |
| `classify_correspondence` uses `any()` instead of `all()` | 1 fails: `accept_m02_b_a_corroboration_disagreement_is_a_refusal` |
| `blocks_correspond` accepts a tie (`<` → `<=`) | 1 fails: `accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows` |
| `blocks_correspond` drops the empty guard | 1 fails: `accept_m02_b_a_block_pair_corresponds_only_through_its_own_rows` |
| `blocks_correspond` forces `false` for every pair | 5 of 6 fail, including all three retail tests: forcing a disagreement flips `join_agreement().state` to `Disagreed`, so the fold in `join_agreement` is observed on real data and not only through the pure functions |

The last row is the important one: a retail test fails when the correspondence
is removed from the join, so the wiring is covered even though the retail
installation agrees with itself.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m02_b_ --include-ignored` | 0 (6 tests: 3 retail, 3 synthetic) |
| `python3 tools/validate_evidence.py private/evidence/M02-T3/acceptance.json --artifact-root private/evidence/M02-T3 --require-pass` | 0 (`structurally_valid: true`) |

## Recorded unknowns (not guessed)

- **The join remains an inference**, now with two independent structures
  checking its order. Nothing establishes the mapping itself.
- **What a region name means is not established**, and this task deliberately
  does not bind one to a chapter.
- **A tie is refused**, so two identical blocks with a duplicated token set
  would not corroborate. Not observed on this installation.
- **The rule is an order test, not an equality test**: a row may add or drop a
  word and still correspond.
- **No original run was observed.** Nothing here is a behaviour claim.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_content::config::StringCatalog` and the platform filesystem;
`schemas/evidence.schema.json`; `missions/M02.md`;
`docs/contracts/IDENTITY-CONTENT.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-01-m02-a-source-binding.md`.

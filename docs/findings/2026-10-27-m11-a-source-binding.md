# M11-A: the eleventh source-derived mission binding

Date: 2026-10-27. Task: M11-A "Bind original mission data and branches"
(#288, completed through follow-up #477 under the owner ruling of #470;
`missions/M11.md`, work order `M11-A`). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written). Implementer: **deepseek-1**
(DeepSeek V4.1 Flash, session of 2026-10-27). Reviewer: none yet; an agent
review is not independent original-reference evidence and no agent review
replaces the owner's human approval.

The minimum acceptance scenario, *"Source-derived binding has no unresolved
critical dependencies"*, holds for M11 under the owner ruling below.

This supersedes the earlier blocked attempt of 2026-10-02 (finding
`2026-10-02-m11-a-source-binding.md`, kept on branch
`rally/288-bind-original-mission-data-and-branches`): it asked for exactly the
decision that has now been made, and it was never merged because M11 could not
meet its scenario without it. The ruling itself is
`2026-10-25-owner-ruling-guide-titles.md` (Rally #470, owner note 22711).

## What changed

- `missions/bindings/M11.json` — regenerated with production
  `SourceBinding::to_json` and pinned by
  `accept_m11_a_the_committed_record_is_what_the_installation_derives`
  (`CS_REGENERATE_M11_BINDING=1` rewrites it).
- `crates/cs_app/tests/campaign/m11_a.rs` — nine `accept_m11_a_*` tests
  (six retail `#[ignore = "requires CS_GAME_DIR"]`, three synthetic).
  Replacing the blocked attempt's suite, they pin the resolved binding and
  keep its two measurements.
- `crates/cs_content/src/campaign_bindings.rs` — one production change,
  `unresolved_critical_entries(dependencies) -> Vec<String>`, which
  `SourceContext::bind` extends `SourceBinding::unknowns` with. The record's
  `to_json` carries no dependency states, so a refused record would otherwise
  hold three `null` identities and no reason at all. A binding that resolves
  all five contributes nothing, so every record that already resolved them is
  byte-identical; the resolved M11 record carries no such entry.
- `crates/cs_app/tests/campaign/evidence/m11_a.rs` and the `mod`/doc wiring in
  `campaign/mod.rs` and `campaign/evidence.rs` (wiring only).
- `missions/bindings/README.md` — the M11 paragraph.

The ruling needed no change to the comparison: `SourceContext::bind` confirms
the declared title exactly as before, and for a label with a ruling row it
then confirms the ruled retail short name exactly at the ruled row, requiring
the ruled long row at the same campaign position in the other block. No fuzzy,
normalising or ordinal matching exists anywhere.

## What was read from the installation

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (same installation as the other bindings) |
| Declared title | `The Stolen Scarlet`, carried by no localized row |
| Ruled retail short name | `The Stolen Starlet` (row 3490, display tag stripped), long-name row 3460 `Hollywood - Nathan Zachary & The Stolen Starlet` |
| Campaign position | 10 (the eleventh), chapter 3 mission 1 |
| Mission id | `mission/ch3-m01` |
| World group | `world/c2` |
| Program | `script/c2-m02-zrdr`, `ZBD/C2/M02/zrdr.zbd`, 78 405 bytes, SHA-256 `6154e4ea…80a5d` |

The offset is a boundary on both structures: it is chapter 3's first mission
and the first row of the long names' third region group. It is *not* evidence
for the join by itself — the join is the ruling's exact spelling, confirmed at
row 3490, and the position comes from that row.

Two facts measured again for this binding, both asserted by
`accept_m11_a_*` tests:

1. **Order agreement.** Over all 24 declared work orders, every title the
   installation carries exactly selects its own index of
   `missions/bindings/campaign-inventory.tsv`; exactly the seven of #470
   ({M09, M11, M14, M15, M20, M22, M23}) are carried by no row. With the
   ruling table withdrawn, the same installation refuses exactly those seven,
   and nothing is inferred by adjacency.
2. **M11's discrepancy is one character in one word, and the original agrees
   with itself.** `The Stolen Scarlet` and `The Stolen Starlet` have the same
   words and the one differing word has the same length and differs in exactly
   one character. Row 3460 ends with exactly row 3490's text. That is a
   measurement, not a comparison the engine performs; the near-miss
   `The Stolen Starle` is refused.

## Evidence class

Mission id, world group **and title** are `Inferred`: the title is not
observed in the installation but joined by the owner ruling, which
`SourceContext::bind` then confirms exactly at the ruled rows. The install
hash and program archive are `ObservedTool`. The record's last `unknowns`
entry (`title ruling:`) states the inference. Nothing is `verified_original`.

## Failure cases pinned

- Without the ruling table the title is `Uncarried`, and title, mission,
  world and program all stay unresolved; the partial record names each of
  them in its own `unknowns` with the refusal that caused it.
- A near-miss spelling (`The Stolen Starle`) is not confirmed.
- `title_form` refuses every non-exact arm the ruling could be mistaken for:
  a one-letter difference, a differing long-name tail, case alone, a missing
  or added word, a trailing space, an empty row or title, and the
  `-`-without-space separator.
- A table without M11's row has no ruling for it, and a ruled spelling the
  installation does not carry refuses.

## Unresolved

All checklist entries other than the five critical dependencies stay unknown
(`missions/bindings/M11.json`), so M11 is not ready; M11-B owns the
mission-specific gaps, including the sheet's ordered stunts, escort
obstruction and branch-sensitive response priorities, none of which has an
actor, objective or predicate bound yet.

## Follow-up found

`accept_f50_b_unresolved_identities_stay_unresolved_and_the_campaign_stays_unready`
and two `accept_f50_c_*` retail tests were already updated for the ruling by
other stages; the M11-A selection passes on this installation. No new
follow-up was needed.

## Evidence report

`private/evidence/M11-A/acceptance.json` (generated by
`cargo test -p cs_app -- evidence_report_m11_a --ignored` from the recorded
retail run, validated with `tools/validate_evidence.py --require-pass`) and its
committed copy `docs/findings/evidence/M11-A.json`. The report's `unknowns` are
empty because the acceptance run passed with all five critical dependencies
resolved; the binding's own still-unknown checklist entries live in
`missions/bindings/M11.json`, never dropped from the report.

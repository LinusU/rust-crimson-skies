# Owner ruling: guide titles the installation spells differently (Rally #470)

Owner ruling by Linus / Claude M3, 2026-10-25, Rally task note 22711, option (a).
`missions/README.md` and `missions/bindings/campaign-inventory.tsv` are unchanged.

## Ruling

Work orders M09, M11, M14, M15, M20, M22 and M23 name the retail missions at
their own campaign index. Claim status: `ClaimStatus::Inferred`, never
`verified_original`.

| Work order | Declared title | Retail short name (row) | Long name row |
| --- | --- | --- | --- |
| M09 | `Perils for Blake` | `Peril for Blake` (3488) | 3458 |
| M11 | `The Stolen Scarlet` | `The Stolen Starlet` (3490) | 3460 |
| M14 | `Clash of the Dreadnaughts` | `Clash of Dreadnaughts` (3493) | 3463 |
| M15 | `The Fight for the Figaro` | `Fight for the FIGAROA` (3494) | 3464 |
| M20 | `Unholy Alliance` | `The Unholy Alliance` (3499) | 3469 |
| M22 | `Runaway Witness` | `The Runaway Witness` (3501) | 3471 |
| M23 | `Criminal Exodus` | `The Criminal Exodus` (3502) | 3472 |

## Evidence

1. Order agreement: all 17 carried titles select their own inventory index
   (`accept_m11_a_the_declared_order_agrees_with_the_retail_campaign_order_where_it_is_carried`),
   and each of the 7 open positions is filled by exactly one row pair.
2. Each row pair shares the declared title's words apart from an article, a
   plural or one character (Perils/Peril, Scarlet/Starlet,
   Dreadnaughts/Dreadnaoughts, Figaro/FIGAROA).

## Implementation

The ruling is committed as `missions/bindings/title-rulings.tsv`.
`SourceContext::bind` first confirms the declared title exactly. Only for a
label with a ruling row, whose declared title equals the inventory's, does it
then confirm the ruled retail short name, also exactly, at the ruled row, and
check that the ruled long-name row sits at the same campaign position in the
other block. The `TitleString` dependency is then `Inferred`. Nothing fuzzy,
normalising or ordinal is added; a ruling row that is not carried exactly
refuses, and without the table the seven orders stay `Uncarried`.

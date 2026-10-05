# F43-B.1: the sell half of the economy transaction

Task #621. Implements `CampaignState::sell`, the mirror of `purchase` in
`docs/contracts/STATE-TRANSACTIONS.md` ("Purchase/sell uses a draft and
expected profile revision").

## Designed semantics (no original rule is known for any of these)

1. **Where the paid price lives.** `CampaignState` keeps a per-item purchase
   ledger (`paid: item -> minor units`). `purchase` writes the drafted price
   into it in the same commit that charges. `cs_sim` cannot depend on
   `cs_content`, and the declared schema (`RewardSpec`, `RosterEntry`) has no
   price field, so the refund is read from the profile, not a price list.
2. **Granted items are not sellable.** A reward grant has no ledger entry, so
   `sell` refuses it with `NotPurchased`. Refunding it would need an amount
   nobody has measured, and none is invented.
3. **Roster gates survive a sale.** Availability is derived from completed gate
   nodes, so a sold item is available and unowned again and may be re-bought at
   the newly drafted price. Whether the original lets a sold item be bought back,
   or at what price, is unknown. This is designed behaviour, not observed.
4. **Draft and ordering.** `SellDraft { item, expected_revision }` carries no
   price. Checks run in order: owned (`NotOwned`), bought (`NotPurchased`),
   balance overflow (`CurrencyOverflow`), then the expected revision
   (`StaleRevision`) last, matching purchase. Every refusal returns before the
   single mutation point, so the state is bit-identical afterwards.
5. **Idempotence.** The sale removes the item from `unlocks` and the ledger, so a
   second draft is `NotOwned` and cannot credit twice.

## Unknowns carried forward

* The original's sell-back price rule (full refund, a fraction, none) is
  unmeasured. Refunding exactly what was paid is a designed choice.
* Loadout weight is still not modelled (see the F43-B finding, item 5).
* A sale writes no record other than the revision bump.

## Tests

`accept_f43_b_1_*` in `crates/cs_app/tests/accept_f43_b_progression_and_economy.rs`.
Mutation: removing the balance credit in `sell` makes
`accept_f43_b_1_a_sale_refunds_the_price_paid_once` and
`accept_f43_b_1_a_sold_item_is_available_to_buy_again` fail.

No original data was read and no original executable was run.

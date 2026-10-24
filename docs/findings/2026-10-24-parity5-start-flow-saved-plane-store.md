# The start flow's saved-plane store feeds Sell and the hangar list

2026-10-24, task #2076 (P5-FEED-C, part of #2052). Implemented by `sonnet-1`.
Nothing here is `verified_original`; the original executable was not run.

## What shipped

* `pilot_plane.rs`: `PilotPlane` holds the 25 saved records (51 dwords and a
  name each) and the selected record. `saved_count()` is `E 2018` on the
  construction screen (occupied records, minus one, engine state 13 or 17);
  `hangar_planes()` maps the records to the hangar's list (kind = first word,
  airframe `+0x2c`, value through `plane_value`); `feed_hub(&mut HubPage)` feeds
  the hub's figures, `HubFeed.saved_planes` and `HubPage::set_saved_planes`
  (planes, selected, cash).
* The Plane Name OK feeds the hub through `feed_hub`.
* The Sell-hangar smoke scene (`CS_FRONT_END_SMOKE_SCENE=hangar`) now seeds its
  synthetic planes into the pilot's store instead of the page, so the same
  feed path serves them.

## Readings and limits

* **A new pilot holds one record.** The reset (`0x4113b0`) zeroes the 25
  records and copies the stock template over record 0, so the Sell count on the
  construction screen is 1 - 1 = 0 and **Sell is inactive** for a new pilot.
  Sell needs three saved planes beyond the one being built.
* **`fAllowAll` unknown.** With it set, the reset also fills records 2..12 with
  the eleven airframe templates (`0x41160b`). The flag is still passed as
  false and the templates are not in `engine-facts.toml`, so those records stay
  empty. Affected: a retail profile's Sell state; resolving: the profile
  reading or an owner capture (#2052).
* **Record 0 keeps the stock loadout** even when the Plane Name box is
  unchecked: the reset copy precedes `E 2212`'s emptying of the working plane.
* **Names.** The record's name word (`+4`) is unread for record 0; it is empty.
  Planes the pilot saves later have no writer yet (the save path, `E 2034`, is
  unread here); `save_plane` is the store's entry point for it.
* **Selected record.** `[0x64b67c]` is 0 after the reset. While constructing,
  the engine's row walk returns the selected record whenever it reaches it, so
  with record 0 selected and occupied every row answers record 0.

## Smoke

Owner-command front-end smoke, scenes `hangar` and `flow` (retail, 30 s), both
pass with no failures. Screenshots (private, not committed):
`private/evidence/P5-FEED-C/hangar/hub.png` (Sell Planes active, cash $12000)
and `hangar-list.png` (the list of the pilot's seeded planes). The `flow` scene
shows the hub with Sell inactive, as the new pilot's count requires.
The seeded planes are synthetic.

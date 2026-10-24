# The start flow's working plane and cash feed the hub

2026-10-24, task #2075 (P5-FEED-B, part of #2052). Implemented by `sonnet-1`.
Nothing here is `verified_original`; the original executable was not run.

## What shipped

* `crates/cs_app/src/ui/front_end/pilot_plane.rs`: `PilotPlane` (cash and the
  51-dword working record from the #2074 table, emptied when the Plane Name
  box is unchecked), `airframe_names` (strings `3000 + n`, `3020 + n`).
* The Plane Name OK (`name_step`) creates the pilot's plane and feeds the hub
  (`HubFeed`: cash, `plane_value`, `capacity`, `plane_weight`, the airframe
  strings, `dirty = false`, the record) and the purchase panel.
* The flow smoke no longer overrides the hub's plane; its screenshot
  `flow-plane-construction` shows the pilot's own figures (new pilot, default
  loadout: cash $0, plane cost $9930, weight capacity 10100 lbs, current
  weight 10100 lbs, *Hughes P21-J MKIII Devastator*).

## Readings and limits

* Units are the engine tables' own (dollars, pounds); no conversion.
* **Gun word `+0x84`.** #2074 calls its nibble "presence"; the consumer
  `0x4055c0` (statistics and prices) multiplies a slot's gun by `1 + bit`. The
  record's `gun_pairs` takes the nibble as the consumer reads it. The template
  value 7 therefore reads as three paired guns; which meaning the original UI
  gives it is not established here.
* **`fAllowAll` is passed as false** (cash 0): a retail profile's value is
  unread (see the #2074 note); owner capture or the profile reading decides.
* **Not re-fed yet:** the hub's panels are not populated in the windowed flow
  (no draft supplies their options), so no panel choice changes the record.
  `HubState::feed_pilot` is the re-feed point; wiring choices to the record is
  follow-up work.
* The hub's `saved_planes` stays unknown: the per-pilot saved-plane store is
  #2076.

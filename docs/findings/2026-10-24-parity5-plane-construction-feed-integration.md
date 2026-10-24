# Plane Construction hub feed: integration smoke

2026-10-24, task #2052 (integration step after #2074, #2075, #2076). Run by
`sonnet-1`. Nothing here is `verified_original`; the original executable was
not run and the smoke is scripted, not `human_play`.

Command: `CS_FRONT_END_SMOKE_SCENE=flow cs --cs-path <retail> --front-end
--smoke-seconds 30 --capture-dir private/evidence/P5-FEED-D` on the reviewed
commit (19 framebuffers, path `Campaign → PassengerCabin → PlaneName →
PlaneConstruction → PassengerCabin`, 0 unhandled focus moves).

`flow-plane-construction.png` shows the hub fed from the start flow: name
*Skylark*, `$$$ on Hand: $0`, `PLANE COST: $9930`, `AIRFRAME: Hughes P21-J
MKIII Devastator`, weight capacity and current weight 10100 lbs, Sell Planes /
Ready To Purchase / Cancel Purchase. The figures are the engine tables' own
dollars and pounds, so the draft's unit equivalence question does not arise
(see the #2075 note). The saved-plane count for Sell comes from #2076's store.

Still open, unchanged: the option panel is empty (no draft supplies options),
`fAllowAll` is passed false, and the gun-word `+0x84` reading is unsettled
(see the #2075 note).

Tests: `cargo test --locked -p cs_app -- accept_parity5_cabin_plane_construction_feed_
--include-ignored` runs 9 tests, all passing, with `CS_GAME_DIR` set.

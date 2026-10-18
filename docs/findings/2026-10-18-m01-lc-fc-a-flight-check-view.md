# M01-LC-FC-A: the Flight Check view model (task #1669)

Date: 2026-10-18. Capabilities: `retail`. Nothing here is `verified_original`.

`cs_app::ui::front_end::FlightCheckView` (pure data, no renderer) is built from the
mission's own data and read for M01 (`zbd/c3/m01`) by `FlightCheckView::from_retail`.

| Part | Source | Status |
| --- | --- | --- |
| Title | the region-prefixed long-name row of `langui.dll` at M01's campaign position (`SourceContext::join_agreement().grouped`) | read |
| Objectives | the four `MSG_BRF_HAM1_OBJn` names of M01's `IDENTITY` declarations, in (role, ordinal, index) order, resolved through `MessageCatalog` (`strings.dll`); the message-less secondary has no row | read |
| Pilot plane | `MissionStartConfiguration::airframe()` mapped to the airframe table (`CAMPAIGN_AIRFRAME_ROW`) | read (static analysis of the engine image) |
| Wingman plane | `wingmate_airframes()[0]` | **unknown**: no document or engine fact binds a wingman's airframe, so the view carries the binding's own reason |
| Gun and rocket lists (pilot, wingman) | none | **unknown**, with `LOADOUT_UNKNOWN_REASON` and `LOADOUT_RESOLVING_TASK` |
| Buttons | `FC_B_CHANGEAMMO`, `FC_B_CHANGEAMMOW`, `FC_B_RETURNBRIEF`, `FC_B_FLYMISSION`, each pinned against the retail `[FlightCheck]` section | read |

## Differences from the task wording

The task text and the owner image give four objective titles ("Find the main treasure
site", ...). The retail data's wording differs, as `2026-10-17-m01-window-hud.md` already
recorded for the HUD line: the view shows the data's text, never the image's.

## Unknowns

* **Gun and rocket lists.** The original fills them through scripting natives whose tables
  are not readable (`2026-10-18-f45-c-5-flight-check-options-and-ammunition.md`). Affected
  content: both slots of every mission's Flight Check. Resolving task: the Flight Check
  loadout measurement task named by `LOADOUT_RESOLVING_TASK`. The designed loadout is not
  shown in its place.
* **Wingman plane.** Affected content: M01's `wingman_1`. Resolved by a measurement of the
  wingman airframe binding; the view shows the unknown and its reason until then.
* **Order of the objectives on the original screen** is the HUD read model's designed order
  (the original's is unmeasured).

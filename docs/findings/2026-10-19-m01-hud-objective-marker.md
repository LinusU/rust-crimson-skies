# M01 HUD: the blue objective marker (#1657)

Date: 2026-10-19. Follow-up of #1641 / #1658. Capability: `retail` (owner-command smoke).
Nothing here is `verified_original`.

## Measured / used

* **The world position source** is the mission's `targets.zrd` rows carrying the
  `objective` marker key, already bound by `mission_targets` (#1566) to a node of the
  world container with its composed position. In M01 these are `MSG_OBJ_SITE1/2/3`
  (`t_chamber`, `grasshut2`, `shipwreck`), each with `help_label MSG_OBJ_EXAMINE`.
  `BoundTarget` / `MissionGroundTarget` now carry the row's `description` and
  `help_label` message names.
* **The wording** is `[<help_label text>] - <description text>` (`[Examine] - Site #1`),
  both resolved through `strings.dll` (`MissionMessages`, #1462); off screen the clock
  bearing is appended with `cockpit_hud::marker_text` / `clock_hour`. Colour: blue.
* `ADD_OBJECTIVE_TARGET` does **not** name a site in M01 (blocks 2, 17, 47, 50 name
  `workersvoyagezep`, `pzhookpoint`, `wv_tailhook/peoplehook`), so the sites are not
  shown through the objective-target flag the lifecycle sets.

## Unknown (not guessed)

* **Which site the original shows when.** The capture shows Site #1 only; the rule is
  unmeasured. Designed: the first declared `objective` row (`cockpit_hud::objective_site`),
  never retired when the player reaches it.
* The image drawn inside the off-screen circle (the capture shows a picture of the site),
  the circle's size, the arrow's geometry and the blue's exact value: the circle is an empty
  ring with a text arrow beside it; size and blue are estimated from the screenshot.
* Whether the on-screen form draws an icon above the label (the capture shows a small one).

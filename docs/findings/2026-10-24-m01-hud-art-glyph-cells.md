# M01 HUD art: the ROCKETS/GUNS digit boxes were hidden by the dial face (#1797)

Task #1797. Capabilities: `retail`, `gpu`, `audio`. Nothing here is `verified_original`.

Cause: the `4char_ammo` / `6char_type` glyph quads are authored flat at z = 0 in their gauge's
space, while the dial face mesh (`g819`/`g815`) reaches z = +0.024 toward the camera
(measured from the stored mesh). The glyph materials swapped correctly (`update_hud_art` chose the
right digit every frame) but the face's depth hid them. Unlit, depth-bias and double-sided
variants of the glyph material changed nothing; only moving the quads did.

Fix: each glyph cell stands `GLYPH_LIFT` = 0.05 in front of its face (designed; the original's
draw order is unmeasured). The ROCKETS type row now reads `BOOM` and the GUNS row `SLUG`
(the owner's measured labels, `cockpit_hud::ROCKETS_LABEL`/`GUNS_LABEL`), not a truncated content
id (`M01-PL`).

Evidence: owner-command smoke, 72 checks, no failures; the first-control frame shows ROCKETS
`BOOM 0003` (`private/evidence/M01-HUD-1797b/frame_02411_first_control.png`, not committed).

Still open: the `BOOM <n>` text of `cockpit_hud` is covered by the damage dial in the chase view;
the art dial now carries the count, so it is redundant there.

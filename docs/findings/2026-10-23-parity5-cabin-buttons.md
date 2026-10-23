# PARITY5-CABIN-INTRO.05: where the cabin buttons lead

Date: 2026-10-23. Task #1841 (part of #1749). Implementer `claude-2`; no
independent review yet. Nothing here is `verified_original`; the original
executable was not run. Tests: `accept_parity5_cabin_intro_05_` in
`crates/cs_app/tests/ui/parity5_cabin_intro_05.rs` (synthetic, run in CI).

| Button | Result |
| --- | --- |
| Next Mission (`PC_B_NEWMISSION`) | the table's `OpenBriefing`; the briefing is the mission session's screen, so the start flow hands over (as it did before this task) |
| Return to Main Menu (`PC_B_RETURNMM`) | layout script `MainMenu`: `Cabin --Back--> MainMenu`, asking `CloseProfile` (the profile is released) |
| Previous Missions (`ScrapBook_TOC`) | marked "not implemented" screen |
| Change Memento (`MomentoSelection`) | marked "not implemented" screen |
| Plane Construction (`PlaneName`) | marked "not implemented" screen |

The marked screen (`notice_view.rs`) is a dark panel with red "NOT IMPLEMENTED",
the button's name and "Esc, Return or a click returns to the cabin". It owns
the input while up. It is a project notice, not an original screen.
`StartFlow::not_implemented_seen` records every one opened, so a release claim
for the cabin can be gated on it (rule 5).

## Not done

* The three screens themselves; follow-up tasks are filed for each.
* No framebuffer of the notice was captured or looked at: the owner-command
  smoke of the whole flow is #1842. The input swallowing and drawing live in
  the window system and have no headless test here; the flow side is tested.
* Next Mission's disabled state (callback 2600) is not modelled, as before.

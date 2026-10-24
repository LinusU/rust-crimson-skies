# Flow smoke: Next Mission handover (#1965)

`CS_FRONT_END_SMOKE_SCENE=flow` now ends with a Next Mission press. After
`flow-mainmenu-again` the script presses Campaign (the pilot list shows Eve,
`flow-campaign-again`), Continue (the intro plays again and Esc skips it), photographs the
cabin (`flow-cabin-resumed`) and presses `PC_B_NEWMISSION`. The run ends on the hand-over
(like the start-flow legs) and its checks require: sections
`... → MainMenu → Campaign → PassengerCabin`, two skipped intros, and a hand-over to
`FIRST_MISSION`.

* Command: `CS_FRONT_END_SMOKE_SCENE=flow target/release/cs --cs-path <retail> --front-end
  --smoke-seconds 26 --capture-dir <dir>`; exit 0, 1464 frames, 19 framebuffers.
* `flow-cabin-resumed.png` shows the cabin with Next Mission, Change Memento (photo), Plane
  Construction, Previous Missions and Return to Main Menu.
* Task test: `cargo test --locked -p cs_app -- accept_parity5_cabin_intro_06_ --include-ignored`.

## Still unknown

* The briefing itself is not in this run: the hand-over exits the front end and the briefing
  belongs to the mission process (the start-flow smoke legs). This pass records the handover
  and the last front-end framebuffer before it.
* Whether the original replays the intro on every Continue of an existing pilot. Checked state only.

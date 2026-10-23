# PARITY5-CABIN-INTRO.06: the owner-command smoke of the intro and the cabin

Date: 2026-10-23. Task #1842 (part of #1749). Implementer `claude-2`; no
independent review yet. A scripted run is never `human_play`; nothing here is
`verified_original`; the original executable was not run. Tests:
`accept_parity5_cabin_intro_06_` in `crates/cs_app/tests/ui/parity5_cabin_intro_06.rs`
(one synthetic, run in CI; one windowed, `#[ignore]`d without `CS_GAME_DIR` and a GPU).

## Command

```sh
cargo build --release --locked -p cs_app --bin cs
CS_FRONT_END_SMOKE_SCENE=flow CS_CAPABILITIES=retail,gpu,audio \
target/release/cs --cs-path "$CS_GAME_DIR" --front-end --smoke-seconds 16 \
  --capture-dir private/evidence/1842
```

The scene is chosen by the environment variable because `cli.rs` has no flag
for it (as for the cabin scene). The script ([`flow_script`]): main menu, press
Campaign, type a new pilot name, Continue (the intro movie starts), three
framebuffers of the movie, Esc skips it, the cabin, hover and press Previous
Missions (the marked notice), Esc, Return to Main Menu. The run's own checks:
sections visited `MainMenu → Campaign → PassengerCabin → MainMenu`, the intro
ended exactly once (`Skipped`), exactly one notice (`PC_B_PREVIOUS`) opened
and none is left up, every framebuffer non-uniform. It exited 0 with ten
framebuffers (1600x1200, git-ignored) in `private/evidence/1842/`:
`flow-mainmenu`, `flow-pilot`, `flow-pilot-typed`, `intro-a/b/c`, `flow-cabin`,
`flow-notice`, `flow-cabin-after`, `flow-mainmenu-again`, plus `final` and
`report.json`.

## What looking at the frames found (two defects, fixed here)

1. **The intro was invisible.** #1839 played `chap1.mpg` through the movie
   presenter, whose sprite sits at z 0 under the section's panes and which is
   drawn by a second camera that also sees the cabin. The cabin hid the movie
   completely while the shell swallowed the input. The shell now hides the
   panes, buttons and cursor while the intro is up and shows them on the frame
   it ends. `intro-a` is the movie's fade-in (dark), `intro-c` a sepia frame
   with a zeppelin and palms.
2. **Scripted keys never reached the movie's skip.** The smoke wrote its
   presses inside `frame_shell` (`Update`), whose order against the movie's
   skip system is unspecified, so `just_pressed` was cleared before the movie
   read it. The smoke now injects in `PreUpdate` after the input systems, as a
   real key arrives. The existing main-menu and cabin smokes still pass.

## Beside the original

The cabin was compared with the owner's frame in #1840
(`2026-10-23-parity5-cabin-hub.md`); `flow-cabin` is the same composition
(memento frame interior still the lilac key patch, recorded there). **There
is no original frame of the intro movie in this checkout, and none of the
notice screen exists in the original (it is a project notice).** The intro
frames show the movie decoded from `chap1.mpg`; whether it matches the
original's presentation (letterboxing, timing, audio) is unverified. Audio:
the run reports the intro's outcome only; no audio-started evidence beyond
the player opening with the declared output was captured here.

## Not done

* Natural end of the intro (the script skips at 10 s; the movie's length
  was not measured here).
* The intro's audio is not asserted by the smoke.
* `accept_parity3_pause_map_02e_01_rows_spawn_at_authored_positions_and_return_closes`
  fails locally (`other_rows` is `["T"]`, not `M`): it reads the in-game
  Preferences layout and does not touch this change; not investigated.

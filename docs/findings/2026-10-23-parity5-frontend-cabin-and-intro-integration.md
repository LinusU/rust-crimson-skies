# PARITY5-FRONTEND-CABIN-AND-INTRO (#1749): integration of the intro and the cabin hub

Date: 2026-10-23. Integration step of #1749 after children #1837-#1842.
Implementer `claude-1` (Sonnet 5.5). No independent review yet. A scripted run
is never `human_play`; nothing here is `verified_original`; the original
executable was not run. No code changed in this step.

## Run

```sh
cargo build --release --locked -p cs_app --bin cs
CS_FRONT_END_SMOKE_SCENE=flow CS_CAPABILITIES=retail,gpu,audio \
target/release/cs --cs-path "$CS_GAME_DIR" --front-end --smoke-seconds 16 \
  --capture-dir private/evidence/1749
```

Exit 0; sections `MainMenu -> Campaign -> PassengerCabin -> MainMenu`; intro
outcome `["Skipped"]` (Esc); 11 framebuffers (1600x1200, git-ignored).
`cargo test --locked -p cs_app -- accept_parity5_cabin_intro --include-ignored
--test-threads=1`: 26 passed, 0 failed. With the default parallel threads
`accept_parity5_cabin_intro_04_the_windowed_smoke_draws_the_cabin_and_its_button_states`
failed once (two windowed tests at once) and passed alone and serially.

## Beside the original

`private/evidence/1749/flow-cabin.png` against the owner's
`private/ref-drydock/cabin/original.png`: window and red plane, radio, microphone,
frame, map with one pin, book, blueprint, and all five brass buttons
(Change Memento, Plane Construction, Next Mission, Previous Missions, Return to
Main Menu) agree in placement and art. **Differs:** the memento frame shows a
lilac patch where the original shows a photograph. `intro-b.png` shows the
decoded `chap1.mpg` letterboxed in a 460-row band on black, with film scratches
and grain; there is no owner frame of the intro in this checkout that matches
the instants captured (the owner's frames, man at the desk / woman in a coat, are
`chap1` at 15.4 s and 31.4 s; the smoke skips at ~10 s), so a side-by-side of the
same instant is not done.

## Acceptance state

| Criterion | State |
| --- | --- |
| Intro decoded from retail, Esc/click skip | done, tested and seen |
| Intro audio | player opens with the audio output; audio is not asserted or heard by an agent |
| "Only the first time per pilot" | unknown: gate is engine callback 2151, unread; plays on every entry from the pilot page |
| Cabin from original UI data, five buttons with hover/press/focus | done except the memento picture |
| Next Mission -> briefing, Return -> main menu | flow tests pass; the owner-command smoke exercises Return only, not Next Mission |
| Other three buttons | marked "not implemented" notices; follow-ups filed (rule 5: blocks release) |
| Frames beside the original | cabin done; intro same-instant comparison and human review outstanding |

## Follow-ups filed

Change Memento screen and the memento picture (callback 2150); Previous
Missions (`ScrapBook_TOC`) screen; Plane Construction (`PlaneName`) screen;
callback 2151 (first-time rule) and intro same-instant frames plus audio;
Next Mission step in the owner-command flow smoke.

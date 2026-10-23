# PARITY5-CABIN-INTRO.03: the intro between the pilot page and the cabin

Date: 2026-10-23. Task #1839. Implementer `claude-2`; no independent review
yet. Nothing here is `verified_original`; the original executable was not run.
Tests: `accept_parity5_cabin_intro_03_` in
`crates/cs_app/tests/ui/parity5_cabin_intro_03.rs` (synthetic, run in CI).

## What the front end does now

When the start-flow machine takes the pilot page's confirm row
(`ProfileSelect --ConfirmProfile--> Cabin`), the window plays `chap1.mpg`
(`MoviePlayer`, task #1838) and only then lets the cabin take input. The
machine is already in `Cabin` behind the movie. Esc, Space, Return or a left
press skip it (the player's rule); the natural end and a skip both leave the
cabin shown. Menu input is swallowed while the movie is up and on the frame it
ends, so the skip key does not also act on the cabin. The movie opens one frame
after the confirm, so the Enter or click that confirmed the pilot cannot skip
it. Returns to the cabin from the Scrapbook, Briefing, Construction or Results
do not play it (`intro::follows`), matching `PASSENGERCABIN`'s `XQ` gate that
only the Campaign page sets.

A movie that cannot be opened or stops with a failure is recorded in
`ShellResource::intro().outcomes` (and printed); the cabin still follows. The
menu music is stopped while the movie plays and restarts afterwards.

## Unknown, not guessed

* **First time per pilot only?** The task asked for that rule "if #1837
  measured it". It did not: the gate is engine callback `2151` (value domain
  unread, see `2026-10-21-parity5-cabin-intro-measurement.md`). So the intro
  plays on **every** entry from the pilot page and nothing is persisted per
  profile. Resolving: callback 2151 analysis or an original run.
* Whether the original pauses the screen music during the movie (message
  `11003`/`11004` meanings are unread).

## Not done here

* Smoke frames of the intro and cabin beside the original: the existing
  windowed smoke never confirms a pilot, and the owner-command capture is task
  #1842. No framebuffer of the intro was produced or looked at in this task.
* The cabin screen's own contents (task #1840 onward); the section shown is
  the layout's `PassengerCabin` as before.

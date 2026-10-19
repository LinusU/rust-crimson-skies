# PARITY-M01-LC-PHASE (#1663): the launch cinematic as a session phase

Date: 2026-10-19. Implementer: `claude-2`. Built on
`2026-10-18-parity-m01-lc-cinematic-timeline.md` (length),
`2026-10-19-m01-composed-start-pose.md` (hand-over pose). Nothing here is
`verified_original`. Test prefix `accept_parity_m01_lc_phase_`.

## What it does

`cs_app::mission_session::launch_cinematic`: `LaunchCinematic` is a resource
(session state) counting **integer fixed ticks** at `BASELINE_FIXED_HZ`. Its
length is `generic_intro`'s replay for the stage's own airframe scene root
(`measure_launch_cinematic`: 40.15 s, 30.25 s for Balmoral/Warhawk), rounded up
to whole ticks (`ticks_for`: 4818 ticks for the M01 default at 120 Hz). It is
installed after the Flight Check, so it counts from Fly Mission.

While it plays: the original flight law is not stepped (`run_if`
`player_has_control`), the body's velocity is held at zero and put back on the
last tick, the weapon publisher drops every key and the held-guns repeat, the
cockpit HUD root is hidden and the letterbox bars (#1664) are on. When the last
tick has run, everything switches back at once; the body stands at the composed
start pose (stored `aiv.zrd` record + 100 m), which is the pose the hand-over
uses (#1652).

## Not covered, and why

* **Seen in M01.** The `--smoke-seconds` and capture runs start the mission
  directly (like they skip the Flight Check), so they do not enter the phase and
  no screenshot of it exists. Rule 6's "seen" acceptance is therefore **not
  met** by this task; a follow-up must let the smoke run through the phase.
* The camera shown during the phase (#1665), the bars' slide-in and exit
  (#1664), and what the world's actors do meanwhile (the mission host keeps
  ticking) are unmeasured.
* The cinematic's tick-rate relation is a choice, not a measurement: the
  original steps variable `game_dt` seconds; the length is rounded up to whole
  ticks.

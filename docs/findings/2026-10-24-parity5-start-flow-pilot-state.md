# PARITY5-START-FLOW: per-pilot state across the mission-to-cabin return (#1973)

Date: 2026-10-24. Part of #1747; closes the "Per-pilot state is not saved"
limit of `2026-10-22-parity5-start-flow-03-return-to-cabin.md`. Implementer
`sonnet-2`; no independent review yet. Nothing here is `verified_original`.

## What now crosses the process images

* **The pilot** is the profile population's persisted *active* pointer. The
  pilot page already writes it (`ProfileSession::create` / `select`); the
  mission and resumed legs open the same population and the session selects
  the active pilot on open.
* **The mission record** is written by the mission leg
  (`launch_mission`, `--from-start-flow` only) before it execs back to the
  front end: `pilot_progress::record_mission_end` counts the attempt in the
  pilot's own document as `mission.<id>.attempts`, `mission.<id>.failed|
  abandoned|completed` and `pilot.last_attempt`. The **completed count** is the
  number of missions whose `completed` counter is above zero. A failed flight
  or a quit from the pause map completes nothing; the `Completed` counter has
  no caller until the Mission Succeeded return (#1972) lands.
* **One population per smoke chain.** The legs write to `front-end/`,
  `mission/`, `resumed/` and `replay/` of one capture directory;
  `chain_profile_base` points all of them at `front-end/profiles`.
  A non-smoke run uses `$CS_PROFILE_DIR`, else `~/.crimson-skies`.
* The smoke's legs check it: the first leg must have a pilot, the resumed leg
  must find a pilot with an attempt on record, and the report lists the
  pilot's name, completed count, attempts and last ending.

## Unknown, recorded rather than guessed

* The original's `Status.dat` / `Mission.*` layouts are unknown (F64 owner
  capture, #746). These counters are this reimplementation's own fields and
  say nothing about the original's bytes.
* The cabin hub (#1840) does not draw the pilot yet, so "shows the same pilot"
  is read off the smoke report, not off a screen.
* `OpenProfile` / `ApplyOutcome` machine requests stay recorded-not-performed
  in the resumed walk; the store is the carrier.
* A failed record write is reported on stderr and does not stop the return.
* Not run through the owner-command smoke in this session; the headless
  `accept_parity5_start_flow_pilot_state_` tests cover the store round trip.

# F16-F: the original clock, pause and speed-up policy, declared and compared

Date: 2026-10-06. Task: F16-F "Compare fixed-tick clock probes against an
owner-supplied original capture" (`#391`,
`specs/F16-coordinates-units-origin-management-and-clocks.md`, stage `### F16-D`
follow-up). Shared contract: `docs/contracts/FLIGHT-PHYSICS.md`. Required
capability: ordinary build/test. The machine reports `retail`, `gpu` and
`audio`; **none was used** — this stage reads no original data at run time,
renders nothing and plays nothing, so no `private/evidence/` report is
produced.

## The short version

This stage answers the four questions F16-F inherited from F16-D — *what
freezes on pause, do a weapon cooldown and an objective timer share one clock,
is paused time banked, does single-player speed-up advance time* — from
**static code analysis of the owner-supplied executable**, which the owner
accepted in place of a runtime capture (owner notes on `#391`, 2026-10-05, and
the correction of the same date). The answers are then **declared in code**
(`OriginalClockPolicy`) and compared with the project's `ClockPolicy`/
`PausePolicy` under a tolerance chosen *before* the comparison.

* The claim that declaration can carry is `inferred`
  (`ORIGINAL_CLOCK_POLICY_STATUS`), never `verified_original`: the method is
  code-derived and non-runtime, and `OriginalClockPolicy::claim_status`
  caps the claim there structurally, so attaching a record that *would*
  verify the original still claims `inferred`.
* Four divergences came out of the comparison. Two are already decided by the
  spec (non-negotiable 3 and 4: integer ticks with a fixed dt, speed-up as
  fixed ticks); two are open and were filed as **#721** (the original's
  0.125 s frame-dt cap versus the project's unbounded advance) and **#722**
  (the original's speed-up scales the cooldown and objective timers, the
  project's gameplay clock grants no local authority).
* What only a run could give — measured frame pacing, the distribution of
  frame deltas, timing uncertainty — stays **unmeasured** and is recorded as
  such below.

## Provenance

* Image: `$CS_GAME_DIR/crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` —
  the owner's decryption of `crimson.icd` (`0e3b4724…9833b`), analysed at the
  owner's request with the Kuna decompiler v1.692 and checked in the
  disassembly.
* Addresses below are **virtual addresses**; for `.text`, `.rdata` and `.data`
  below `0x643000`, file offset = VA − `0x400000`.
* Provenance only: addresses and behaviour are committed, **never** the image,
  a disassembly listing, decompiled code or any other executable byte. The
  image sha256 travels as a `Fingerprint` inside the declared policy's
  `EvidenceRecord`; the locator points at this document.
* This is static code evidence, not `ObservationMethod::RuntimeObservation`.
  Nothing here self-awards `verified_original`.

## The four answers, with the addresses that settle them

### 1. What freezes on pause, and what keeps running

Pause (`MSG_CMD_PAUSE_GAME`, default Esc) pushes the escape screen
(`0x4a13f0` → `0x4a0d20`) over the flight state. The flight state's vtable is
at `0x608308`: `+0x04` activate/deactivate `0x4a0a50`, `+0x10` update
`0x4a0a80`, `+0x1c` suspend `0x4a0b50`, `+0x20` resume `0x4a0b70`. The main
loop `0x5b5a30` only ever updates the **top** state of the screen stack, so
once the escape screen is pushed the flight frame routine `0x4a0220` does not
run at all (owner correction, 2026-10-05). Suspend (`0x4a0b50`) snapshots and
pauses the playing sounds (`0x594040`/`0x594580`), sleeping 1000 ms first if
no snapshot exists yet; resume (`0x4a0b70` → `0x4a0b30`) restores them
(`0x594620`) and does **not** reset the clock. The active flag `[state+0x24]`
set through `0x4a0a50` is a separate enable used at activation, not the pause
toggle.

Everything that runs inside `0x4a0220` is frozen, therefore:

| Frozen while paused | Where |
| --- | --- |
| World/node action-callback update — planes, AI, turrets, effects, animations | `0x4d0010` |
| The player plane callback, which advances the vehicle clock `0x71c470 += dt`; weapon, AI and turret intervals are absolute timestamps against it (turret `FIRE_RATE` computes its next shot as `0x71c470 + rand·(a+b)/2 + 1`) | callback `0x4897c0`, turret `0x4a9df0` |
| Camera shake, HUD and input | `0x42eb80` and the rest of the frame routine |
| Mission/objective update: mission elapsed time `+0x6f0`, dormant/nap timers `obj+0x5cc`, and the countdown object `0x71b468` whose remaining value is decremented by dt and drives both expiry and the HUD display | `CZMission::Update` `0x46a490`, countdown `0x46c5f0`/`0x46c640`/`0x455700` |
| The end-of-mission delay | `0x469770` |
| Playing sounds (snapshot-paused, not stopped) | `0x4a0af0` → `0x594040`/`0x594580` |

Keeps running while paused:

| Keeps running while paused | Where |
| --- | --- |
| The frame clock and both accumulators: game time `0x9ad748`, real time `0x9ad750` (the per-frame `game_dt` lives in `0x9ad744`) | frame clock `0x59c0c0` |
| The sound system's per-frame update | `0x5b26e0`, `0x595d10`, `0x595900` |
| Network code, which reads the OS tick count / `0x9ad748` directly | `0x496xxx`–`0x49axxx` |
| Force-feedback and warning effect expiries, which compare against `0x9ad748` — so such an effect ends right after resume | `0x480d60`, `0x481540` |
| The mission countdown's millisecond copy `[0x71b470]`, synced from `GetTickCount` in `0x46c610`; it counts wall time but is used for neither expiry nor display | `0x46c610` |

Reset `0x59c060` zeroes the accumulators and is called only at app/state
init, never on resume.

### 2. Do a weapon cooldown and an objective timer share one clock?

**Same dt source, separate accumulators.** Both read the single `game_dt` in
`0x9ad744`; they do not share a counter:

* weapon, AI and turret timers count the **vehicle clock** `0x71c470`,
  advanced in the player plane callback (`0x4897c0`) during the world update;
* objective timers advance inside the **mission update** `0x46a490`, near the
  end of the frame (`0x4a09c3`).

Both stop together on pause (both run under `0x4a0220`) and both scale
together with the speed-up (both read `0x9ad744`).

**Caveat, recorded not dropped:** the mission update is skipped while the
player flag `[player+0x91d]` is set (see `0x4a0220`, around lines 371–376).
While the player is down the objectives freeze while the vehicle clock keeps
advancing; in network games only the countdown keeps ticking.

### 3. Is paused time banked?

**No.** The frame clock ticks every main-loop iteration, paused frames
included, so the game-time accumulator `0x9ad748` keeps rising during a pause.
Nothing is queued for resume: the first resumed frame's `game_dt` is one
ordinary frame delta, at most 0.125 s. Resume does not reset the clock, and
the only zeroing call (`0x59c060`) happens at init.

### 4. Does single-player speed-up advance time?

**Yes — 2× dt, capped, network-gated, unbound by default.**

* Input command **102 (`0x66`)** toggles a flag (edge-triggered, `0x6288d4` /
  `0x71c520`) inside the player input handler `0x487460`.
* While the flag is set, each frame calls `0x59c1f0(2.0)`: `game_dt = 2 × real
  dt`. The multiplier is a **one-frame** value, reset to 1.0 right after use
  (`0x63b16c`).
* The clamp still applies, so a frame advances at most 0.125 s of game time
  even while sped up: at low frame rates the 2× saturates instead of running
  away.
* There are no fixed ticks in the original: everything that reads `0x9ad744`
  advances at 2×, including the vehicle clock and the mission timers.
* The speed-up is **disabled when the network setting** (`0x64f750`,
  registered in `0x43fb50`) is non-zero, i.e. in multiplayer. Nothing grants
  local simulation-speed authority there, and the countdown display uses a
  network-synced value (`0x49bed0`) in network games.
* Command 102 has **no default binding** and is absent from the controls
  screen (the command table at `0x4936c0` holds 64 commands, see `#505`), so
  a player cannot reach it without an external binding.

Frame clock facts behind all four answers: `real_dt = (GetTickCount() −
last) × 0.001` s (constant `0x6075dc`), `game_dt = scale × real_dt`, clamp on
by default (`0x63b160` = 1) with max `0x63b158` = 0.125 s and min `0x63b15c`
= 0 applied *after* scaling; `game_dt` is stored in `0x9ad744` and
accumulated into `0x9ad748` (game) and `0x9ad750` (real). The debug command
line `-freq F` (`0x4a6ff0`) sets both clamp bounds to `1/F` and thereby forces
a fixed dt; it is a debug option, not the shipping policy.

## What was expressed in code (acceptance criterion 2)

`crates/cs_sim/src/time.rs` (owner path) gained a F16-F section:

* `OriginalClockPolicy` — the declared policy (`measured_original()`): dt
  source `OriginalDtSource::VariableFrameDelta`, `max_frame_dt` 125 ms,
  `min_frame_dt` 0, `frame_dt_clamped` true, `banks_paused_time` false, the
  two subsystem lists, `gameplay_timers_share_dt_source` and
  `gameplay_timers_have_separate_accumulators` both true, and
  `OriginalSpeedUp { factor 2.0, capped_by_max_frame_dt true, network_gated
  true, default_binding false }`. `pause_policy_for` maps every declared
  subsystem to `PausePolicy::Freeze` or `PausePolicy::KeepRunning` and
  returns `None` for anything the declaration does not cover — a gap is never
  a default.
* `EvidenceRecord` (code-derived, non-runtime): source `Document` naming this
  findings entry, `Fingerprint { kind: Installation, sha256 <the image hash
  above> }`, a locator on this entry, and method **`Inference`** — not
  `RuntimeObservation`, which would be fabricated, and not a direct
  observation, so `EvidenceRecord::verifies_original()` is false even though
  the record carries an original fingerprint and a locator. Three limitations
  are recorded: static analysis rather than a run, frame pacing/timing
  uncertainty unmeasured, addresses-only provenance.
* `OriginalClockPolicy::claim_status()` — the ladder is `synthetic →
  unknown`, `authored → designed`, `inference → inferred`, `document review →
  `documented`, `tool probe → observed_tool`, an unlocated/unfingerprinted
  direct method → `unknown`, and a record that *does* verify the original →
  `verified_original` — **capped to `ORIGINAL_CLOCK_POLICY_STATUS`
  (`inferred`)** for a declared policy. A policy compiled from source can
  never claim it watched the original run.
* `PolicyTolerance { dt_slack_nanos }` and `ORIGINAL_POLICY_TOLERANCE` =
  **0 ns slack, declared as a constant before any comparison runs**
  (`FLIGHT-PHYSICS`, "Calibration acceptance"). It is an input to
  `compare_project_clocks(rate, tolerance)`; the comparison reads what it is
  handed and never widens it. It decides two numeric facts: whether the
  project's fixed dt lies inside the original's measured per-frame window
  (`clock.frame-dt-bound`), and whether a declared fixed debug dt equals the
  project's (`clock.tick-source` when the `-freq` variant is declared).
* `OriginalPolicyComparison` — findings, `divergences()`, `not_modeled()`,
  `claim()`, `verified_original()` (always false) and `summary()`. Unlike
  `ProbeComparison`, a divergence here is mostly a *designed* difference
  against a spec requirement, so it never downgrades the claim: the claim is
  what the original policy's evidence supports.

## The comparison at 64 Hz (the project's designed fixed rate)

`OriginalClockPolicy::measured_original().compare_project_clocks(TickRate::new(64), ORIGINAL_POLICY_TOLERANCE)`
produces 14 findings:

| Field | Relation | Gist |
| --- | --- | --- |
| `pause.gameplay` | agrees | flight world frozen ↔ simulation, multiplayer simulation and authoritative gameplay all `Freeze` |
| `pause.presentation` | agrees | frame clock and escape screen keep running ↔ UI wall and media `KeepRunning` |
| `pause.sound-playback` | not-modeled | snapshot-paused sounds ↔ `cs_sim::time` declares no audio policy (F41/F46 own audio pause) |
| `pause.network` | not-modeled | network keeps running ↔ `cs_sim::time` declares no network policy |
| `pause.bank` | agrees | not banked ↔ `PausePolicy::Freeze` clocks drop paused wall time (`SimClock::advance` returns 0, carry untouched) |
| `clock.tick-source` | **diverges** | one variable `game_dt` per frame ↔ fixed dt of 15 625 000 ns (integer accumulator at 64 Hz) |
| `clock.frame-dt-bound` | agrees | 15 625 000 ns lies inside the measured [0, 125 000 000 ns] window with 0 ns slack |
| `clock.frame-dt-cap` | **diverges** | a stalled frame advances ≤ 125 ms ↔ `SimClock::advance` accepts any `Duration` |
| `timers.shared-dt-source` | agrees | one dt source, separate accumulators ↔ one `SimClock` commit feeding two `TickTimer`s, each with its own remaining count |
| `timers.player-down-gate` | not-modeled | mission update skipped while the player is down ↔ `GameplayTimeline` has no player-down gate |
| `speed-up.network-gate` | agrees | 2× only while not networked ↔ single player `AdvanceFixedTicks`, multiplayer `NoLocalAuthority` |
| `speed-up.mechanism` | **diverges** | one-frame 2× multiplier on the variable dt ↔ whole fixed ticks |
| `speed-up.reaches-gameplay-timers` | **diverges** | everything reading dt scales ↔ authoritative gameplay grants no local authority |
| `speed-up.default-binding` | not-modeled | no default binding ↔ the clock API exists, the input layer binds no speed-up command yet |

## Divergences recorded and filed (acceptance criterion 4)

| Divergence | Disposition |
| --- | --- |
| `clock.tick-source` — the original's variable dt versus the project's fixed 64 Hz tick | **Owner decision already standing:** F16 non-negotiable 3 ("simulation time is integer tick count with fixed dt") and 4. Recorded here, not changed. The 64 Hz rate itself is a designed development value, not a measured original rate. |
| `speed-up.mechanism` — the original's 2× variable-dt multiplier versus whole fixed ticks | **Owner decision already standing:** F16 non-negotiable 4 ("speed-up advances fixed ticks, not a variable dt"). Recorded here, not changed. |
| `clock.frame-dt-cap` — the original clamps every frame to 0.125 s, `SimClock::advance` is unbounded | **Filed: #721** (`F16-F-CAP`) — adopt a per-frame cap or record it as accepted; either way update the finding and this entry. |
| `speed-up.reaches-gameplay-timers` — the original's speed-up scales cooldowns and objective timers, `ClockPolicy::authoritative_gameplay` grants no local authority and `GameplayTimeline::advance_fixed_ticks` always refuses | **Filed: #722** (`F16-F-SPEEDUP`) — decide how extra fixed ticks reach the gameplay timers, or accept the divergence; keep the multiplayer refusal and AC04. |

No `ClockPolicy`/`PausePolicy` pairing was changed, and no `accept_f16_d_`
test was touched. The four `not-modeled` findings are coverage gaps rather
than divergences and are attributed above to the subsystems that own them
(F41/F46 audio pause, the networking crate, the mission runtime, F22 input
bindings); they are listed so a reader cannot mistake silence for agreement.

## Tests (`accept_f16_f_`)

`crates/cs_sim/tests/accept_f16_f_original_clock_policy_comparison.rs`, five
tests, all driving production code in `cs_sim::time` (plus one that pins this
findings entry against the declaration):

| Test | Covers |
| --- | --- |
| `accept_f16_f_measured_original_policy_pins_the_static_analysis_constants` | the measured constants (125 ms/0 ns window, clamp on, no banking, 2×, capped, network-gated, unbound, shared dt source, separate accumulators), the five frozen and five running subsystems, and that every declared subsystem maps to exactly one `PausePolicy` |
| `accept_f16_f_code_derived_evidence_never_verifies_the_original` | the evidence record: `Document` source naming this entry, the installation fingerprint with the owner's sha256, `Inference` method, `verifies_original() == false`, claim `inferred` == `ORIGINAL_CLOCK_POLICY_STATUS`; a record that *would* verify the original is still capped to `inferred`, a synthetic record claims `unknown`; and this findings entry exists and carries the sha256 and the addresses |
| `accept_f16_f_comparison_pins_the_measured_policies_against_the_project_clocks` | the exact three-way split of the 14 findings, the declared 0 ns tolerance, `claim == inferred` and `verified_original() == false`, and that every finding states both sides |
| `accept_f16_f_tolerance_is_selected_before_the_comparison` | 4 Hz (250 ms of game time per tick) falls outside the original's window at the declared tolerance and inside it when a larger slack is declared *beforehand*; the `-freq` fixed-dt variant agrees exactly at 64 Hz and diverges at 30 Hz, admitting the declared slack only |
| `accept_f16_f_project_clock_behaves_where_the_policies_agree` | the project actually behaves as the declaration requires where the comparison says "agrees": pause commits zero ticks and empties neither timer, resume banks nothing, and the multiplayer policy refuses local speed-up while the single-player policy accepts whole ticks |

## Mutation probes (implementation neutered → tests fail; all reverted)

Filled in after the runs below.

## Recorded unknowns (recorded, not guessed)

- **Frame pacing, frame-delta distribution and timing uncertainty are
  unmeasured.** Only an original run could give them (the owner's update says
  so explicitly); the policy declares the clamp, not what real frames did.
- **No runtime capture exists, so `verified_original` is unreachable here.**
  `OriginalClockPolicy::claim_status` enforces that structurally; only
  owner-supplied original-run evidence can raise
  `ORIGINAL_CLOCK_POLICY_STATUS`, and a task may not raise it itself.
- **The original's cooldown and objective *periods* are not measured.** The
  declaration says the two timers share a dt source and separate
  accumulators; it says nothing about how long either takes, and no period is
  invented.
- **The 64 Hz rate, and every tick count in the acceptance tests, are
  designed development values**, inherited from F16-C/F16-D, not measured
  original rates.
- **The player-down skip is recorded as `not-modeled`, not as agreement.**
  Whether the project's mission runtime freezes objectives while the player
  is down belongs to the mission stages (F37/F39), not to `cs_sim::time`.
- **`cs_sim::time` covers four time domains, not every subsystem the original
  pauses.** Audio pause, network timing, the input binding for a speed-up
  command and mission-specific gates are named as `not-modeled` findings so
  the gap stays visible.

## Follow-ups filed

- **#721 (`F16-F-CAP`)** — decide whether the fixed-step accumulator needs
  the original's 0.125 s frame-dt cap.
- **#722 (`F16-F-SPEEDUP`)** — decide how single-player speed-up reaches
  weapon cooldowns and objective timers.

Both are `todo` with `ordinary build/test` as the required capability; neither
may be settled by silently changing a policy pairing.

## Commands run

Filled in after the runs below.

## Wiring edits (outside owner paths, logic-free)

None. The new types live in `crates/cs_sim/src/time.rs`, which is already a
public module, and the new test file is auto-discovered by Cargo. No
`Cargo.toml`, no `Cargo.lock`, no `src/lib.rs` change.

No protected path, original datum or binary file is involved: the only
original-derived values committed are virtual addresses, behaviour in prose
and one sha256.

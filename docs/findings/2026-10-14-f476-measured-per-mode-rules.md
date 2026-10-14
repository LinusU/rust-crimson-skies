# F56-A follow-up (task #476): the per-mode multiplayer rules measured by static analysis, and what stays unknown

Date: 2026-10-14. Task: #476 "Measure per-mode multiplayer rules (limits,
respawn, lives, friendly fire, draw, disconnect, scoring events)" under stage
`### F56-A` (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`).
Capabilities used by this stage: plain build/test. The measurement itself is
the owner's, supplied through the task note of 2026-10-05; the owner accepted
that static code evidence in place of a capture. Evidence class:
`observed_tool` (static analysis), **never** `verified_original` — nothing here
ran the original executable.

Supersession: this file records the encoding of the owner's measurement into
production and supersedes the "What stays unknown" list of
`docs/findings/2026-10-03-f56-a-scoring-and-respawn-from-player-zrd.md` (that
file remains the record of what *retail data files* state and of the negative
search).

## Source and provenance

- Owner note of 2026-10-05 on task #476: static analysis (Kuna v1.692) of the
  owner-supplied decrypted executable `crimson.decrypted.exe`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75`
  (`cs_net::rules::measured::EVIDENCE_IMAGE_SHA256`), plus
  `MULTIPLAYERLOBBY_MISSION.SCRIPT` from `GOSDATA/ASSETS/crimson.rof` and UI
  string ids (langui.dll 10094–10128, messages strings.dll 7004–7083 and
  209–211). VAs; file offset = VA − 0x400000 below 0x643000.
- The `player.zrd` scoring constants and respawn offsets come from the
  installation itself and keep the span/digest provenance recorded in
  `docs/findings/2026-10-03-f56-a-scoring-and-respawn-from-player-zrd.md`.
- No decompiled code and no executable bytes are committed; only addresses,
  string ids, constants and behaviour are recorded, as the owner note directs.

## Where the measurement lives in production

1. **`crates/cs_net/src/rules/measured.rs`** (new): the typed measurement.
   `MissionOptions` is the host's Mission Options block (`0x642f8c`,
   broadcast as message `0x26`); `draft(&MissionOptions, TeamMode, tick_hz)`
   fills every measured field of a `RuleDraft` and leaves the unmeasured ones
   empty; `MissionType` carries the launch codes, the two-team restriction and
   the per-type policy table; `scoring()` carries the per-type scoring events;
   `end_triggers()` the match-end reasons. `POLICY_ADDRESSES` cites every
   policy's addresses against the image sha256. The vocabulary grew two
   measured shapes: `Lives::Limited(u32)` (the original stores `10000`, which
   does not fit the old `u8`) and `Respawn::OnCommand` (Auto Respawn off means
   the dead pilot respawns on the rebound fire-weapons command, original
   command 19).
2. **`crates/cs_content/src/multiplayer.rs`**: every mode the catalog names
   carries a `MeasuredRule` per answered field — the same claim id its
   `UnknownRule` carried, the policy label, the citation and
   `EVIDENCE_IMAGE_SHA256`, class `observed_tool` — and an `UnknownRule` with
   its reason for every field nothing measured answers. The catalog may not
   depend on `cs_net`, so the mirror is labels and citations, pinned by a
   dev-dependency test.
3. **`crates/cs_sim/src/multiplayer/result.rs`**: documentation only. The
   resolver's resolution order stays a declared engine design (below).

## The measured rule table (all four modes; values by host option)

The original does **not** fix most rules per mode: the host chooses them on
one Mission Options screen shared by every mode. `draft` encodes that shape.

| field | measured policy | where |
| --- | --- | --- |
| `teams` | Dogfight: the host's team-count restriction (up to 16; engine `MAX_TEAMS` may be smaller and is unchanged). CTF and Zeppelin: exactly 2. Launch codes: CTF 3, Dogfight 1 without / 2 with teams, Zeppelin 4; mp mission index 1/2/3 = DM/CTF/ZvZ | `0x4136e0`, langui 10518/10519 |
| `late_join` | **Closed**: a session in progress refuses joins ("Cannot join session: game in progress", langui 7083) | `0x412ed2` |
| `time_limit` | Host option: victory by time counts minutes × 60 s; UI default 10 minutes (2 digits); disabled in score mode | `0x4972c0` → `0x46c510`, end reason 1 at `0x496d60`, langui 10106 |
| `score_limit` | Host option: victory by score; UI default 40 (3 digits); FFA ends when any player ≥ limit, team modes when the team total (bonus `+0x14` + Σ member scores) ≥ limit; disabled in time mode | `0x499270`, `0x46ea40`, end reason 2 |
| `lives` | Host option: Limited Lives count, else the stored count **10000**; every death decrements (`0x498c41`); the HUD shows the count when ≤ 99 (langui 209/210/211); at 0 the pilot cannot respawn | `0x4136e0` |
| `respawn` | Host option: Auto Respawn on → immediate; off → respawn by pressing the rebound fire-weapons command while dead; placement offsets `respawn_rad` 1200 / `respawn_el` 100 | `0x470a10`, `0x4709f0` |
| `friendly_fire` | **On, by scoring evidence**: a same-side kill scores `score_suicide` to the killer, so team damage occurs. The damage routine itself was not traced | `0x46ecd0` |
| `disconnect` | **Strike score**: the leaver's plane is destroyed, a carried flag is reset, the leaver is removed from the scoreboard and team totals recomputed; the no-enemies check may then end the match. Loading players are dropped after 600 s; the scores screen waits 5 s after the end message | `0x4995a0` → `0x46e1f0`, `0x46ea40`, `0x49ab50`, `0x60818c`, `0x6036bc` |

End triggers per type (`end_triggers()`): time expiry, score limit and "no
enemies left" (fewer than two sides hold a player with lives > 0) for every
type; Zeppelin vs. Zeppelin additionally ends when the zeppelin is destroyed.
Message ids 7005/7006/7066/7067.

Scoring events (`scoring()`), common to every type: enemy kill `+score_kill`
(2), same-side kill `score_suicide` (−2) **to the killer**, turret kill `+1`
(code default). CTF adds flag return `+8`, enemy flag capture `+10`. ZvZ adds
zeppelin destroyed `+10` to every other team, gas kill `+10`, own gas kill
`−10`, and killed-by-zeppelin, which **sets** (MOV, not ADD — `0x46e1b0`) the
team bonus to `score_zep_kill` (1). This is an original quirk; `ScoreApply::Set`
records it instead of smoothing it into an add. The loader's missing-key
fallbacks (`0x4735b0`) differ from the event code defaults and are recorded
separately (`LOADER_DEFAULTS`: kill 1, suicide −1, zep 100, return 1, enemy
flag 5). A death whose cause is none of these events carries no score.

Environment indices select worlds at launch (`ENVIRONMENT_WORLDS`: 0 c1c,
1 c3, 2 c2, 3 c5, 4 c1, 5 c1b, 6 c4, `0x4136e0` → `0x71c1a8`).

## What stays unknown (and why), exactly as before where the owner said so

`draft` leaves these six fields empty for every type
(`cs_net::rules::measured::RESIDUAL_UNKNOWN`), and each mode keeps them as an
`UnknownRule` with this text (`RESIDUAL_UNKNOWN_RULES`), so a measured mode
still refuses to resolve and never reaches a launch:

1. **`spawn`** — the respawn offsets place a pilot near a point but do not say
   which of a scenario's spawn points is chosen.
2. **`victory`** — no winner/draw computation was found at match end; the
   result is the final scoreboard.
3. **`humans`** — the launch player-count thresholds were not recovered
   ("There are not enough players in the game.", langui 10520).
4. **`human_scaling`** — no scaling mechanism was found, and the absence of a
   mechanism is not a measured "none".
5. **`custom_planes`** — the lobby flag exists (`+0x25`) but its launch/match
   enforcement was not traced.
6. **`component_limit`** — the outlawed-components list (uidata 5044) and its
   enforcement were not traced.

`teams` stays unknown only for a name whose text states no team play (none of
the retail four); the executable measures the lobby types, not which of the
two deathmatch names a catalog identity is.

## The tie/draw policy stays a declared design choice

The original computes no winner at match end (owner note: no winner/draw
computation found; the result is the final scoreboard). Therefore
`cs_sim::multiplayer::result`'s resolution order — all events of a tick applied
before judging, the highest score wins, equal top scores are a `Draw`, events
after the limit unscored, one sealed result — remains **declared engine
design**, not measured original behaviour. The module docs of
`crates/cs_sim/src/multiplayer/result.rs` now say so and cite this file; the
resolver keeps taking a caller-supplied `ScoreTable` so the measured constants
are passed in by the wiring and never become a silent default.

## Tests

Task-test prefix **`accept_f476_`** (task #476 has no feature key; the work
completes the F56-A rule measurement):

- `crates/cs_net/tests/accept_f476_measured_mode_rules.rs` (11 tests): pins
  the drafted values per field, the UI defaults and ranges, the two-team
  restriction, the launch codes, the residual unknowns as an exact partition
  of `RuleField::ALL`, the refusal of a measured mode to resolve, the scoring
  tables including the MOV quirk and the loader defaults, the end triggers and
  timing constants, and that every cited policy names the analyzed image and
  the task note. A value removed or changed fails these tests.
- `crates/cs_content/tests/accept_f476_mode_catalog_rules.rs` (4 tests): pins
  the catalog side — `MeasuredRule` per answered field with unchanged claim
  ids, `observed_tool` class and the image sha256; the residual unknowns with
  their reasons; a name that states no team play keeping `teams` unknown; and
  the cross-crate mirror of labels, citations and image against
  `cs_net::rules::measured` through the dev-dependency.

Updated pinned expectations (the measured values left the old unknown lists,
which is the point of the task): `accept_f56_a_catalog.rs` and
`accept_f14_d_2_multiplayer_rules.rs` now pin the residual set instead of the
full rule list. The F56-A evidence harness
(`evidence_report_f56_a.rs`, its committed `docs/findings/evidence/F56-A.json`)
is untouched: it is the record of that stage's run, and its `catalog.json`
records the unknown count it observes at run time.

## Follow-ups

- F56-B/F56-C wiring: build `cs_sim::multiplayer::result::ScoreTable` from
  `cs_net::rules::measured::scoring()`/`LOADER_DEFAULTS` at the wiring (never a
  default), and drive `RuleDraft` from the lobby's host options via
  `MissionOptions` + `draft`.
- Still blocked on owner/human evidence: the winner/draw rule, launch
  player-count thresholds, custom-plane and outlawed-component enforcement,
  spawn-point selection, human-count scaling (`F56-D` needs `network_real`;
  `human_play`/`human_review` are never agent-available).
- The original Dogfight restriction offers up to 16 teams while the engine's
  `cs_net::lobby::MAX_TEAMS` is 8 (engine design, unchanged here); whoever
  wires team options must decide deliberately, not silently.

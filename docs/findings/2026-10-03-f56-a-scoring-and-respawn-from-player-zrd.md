# F56-A follow-up (task #476): scoring-event constants and respawn offsets decoded from `player.zrd`; the remaining per-mode rules stay unknown

Date: 2026-10-03. Task: #476 "Measure per-mode multiplayer rules (limits,
respawn, lives, friendly fire, draw, disconnect, scoring events)" under stage
`### F56-A` (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`).
Capability: `retail` (the installation at `$CS_GAME_DIR`, one language, 1033).
Parent finding: `docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`, whose
"Not known" items 2 and 4 this file partially resolves.
Result: **blocked, not complete.** The measurable slice (the scoring-event
constants and the two respawn-offset parameters) is recorded below with its
span and digest. Every other per-mode rule the task names is **not present in
any data the installation ships**, so it cannot be measured from `retail`; it
needs an owner-supplied original capture (`human_play` / `network_real`).

## What was searched, and what the search proves

The mode rules were looked for the way the parent finding's residual item 2
asked, in three passes. All three are first-hand on this installation.

1. **Every reader archive's members.** `cs-inspect zbd-audit` routes 62
   containers to the reader family. Decoding **every** member of **every**
   `zrdr.zbd` with the production `cs_content::stunts::decode_zrd` and scanning
   each decoded tree's text keys for the rule vocabulary (`score_kill`,
   `score_return_flag`, `score_suicide`, `score_zep`, `score_enemy_flag`,
   `respawn_rad`, `respawn_el`, `lives`, `time_limit`, `score_limit`,
   `friendly_fire`, `disconnect`, `late_join`, `custom_planes`,
   `component_limit`, `human_scaling`, `mission_type`) matches exactly two
   members in the whole installation: the root archive's `player.zrd` (the
   constants below) and `ia.zrd` (only `mission_type`, which is the F56-A/F49
   scenario selector, not a rule). The `MP<n>` slot archives add nothing: their
   members are `aiv`, `egen`, `location`, `map`, `mis_anim`, `net`,
   `objectives`, `startanims`, `targets`, `weather`, `zeppelins` and the slot's
   own extra props, none of which states a rule.
2. **Raw strings of the 4 MB root `ZBD/zrdr.zbd`** (161,222 printable runs, all
   221 member names). There is no `lives`, `friendly`, `warmup`, `victory`,
   `disconnect`, `time_limit` or `score_limit` text anywhere; the only rule-like
   key names are the seven below.
3. **Outside the reader family.** The other container families are `gamez`
   meshes, `texture`, `sound` and `animation` (no rule data). A case-insensitive
   scan of `crimson.exe` (344,851 B), `crimson.icd` (2,580,578 B),
   `strings.dll`, `dplayx.dll`, `mcp.dll`, `ifc21.dll`, `crimsonff.ifr` and
   `00000409.016/.256` finds no `friendly`, `respawn`, `lives`, `score_limit`,
   `time_limit`, `disconnect`, `late_join`, `custom_plane` or `component_limit`
   text. `strings.dll` carries only the `lives` hints the parent finding already
   recorded (ids `209`/`210`), never a mode rule.

The conclusion is a measured negative: **the installation's data files state
the scoring-event constants and the two respawn offsets, and nothing else the
task lists.** Anything further would be a guess.

## The decoded member and its values (this installation only)

Container: `ZBD/zrdr.zbd`, 3,988,394 bytes, SHA-256
`76b510d821edd2268040d2ccb18c462ec07ad580cdba571b3066228e2cf592dd`. It holds
**two** members named `player.zrd` (indices 22 and 100 of 221); the rule fields
are in the first, a player/vehicle defaults document:

| key | `.zrd` type | value |
| --- | --- | --- |
| `respawn_rad` | float | `1200` |
| `respawn_el` | float | `100` |
| `score_kill` | int | `2` |
| `score_return_flag` | int | `8` |
| `score_suicide` | int | `4294967294` (= `-2` as a two's-complement `i32`) |
| `score_zep` | int | `10` |
| `score_enemy_flag` | int | `10` |

Member span: `player.zrd` at byte offset `938413`, length `3414`, member SHA-256
`a8cc7547bbc1061ef1cff900ca3e3835474045186605962f11c557d37c84d176`. (The
second `player.zrd`, offset `2216404`, length `34711`, is an animation-definitions
document and carries none of these fields; a consumer must not take the first
name match blindly.)

These are engine/global values, not per-mode overrides: no other member holds
any of the seven keys, and `player.zrd` is not in any slot archive. The bridge
to the modes is arithmetic, not an assumption — see the next section.

## The briefing point values are now mapped

The parent finding recorded the printed briefing point values and left them
unmapped. Every printed value is now accounted for by exactly one constant, and
every constant is used:

| mode | printed points | mapping |
| --- | --- | --- |
| Capture the Flag | `10, 8, 2, -2` | `score_enemy_flag` 10, `score_return_flag` 8, `score_kill` 2, `score_suicide` -2 |
| Zeppelin vs. Zeppelin | `10, 2, -2` | `score_zep` 10, `score_kill` 2, `score_suicide` -2 |
| Deathmatch (both) | `2, -2` | `score_kill` 2, `score_suicide` -2 |

The event names are the constants' own keys (`score_kill`, `score_return_flag`,
`score_enemy_flag`, `score_zep`, `score_suicide`), so the *event each value
rewards* is read, not inferred. The two `10`s are disambiguated by event: the
10 that appears in the Zeppelin vs. Zeppelin briefing is `score_zep`; the 10
that appears in the Capture the Flag briefing is `score_enemy_flag`. The
remaining value (the Capture-the-Flag `8`) is the only other constant not
already used, `score_return_flag`. This resolves parent "Not known" item 4 for
the point values; it does **not** supply `cs_sim`'s `ScoreTable` a default, and
the resolver should keep taking a caller-supplied table.

Evidence class: the values themselves are `observed_tool` (decoded bytes with a
member span and digest). The mode→value assignment is arithmetic over the
observed constants and the observed printed values; it carries no new claim.

`respawn_rad`/`respawn_el` are respawn **placement** offsets (radius, elevation)
in the same global document. They are real measured numbers, but they are not
the `cs_net::rules::RuleField::Respawn` policy (immediate / after N ticks /
never), which the installation still does not state.

## What stays unknown (why this task is blocked)

The task's remaining categories are absent from every readable source:

- **Time limit** and **score limit** — no such value or key in any file.
- **Lives** — the game has a "You Are Out of Lives!" string (id `209`) but no
  per-mode life count; the count is not in any data file.
- **Friendly fire** — no value; the briefings only say shooting teammates costs
  points, which is the `score_*` sign convention, not a damage rule.
- **Victory / draw** — no value; `cs_sim::multiplayer::result` remains engine
  design.
- **Disconnect policy**, **late join**, **human-count scaling**,
  **custom planes**, **component limit** — no value or key anywhere.

These are native code or host/lobby state, not shipped data. Measuring them
requires the owner to run the original and capture the mode's host options and
in-match behavior (an original capture supplied through the owner; `network_real`
for the disconnect/late-join behavior). Until then they remain `UnknownRule` on
`ModeEntry` and blocked `RuleField`s, as `AGENTS.md` rule 5 requires.

## What the owner must provide to unblock

1. An **original capture** (screenshot/video, cited by a stable `REF-` id) of
   the original's multiplayer host/options screen for each of the four modes,
   showing any time/score/team/lives/friendly-fire settings it exposes.
2. An **original play capture** (`network_real`) of a match ending, including a
   mid-match disconnect and a late join, so the victory/draw and disconnect
   policies can be observed.
3. If the original exposes no such options (fixed per-mode rules), say so; the
   rules are then native constants and this task should be re-scoped to a
   non-`retail` design contract, not measured.

## Method and sensitivity

The measurement used the production discovery (`cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`) and decoder
(`cs_content::stunts::decode_zrd`); the probes that produced these numbers were
temporary tests and were deleted, so the branch carries no test-only code. The
member offset/length/digest and the container digest above let a reviewer
re-derive the bytes independently. `zbd-audit` (exit 0) and the raw string
scans are reproducible from `$CS_GAME_DIR`. No original executable ran: nothing
here is `verified_original`, and reading the files is not evidence of how the
game behaves.

## Follow-ups filed

- The measured constants should be consumed by F56-B/F56-C (a scoring-event
  table for `cs_sim::multiplayer`, not a guessed default), once the mode rules
  are unblocked. This task leaves `ModeEntry`'s rule unknowns in place rather
  than half-filling them.

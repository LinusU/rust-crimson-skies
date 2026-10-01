# F56-A: the original multiplayer catalog — what was measured and what stays open

Date: 2026-10-02. Task: F56-A "Discover the complete original multiplayer
catalog" (`specs/F56-original-multiplayer-scenarios-and-mode-rules.md`).
Capability: `retail` (the installation at `$CS_GAME_DIR`, one language, 1033).
Evidence report: `docs/findings/evidence/F56-A.json`.

## Files and the observable failure

- `crates/cs_content/src/multiplayer.rs`: `discover_modes` (mode names and
  briefings from the localized string rows), `discover_slots` (every
  `ZBD/<group>/MP<n>/` slot with archive digest and markers), `ModeEntry`,
  `ScenarioSlot`, `ModeCatalog` (keeps gaps visible), `RULE_LABELS`.
- `crates/cs_net/src/rules.rs`: `RuleDraft` / `MatchRules` / `RuleField`:
  every rule a mode must define; an unknown one blocks (`RulesBlocked`), and
  `validate_start` checks human count, custom planes and component limit.
- `crates/cs_sim/src/multiplayer/result.rs`: `MatchResolver`, the one sealed
  `FinalResult` of a match (AC01).
- Tests: `crates/cs_content/tests/accept_f56_a_catalog.rs` (12, two retail),
  `crates/cs_net/tests/accept_f56_a_rules.rs` (5),
  `crates/cs_sim/tests/accept_f56_a_final_result.rs` (11), harness
  `crates/cs_content/tests/evidence_report_f56_a.rs`.
- Wiring edits: `pub mod multiplayer;` plus a doc paragraph in
  `cs_content/src/lib.rs` and `cs_sim/src/lib.rs`; `pub mod rules;` plus a doc
  paragraph in `cs_net/src/lib.rs`; `cs_net` as a **dev**-dependency of
  `cs_content` (and `Cargo.lock`) so a test can check `RULE_LABELS` against
  `RuleField::label`.
- Observable failure without the implementation: two pilots shoot each other
  down on the tick the clock expires; without the resolver there is no defined
  result, with an arrival-order-dependent one possible. With it, every arrival
  order yields the same sealed result
  (`accept_f56_a_simultaneous_kills_on_the_limit_tick_all_count_in_one_result`,
  `..._arrival_order_does_not_change_the_result`).

## Measured in the installation (this installation only)

**Four modes.** The string table (`strings.dll`, RT_STRING, language 1033)
holds one run of mode names, ids `7011..=7014`: `Deathmatch without Teams`,
`Deathmatch with Teams`, `Capture the Flag`, `Zeppelin vs. Zeppelin`. The run is
bounded by `7010` (`score` column heading) and `7015` (`TCP/IP`, the first
network transport). Each name has one briefing block in the family starting at
id `16600`, stride 20 (`CAPTURE THE FLAG` 16600, `ZEPPELIN vs. ZEPPELIN`
16620, `DEATHMATCH` 16640, `TEAM DEATHMATCH` 16660). The walk ends at 16680,
whose title is `INSTANT ACTION` and whose third row is not `POINTS`: that is
the next family (Instant Action, F49), not a fifth multiplayer mode. The
retail test pins the boundary row. Four is therefore a measured count of what
the string table names, not an assumed one; it does not prove that no mode
exists without a string.

**Printed point values** (display text only): Capture the Flag `10, 8, 2, -2`;
Zeppelin vs. Zeppelin `10, 2, -2`; both deathmatches `2, -2`. The briefing
text says crashing and shooting down teammates cost points. Which event each
number rewards is **not** measured; the printed order suggests a pairing and
nothing here claims it.

**Team play** is stated by the two deathmatch names and implied by the other
two briefings ("your team"); the evidence class is recorded per mode
(`observed_tool` for names, `inferred` for the briefing).

**21 scenario slots.** `ZBD/<group>/MP<n>/` exists for: C1 (MP1-3), C1B (MP1,
MP3), C1C (MP1, MP3), C2 (MP1-3), C2B (MP1, MP3), C3, C4, C5 (MP1-3 each).
Each holds a `zrdr.zbd` and a `mis_anim.zbd`. The slot archives' digests are in
the evidence report's artifact.

## Not known (blocks F56-B and the fidelity claims)

1. **Slot → mode binding.** MP2 reads as capture-the-flag in several groups
   (`Flag base` text in C1, C2) and MP3 mentions zeppelin data in most groups,
   but the markers are mixed: C2/MP3 carries both, C3 and C4 mention zeppelin
   data in MP1 and MP2, C5/MP1 and C5/MP2 carry neither. The binding is
   `Resolved::Unknown` for every slot. The retail test pins the observed marker
   table so a change is noticed. Resolving it needs the slot programs decoded
   (the F37/F50 program route) or a reference capture.
2. **Every per-mode rule except team play**: spawn points, respawn, lives,
   time limit, score limit, friendly fire, victory and draw, disconnect, late
   join, human-count scaling, custom-plane and component limits. Each is an
   `UnknownRule` on the mode entry and a blocked `RuleField` in `cs_net`.
   Leads, none verified: ids `209`/`210` (`You Are Out of Lives!`, `You Have %1
   Lives Left!`) show lives exist in the game, not in which mode; `7067`
   (`No Enemies Left`), `7062` (`Waiting for %1`), `7077` (`Rearmed!`), `11056`
   (`Display Scores (Multiplayer Only)`).
3. **Simultaneity, tie and limit-expiry rules.** The resolution order in
   `result.rs` (all events of a tick applied before judging; ties are draws;
   events after the limit are not scored) is **engine design** so every host
   computes one result. The original behavior is unmeasured.
4. **Score values and what earns them** (see the printed values above). The
   resolver takes a caller-supplied `ScoreTable` with no default.
5. **Other languages.** Only language 1033 exists here; another localization
   could carry a different run. `discover_modes` takes the language as input.
6. **Hidden modes.** A mode with no string, or one only reachable from a
   network lobby option, would not appear. A running original session (F56-D,
   `network_real`) is what could show one.
7. **The mode-name run is a bounded read, not a scan.** `MODE_NAME_IDS` is
   the measured `7011..=7014`; the walk does not search for further mode
   names. It cannot silently mis-pair, because the briefing walk *is*
   self-terminating and any extra family block lands in
   `briefings_without_name`, which the retail test pins through
   `is_complete()` and `briefing_blocks`. What it cannot catch is a mode with
   neither a briefing block nor a name row.
8. **`MP<n>` outside `1..=9` and other `MP` spellings are not slots.**
   `parse_slot_path` returns `None` for them, so such a directory would be
   skipped rather than reported as a gap. The installation has only `MP1` to
   `MP3`, so nothing is skipped here; the bound is documented on
   `MAX_SLOT_NUMBER`.

## Review corrections (bunny-alpha-2, 2026-10-02)

Independent review re-read the installation through a temporary probe
(deleted again) and confirmed the measurements above: the string table holds
language 1033 only (1792 rows), `7010` is the `score` column heading, `7015`
is `TCP/IP`, the four briefing blocks sit at `16600`/`16620`/`16640`/`16660`
with `16680` (`INSTANT ACTION`) as the boundary, and no fifth multiplayer mode
name exists anywhere in the table. Three defects were found and fixed:

1. `Briefing.instructions` carried the blocks' blank padding rows (ids
   `16614..16619`, `16633..16639`, `16648..16659`, `16668..16679`) as if they
   were instruction lines. Blank rows are now skipped; the retail test pins
   the real instruction counts (`2, 2, 6, 6`).
2. `SlotCatalog.without_program` reported a lower-cased world group
   (`ZBD/c1b/MP4`) while `ScenarioSlot::world_group` reported the directory as
   spelled (`C1B`), so a gap did not join with a slot. Both now use the
   inventoried spelling; the content id keeps its lower-cased key.
3. The evidence harness's "not set" panic named a `cs_assets` path instead of
   its own `crates/cs_content` path.

Three mutations of the production code (apply only the first event of a tick;
force `team_play` to a value; make `RuleDraft::missing` always empty) each
fail 1 to 6 `accept_f56_a_*` tests, so the suite is sensitive. All 28 task
tests also pass individually with `--exact --include-ignored`, as
`docs/contracts/CLI-EVIDENCE.md` requires.

### Noted for F56-B, not fixed here

`cs_net::rules::RuleField` has no slot for **spawn points** or for the
**victory/draw condition**, both of which the F56 sheet's non-negotiable 1
names per mode. The resolver instead fixes victory to "highest score wins,
equal top scores are a draw" as engine design. Adding a field now would mean
inventing a vocabulary for values the installation does not state, so the gap
is recorded here instead and F56-B must decide how a mode's victory condition
becomes expressible and measured.


## Follow-ups filed

See the task's handover summary: slot-program decoding for the mode binding,
per-mode rule measurement against an original capture, and the point-event
mapping.

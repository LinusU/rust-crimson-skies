# M01-ARCHIVE-DECISION (#1565): M01 "The Lost Treasure" is `ZBD/C3/M01`

Verdict, the measured join behind it, and the census of earlier M01 work that
was performed on the wrong archive.

## Verdict

**M01 "The Lost Treasure" is `ZBD/C3/M01/zrdr.zbd`.** The binding record
`missions/bindings/M01.json` has been re-derived through production code and
now cites `world/c3`, `script/c3-m01-zrdr` and the `ZBD/C3/M01/zrdr.zbd`
source span. `ZBD/C1C/M01/zrdr.zbd` — the archive the directory digit had
bound to M01 since #258 — is campaign position `ch2-m01`, work order **M06**
"The Red Menace".

The directory digit was never the chapter. The storage layout interleaves
world-group variants (`C1`, `C1B`, `C1C`, `C2`, `C2B`, …); the digit is the
group family, not the campaign chapter.

## How the join was re-measured

`SourceContext::read` now derives each mission archive's campaign position
from measured content, not directory names (see
`crates/cs_content/src/campaign_bindings.rs::scan_campaign`):

1. **Sound-group codes.** Every mission `zrdr.zbd` declares one dominant
   `snd_c<N>-<REGION>-m<N>` code inside its member contents (found through
   `discover_container`, the production member enumerator). The chapter digit
   and mission number of that code are the archive's campaign position.
   Codes observed, one dominant code per archive, `MP*` directories carry
   none and are excluded:

   | Archive | Dominant code | Campaign position |
   |---|---|---|
   | `ZBD/C3/M01` … `ZBD/C3/M05` | `snd_c1-HA-m1` … `snd_c1-HA-m5` | ch1-m01 … ch1-m05 |
   | `ZBD/C1C/M01` | `snd_c2-NW-m1` | ch2-m01 |
   | `ZBD/C1/M02`, `ZBD/C1/M04`, `ZBD/C1/M05` | `snd_c2-NW-m2/m4/m5` | ch2-m02/m04/m05 |
   | `ZBD/C1B/M03` | `snd_c2-NW-m3` | ch2-m03 |
   | `ZBD/C2/M02` | `snd_c3-HW-m1` | ch3-m01 |
   | `ZBD/C2/M01` | `snd_c3-HW-m2` | ch3-m02 |
   | `ZBD/C2/M03` | `snd_c3-HW-m3` | ch3-m03 |
   | `ZBD/C2B/M04` | `snd_c3-RM-m4` | ch3-m04 |
   | `ZBD/C2/M05` | `snd_c3-HW-m5` | ch3-m05 |
   | `ZBD/C4/M01` … `ZBD/C4/M05` | `snd_c4-RM-m1` … `m5` | ch4-m01 … ch4-m05 |
   | `ZBD/C5/M01` … `ZBD/C5/M04` | `snd_c5-MN-m1` … `m4` | ch5-m01 … ch5-m04 |

   Note the two within-`C2` swaps (`C2/M01` declares m2, `C2/M02` declares
   m1) and the `C2B` region drift (`snd_c3-RM-m4`): directory digits would
   have silently mis-ordered both.

2. **Localized campaign title/loading tables** (`langui.dll`): the
   region-prefixed long-name blocks read Hawaii ×5, Northwest ×5, Hollywood
   ×5, Rocky Mountains ×5, Manhattan ×4 — the `HA/NW/HW/RM/MN` order of the
   sound codes — and the title block binds each campaign position to its
   title, which is what `SourceContext` joins the layout against.

3. **Briefing ids.** `ZBD/C3/M01` declares `MSG_BRF_HAM1_*` (Hawaii mission
   1); `ZBD/C1C/M01` declares `MSG_BRF_NWM1_*` (Northwest mission 1). The
   archives self-declare their chapters.

4. **Scrapbook ordering** (`SCRAPBOOK.CSV` / `SCRAPBOOK_TOC.SCRIPT` in
   `interp.zbd`): mission numbering runs Hawaii 1–5, Northwest 6–10,
   Hollywood 11–15, Skyhaven 16–20, Manhattan 21–24 — the same chapter order.

5. **Ground-truth content** (#1549's census): `ZBD/C3/M01` carries
   `targets.zrd` with the `susp_bridge`, gun trucks `t_truck01..03`,
   `bridge_truck01` (tank), `u_camp*`, the shipwreck, Betty and
   `medkestrel_1..3` in `aiv.zrd` — the Lost Treasure convoy the owner-facing
   sheet describes. `ZBD/C1C/M01` carries none of these.

Every measurement agrees; the `cs-inspect campaign` report derived through
production prints the corrected 24-position layout end to end.

## What changed in the repository

- `scan_campaign` orders the campaign by the per-archive sound-group census
  (dominant `snd_cN-REG-mN` code in each `zrdr.zbd`'s member contents),
  joined against the title block exactly as before. Program ids are now
  derived from the storage archive path (`ZBD/C2/M01` → `script/c2-m01-zrdr`)
  rather than the declared mission number, fixing the `C2` swap case; catalog
  ids stay positional (`mission/ch3-m02` for work order M12).
- The 11 committed records in the corrected span — `M01`–`M08`, `M10`,
  `M12`, `M13` — were regenerated through production (`regenerate.rs` in the
  campaign test binary) and byte-compare clean.
- Scoped binding knowledge follows the archive the measurements were taken
  on: the three settled checklist entries measured on `zbd/c1c/m01` moved
  from M01's record to **M06**'s; the passenger-identity unknown measured on
  `zbd/c2/m01` moved from M06's record to **M12**'s. Original task citations
  (`M01-B-OBJ`, `M06-B-FU4`, …) are kept inside the moved entries as
  provenance.
- `missions/bindings/playtest-routes.md` rows were updated to the same
  derivation (the `accept_f50_c_*` doc pin enforces it).
- Every test suite that launches a mission by label+title and measured
  `zbd/c1c/m01` now launches `M06` / "The Red Menace" — the work order the
  measured bytes belong to — with suite names kept as provenance.

## M01 tasks and findings that read `ZBD/C1C/M01`

Every finding produced under the "M01" label before this decision measured
`zbd/c1c/m01` — i.e. **M06 "The Red Menace"**, not The Lost Treasure. The
list (grep `C1C/M01|c1c/m01|c1c_m01` over `docs/findings/`):

- `2026-09-29-m01-a-source-binding.md` — the original binding finding (the
  directory-name join this task replaces)
- `2026-10-04-m01-lc-anim-carriers.md`
- `2026-10-04-m01-lc-world-actors.md`
- `2026-10-04-vs-m01-runtime-m01-launch-closure.md`
- `2026-10-05-m01-lc-actor-anim-playback.md`
- `2026-10-05-m01-lc-anim-records.md`
- `2026-10-05-m01-lc-player-config.md`
- `2026-10-05-m01-lc-weather-bind.md`
- `2026-10-06-m01-lc-directive-a-objective-directive-parser.md`
- `2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`
- `2026-10-06-m01-lc-directive-meaning.md`
- `2026-10-06-m01-lc-player-airframe-source.md`
- `2026-10-07-m01-lc-actor-anim-consumers.md`
- `2026-10-07-m01-lc-directive-lowering-adapter.md`
- `2026-10-07-m01-lc-directive-lowering-integration.md`
- `2026-10-08-m01-lc-campaign-airframe-engine-state.md`
- `2026-10-08-m01-lc-placezeps-fields.md`
- `2026-10-08-m01-lc-world-facts.md`
- `2026-10-08-m01-lc-zeppelin-faction.md`
- `2026-10-09-m01-lc-zeppelin-attitude.md`
- `2026-10-09-m01-lc-zeppelin-placement-position.md`
- `2026-10-10-vs-m01-geometry-verdict-on-main.md`
- `2026-10-11-vs-m01-rt-block-lifecycles.md`
- `2026-10-11-vs-m01-rt-evidence-audio-binding.md`
- `2026-10-12-m01-b-binding-the-measured-results.md`
- `2026-10-12-m01-b-obj-1-completion-effects-and-tick-dependency.md`
- `2026-10-12-m01-b-obj-2-objective-lifecycle-count-identity-cues.md`
- `2026-10-12-m01-b-obj-3-stage-conditions-and-outcomes.md`
- `2026-10-12-m01-b-obj-4-mission-language-directive-keys.md`
- `2026-10-12-m01-b-obj-integration-family-verdict.md`
- `2026-10-12-m01-b-wake-world-facts-that-wake-m01-finish-blocks.md`
- `2026-10-12` scripts notes `m01-b-opc-*` under `docs/findings/scripts/`
- `2026-10-13-m01-lc-directives.md`
- `2026-10-14-m01-play-actors-premise-and-census.md`
- `2026-10-14-m01-play-sky-horizon.md`
- `2026-10-14-owner-facts-table.md` (M01 rows)
- `2026-10-15-m01-aiv-layout.md`
- `2026-10-15-m01-play-radio-dialogue.md`
- `2026-10-15-m01-play-triggers-anims-wake-anim-record-join.md`
- `2026-10-15-m01-play-weather-clouds-haze.md`
- `2026-10-16-m01-play-music-state-requests.md`
- `2026-10-16-m01-play-triggers-anims.md`
- `2026-10-17-m01-window-pose-record.md`
- `2026-10-17-f47-d2-objective-replay-and-mementos.md` (M01 references)

Findings that read other mis-categorised archives:

- `2026-10-10-m06-b-fu2-passenger-identity-binding.md` and
  `2026-10-10-m06-b-fu4-mission-scoped-binding-unknowns.md` measured
  `zbd/c2/m01` — under the corrected layout **M12**'s archive, not M06's.
  (The FU4 unknown already moved to M12's record; FU2's prose still says
  M06 — flagged below.)
- Format/inventory findings that enumerate the archive set itself
  (`f14-d`, `f04-d`, `f20-d`, `f39-*`, `f15-e`, `t574`) name `C1C/M01` as a
  *path*, which stays factually correct — the archive exists and is measured;
  only its campaign attribution changed.

Nothing was guessed: the reattribution above is mechanical (the bytes each
finding measured did not move; the campaign position they belong to did).

## Known consequences and follow-ups

- **`missions/M01.md` is protected** (`allowProtectedChanges: false`): its
  prose describes The Lost Treasure while neighboring M06's sheet describes
  The Red Menace — the *sheets* are correct; it is the archive attribution
  underneath that moved. No protected file was touched. Owner decision
  handed off as #1569 `M01-SHEET-ARCHIVE-OWNER-NOTE`.
- **Runtime launch path.** `cs`'s M01 launch still resolves `zbd/c1c/m01`
  through its own path table, so `cs --mission M01` today runs M06's archive
  under M01's label. Re-pointing the runtime at `zbd/c3/m01` is follow-up
  work (done by #1567, see `2026-10-18-m01-runtime-archive-repoint.md`) — filed as #1567 `M01-RUNTIME-ARCHIVE-REPOINT` (M01's real content
  wiring, the bridge convoy, is dependent task #1566
  `M01-BRIDGE-CONVOY-SPAWN`), not part of this decision.
- **Past evidence reports** (`docs/findings/evidence/M01-*.json`, the
  committed `mNN_*` harnesses) embed task-era prose and test lists from
  before this decision; their artifacts re-derive through production code,
  so any reissue will cite corrected archives. The census-wide attribution
  notes and the reissue/annotate decision for those reports are filed as
  #1568 `M01-M06-ARCHIVE-FINDING-PROSE`; stale prose inside them is
  historical record, not re-derivation.
- **Unknowns kept.** M01's real `zbd/c3/m01` record carries the shared
  unknowns and no measured overrides — nothing about its objectives,
  directives or lifecycle is known yet beyond the #1549 content census and
  this join. All M01-B/M01-LC/M01-PLAY measurements belong to M06's archive
  and now read as M06 findings.

## Acceptance

`cargo test -p cs_app --test campaign` retail suites pinning the corrected
join (`accept_m01_a_*`, `accept_m06_a_*`, `accept_m12_a_*`,
`accept_m01_b_*`, `accept_m06_b_fu4_*`, `accept_m01_b_wake_*`,
`accept_f50_c_*` doc pin) plus the regeneration byte-compares all pass
against `$CS_GAME_DIR`. Evidence report:
`docs/findings/evidence/M01-ARCHIVE-DECISION.json` (harness:
`crates/cs_app/tests/campaign/evidence/m01_archive_decision.rs`).

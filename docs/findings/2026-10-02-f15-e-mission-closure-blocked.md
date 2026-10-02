# F15-E: the retail mission dependency closure is not yet rich enough to measure AC04

Date: 2026-10-02. Task #402 `F15-E`, a follow-up to #64 `F15-D`
(`specs/F15-asynchronous-asset-loading-and-private-cache.md`; the spec has no
`F15-E` section — the stage is defined by the task description).
Capability used: `retail` (read-only `$CS_GAME_DIR`). No `gpu`, `audio`,
`human_play` or `human_review` claim is made.

**Outcome: blocked.** The task cannot be finished without missing product
data; nothing was guessed, stubbed or replaced with a synthetic substitute.

## What the stage has to measure

Task #402 requires `accept_f15_e_*` tests to measure spec F15 **AC04** ("warm
and cold loads produce equal content hashes and gameplay state") over a
**mission** dependency closure resolved through `cs-inspect closure` (the
`docs/contracts/IDENTITY-CONTENT.md` dependency-closure algorithm), for
*every content kind the closure reaches that has a working decoder*, while
naming every closure member the stage cannot serve. The gap F15-D left was
that its closure was each world group's own texture archives, not a mission
closure.

## What production code reports on the reference installation today

Both commands were run on the current task branch (`c3bdbbb`, from
`origin/main`), reading the owner's installation at `$CS_GAME_DIR`
(`install_sha256 b4e780ab…`):

`cs-inspect closure --cs-path "$CS_GAME_DIR" --mission mission/ch1-m01`
(`private/evidence/F15-E/closure.json`, not committed):

* schema `cs-inspect-closure/1`; closure hash
  `593a2312c560ccb7aa103f7081d431ac1ade44d4e144b5dfebe821a242a9ba48`;
  `complete: false`, no unresolved references;
* exactly **three** reached nodes, all `ready: false`, every one carrying
  `not_parsed`, `not_normalized` and `missing_runtime_consumer`:
  * `mission/ch1-m01`
  * `script/c1c-m01-zrdr`
  * `install_file/zbd_2f_c1c_2f_m01_2f_zrdr.zbd`

  i.e. `mission → script → install_file`, and all three point at the same
  bytes: the mission's reader archive `ZBD/C1C/M01/zrdr.zbd`.

`cs-inspect catalog --cs-path "$CS_GAME_DIR"`
(`private/evidence/F15-E/catalog.json`, not committed):

* rows 334, `ready 0`, `unavailable 334`, `launchable 53`
  (`24 mission + 8 ia_scenario + 21 multiplayer_scenario`),
  `is_retail_ready: false`;
* the only kinds present are `install_file` (228), `script` (53),
  `mission` (24), `ia_scenario` (8), `multiplayer_scenario` (21). There is
  **no** row of kind mesh/model, material, collision, audio, animation,
  camera, UI or font at all.

The published per-mission binding agrees: `missions/bindings/M01.json` binds
only `GOSDATA/ASSETS/BINARIES/langui.dll` and `ZBD/C1C/M01/zrdr.zbd`, records
`closure_sha256: null`, and lists "required geometry/collision/materials",
"audio/dialogue/video/camera cues" and "closure_sha256 … needs the retail
content catalog (F14-D) and decoded mission programs (F37/F38)" as unknowns.

## Why no reached member has a working decoder

* The reached `script`/`install_file` bytes are a **reader archive**
  (`ZbdFamily::Reader`). `cs_formats::zbd::read_reader_archive` gates the
  family and hands out entry bytes verbatim; every entry's encoding is
  `EncodingEvidence::Undeclared` and the module says in terms "What it
  deliberately does not do: decode anything." There is no canonical
  conversion for this family.
* The `mission` row has no mission-program decoder (F37/F38) and no runtime
  consumer; the baseline sets `runtime_consumers: Vec::new()` and
  `readiness: Unavailable` on every row.
* The **script adapter's conservative dynamic candidate sets** are empty:
  `cs_formats::LoadCommandTable::new()` registers no loading command, so
  `cs_content::loading::resolve_loading_plan` reports every line as
  unclassified/unsupported and produces zero dependencies with which the
  closure could be expanded. F07-D is the stage that measures the commands.

So the mission closure has **zero** members with a working decoder or runtime
consumer. An AC04 measured over it would run over zero load items and would be
vacuous, not a measurement; and the evidence report required by AC5 could not
pass `tools/validate_evidence.py --require-pass`, because the stage would not
have measured the acceptance case.

## What is missing (resolving work already filed)

* **Content collections are unpopulated.** F14-D.2 (`#389`) is in review and
  adds only the multiplayer-rules collection. The collections a mission
  closure needs are unstarted: F14-D.3 worlds (`#486`), F14-D.4
  scene/mesh/material (`#487`), F14-D.5 faction/paint (`#488`), F14-D.6
  airframe/flight configuration (`#489`), F14-D.7 sound/music/dialogue
  (`#490`), F14-D.8 stunt/scrapbook/legacy (`#491`).
* **No measured mission-to-content edges.** The binding of a mission's
  prerequisite closure to those collections is F50-B (`#205`, todo) and
  M01-B (`#259`, todo); `missions/bindings/M01.json` already records it as an
  unknown (`closure_sha256: null`).
* **No script-adapter candidate sets.** F07-D's load-command classification
  is what would let the script adapter expand the closure; until it lands the
  adapter yields no dependencies.

## What would unblock this stage

When F14-D.3..D.8 have populated the content collections **and** mission rows
carry measured static dependency edges to the content they use (or the script
adapter's load-command table is populated and its candidate sets are walked),
`cs-inspect closure --mission <id>` will reach content kinds beyond
`mission/script/install_file`. F15-E can then run the F15 cold/warm/restart
cycle over those reached kinds with the working decoders, naming the ones it
still cannot serve. The dependency must not be guessed to make the stage pass
(`AGENTS.md` rules 4 and 5).

## What was produced

`private/evidence/F15-E/catalog.json` and
`private/evidence/F15-E/closure.json` (both outside Git; paths, ids and
hashes only, no original bytes). This note is committed. No test with the
`accept_f15_e_` prefix was added because there is no non-vacuous production
assertion for the stage to make yet.

## Sources

* Task #402 description (F15-E) and its gap analysis of #64 F15-D.
* `docs/findings/2026-09-30-t64-f15-d-cold-warm-restart.md`.
* `docs/contracts/IDENTITY-CONTENT.md` (dependency closure algorithm).
* `docs/contracts/CLI-EVIDENCE.md` (`--require-pass`, task-test discovery).
* `docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md`,
  `docs/findings/2026-10-02-f14-d-1-reader-archive-directories.md`.
* `missions/bindings/M01.json`.
* Production output of `cs-inspect closure` and `cs-inspect catalog` over
  `$CS_GAME_DIR` on branch `rally/402-…` at `c3bdbbb`.

# F14-D.1: classifying the reader-archive directories F14-D left outside the denominator

Date: 2026-10-02. Task: F14-D.1 "Classify the retail reader-archive directories
F14-D leaves outside the denominator" (follow-up of F14-D, #60). Capability:
**`retail`** (read-only `$CS_GAME_DIR`); no `gpu`, `audio`, `human_play` or
`human_review` capability was used, nothing was run, rendered or played, and
the claim stays `implemented` (at most `checked` after review).

## Files and the observable failure

- `crates/cs_content/src/catalog/reader_dirs.rs` (new): `ReaderDirRole`,
  `ClassifiedReaderDir` and `classify`, the rules below.
- `crates/cs_content/src/catalog/baseline.rs`: `retail_baseline` lists each
  leftover reader archive's own member index (the F06-C producer,
  `cs_assets::zbd::audit_containers` over a mounted session), classifies it,
  gives every launchable scenario directory a `Script` row and an
  `IaScenario`/`MultiplayerScenario` row declared launchable, and reports
  `classified_reader_dirs`. `unrecognized_program_dirs` now holds only what no
  rule classifies.
- `crates/cs_content/src/catalog/mod.rs` (wiring): `pub mod reader_dirs;` and a
  doc paragraph.
- `crates/cs_content/tests/accept_f14_d_baseline.rs`: the retail denominator
  test expects the new count; a new synthetic test (`accept_f14_d_1_…`) builds
  valid version-one reader archives. `crates/cs_content/tests/evidence_report_f14_d.rs`
  follows the new denominator and takes `CS_EVIDENCE_TASK_ID`.

**Observable failure without the change:** the 29 scenario directories are not
in the denominator, so `launchable` is 24 and any "every launchable scenario"
claim reads a denominator that omits them. The retail test counts the `IA<n>` /
`MP<n>` directories with a separate `std::fs` walk (not the classifier) and
fails if the baseline's launchable count is not `24 + 8 + 21`.

## What each directory is (measured from the original data)

The 38 `unrecognized_program_dirs` of F14-D, each read through its own
version-one member index (names only; no record was decoded):

| Directories | Count | Shape observed | Role |
| --- | --- | --- | --- |
| `ZBD/<group>/IA1` | 8 (every world group) | `zrdr.zbd` + `mis_anim.zbd`, the campaign-mission shape; 12 members: the per-mission set (`map`, `aiv`, `objectives`, …) plus **`ia.zrd`** | instant-action scenario |
| `ZBD/<group>/MP1`…`MP3` | 21 (MP1 and MP3 in all 8 groups, MP2 in C1, C2, C3, C4, C5) | same two files; 10–11 members: the per-mission set plus **`net.zrd`** | multiplayer scenario |
| `ZBD/<group>/zrdr.zbd` | 8 | no `mis_anim.zbd` beside it; 25–70 members of shared world data (`templates.zrd`, `cam_anim.zrd`, …) and **none** of the per-mission members | world-group reader, not launchable |
| `ZBD/zrdr.zbd` | 1 | 221 declared members of install-wide definitions, including `instantaction.zrd` and `multiplayer_setup.zrd`, and none of the per-mission members | shared reader, not launchable |

(`player.zrd` is listed twice by the install-wide reader, so the report's
`members` field, which counts distinct lowercase names, is 220 there.)

Corroboration outside the directories (names only, from the top-level reader):
`ia_escape.zrd`, `Loading.zrd` and `escape.zrd` carry the instant-action
briefing message ids (`MSG_BRF_IA…`); `multi1_*`, `multi2_*`,
`mp1_fighter_release.zrd` and `mp2_fighter_release.zrd` define the two
multiplayer zeppelins. F56-A independently measured the same 21 `MP<n>` slots
(`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`).

**Rule (code, `reader_dirs::classify`):** a leaf named `IA<n>` is an
instant-action scenario only if its reader also lists `ia.zrd`; a leaf named
`MP<n>` is a multiplayer scenario only if its reader also lists `net.zrd`; both
need a `mis_anim.zbd` beside the archive, the three per-mission members and a
declared world group. A world-group reader needs `templates.zrd` +
`cam_anim.zrd`, no per-mission member and no `mis_anim.zbd`; the shared reader
needs `instantaction.zrd` + `multiplayer_setup.zrd` with the same absences.
Anything else — including an archive that cannot be listed — stays in
`unrecognized_program_dirs`.

**What the corroboration is worth differs per role, and the rules do not
hide it.** `ia.zrd` occurs in no other reader of the installation, so it
decides instant action on its own. `net.zrd` occurs in the campaign-mission
readers too (every `M<nn>` reader lists it), so it marks a *networked* reader
and rules instant action out; what separates a multiplayer scenario from a
campaign mission is the directory name. The multiplayer role therefore rests
on the `MP<n>` name plus the shape, with F56-A's independent measurement of
the same 21 slots as the second witness — not on a member that no campaign
mission lacks. A leaf named `M<nn>` is never classified as a scenario, and a
group is only a group because the campaign walk declares it.

## Denominator

| | Before | After |
| --- | --- | --- |
| launchable rows (`launchable`) | 24 | **53** = 24 missions + 8 `ia_scenario` + 21 `multiplayer_scenario` |
| rows | 276 | 334 (+29 scripts, +29 scenarios) |
| `unrecognized_program_dirs` | 38 | 0 |
| `classified_reader_dirs` | — | 38 (29 launchable, 9 not) |

Every new row is `Origin::Installation` with a span over the reader archive
(`offset 0`, the file's length, the installation fingerprint) and that file's
SHA-256; its ids are `ia_scenario/<group>-ia1`,
`multiplayer_scenario/<group>-mp<n>` and `script/<group>-<leaf>-zrdr`. All are
`unavailable` (nothing is decoded), so `ready` stays 0 and `is_retail_ready`
stays false. The edge provenance is `observed_tool`, never `verified_original`.

## Recorded unknowns (not guessed)

- **"Launchable" is structural.** A scenario directory is declared launchable
  because it is shaped like (and named and populated like) a directory the game
  starts; whether and how the original launches it was not observed (no
  original run exists, and `retail` is file access only). F49/F56 and an
  original capture can overturn a row.
- **Presets inside one IA1.** `ia.zrd` is not decoded here; it appears to hold
  several selectable mission types (key names such as mission type, zeppelin
  type, player plane and wingman count are visible). How many player-selectable
  presets one IA1 directory yields is **unmeasured**, so the denominator unit
  is the directory and may undercount instant-action configurations.
  Resolving task: F49.
- **Slot-to-mode binding.** Four multiplayer modes are named in the string
  table (F56-A) but which mode a slot is launched under is unknown there too,
  so a launchable multiplayer unit may be a slot × mode pair. Resolving task:
  F56-B / the slot program decoding.
- **World rows.** The world-group and shared readers are classified, not
  turned into `World` rows: worlds are a collection #389 (F14-D.2) owns.
- **The multiplayer role is named, not read.** No member separates a
  multiplayer scenario reader from a campaign-mission reader (both list
  `net.zrd`); the `MP<n>` directory name plus the campaign-mission shape and
  F56-A's independent slot count are the evidence. An installation that
  reused an `MP<n>` name for something else would be miscounted, and nothing
  in the archives alone would catch it. Resolving task: F56-B (the slot
  programs) plus an original capture.
- **A group must be declared.** The classifier only accepts a leaf whose
  parent group the campaign layout declares, so scenario directories in a
  world group with no campaign mission would stay unknown (loudly: the retail
  acceptance test fails on any unrecognized reader directory) instead of being
  counted. Every group of the owner's installation has missions today.
  Resolving task: #389 (F14-D.2), which declares worlds from the world data.

These limits are quoted in the evidence report's `review.method` and gate any
claim that the denominator is "every launch configuration of the original
game".

## Tests (`accept_f14_d_` prefix; `accept_f14_d_1_` for this task)

- `cs_content::catalog::reader_dirs::tests::accept_f14_d_1_scenario_roles_need_a_name_and_a_corroborating_member`
  and `…accept_f14_d_1_shared_readers_are_classified_and_not_launchable`: the
  rules, including the refusals (name without member, member without name,
  missing `mis_anim.zbd`, unknown group, and — added in review — an `M<nn>`
  directory whose reader is campaign-mission shaped, which never becomes a
  multiplayer scenario).
- `accept_f14_d_1_reader_directories_are_classified_from_their_member_index`
  (synthetic, valid archives): roles, rows, edges, denominator 3, the
  uncorroborated `MP2` stays unrecognized, byte-stable report.
- `accept_f14_d_baseline_inventory_covers_every_inventoried_file_and_declared_mission`
  (synthetic, unlistable archives): an archive whose member index cannot be
  read classifies nothing and stays named, which is the negative case for
  "classify from the member index".
- `accept_f14_d_retail_baseline_inventory_is_complete_and_never_synthetic`
  (retail): `24 + 8 + 21`, independent directory walk (now also counting the
  world-group directories, so `29 + 8 + 1` is derived rather than typed), 38
  classified, 0 unrecognized, `3 × 53` reachable rows, no synthetic row.

## Review (bunny-2, 2026-10-02, independent context from the implementer)

Checked against the spec, `AGENTS.md` and the owner's installation read
through production code only (`cs-inspect zbd-audit` over `$CS_GAME_DIR`).

Verified, not changed:

- every rule matches the data: the 8 `IA1` readers list `ia.zrd` and no
  `net.zrd`; the 21 `MP<n>` readers list `net.zrd`; the 8 world-group readers
  list `templates.zrd` + `cam_anim.zrd` and none of `map`/`aiv`/`objectives`
  with no `mis_anim.zbd` beside them; `ZBD/zrdr.zbd` lists
  `instantaction.zrd` + `multiplayer_setup.zrd` and none of the per-mission
  members — so 38 of 38 classify and none is left over;
- the reported counts (`launchable` 53, rows 334, 228 install files, 24/8/21
  rows, 38 classified, 0 unrecognized, `3 × 53` reachable) reproduce from
  `cs-inspect catalog --cs-path "$CS_GAME_DIR"` on this branch;
- the classification adds no measurable cost: the retail acceptance run costs
  the same with the classifier neutered (50 s either way, the time is the
  804 MB discovery hash), because a reader audit reads the member index and
  not member payloads;
- no other consumer of `Baseline::roots`, `launchable_count` or
  `unrecognized_program_dirs` exists, and no protected path is touched.

Fixed in review:

1. **The multiplayer role was documented as if a member proved it.** Every
   campaign-mission reader lists `net.zrd` too, so `net.zrd` marks a networked
   reader and rules instant action out; it does not distinguish `MP<n>` from
   `M<nn>`. The module doc, `classify`'s doc, the table above and the evidence
   limitation now say so, and a negative unit test pins that an `M<nn>`
   directory is never classified as a scenario.
2. **A stale claim in the machine-readable evidence.** The harness limitation
   still said "only three collections are populated"; the report now has five
   (`install_file`, `mission`, `script`, `ia_scenario`,
   `multiplayer_scenario`). Its display-name and readiness limitations were
   widened from missions to every launchable row.
3. **`ClassifiedReaderDir::members` claimed the archive's declared count**; it
   counts distinct lowercase names (the install-wide reader declares 221 and
   lists `player.zrd` twice, so 220).
4. **The retail test typed the world-group count as `8`.** It is now derived
   from the same independent directory walk, and the world-group and shared
   roles are counted separately.
5. **The older synthetic tree's comment** said its `IA1` was unclassified
   because no campaign mission claims it; the real reason is that its fixture
   bytes hold no member index. The comment and an explicit
   `classified_reader_dirs.is_empty()` assertion now say that.
6. The evidence harness's module doc documented only the `F14-D`
   invocation; the `F14-D.1` one (`CS_EVIDENCE_DIR`,
   `CS_EVIDENCE_TASK_ID`, `CS_EVIDENCE_REVIEW`) is documented too, and the
   evidence report was regenerated on the reviewed commit with the reviewer's
   identity.

Reviewer mutation probe: `classify` forced to `None` fails four selected
tests (both `reader_dirs` unit tests, the synthetic classification test and
the retail one). Reverted and byte-compared clean.

Reviewer: bunny-2 (Space Bunny Free), a different agent identity with a fresh
context from the implementer (claude-2/claude-1, Claude Sonnet 5.5). No agent
review awards more than `checked`; nothing here is `verified_original`.

## Sources

`specs/F14-…` (non-negotiable behavior 4), `docs/contracts/IDENTITY-CONTENT.md`,
`docs/findings/2026-09-29-f14-d-retail-baseline-inventory.md`,
`docs/findings/2026-10-02-f56-a-multiplayer-catalog.md`, the F06-D reader
member listings (`cs-inspect zbd-audit`), and the owner's installation read
through `cs_assets` (no original bytes, names beyond member filenames, or
binaries are committed).

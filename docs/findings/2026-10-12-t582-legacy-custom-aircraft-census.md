# T582: the legacy custom-aircraft census — a measured negative, with every candidate recorded

Date: 2026-10-12. Task: Rally #582 "Measure the legacy custom-aircraft
layout before a custom-plane catalog collection". Feature sheets:
`specs/F64-legacy-custom-aircraft-and-optional-save-import.md` and
`specs/F14-canonical-content-catalog-and-dependency-closure.md` (stage
`### F14-D`). Shared contracts: `docs/contracts/IDENTITY-CONTENT.md`,
`docs/contracts/CLI-EVIDENCE.md`. Required capability: **`retail`**, used
read-only. `gpu` and `audio` were available and **not used**: nothing is
rendered or played, no original run happened, and nothing here claims
`verified_original`.

## The question, and what was already measured

F14-D.8 (#491) left `ContentKind::CustomPlane` with **no row and no
collection record** because the layout of a legacy custom aircraft was
unmeasured in either direction, and assigned the measurement to this task.
F64-B (#255, landed 2026-10-07) had since measured two halves of the answer:

* the installation's 228 inventoried **file spellings** match none of the
  storage shapes the engine image's own path templates name (`SavedGames`,
  `Planes\`, `.sav`, `Status.dat`, `Mission.`, `Persist.`) — zero matches;
* `ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT` fills a four-slot grid of
  stored planes, so `LEGACY_LAYOUT_INVENTORY[CustomAircraft]` carries
  `referenced_by: &["ASSETS/SCRIPTS/PLANECONSTRUCTION.SCRIPT"]`, its import
  requirement is on, and its byte layout stays `Unknown`.

What neither sweep saw is what ships **inside a container**: a stored plane
could in principle have been packed as a member of `crimson.rof` or of a
`ZBD` archive, and no file-name sweep can see a member. This task closes
that gap with a member-level census through production readers, and records
the result as a **measured negative**: the 2000 PC installation ships **no**
legacy custom aircraft.

## What the census measured

`$CS_GAME_DIR`, read-only, through production code only
(`cs_assets::install::discover`, `cs_formats::rof::read_tree` +
`read_member`, `cs_formats::zbd::dispatch` +
`read_version_one_index` + `list_members`), twice: once by the retail
acceptance test and independently again by the evidence harness
(`census.json`, hashed by `docs/findings/evidence/T582.json`). Install
digest `b4e780ab84cf31d8…`, content digest `a0223506e512b50c…` — the same
fingerprints F12-D and F64-B recorded for this corpus.

| quantity | value |
| --- | --- |
| inventoried files | **228**, every one classified, **0** unclassified |
| ROF containers | **2** (`GOSDATA/ASSETS/crimson.rof`, `GOSDATA/ASSETS/crimptch.rof`) |
| ROF members | **847** (846 + 1), **847/847 decoded** by the production member reader |
| ZBD files by dispatched family | animation **61**, gamez **9**, interp **1**, reader **62**, sound **2**, texture **49** (= 184) |
| trailer-indexed containers listed | **64** (62 reader + 2 sound) |
| indexed members | **6 334**, all readable, 0 failures |
| non-container files | **42** (platform support, optional media, loose TGA/IFR — counted, never typed by extension) |
| storage-shape hits | **6** substring candidates, **0** template-faithful |

"Classified" means what a production reader parsed the file as: a ROF tree,
a dispatched ZBD family, or a file outside the container roots that no
container reader claims. A file's extension is never used to type it.

## Every candidate, with its byte evidence

The storage-shape sweep (the six shapes F64-B measured in the engine image)
was run over every file spelling **and every container member name**. Six
hits came back, all inside `GOSDATA/ASSETS/crimson.rof`, all matching the
`Mission.` shape as a bare substring, none of them a spelling the engine's
own templates could open (`%s\Mission.%1d%02d` spells a dot **followed by
digits**, and none of these names has that; none sits under a `SavedGames`
path and none ends in `.sav`). Each is recorded here with its stored extent
inside the container (offset, length in bytes; the container is identified
by the install digest above):

| candidate member | shape | offset | stored length |
| --- | --- | --- | --- |
| `ASSETS/GRAPHICS/FC_B_FLYMISSION.PNG` | `Mission.` | 2 727 595 | 25 930 |
| `ASSETS/GRAPHICS/IA_B_FLYNEWMISSION.PNG` | `Mission.` | 3 721 576 | 45 958 |
| `ASSETS/GRAPHICS/IA_B_WEAPONLOADOUTFLYMISSION.PNG` | `Mission.` | 3 862 794 | 15 829 |
| `ASSETS/GRAPHICS/MP_LOBBY_MISSION.PNG` | `Mission.` | 7 923 155 | 23 376 |
| `ASSETS/GRAPHICS/PC_B_NEXTMISSION.PNG` | `Mission.` | 11 108 234 | 90 586 |
| `ASSETS/SCRIPTS/MULTIPLAYERLOBBY_MISSION.SCRIPT` | `Mission.` | 57 637 562 | 4 575 |

They are UI images and one lobby script whose names contain `FLYMISSION.`,
`NEXTMISSION.` and friends; the substring `mission.` inside a longer word is
not the `%s\Mission.%1d%02d` save file. They are **recorded, not dropped**
— the acceptance test pins the exact six names and requires every candidate
to carry its stored extent, so a corpus change fails loudly instead of
quietly moving the verdict. The faithfulness tier follows the measured
templates byte for byte (`Planes\`/`SavedGames` as a path component, `.sav`
as the file ending, `Status.dat` as the whole file name, `Mission.`/
`Persist.` only before digits); it is a statement about the engine's own
path formats, not about any extension.

## The verdict, and what it changes

**No legacy custom aircraft ships, hidden or not.** Combined with F64-B's
file-level sweep and the engine image's `Planes\%s` / `wb+` write path (the
originals are runtime-created), the measured answer to this task's question
is a negative, and criterion 3 applies:

* **No `ContentKind::CustomPlane` row and no collection record** are added
  to `crates/cs_content/src/catalog/baseline.rs` — the F14-D.8 refusal now
  rests on measurement in both directions: no shipped file by spelling
  (F64-B) and no shipped container member by census (this task). The
  fabrication guard test proves the emptiness is not vacuous: a synthetic
  tree that really contains `Planes/TESTPLANE.DAT` and
  `GOSDATA/SAVEDGAMES/SLOT1.SAV` still yields zero custom-plane rows, zero
  collection record and no `custom_plane` report key, while the stray files
  are asserted present as their own `install_file` rows.
* **F64-A's inventory is already in its measured state** and needed no
  further change: `LEGACY_LAYOUT_INVENTORY[CustomAircraft]` records the one
  measured referencing member, keeps `evidence: Unknown` (no stored-plane
  file exists to measure a layout from) and keeps its open questions. The
  inventory guard test pins exactly that, so neither the measured reference
  nor the unknown layout can drift without a failing test. (The F64-A
  findings note stays as that stage's historical record; this note and
  F64-B's supersede its open question about whether a reference exists.)
* **`CustomPlane` stays non-launchable**, so the coverage denominator (53
  declared roots) cannot move from here; the inventory guard test pins
  `is_launchable() == false`.

Everything the bytes do not define stays unknown: the byte layout, version
field and id encoding of a stored plane (criterion 4) remain
`UnsupportedReason::Unknown`-grade open questions on the inventory row —
measuring them needs a runtime-created `Planes\` file from an original run
or an owner capture, which no agent can synthesize.

## What this task adds

- `crates/cs_content/tests/t582_support/mod.rs` (new): the census through
  production readers — `Census`, `RofCensus`, `ShapeCandidate`, `census`,
  `shape_candidate` — shared by the acceptance test and the evidence
  harness so the two can never disagree.
- `crates/cs_content/tests/accept_t582_legacy_custom_aircraft_census.rs`
  (new): three `accept_t582_*` tests (two synthetic, one
  `#[ignore = "requires CS_GAME_DIR"]` retail).
- `crates/cs_content/tests/evidence_report_t582.rs` (new): the evidence
  harness, deliberately **not** named with the acceptance prefix.
- `docs/findings/evidence/T582.json` (new): the validated acceptance
  report.
- This file.

No production crate is changed: the census reads the same production
readers the baseline inventory already uses, and the measured state it
pins is the state `main` ships.

## Test inventory

| `accept_t582_*` test | Covers | Fails when |
| --- | --- | --- |
| `a_file_named_like_a_stored_plane_never_becomes_a_custom_plane_row` (cs_content/tests/accept_t582_legacy_custom_aircraft_census.rs) | production `retail_baseline` over a synthetic tree containing `Planes/TESTPLANE.DAT` and `GOSDATA/SAVEDGAMES/SLOT1.SAV`: both files present as their own `install_file` rows, zero `CustomPlane` rows, no collection record, no `custom_plane` key in the report | a custom plane is fabricated from a file name or directory, a collection record appears without a located source, or the stray files stop being inventoried (the empty result can no longer pass vacuously) |
| `the_inventory_row_keeps_its_measured_reference_and_unknown_layout` | the production inventory row: requirement on, `referenced_by` the one measured member, `evidence == Unknown`, unknowns listed, `CustomPlane` not launchable | the measured reference is dropped, the layout evidence is upgraded without a file, the open questions are dropped, or the coverage denominator becomes movable |
| `retail_every_container_member_is_accounted_for_and_no_stored_plane_ships` (`#[ignore = "requires CS_GAME_DIR"]`) | the real installation: 228 files, 0 unclassified, both ROF containers with 847/847 members decoded, the six dispatched families, 64 indexed containers with 6 334 readable members, 0 template-faithful shape hits, the exact six recorded candidates with their stored extents, and `PLANECONSTRUCTION.SCRIPT` really shipping inside `crimson.rof` | the corpus changes, a container stops parsing or listing, a member stops decoding, a stored-plane spelling appears, a candidate loses its byte evidence, or the construction-screen member disappears |

## Sensitivity checks run

Each mutation was applied, observed to fail the named test, and reverted:

| # | Mutation | Killed by |
| --- | --- | --- |
| 1 | the census stops classifying `.rof` files (they fall through as non-container) | the retail test, on `rof_containers.len() == 2` (and the family arithmetic) |
| 2 | the template-faithful tier always fires | the retail test, on the faithful-hits assertion |
| 3 | the census never lists the trailer-indexed containers | the retail test, on `zbd_indexed_members == 6334` |
| 4 | `LEGACY_LAYOUT_INVENTORY[CustomAircraft]`'s `referenced_by` reverts to `&[]` (the pre-measurement state) | the inventory guard test, on `referenced_by()` |
| 5 | `ContentKind::CustomPlane` becomes launchable | the inventory guard test, on `is_launchable()` |
| 6 | the fabrication fixture stops writing the stray files | the fabrication guard, on the `install_file` containment check |

## Retail run facts

`cargo test --workspace --locked -- accept_t582_ --include-ignored`, exit
0: 3 tests discovered, 3 executed, 3 passed, 0 failed (2 synthetic, 1
retail; the retail run over the installation takes ~20 s). Evidence artifact
`census.json` + `cargo-test.log`, hashed by
`docs/findings/evidence/T582.json`, validated with
`tools/validate_evidence.py --require-pass`. **No string text from the
installation is reproduced beyond the spellings, counts, families, extents
and digests listed above.**

## Reviewer regeneration (2026-10-12, review of the same task)

The reviewing agent was the same agent name (`bunny-alpha-1`, same model)
in a **fresh session that had never seen the implementation work**; Rally
did not hand the review to a different instance, so this is recorded as
context-fresh but not instance-independent, and nothing here is above
`checked`.

The committed evidence report originally named `candidate_tree
e4b700c7…`, a tree reachable from none of the branch's commits (it had
been generated before the final rebase). The reviewer regenerated the
whole report on the reviewed tree and replaced
`docs/findings/evidence/T582.json` with it — three times, so that the
committed report always names the tree its acceptance run actually
tested: first on the tree reviewed at that moment, then again after the
branch was rebased onto the then-current `main` (the M01-B-OBJ-4
commits, no file overlap, no Cargo changes), and finally on the rebased
head itself, so the committed report names `candidate_tree
0b3d45c4…`, the tree of the commit it sits on top of. All
regenerations reproduced every substantive field: same install/content
digests, same 228 files, same per-family counts, same 847/847 ROF
members, same 6 334 indexed members, the same six candidates with the
same extents, and the same three passing assertions
(`private/evidence/T582-review*/`, validated with
`tools/validate_evidence.py --require-pass`). The reviewer also reran
the checks, each `accept_t582_` test alone with `--exact`, and one
sensitivity mutation (dropping the fabrication fixture's stray files
kills the fabrication guard, observed and reverted).

## Unknowns and limitations (all recorded, none guessed)

- **The byte layout of a stored plane is still unmeasured — because none
  ships.** This census proves the negative it was built for; it cannot
  produce the layout F64-B named as needing an original run.
  **Affected content:** every F64 legacy-custom-aircraft import row.
  **Resolving task:** an owner capture of a runtime-created `Planes\*`
  file (or an original run); `human_play`/`human_review` are never
  available to an agent.
- **Whether a stored loadout lives inside the plane file or separately is
  still unmeasured.** **Affected content:** the `CustomLoadout` inventory
  row. **Resolving task:** the same capture.
- **The 42 non-container files are counted, not typed.** The census says
  no container reader claims them and no spelling matches a storage shape;
  it makes no claim about their interiors. **Affected content:** none
  reachable — the engine's own templates name where stored planes live,
  and no such file ships.
- **Interp, gamez, animation and texture ZBD containers are
  family-dispatched but not member-enumerated** (no production member
  index exists for those families at this stage). The custom-plane
  question does not hinge on them: a stored plane lives in a `Planes\`
  file per the measured engine templates, and the census covers every
  family that *does* expose members plus every file spelling.
- **Evidence class.** The census is `observed_tool`-grade: production
  readers parsed the bytes. `retail` is file access, not proof that the
  original executable ran; the runtime-created `Planes\` directory was
  never seen by any agent. No original run happened and nothing claims
  `verified_original`.
- **Fixture scope.** The two unignored tests are synthetic and newly
  authored; only the `#[ignore]`d retail test reads original data. Nothing
  derived from it beyond ids, counts, extents and digests is committed.

## Sources used

- `specs/F64-legacy-custom-aircraft-and-optional-save-import.md`
  ("Custom-aircraft import is required when an original content path
  references it") and `specs/F14-canonical-content-catalog-and-dependency-closure.md`
  (stage `### F14-D`, non-negotiable behavior 4).
- `docs/contracts/IDENTITY-CONTENT.md`, `docs/contracts/CLI-EVIDENCE.md`,
  `schemas/evidence.schema.json`.
- `docs/findings/2026-10-01-f64-a-legacy-import-inventory-and-contracts.md`
  (the inventory of what must be known; its open custom-aircraft questions
  were answered by F64-B and this census).
- `docs/findings/2026-10-07-f64-b-verified-read-only-import-subset.md` and
  `docs/findings/evidence/F64-B.json` (the file-spelling sweep, the engine
  image's storage templates, the measured construction-screen reference).
- `docs/findings/2026-10-03-f14-d-8-stunt-scrapbook-collections.md` (the
  no-row-no-record precedent and the refusal this task resolves).
- `docs/findings/2026-10-02-f12-d-installation-wide-configuration-account.md`
  (the 846-member `crimson.rof` account; the census independently measures
  846 + 1 members and decodes every one).
- `crates/cs_formats::rof` (`read_tree`, `read_member`), `crates/cs_formats::zbd`
  (`dispatch`, `read_version_one_index`, `list_members`, `indexed_by_trailer`),
  `crates/cs_assets::install` (`discover`, `fingerprint`,
  `content_fingerprint`) and `crates::cs_content::catalog::baseline`
  (`retail_baseline`, `install_file_key`).
- The owner's installation, read-only, through the two production readings
  recorded above.

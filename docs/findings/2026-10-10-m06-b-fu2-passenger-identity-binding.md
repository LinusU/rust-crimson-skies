# M06-B-FU2: where M06's passenger identity would live, and why nothing binds it

Date: 2026-10-10. Task: M06-B-FU2 (Rally #818) "Locate M06's passenger-identity
binding: `Passenger_hangar` is referenced by nothing", the follow-up of M06-B
(#274, `missions/M06.md`, work order `M06-B`). Owner paths used:
`crates/cs_app/tests/campaign/` and `docs/findings/`; `missions/bindings/`
was read but left byte-identical — see
[The binding record stays production-derived](#the-binding-record-stays-production-derived).
Capabilities used: `retail` (`$CS_GAME_DIR`, read-only, never written) and the
owner-supplied decrypted image `$CS_ENGINE_IMAGE` (read-only, never committed).
Implementer: **bunny-alpha-1/bunny-alpha-1** (Rally #818 claim of 2026-10-10).

## Verdict

**The shipped data binds no passenger or extraction entity to M06.** The
answer to the task's question is (b), the negative one, and it is now measured
over the surfaces the task named rather than over M06's reader archive alone:

* M06's own archive spells exactly one passenger-named string, and it is a
  **teleport destination**: `Passenger_hangar` is one of four entries of
  `location.zrd`, a member that is byte-identical to the sibling mission's
  copy and whose consumer the original executable spells as a Teleport
  feature;
* the installation's other passenger vocabulary is **shared, cosmetic or
  instant-action**, and none of it is reachable from M06's data: the library
  member `passengers.zrd` declares seventeen `ON_CALL` crew animations that
  drive the world node `apassengers` (a mission fires the crew it wants — M01
  fires `call_add_jack`; M06 fires none), the chapter world carries one
  `apassengers` and one `passall` node that nothing in M06 names, and the
  instant-action record `ia.zrd` names `passenger_zeppelin` as one of its
  three zeppelin classes;
* the one message id carrying the word, `MSG_OBJ_PASSENGERHANGER`, is carried
  by **one** archive of the retail control census — chapter one's instant
  action — and by no campaign mission.

So the sheet's *passenger identity* priority has no actor, program, world node
or message to predicate in this installation, and this stage assigns it none
(AGENTS.md rule 4). What remains is a **reference run of the original**
(M06-C, blocked on the owner) — or the owner's ruling that the priority's
"airborne extraction" cue describes a mission that the shipped data does not
give a passenger mechanic. Nothing here reads a mechanic out of the cue; it is
a research label, and `missions/M06.md` says so itself.

The verdict does **not** (yet) sit in `missions/bindings/M06.json`, and that
is a measured constraint rather than an oversight — see
[The binding record stays production-derived](#the-binding-record-stays-production-derived).
It is filed as **M06-B-FU4** (#1184), and until that lands the limitation
lives in this note and in the tests below, which is why the note names it in
the same words the record will.

## What changed

No production code. Four `accept_m06_b_fu2_*` tests in
`crates/cs_app/tests/campaign/m06_b_fu2.rs` pin the measurements through
production code (three retail/image members and one synthetic member that runs
in CI), and `crates/cs_app/tests/campaign/main.rs` gains the module
declaration and one doc paragraph (wiring only). `missions/bindings/M06.json`
is **unchanged**, for the reason in
[The binding record stays production-derived](#the-binding-record-stays-production-derived);
the follow-up that can change it is M06-B-FU4 (#1184). No `Cargo.toml` or
`Cargo.lock` change.

## What was read from the installation

Measured on this installation, re-derived by the acceptance tests on each run:

| Fact | Value |
| --- | --- |
| `install_sha256` | `b4e780ab…c631978` (M06-A/M06-B's fingerprint) |
| Decrypted image | `ORIGINAL_IMAGE_SHA256` = `43540fc9…c37d75`, 2 580 480 bytes, accepted by `load_engine_image` |
| M06's reader archive | `ZBD/C2/M01/zrdr.zbd`, 100 905 bytes, SHA-256 `d6e9315d…1fd9f63`, 15 members, every one decoding |
| `location.zrd` | offset 14 989, length 378, own SHA-256 `ece361bc…0c8b724` |
| The sibling's copy | `ZBD/C2/M02/zrdr.zbd`'s `location.zrd` has the **same digest and the same bytes** |
| The four locations | `Airport_terminal` `[-5249, 199, -4747]` `[-11, 22, 0]`; `Passenger_hangar` `[-5177, 325, -6842]` `[-15, 9, 0]`; `Crops` `[-3295, 390, -6837]` `[-12.8, 151, 0]`; `Coast` `[-1968, 128, -2465]` `[9, -23, 0]` — a name, a position and a heading, and nothing else |
| Chapter world | `ZBD/C2/gamez.zbd`, 4 908 288 bytes, SHA-256 `2b2cf09b…4c924b6`; node array of 4 956 slots at offset 3 111 828; exactly one `apassengers` (slot 503), one `passall` (504) |
| The mission's own world nodes | `sprucegoose`, `propane`, `kkgate`, `tugandbarge01..04` and `g_engine1..8` each resolve **exactly once** in that array — the mission addresses what it uses; the sibling missions' `sghangar` resolves once too and M06 spells it nowhere |
| Library archive | `ZBD/zrdr.zbd`, 3 988 394 bytes, SHA-256 `76b510d8…cf592dd`; member `passengers.zrd` at offset 2 128 952, length 19 418, own SHA-256 `2dedd152…8ab092d` |
| What that member declares | 17 `ANIMATION_DEFINITION` records, every one driving node `apassengers`, every one `ON_CALL`: `add_waldo`, `add_spks`, `add_jack`, `call_add_jack`, `add_bjon`, `call_add_bjon`, `add_pick`, `call_add_pick`, `add_fas`, `call_add_fas`, `add_ilsa`, `call_add_ilsa`, `add_boothe`, `call_add_boothe`, `add_swan`, `call_add_swan`, `rem_pas` |
| M06's startup member | `startanims.zrd` fires `player_setup`, `kktorch_burning1`, `kktorch_burning2`, `place_the_goose` at `NEW_GAME_START` and `player_setup` again at `LOAD_GAME_START` — **no crew animation** |
| M06's message vocabulary | 15 ids: the six crew-name ids, `MSG_OBJ_DEFEND`, `MSG_OBJ_DESTROY`, the four `MSG_TRGT_*` targets and its three objective texts `MSG_BRF_HWM2_OBJ1..3` — none passenger-named |
| `MSG_OBJ_PASSENGERHANGER` | carried by exactly one census archive: `ZBD/C1/IA1/zrdr.zbd` |
| The instant-action record | `ZBD/C2/IA1/zrdr.zbd`'s `ia.zrd` names `cargo_zeppelin`, `passenger_zeppelin` and `military_zeppelin` — the installation's one **gameplay** sense of "passenger", and it is instant action |

Installation-wide ASCII and UTF-16LE scans were run first (read-only) so the
production walks below were aimed rather than blind; the production tests
re-derive what they assert.

## The binding record stays production-derived

The task asks for the verdict to appear in `missions/bindings/M06.json`'s
`unknowns` *and* in `docs/findings/`. This stage delivers the second and
**cannot** deliver the first from its owner paths, for two measured reasons:

* `accept_m06_a_the_committed_record_is_what_the_installation_derives`
  asserts `missions/bindings/M06.json` **byte-for-byte equals**
  `SourceBinding::to_json()`, so a hand-written entry there fails M06-A's own
  acceptance test rather than recording anything;
* that list is `SOURCE_BINDING_UNKNOWNS` in
  `crates/cs_content/src/campaign_bindings.rs`, one **global** table shared by
  every mission's binding. An M06-specific line in it would assert that every
  mission leaves a passenger identity unbound — a claim this stage measured
  for **M06 only**. Its neighbours already show why breadth would be a guess:
  M07's archive carries pickup members of its own (`pickford_pickup.zrd`,
  `car_truck_pickford.zrd`, `pickups.zrd`, measured in
  `docs/findings/2026-10-09-m07-b-compatibility-gaps.md`), whose interaction
  is unmeasured rather than absent, so "no mission binds a passenger-like
  entity" is not something this task can say about anyone but M06.

`crates/cs_content` is also outside this task's owner paths
(`missions/bindings/`, `crates/cs_app/tests/campaign/`, `docs/findings/`), so
the change is filed rather than made: **M06-B-FU4 (#1184)** asks for a
mission-scoped unknown hook, the regenerated record and a test that pins both
directions (M06 names the limitation; a mission that binds one does not).
Nothing in this stage weakened the equality test to get around it — it is
unchanged and still passes.

Until #1184 lands, the limitation is carried by this note, by the tests below
and by the *existing* unknown the record already carries — *interaction
authorizations: not bound from original data at this stage* — which this
stage's verdict deepens rather than replaces: F36's interaction schema still
has no original importer, so even a passenger pickup that existed could not be
read into the record today.

## `location.zrd` is teleport data, not an actor list

Three independent measurements say the same thing:

1. **Shape.** Each entry is a name followed by exactly two float triples — a
   world position and a heading. There is no kind, no plane, no net, no spawn
   and no node path, so the record cannot describe an actor under any
   reading.
2. **Sharing.** M06's copy is byte-identical to `ZBD/C2/M02/zrdr.zbd`'s copy
   (same SHA-256 `ece361bc…`), and the same four names appear in the
   `location.zrd` of every chapter-one and chapter-two archive measured
   (`ZBD/C1/{M02,IA1}`, `ZBD/C1C/M01`, `ZBD/C2/{M01,M02}`, `ZBD/C2B/IA1`),
   with `Race Start` added by `ZBD/C2/{M03,M05,IA1,MP1}`. Other world groups
   carry their own lists entirely: `ZBD/C1B/IA1` and `ZBD/C3/IA1` spell
   `Lighthouse` and `Tanker`, `ZBD/C5/IA1` spells three `*_cblock*` names, and
   `ZBD/C4/IA1`'s member is 16 bytes and spells nothing. It is the **world
   group's** place list, copied into each mission that wants it, not
   M06-authored content.
3. **The consumer.** The decrypted image carries, in one `.data` run, the
   member's name five times beside the feature's `Teleport` label, a
   `Current Location` label, a `location.bak` backup name, the two diagnostics
   a writer emits, and the developer's own annotation naming the file's
   purpose (asserted verbatim at file offset `0x228DE9` by
   `accept_m06_b_fu2_the_image_spells_location_zrd_as_teleport_data`, which
   does not quote it anywhere else). The feature also **writes the member
   back**, which no actor binding does.

This is deliberately a **string-level** result: no instruction of the loader is
traced here, so what the teleport feature does with an entry — whether it
moves the player, the camera or nothing until a key is pressed — is unmeasured
and stays unknown. What the three measurements settle is the task's question:
a `Passenger_hangar` entry is a **teleport point** the developer tooling
carries, and it is not an actor, an objective or an interaction.

## The rest of the installation's passenger vocabulary

| Where | What it is | Why it is not M06's |
| --- | --- | --- |
| `ZBD/zrdr.zbd`'s `passengers.zrd` (+ `passenger_plane_destruct.zrd`, offset 1 901 845, length 11 253) | 17 `ON_CALL` crew animations over the node `apassengers`, and a destruction animation | Every definition is `ON_CALL`: a mission fires the crew it wants. M06 fires none of the 17; M01's startup fires `call_add_jack` (pinned by `accept_m01_lc_world_actors_retail_*`) |
| `ZBD/C2/gamez.zbd` nodes `apassengers` (503) and `passall` (504) | The world objects those animations drive: the player's crew and a crowd | M06's directives, targets, actor-init and startup name neither, while the eight world nodes its data *does* name each resolve exactly once |
| `ZBD/*/IA1/zrdr.zbd`'s `ia.zrd` | Instant-action configuration: mission classes `dogfight_ace`, `dogfight_squadron`, `zeppelin_run`, `ground_target`, `stunt_flying`, with zeppelin classes `cargo`/`passenger`/`military` | Instant Action, not a campaign mission; M06's archive has no `ia.zrd` |
| `strings.dll` / `langui.dll` | The display string for the map place ("Passenger Hangar", one UTF-16 row of `strings.dll`) and the objective-message id `MSG_OBJ_PASSENGERHANGER` | The id is carried by `ZBD/C1/IA1/zrdr.zbd` only; M06 references 15 ids, none of them passenger-named |
| `crimson.rof`'s `ASSETS/SCRIPTS/PASSENGERCABIN.SCRIPT`, and the image's UI-page table | A `PassengerCabin` UI page beside `FinalCinema`, `ScrapBook`, `IA_WrapUp`, `Load` and `Save` | A between-missions UI page, not mission data; nothing in M06's archive references it |
| `ZBD/interp.zbd` loading scripts, and the `cam_anim.zbd` front indexes | Loading-screen and camera-animation references to the people models and to `passenger_plane_destruct.zrd` | Loading/camera plumbing shared by every chapter |

A detail worth keeping for whoever reads the map next: the archive-member
vocabulary splits cleanly, and the split is measurable. **Node names** —
`propane`, `kkgate`, `tugandbarge01..04`, `sprucegoose`, `g_engine1..8` —
are what M06's own directives, targets and evaluator member lists spell, and
each resolves exactly once in the chapter's world array. **Location names** —
`Airport_terminal`, `Passenger_hangar`, `Crops`, `Coast` — reach only the
teleport list, and M06 spells none of them outside `location.zrd`. The
neighbouring node `sghangar` (the Spruce Goose hangar) makes the split
visible from the other side: `ZBD/C2/M02/zrdr.zbd`, `ZBD/C2/M03/zrdr.zbd` and
the chapter's instant-action record spell it, `ZBD/C2/M01/zrdr.zbd` spells it
nowhere, and it still resolves in the world exactly once. M06 uses the first
kind of name and never the second.

## Recorded unknowns (not guessed)

- **What the teleport feature does with an entry** is unmeasured: the strings
  name the feature and its file, and nothing here traces its code. A
  reference run or a static trace would settle it; neither is this task's.
- **`MSG_BRF_HWM2_OBJ1..3`, M06's own three objective texts, were not read.**
  Their names are in M06's archive, but the two resource-compiler headers the
  installation ships (`GOSDATA/ASSETS/crimson.rof`'s
  `ASSETS/SCRIPTS/RESOURCE.H` and `RESRC1.H`, re-derived through
  `cs-inspect rof`) define no such name, so the name → string-id join those
  three texts need is **unmeasured**. Reading them by a positional guess would
  be a fabricated binding, so this stage records the gap instead. It is
  independent of the verdict: the three ids are objective *text*, and the
  archive's whole message vocabulary is pinned above.
- **Whether M06's briefing prose mentions passengers** is likewise unmeasured
  for the same join reason. The installation's briefing text blocks that do
  mention passengers belong to other missions' stories (measured by scan, not
  bound to ids here).
- **The runtime halves stay open.** Whether any passenger can be picked up,
  delivered or extracted in M06 is ordinary-play behaviour (M06-C), and no
  reference capture for M06 exists (REF-OWNER-FIRST-CAPTURE is still blocked
  on the owner).
- **Nothing here is `verified_original`** (AGENTS.md rule 8): the work-order ↔
  mission join remains M06-A's inference, no original executable was run, and
  the engine-image half is a string-level static reading of the owner's
  decrypted image, not behaviour.
- **`M06.json`'s other unknowns are unchanged**, including *interaction
  authorizations*: F36's interaction schema still has no original importer, so
  this stage could not read a passenger-pickup authorization into the record
  even if one existed.

## Tests (`accept_m06_b_fu2_*`, 4 tests)

| Test | What it pins |
| --- | --- |
| `accept_m06_b_fu2_the_only_passenger_string_in_m06s_own_data_is_a_map_location` (retail) | the archive's digest and its 15 members all decode; the whole archive's passenger vocabulary is the one location name; the four locations with their positions and headings, in member order; the sibling mission's `location.zrd` has the measured digest and the same bytes |
| `accept_m06_b_fu2_the_image_spells_location_zrd_as_teleport_data` (image) | `load_engine_image` accepts the image at `ORIGINAL_IMAGE_SHA256`; the member name, the missing-member diagnostic, the teleport label, the backup name, the write-failure diagnostic and the developer annotation at their measured file offsets |
| `accept_m06_b_fu2_no_shipped_record_binds_a_passenger_entity_for_m06` (retail) | M06's five startup animation names; the library member's digest and its 17 `ON_CALL` crew animations driving `apassengers`, none of them fired by M06; the world container's digest, the 8 mission nodes resolving exactly once, and one `apassengers`/one `passall` that M06's members never name; the census-wide carrier set of `MSG_OBJ_PASSENGERHANGER` is exactly chapter one's instant action; M06's 15 message ids, none passenger-named; the instant-action record's three zeppelin classes |
| `accept_m06_b_fu2_a_location_entry_is_a_name_plus_two_float_triples` (synthetic, runs in CI) | on an authored document in the retail shape: a location is its name plus two float triples and carries nothing else, through `zrd_flat_fields` and through `decode_zrd` |

Every retail test calls production code
(`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`
and `zrd_flat_fields`, `cs_formats::gamez::read_gamez_nodes`,
`cs_app::mission_control::survey_mission_control_programs`,
`cs_content::coordinates::load_engine_image`,
`cs_assets::install::sha256`). The retail and image members are
`#[ignore]`d without their variable; the synthetic member runs in CI.

## Evidence

This task needs the `retail` capability (and the owner's decrypted image), so
it follows `docs/contracts/CLI-EVIDENCE.md`:

1. `cargo test --workspace --locked -- accept_m06_b_fu2_ --include-ignored`
   tee'd into `private/evidence/M06-B-FU2/cargo-test.log` (exit 0, 4 of 4);
2. `evidence_report_m06_b_fu2_writes_the_acceptance_report`
   (`crates/cs_app/tests/campaign/evidence/m06_b_fu2.rs`, selected by test
   name so no acceptance selection picks it up) writes
   `private/evidence/M06-B-FU2/acceptance.json` from that log, the candidate
   tree, the toolchain and production discovery of `$CS_GAME_DIR` — nothing in
   it is typed by hand;
3. `python3 tools/validate_evidence.py private/evidence/M06-B-FU2/acceptance.json
   --artifact-root private/evidence/M06-B-FU2 --require-pass` → structurally
   valid, one artifact (the log), exit 0;
4. the report is committed as `docs/findings/evidence/M06-B-FU2.json`.

The report committed here is the **reviewing** agent's own regeneration on the
reviewed commit — **bunny-alpha-1/bunny-alpha-1**, Rally #818 review claim of
2026-10-10, a separate session with a fresh context — replacing the
implementer's hand-over run. `review.identity` names both with the context
statement and says plainly that the same agent name and model implemented the
work, so this is a fresh-context agent review rather than independent-model
evidence. The claim is `implemented`, never `checked` or `verified_original`.

## Review round (2026-10-10)

The reviewing agent (same agent name and model as the implementer, fresh
session, so not independent evidence) read the task description and history,
the feature sheet's priorities, the whole diff and this note, and checked the
work against the task's acceptance criteria and AGENTS.md:

* one defect found and fixed: the sibling-node pin (`sghangar` resolves once;
  M06's archive spells it nowhere) was asserted **twice** in
  `accept_m06_b_fu2_no_shipped_record_binds_a_passenger_entity_for_m06`, a
  leftover of the two commits that split M06's own node names from its
  siblings'; the second copy was removed (commit `ac81dd33`), no assertion's
  meaning changed;
* everything else checked out: the four tests exercise production code
  (`discover_container`, `decode_zrd`, `zrd_flat_fields`, `read_gamez_nodes`,
  `survey_mission_control_programs`, `load_engine_image`, `sha256`), the
  retail/image members are properly `#[ignore]`d without their variables, the
  synthetic member runs in CI, no protected path is touched,
  `missions/bindings/M06.json` is byte-identical to main (the unmet criterion
  is reported and filed as M06-B-FU4 (#1184), not papered over), and the
  recorded unknowns stay unknown rather than guessed;
* the review re-ran the four checks below on the reviewed tree (all exit 0,
  the task selection again 4 of 4) and regenerated this task's acceptance
  report on that tree with its own reviewer identity, validated by
  `tools/validate_evidence.py --require-pass`.

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m06_b_fu2_ --include-ignored` | 0 (4 tests: 2 retail + 1 image + 1 synthetic) |

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`,
`cs_formats::gamez::read_gamez_nodes`,
`cs_app::mission_control::survey_mission_control_programs`; `$CS_ENGINE_IMAGE`
read-only through `cs_content::coordinates::load_engine_image`;
`missions/M06.md`; `missions/bindings/M06.json`;
`docs/contracts/SCRIPT-MISSION.md`;
`docs/findings/2026-10-01-m06-a-source-binding.md`;
`docs/findings/2026-10-09-m06-b-compatibility-gaps.md`;
`docs/findings/2026-09-29-f12-g-strings-dll-resources-and-header-id-correlation.md`;
`docs/findings/2026-10-02-t374-string-id-numbering.md`;
`crates/cs_app/tests/campaign/m06_b.rs`;
`crates/cs_app/tests/accept_m01_lc_world_actors.rs`;
`crates/cs_app/tests/campaign/m02_b_fu2.rs` (the image-pinning pattern);
`tools/cs_inspect` (`rof` subcommand, for the two header members).

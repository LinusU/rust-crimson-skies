# M01-LC-PLAYER-AIRFRAME-SOURCE: where the original assigns the player's airframe, and what of the start pose is measured

Date: 2026-10-06. Task: #715 `M01-LC-PLAYER-AIRFRAME-SOURCE`. Capabilities used:
**`retail`** (the owner's installation read-only) and **static analysis** of the
owner-supplied decrypted executable. No original run happened; nothing here is
`verified_original`, and no airframe is guessed for M01 (AGENTS.md rule 4).

## Sources and method

* Retail data, read-only: `$CS_GAME_DIR` (`CS_CAPABILITIES=retail,gpu,audio`),
  every `ZBD/*/*/zrdr.zbd`, `ZBD/zrdr.zbd`, `ZBD/interp.zbd`, `GOSDATA/`.
* `$CS_ENGINE_IMAGE`, sha256
  `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — the
  owner-supplied image #390/#436 work from. Virtual addresses below `0x643000`
  are file offset `VA − 0x400000`. Read with `strings` and radare2 (`izz`, `/r`,
  `pdf`); only addresses, constants and behaviour are recorded here, never bytes
  or decompiled code.
* Prior measurements this builds on: #676 (the `aiv.zrd` player record),
  #436's owner note of 2026-10-05 and #390's landmark list (units), #709 (the
  stored airframe's nose), F13-B/C and F38 (mission language).

## 1. The executable's airframe table

`.data` holds an array of **eleven rows of seven pointers**, `0x620c70` to
`0x620da4` (11 × 28 bytes; the loop bound of the routine below is the string
`Autogyro` at `0x620da4`). Read in index order, rows 0..10:

| # | display name | scene root (`player_*`) | `common\planes\<model>` | row's other pointers |
| --- | --- | --- | --- | --- |
| 0 | Autogyro | `player_autogyro` | `autogyro` | `pautogyro` `rautogyro` `wautogyro` `autogyro` |
| 1 | Hellhound | `player_avenger` | `avenger` | `pavenger` `ravenger` `wavenger` `avenger` |
| 2 | Balmoral | `player_balmoral` | `balmoral` | `pbalmoral` `rbalmoral` `wbalmoral` `balmoral` |
| 3 | Bloodhawk | `player_bhawk` | `bloodhawk` | `pbloodhawk` `rbloodhawk` `wbloodhawk` `bloodhawk` |
| 4 | Brigand | `player_brigand` | `brigand` | `pbrigand` `rbrigand` `wbrigand` `brigand` |
| 5 | Devastator | `player_pfighter` | `piratefighter` | `pdevastator` `rdevastator` **`wingman`** `devastator` |
| 6 | Firebrand | `player_fbrand` | `firebrand` | `pfirebrand` `rfirebrand` `wfirebrand` `firebrand` |
| 7 | Fury | `player_fury` | `fury` | `pfury` `rfury` `wfury` `fury` |
| 8 | Kestrel | `player_kestrel` | `kestrel` | `pkestrel` `rkestrel` `wkestrel` `kestrel` |
| 9 | Peacemaker | `player_peacemaker` | `peacemaker` | `ppeacemaker` `rpeacemaker` `wpeacemaker` `peacemaker` |
| 10 | Warhawk | `player_warhawk` | `warhawk` | `pwarhawk` `rwarhawk` `wwarhawk` `warhawk` |

Three columns are bound because their meaning is **measured**:

* the **display name** is what the document keys write (`Fury` below) and what
  the name-to-index routine compares;
* the **scene root** is exactly one of the eleven `set planeOutput player_<x>`
  nodes `support\planes.gw` in `ZBD/interp.zbd` creates (pinned on main by
  `RETAIL_DECLARED_AIRFRAMES` in `crates/cs_content/tests/scene.rs`), and those
  are the names the script's own `FindNode %player_plane%` lines consume;
* the **model** is the `set planeInput common\planes\<model>\<model>.flt` of the
  same block.

The remaining four pointers per row (`p…`, `r…`, `w…`, second base name) are
**not** transcribed: no evidence says what they select. Row 5 keeps its own
oddity rather than a smoothed-over name: its `w…` field is the literal
`wingman`, not `wdevastator`, and `Devastator` — the display name — is the row
whose scene root is `player_pfighter` and whose model is `piratefighter`.

**Name → index.** One routine, `0x426d80`, walks the table from `ebp = 0x620c70`
in `+0x1c` steps while `ebp < 0x620da4`, comparing the whole argument against
row 0's pointer with `tolower` on both sides. It returns the running index on
the **first** full match; otherwise — no row whose name the argument exhausts,
or a **second** row matching a name that has already matched — it returns `0xb`
(11), and every caller tests `cmp eax, 0xb` and treats 11 as "none",
substituting its own default. Its five call sites:

| caller | what it reads |
| --- | --- |
| `0x4593fe` | `player_plane` — the player's airframe |
| `0x459458` | `wingman_plane` |
| `0x4594f8` | `ace_plane` |
| `0x458f16` | `enemy_plane` |
| `0x43de51` | a console command ("You now have the %s.") that names a plane |

## 2. The one document key that names the player's airframe

The string `player_plane` lives at `0x62556c` and has **exactly one code
reference**: `push str.player_plane` at `0x4593e5`, inside a routine whose
prologue is `0x4574d0`. That routine reads, in order, `mission_type`,
`player_plane`, `num_wingmen`, `wingman_plane`, `ace_name`, `ace_plane`,
`ace_stats`, `ace_accentID`, `ace_pattern`, `ace_decal1..3`, `ace_color1..3`,
`ace_skill` (`novice`/`veteran`/`ace`), `group%d`, `zeppelin_type`,
`ground_target_name`, `ground_target_node`, `dzones`, `_zeppelin`,
`_zep_start_anim`, and near its end opens **`ia.zrd`** (`0x45a15b`) and reads
`disallow_missions` and `spawn_points`; it also holds the mission-type labels
`dogfight_ace`, `dogfight_squadron`, `zeppelin_run`, `ground_target`,
`stunt_flying`. It is the instant-action scenario setup, and `ia.zrd` is its
document.

Read for `player_plane` it takes the value's string, calls `0x426d80`, and
stores the index unless the answer is 11, in which case it keeps the value the
field already had. So the assignment is a **name from the table**, resolved at
read time, not an index stored in the data.

**Retail check of where the key lives.** `grep` over the whole installation:

```
ZBD/C1/IA1/zrdr.zbd    ZBD/C1B/IA1/zrdr.zbd   ZBD/C1C/IA1/zrdr.zbd
ZBD/C2/IA1/zrdr.zbd    ZBD/C3/IA1/zrdr.zbd    ZBD/C4/IA1/zrdr.zbd
ZBD/C5/IA1/zrdr.zbd    (+ ZBD/zrdr.zbd, which carries only the unrelated
                        member names `player_plane_destruct.zrd`, and
                        ZBD/interp.zbd, which carries `%player_plane%`)
```

— **seven of the eight** `IA1` archives: `ZBD/C2B/IA1/zrdr.zbd` contains no
`player_plane` byte sequence at all, although its `ia.zrd` has `mission_type =
"zeppelin_run"`, `zeppelin_type = "cargo"` and four `enemy_plane` groups
(`Firebrand`, `Kestrel`, `Bloodhawk`, …) plus `spawn_points`. So even an
instant-action scenario can leave the player's airframe unstated in the data.
The production walk behind the evidence report agrees independently: 62 reader
archives, 1293 members decoded, 0 decode failures, the key in **7** members —
all of them `ia.zrd`, one per chapter except `C2B` — and **no campaign mission
archive**. `ZBD/C1C/IA1/zrdr.zbd`'s `ia.zrd` reads `mission_type =
"zeppelin_run"`, `zeppelin_type = "cargo"`, **`player_plane = "Fury"`**,
`num_wingmen`, `enemy_name`, `enemy_plane = "Firebrand"`, `enemy_skill =
"novice"`. M01 (`ZBD/C1C/M01`) has no `ia.zrd` member at all — its twelve are
`aiv`, `egen`, `location`, `map`, `mis_anim`, `net`, `objectives`,
`startanims`, `weather`, `zeppelins`, `placezeps`, `wv_tailhook` — and **every
one of them decodes without the key**, asserted member by member by
`accept_m01_lc_player_airframe_source_retail_m01_has_no_airframe_key_and_the_scenario_does`,
which also pins `Fury → (7, player_fury, fury)` for the chapter's scenario.

## 3. The other candidate sources, measured

* **Mission-program statement.** No directive key of the measured mission
  language names an airframe (the keys are listed in
  `crates/cs_content/src/mission_control.rs`; `SET_AI_*`, `WARP_VEHICLE`, …
  among them), and M01's twelve members contain no airframe display name:
  `Bloodhawk`, `Warhawk`, `Hellhound`, `Brigand`, `Firebrand`, `Kestrel`,
  `Peacemaker`, `Balmoral` and `Autogyro` occur **zero** times; `Fury` and
  `Devastator` occur only inside scripted record names (`bsfury_1`,
  `bsfury_2`, `rusdevastator_1..5`, `devastator_2`, `devastator_3`,
  `devastator*`). This confirms and sharpens #676, which had searched the
  members for `bloodhawk` alone.
* **Profile / save.** The installation carries **no profile, save or hangar
  file**: `GOSDATA/ASSETS` holds only `BINARIES`, `GRAPHICS` and two `.rof`
  archives, and there is no loose `*.zrd`, `*.cfg` or save file anywhere under
  `$CS_GAME_DIR`. The executable's save keys are
  `CRIMSON_SKIES_SAVELOAD_VERSION`, `Pilot_Status`, `UIData`, `Mission_Data`
  and `SavedGames`, i.e. a state written to the player's own directory —
  nothing in the installation states a campaign airframe. The engine's own
  strings spell the pre-mission screen `FlightCheck` / `FLIGHTCHECK`, and the
  words "hangar", "garage", "shop" and "loadout" do not occur in the image at
  all.
* **Executable table.** Measured above: it converts a *name* into a row. It
  does not choose one for a mission, and it holds no mission → airframe map.

**Verdict.** For a campaign mission the assignment source is **still unknown**,
now with the candidate list narrowed by measurement: it is not the mission's
data, not a file in the installation, and not the mission program as far as the
measured directive keys go. What is left is state the engine keeps outside the
installation (profile / flight-check selection) or a default inside the image
that this static pass did not locate. `MissionStartConfiguration::airframe()`
stays `Resolved::Unknown`, with `AIRFRAME_UNKNOWN_REASON` rewritten to record
exactly this. Affected: M01's player spawn; the `player_configuration` surface
of `MissionLaunchPlan::launchable` (#359, whose branch carries that gate) stays
unsatisfied for M01.

## 4. The start pose: unit measured, heading zero not

* **Unit = metre, citing #436.** The owner note of 2026-10-05 on #436 measures
  the original's world unit as the metre, +Y up, right-handed, with "stored
  positions map to the canonical frame with an identity axis map and a scale of
  1.0" (landmarks S1–S4: `position.y` × 3.2808399 → feet at `0x48fc40`,
  m/s × 2.2369363 → mph at `0x453aa2`, gravity −9.8 / 9.82; A1–A3 for +Y up;
  H1–H3 for right-handed). It also measures `.zrd` angle fields as degrees
  (`π/180` at `0x6040e8`, 123 references). #676's own measurement of the record
  agrees: axis 1 is the vertical one, and the heading values are multiples of 5
  reaching 330 — degrees-like, never radians.
  `StoredStartPose::position_metres()` and
  `STORED_POSITION_METRES_PER_UNIT = 1.0` bind the scale; the retail test pins
  M01's stored `(-3694, 1318, -12482)` in metres.
* **Heading zero and handedness: still unknown, by name.** The same #436 owner
  note lists what it did *not* measure: "The aircraft body forward axis …
  Compass/heading zero direction … Any behaviour landmark". #709's nose
  measurement (`−Z` for all eleven stored models) is about how the *drawn*
  model composes, not about which way a stored heading of `170` points in the
  world, and it says so. Without the original's own spawn/conversion code
  running, turning `170` into a world yaw would be a guess.
* **Frame relation: open.** M01's player start lies 194 stored units outside
  `c1c`'s `[-12288, 0]^2` node bounds, and its wingmen 777 and 826 (#676). That
  is recorded as an unmeasured relation between the `aiv.zrd` frame and the
  world grid — not as evidence against the metre, and not smoothed away.
* The mission program may still move or replace the aircraft before launch
  (F13-B/C, F38).

`MissionStartConfiguration::initial_pose()` therefore stays `Resolved::Unknown`
with `POSE_UNKNOWN_REASON` rewritten to say what *is* measured (the metre) and
what is not (zero direction, handedness).

## 5. What changed

* `crates/cs_app/src/mission_start.rs`: `AIRFRAME_TABLE` / `AirframeEntry`,
  `airframe_index`, `airframe_entry`, `PLAYER_PLANE_KEY`,
  `scenario_player_airframe`, `STORED_POSITION_METRES_PER_UNIT`,
  `StoredStartPose::position_metres`, and the two refusal reasons rewritten.
  Module documentation records the #715 measurement with its addresses.
* `crates/cs_app/tests/campaign/m01_lc_player_airframe_source.rs` (registered in
  `tests/campaign/main.rs`): five `accept_m01_lc_player_airframe_source_*`
  tests — four synthetic (table order and rows, index lookup, both `.zrd`
  shapes of the key, the metre and both refusal texts) and one retail
  `#[ignore = "requires CS_GAME_DIR"]` that re-derives the absence over every
  M01 member, the `Fury` assignment of `ia.zrd`, and M01's stored pose in
  metres. Run with
  `cargo test -p cs_app --test campaign -- accept_m01_lc_player_airframe_source_ --include-ignored`.
* `crates/cs_app/tests/evidence_report_m01_lc_player_airframe_source.rs`: the
  evidence-report harness described in section 7 (not an acceptance test; it
  fails loudly when its inputs are missing).
* `docs/findings/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE.json`: the committed
  acceptance report.

## 6. Unknowns, affected content and resolving work

| unknown | affected content | resolving work |
| --- | --- | --- |
| Which state assigns a **campaign** mission's player airframe (profile/flight-check selection or an image default not located here) | M01's player spawn and every campaign mission's; the `player_configuration` launch surface | needs evidence outside this static pass: the flight-check/profile save shape (F45, F48), a decoded native binding (F44 + F38), or an original run (#358). Never an assumed airframe. |
| The heading's **zero direction and handedness** | `initial_pose()` for every mission; the camera/spawn yaw | #358's owner-supplied original capture, or a decoded native spawn/conversion routine |
| The **frame relation** between `aiv.zrd` start positions and the world grid (194 / 777 / 826 stored units outside `c1c`'s bounds) | every start pose's world placement | #436's remaining work and F16-E-2's prose sync |
| What the table's `p…`/`r…`/`w…`/second-base pointers select, and why row 5's is the literal `wingman` | AI and wingmate node naming | a follow-up of F11/F33 over the same table |
| Whether the mission program moves the aircraft before launch | the whole start pose | F13-B/C, F38 |

## 7. Evidence

`crates/cs_app/tests/evidence_report_m01_lc_player_airframe_source.rs` writes
the acceptance report (`docs/contracts/CLI-EVIDENCE.md`, schema
`schemas/evidence.schema.json`), following the sibling M01-LC reports:

```sh
cargo test --workspace --locked -- accept_m01_lc_player_airframe_source \
  --include-ignored 2>&1 | tee \
  private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE/cargo-test.log
# then CS_EVIDENCE_DIR / CS_CANDIDATE_TREE / CS_EVIDENCE_ARGV /
# CS_EVIDENCE_EXIT_CODE / CS_EVIDENCE_REVIEWER into
#   cargo test --locked -p cs_app \
#     --test evidence_report_m01_lc_player_airframe_source -- --ignored
python3 tools/validate_evidence.py \
  private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE/acceptance.json \
  --artifact-root private/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE --require-pass
```

The committed copy is `docs/findings/evidence/M01-LC-PLAYER-AIRFRAME-SOURCE.json`.
Besides the recorded test log its second artifact, `airframe-source.json`, is a
second production run: every reader archive of the installation walked, every
member decoded, the `player_plane` key read where it occurs, M01's start
configuration re-read, and the table as this crate binds it — counts, member
names, airframe names and digests only, never original bytes. Its `claim` is
`implemented`, never `checked` or `verified_original`; the product unknowns
above are limits on that claim, recorded here and in `review.method` rather
than dropped from the machine-readable report.

## 8. Review

**Identities** (AGENTS.md's review section): the *implementer* was `bunny-2`
in the session of 2026-10-06 (fresh context); the *reviewer* is `bunny-2` in a
separate review session of 2026-10-06, also fresh — no earlier conversation
about this branch — but the same agent name and model as the implementer, so
this is **not** an independent instance, and Rally does not enforce reviewer
assignment. Nothing here is `verified_original`: no original executable ran,
and no owner approval is claimed.

The branch was rebased onto `origin/main` at `d1f660a0` before review; the
rebase applied cleanly, the commits it brought in touch no `Cargo.toml`,
`Cargo.lock` or file this branch changes, and the review added commits of its
own, so the full four checks (not the owner directive's lighter re-push set)
were run.

### Two problems found, fixed in review

* **A report field was written instead of measured.** `airframe-source.json`'s
  `measured_mission.player_plane_carried` was the literal `false` in the
  harness's format string. The walk now derives it: any of M01's members
  carrying `player_plane` sets it to `true`. On this installation it derives
  `false`, so the report's meaning is unchanged — but the field is now an
  observation of this run rather than an assertion the run could not contradict.
* **The index lookup's equivalence was asserted, not pinned.** `airframe_index`
  answers with `position` (first row that matches) while `0x426d80` answers
  `11` (none) for a name two rows match; the two agree only while the display
  names are distinct. The table test now pins that invariant (distinct after
  case folding, as the original's `tolower` compares).

### Mutation probes (each reverted; `git status --porcelain` empty afterwards)

| probe | result |
| --- | --- |
| `airframe_index` returning `None` | 3 of the 5 `accept_m01_lc_player_airframe_source_*` tests fail: lookup, both `.zrd` shapes, retail |
| `scenario_player_airframe` returning `None` | 2 fail: both `.zrd` shapes, retail |

### Evidence regenerated on the rebased commit

`docs/contracts/CLI-EVIDENCE.md` requires the reviewer to regenerate the report
on the rebased commit. Run again on `candidate_tree` `a1a6788c…` (the tree the
code under review was tested at; the report itself is committed on top of it),
with the same commands as above: **62** reader archives walked, **1293**
members decoded, **7** `player_plane` assignments (one `ia.zrd` per chapter
except `ZBD/C2B/IA1`), 0 decode failures, **5/5** assertions — identical to the
implementer's run — and `python3 tools/validate_evidence.py … --require-pass`
→ `{"structurally_valid": true, "artifact_count": 2}`.

### Commands run in review (exit codes)

```text
cargo fmt --all -- --check                                                   → 0
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings → 0
cargo test --workspace --locked                                              → 0
cargo test --workspace --locked -- accept_m01_lc_player_airframe_source \
  --include-ignored                                                          → 0 (5 discovered, 5 executed, 5 passed)
python3 tools/validate_evidence.py <acceptance.json> --artifact-root … \
  --require-pass                                                             → 0
```

### Evidence convention kept

`unknowns` stays `[]` and the report stays `--require-pass`-green, following
the sibling M01-LC and F39 reports: every unresolved item above is recorded
machine-readably in `review.method` in full and here in section 6 with its
affected content and resolving task, and the report's own acceptance run has
nothing unresolved in it. The report does **not** drop an open question to go
green: the campaign airframe source, the heading's zero direction, the frame
relation and the table's unused pointers remain open and are named above.

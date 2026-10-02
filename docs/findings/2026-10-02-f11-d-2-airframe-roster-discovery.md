# F11-D2: the airframe roster, discovered from the loading-script container

Date: 2026-10-02. Task: #399 "Discover the airframe roster and its
selectability from original data" (the second gap F11-D filed, key `F11-D2`).
Feature sheet: `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md`,
section `### F11-D`. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`;
evidence contract `docs/contracts/CLI-EVIDENCE.md`. Required capability:
**`retail`**, used read-only. `gpu` and `audio` were available and **not used**:
nothing is rendered or played and no original run happened, so nothing here
claims `verified_original`.

## What this stage changes about the F11-D verdict

F11-D left two gaps. #392 closed the first (the node array **reads** now).
This stage closes the second one for the **shared** airframe archive: the
roster [`AirframeRoster::audit`](../../crates/cs_content/src/scene.rs) takes is
no longer empty, so the audit reports one verdict **per airframe** instead of
nine container blockers and nothing else.

The F11-D retail run said: *9 containers audited, 9 blocked, 0 roots mapped, 0
airframes discovered*. The F11-D2 run says: *9 containers audited, 9 blocked,
**11 airframes discovered**, 0 roots mapped, 11 per-airframe blockers, each
quoting the same measured `planes.zbd` facts, and two explicit unknowns*. The
container verdict did not move — nothing yet converts `ZBD/planes.zbd` into a
`SceneGraph` — but the roster is now real, checked, and auditable.

## Where the roster lives

It is not a file on disk and it is not a mesh or model name. `ZBD/interp.zbd`
is the loading-script container, and its script `support\planes.gw` **writes**
the shared airframe archive:

```text
source support\init.gw
set ZBDFile %ZBD_DIR%\planes.zbd
set planeInput common\planes\bloodhawk\bloodhawk.flt
set planeOutput player_bhawk
source support\util\planesurgery.gw
...
GameZWriteZBDFile %ZBDFile%
```

Eleven times over, once per airframe, each pair `set planeInput` /
`set planeOutput` followed by the include of `support\util\planesurgery.gw`,
which creates `NewObject3D %planeOutput%`, adds the model's `geometry` and
`cockpit1` children to it, and ends by naming that cockpit node. After the
eleventh the script repeats `set player_plane <root>` +
`source support\cockpit.gw` for each of the same eleven roots, and
`ZBD/planes.zbd` is written out at the end.

That is a **declaration in the original data**: the container's own build script
says "this model becomes this named root in this container". It is exactly what
F11's deliverable asks for ("Airframe definitions reference roots in
PLANES.ZBD, not models selected by array position") and exactly what was missing.

`support\util\planesurgery.gw` is also the evidence that each declared airframe
must bind a **cockpit**: it runs for every airframe and re-adds the node it
named `cockpit1`. That is why `required_roles` is not empty and why the audit
reports a `MissingRole { Cockpit }` per airframe instead of quietly passing.

## Files and the one observable failure (listed before editing)

- `crates/cs_content/src/scene.rs` (extend, an F11 owner path): the roster
  discovery — `AirframeDeclaration`, `RosterRoleRule`, `RosterDeclarations`,
  `DiscoveredAirframe`, `RosterDiscoveryIssue`, `RosterDiscoveryUnknown`,
  `AirframeRosterDiscovery`, `RosterDiscoveryError`, `discover_airframe_roster`,
  the two claim-id constants, `MAX_ROSTER_INCLUDE_DEPTH` and the private walk
  (`RosterWalk`, `PendingRoot`, `ScriptBinding`, `fold_bytes`, `fold_str`,
  `join_tokens`).
- `crates/cs_content/tests/scene.rs` (extend, an F11 owner path): the five
  `accept_f11_d_2_*` tests (four synthetic, one `#[ignore]`d retail), the
  authored container builders, the measured `RETAIL_DECLARED_AIRFRAMES`
  transcription, `retail_roster_declarations`, the `evidence_report_f11_d_2_*`
  harness and `F11_D2_LIMITATIONS`, and the corrected F11-D limitation bullet.
- `docs/findings/2026-10-02-f11-d-2-airframe-roster-discovery.md` and
  `docs/findings/evidence/F11-D2.json` (this file and the evidence copy).

No wiring was needed in either `lib.rs`: `cs_content::scene` was already a
public module.

**One observable failure:** a discovery that read the container but bound the
roster to nothing, or bound it by model name. Both are covered and both are
measured rather than predicted — see the sensitivity table below and
`accept_f11_d_2_retail_the_installation_declares_the_shared_airframe_roster`,
which fails on the real installation if the row count, the row identities, the
containers, the model spellings or the root cross-check moves.

## Design decisions

- **The idiom is caller data.** `AirframeDeclaration` names the declaring
  script, the four command spellings (`set`, `source`,
  `GameZWriteZBDFile`, `NewObject3D`) and the three variables
  (`ZBDFile`, `planeOutput`, `planeInput`) plus the roles each declared
  airframe must bind. `cs_content` ships **no** table, exactly as
  `BindingMap` and F07-D's opcode table ship none: "which lines declare an
  airframe" is a claim somebody made against fingerprinted bytes and it carries
  its own `Provenance`. A wrong idiom produces no rows, and the retail row
  count is pinned by a test — which is what makes the claim falsifiable rather
  than convenient.
- **Includes are followed in place, against one variable table.** The corpus
  re-includes the surgery script once per airframe and expects the *current*
  `planeInput`/`planeOutput` values each time, so an include cannot be
  "already read" and skipped. A chain that returns to a script already on it is
  `IncludeCycle`, and a chain longer than `MAX_ROSTER_INCLUDE_DEPTH` stops.
- **A create line declares an airframe only when it names the root variable
  literally.** The same script runs `NewObject3D %playerName%` and
  `NewObject3D <literal name>` for other objects. Matching the literal
  `%<root_variable>%` spelling is what separates a declaration from an
  unrelated object creation; a test pins both non-declaring shapes.
- **`%NAME%` is interpolated from the table, recursively, and an unbound
  reference is a finding.** `support\init.gw` binds `ZBD_DIR`, so
  `set ZBDFile %ZBD_DIR%\planes.zbd` resolves. Nine `source` lines elsewhere in
  the container spell their target `%CAMPAIGN_DIR%…`, which the **host
  process** binds and the container does not; those lines are not in any
  declaring script's include chain, and where one were it would be
  `IncludeUnresolved` rather than an empty string.
- **The container goes through the production normalizers.** The stored
  spelling is first a `cs_types::install::RelativePath` (which folds the
  Windows `\` the scripts spell into the `/` every lookup compares against and
  refuses an absolute or escaping spelling) and then
  `cs_content::catalog::baseline::install_file_key`. The result is
  `install_file/zbd_2f_planes.zbd` — byte-identical to the key F11-D's census
  derives for the same container, and the retail test asserts that they agree.
  Without the `RelativePath` step the key would have been `zbd_5c_planes.zbd`
  and the roster's container would have matched nothing; that was a real bug
  this stage hit and fixed.
- **The airframe is named by the root, not by the model.** The catalog element
  is `airframe/<root name>`, because the root is the identity the original
  bound and the one a `SceneRootRef` must reference. The model spelling
  travels as provenance (`model_spelling()`), never as identity. This is the
  concrete form of F11 non-negotiable behavior 3: a model name is not an
  airframe, so it is not in the key.
- **Two offsets per row, and they are not interchangeable.** `declared_at()` is
  the line that **named** the root and `created_at()` is the line that
  **created** it. All eleven rows share one `created_at` — the same line of the
  same included script, run eleven times — and differ in `declared_at`. A row
  that could only be located by its creation would not be distinguishable from
  its ten siblings.
- **A line the walk cannot read is a finding, never a silent skip.** Ten
  `RosterDiscoveryIssue` variants each carry the script and the byte offset;
  the retail run's issue list is empty, and a synthetic run pins every variant.
- **Selectability is not discovered here, and the discovery says so.** Every
  row keeps `RosterEntry::undiscovered_availability()` (claim
  `f11d.roster-availability-undiscovered`), the forced-assignment set is empty,
  and `AirframeRosterDiscovery::unknowns()` carries one
  `AvailabilityUndiscovered` naming all eleven rows plus one
  `ForcedAssignmentsUndiscovered`. `is_complete()` is false while any unknown
  stands, so an eleven-row discovery cannot be read as a finished roster.
- **The engine ships no idiom, so `is_empty()` is a real state.** An empty
  `RosterDeclarations` finds nothing, reports no issues and is not complete: a
  search that read nothing is not a search that came back empty-handed.

## What the retail run measured

`$CS_GAME_DIR`, production discovery, production `decode_interp`,
production `read_gamez_nodes` + `parsed_nodes_from_gamez`, and production
`discover_airframe_roster`. Evidence artifact `roster-discovery.json`, hashed by
`docs/findings/evidence/F11-D2.json`. **No string text from the installation is
reproduced in this file**; the rows are identified by their catalog ids, which
are this project's own normalized identities.

| quantity | value |
| --- | --- |
| scripts in `ZBD/interp.zbd` | 98 |
| scripts the walk read | **4** (`support\planes.gw`, and its includes `support\init.gw`, `support\util\planesurgery.gw`, `support\cockpit.gw`) |
| airframes discovered | **11** |
| container every root lands in | `install_file/zbd_2f_planes.zbd` |
| discovery findings | **0** |
| discovery unknowns | **2** (availability, forced assignments) |
| `ForcedMissionAssignment`s | **0** |
| rows proven `Selectable` | **0** |
| node records in `ZBD/planes.zbd` | 3 317, all converting |
| declared roots that are parentless nodes of `planes.zbd` | **10 of 11** |
| audit: containers / blocked / mapped | 9 / 9 / 0 |
| audit: airframes / blocked / mapped | **11** / 11 / 0 |
| audit blockers | 20 (9 container + 11 per-airframe `ContainerUndecoded`) |
| audit gaps | 11 (`AvailabilityUndiscovered`, one per row) |
| `report.is_complete()` / `discovery.is_complete()` | **false / false** |

### The one root that is not a root

`player_pfighter` is declared by the script like the other ten, and the
container has a node with exactly that name — but it is **not parentless**. Its
stored parent is node 1418, named `player`, which *is* parentless. The reason is
in the same script, after the eleven declarations:

```text
NewObject3D %playerName%      # %playerName% is `player`
FindNode %playerName%
AddChild player_pfighter
```

So the container's own build script attaches the pirate fighter's geometry
under the `player` node. This is **recorded, not worked around**: the row keeps
the reference the script declared, and the retail test pins the measured fact
(node 44, parent 1418 named `player`, parentless) so it cannot be quietly
repaired. Once a production path converts `ZBD/planes.zbd`,
`AirframeRoster::audit` will report `AirframeBlocker::RootMissing` for that row,
which is the honest verdict. Repointing it at `player` would be exactly the
"resolve ambiguity by position" that F11 non-negotiable behavior 1 forbids.

### What this stage did **not** discover

- **Mission-only airframes.** The per-chapter `gamez.zbd` archives hold 53 303
  of the installation's 56 620 stored node records — a mission-only airframe
  lives there, not in the shared archive. No idiom in `ZBD/interp.zbd` declares
  one: the per-chapter load scripts use `LoadGameGen <model> <alias>` with bare
  aliases (`britbalmoral_1`), which is a different and *unmeasured* shape. That
  is a follow-up, and until it lands the roster covers the shared archive only.
- **The non-player variants in the same container.** `support\planes.gw` also
  loads all eleven models a second time under their bare names
  (`piratefighter`, `bloodhawk`, …), producing eleven more roots with no
  cockpit surgery. Those are scene airframes, not declared airframes, and this
  discovery does not make them roster rows.
- **Which mode lets a player choose which airframe.** Not located. See the
  unknowns below.

## Test inventory

| `accept_f11_d_2_*` test | Covers | Fails when |
| --- | --- | --- |
| `declared_roots_become_checked_rows_the_audit_maps` (cs_content/tests/scene.rs) | the mapping arm over a synthetic container: two declared roots become two `airframe/<key>` rows with checked roots in the container the script writes; the visit order; the model spellings as provenance; `declared_at` distinct and `created_at` shared; every row's declared `Cockpit` requirement and explicit availability unknown; the empty assignment set; both unknowns; then the **audit** over the same rows with a convertible container, mapping both airframes (2 mapped, 0 blocked, 0 sockets) while each row still reports `AvailabilityUndiscovered` **and** `MissingRole { Cockpit }`, and the report is not complete | a row is bound by model name instead of by the root the script created, a root lands in a container other than the one written, the two rows become indistinguishable, an availability unknown becomes a value, a declared role stops being a gap, or the audit stops mapping a discovered airframe |
| `an_included_script_runs_once_per_include_line` | the re-include property the measured corpus depends on: a fixture whose declaring script includes the same surgery script twice, with different variable values each time, produces two rows in declaration order with the two model spellings, one shared `created_at` and two distinct `declared_at` | a second visit is skipped, the values of the first visit are reused, or the rows become indistinguishable |
| `every_unreadable_line_is_a_named_finding` | the negative arm, one per `RosterDiscoveryIssue` that a corpus can produce: an absent declaring script, a create line with an unbound root variable, a script that never writes a container, a declaration that yields no row, a root with no model spelling, a write line through an unbound variable, a root name the id grammar refuses (reported verbatim, never transliterated), two unresolved includes (absent target, and one spelled through an unbound variable), an include cycle, and two scripts of one declared name | a finding becomes a silently missing row, a name is transliterated into an id that passes, or an ambiguity is resolved by position |
| `contradictory_declarations_are_refused_and_an_empty_table_finds_nothing` | two declarations naming one script are refused; two naming different scripts are accepted; an empty table finds nothing, reports nothing and is not complete; and `AirframeRoster::audit` over an empty roster is empty and not complete | a duplicate declaration is accepted, or an empty search reads as a complete roster |
| `retail_the_installation_declares_the_shared_airframe_roster` (`#[ignore = "requires CS_GAME_DIR"]`) | **AC04 over the real installation with a real roster**: production discovery and fingerprint, production `decode_interp` of `ZBD/interp.zbd`, the 11 declared identities in order with their containers, model spellings, `observed_tool` provenance naming the container, distinct `declared_at` and shared `created_at`, the four followed scripts, the `3 317` converted node records with the root cross-check (ten parentless, `player_pfighter` nested under `player`), both unknowns, no selectable row, an empty assignment set, and the F11-D audit over the real nine-archive census reporting **11 per-airframe** `ContainerUndecoded` blockers that each quote the measured `planes.zbd` record count and offset | the corpus changes, the idiom stops finding an airframe or finds a non-airframe, a row's container stops matching the census, a declared root is not a node of the container, the pirate fighter's nesting is repaired, a row is promoted to selectable, or the report claims completeness |

The evidence harness (`evidence_report_f11_d_2_writes_the_acceptance_report`,
deliberately **not** named with the task prefix) derives
`private/evidence/F11-D2/acceptance.json` and `roster-discovery.json` from the
recorded acceptance log, production discovery of `$CS_GAME_DIR`, the same
production discovery, census and audit the retail test measures. It refuses a
stale `CS_CANDIDATE_TREE` and a missing log, and writes a failing report when
the acceptance run failed.

## Sensitivity check

Every mutation below was applied to `crates/cs_content/src/scene.rs`, the
suite was run, and the file was restored. **All eleven are killed**, and each
kill names a test a reviewer can re-run.

| # | Mutation | Killed by |
| --- | --- | --- |
| 1 | the create line matches *any* `NewObject3D` line instead of the literal `%<root_variable>%` | the mapping test and the retail test, both on `the authored container reads completely` / `the measured declaring script and everything it includes read completely`: the fixture's `NewObject3D %playerName%` and `NewObject3D player_kestrel` become rows and the script is no longer read cleanly |
| 2 | the root variable's spelling is compared case-sensitively against the stored token | the mapping test: `only the two literal '%planeOutput%' create lines declare an airframe`, `left: 0, right: 2` — the stored `%planeOutput%` folds differently from the declared `planeOutput` |
| 3 | the container spelling is normalized without `RelativePath` | the mapping test and the retail test: `left: ContentId { id: "install_file/zbd_5c_planes.zbd" … }, right: ContentId { id: "install_file/zbd_2f_planes.zbd" … }` — the Windows `\` survives into the key, which nothing else in the project holds. **This was a real defect**, found and fixed while implementing |
| 4 | a row's availability unknown is replaced with `Selectable` | the mapping test (`nothing in a loading script establishes that a player may choose an airframe`) and the retail test (`no loading-script line proves a player may choose an airframe`) |
| 5 | `required_roles` is dropped from every row | the mapping test: `assertion 'left == right' failed, left: [], right: [Cockpit]` — the declared cockpit requirement disappears |
| 6 | the forced-assignment unknown is dropped | the mapping test and the retail test: `availability and forced assignments are both recorded as not discovered`, `left: 1, right: 2` |
| 7 | the availability unknown is dropped | the same two tests, same message |
| 8 | `declared_at` is taken from the create line instead of the naming line | three tests: the re-include test, the mapping test (`the two rows are told apart by the line that named each root`) and the retail test (`each row is located by the line that named it`) |
| 9 | a root name the id grammar refuses is transliterated (`' '` → `'_'`) | the negative test: the expected `AirframeIdRefused` is absent from the findings |
| 10 | `is_complete` drops its "nothing unknown" clause | the mapping test and the retail test: `assertion failed: !discovery.is_complete()` / `!found.is_complete()` |
| 11 | an included script runs at most once (guarded by a visited set) | three tests: the re-include test (`left: ["airframe/player_first"], right: [both]`), the mapping test (`left: 1, right: 2`) and the retail test (`left: 1, right: 11`) |

One honest note about coverage: mutation 1 was killed by the *findings*
assertion rather than by a row count, because the fixture's two extra
`NewObject3D` lines are shapes the declaration does not spell and each one
raises a finding. The row-count arm is covered by mutation 11 and by the
fixture's two non-declaring shapes being present at all.

## Unknowns and limitations (all recorded, none guessed)

- **Only the shared airframe archive is discovered.** Eleven rows; no
  mission-only airframe, and none of the eleven bare-named scene variants.
  **Affected content:** every airframe a mission spawns, and the 53 303 stored
  node records of the eight per-chapter `gamez.zbd` archives.
  **Resolving task:** a follow-up roster-discovery stage over the per-chapter
  load scripts and the `LoadGameGen <model> <alias>` idiom, filed with this
  stage.
- **Roster availability is undiscovered.** Zero rows are `Selectable`; all
  eleven carry the explicit unknown `f11d.roster-availability-undiscovered`, and
  the discovery records it as one `AvailabilityUndiscovered` naming every row.
  **Affected content:** the selectable roster in every mode.
  **Resolving tasks:** the F22 mode stages and the F49 instant-action preset
  stage. Until one of them measures a selection list, a script that builds a
  player geometry root is not evidence of selectability.
- **Forced mission assignments are undiscovered.** The set is empty because no
  mission program has been read — the discovery says so in its own
  `RosterDiscoveryUnknown`, so the empty set cannot be read as "no mission
  forces an airframe".
  **Affected content:** mission-only airframe identification and every
  mission's forced configuration.
  **Resolving tasks:** the F39 mission-language stage and the F13 mission-opcode
  stage.
- **`player_pfighter`'s root is nested.** Its declared `SceneRootRef` names a
  node the container does not hold as a root.
  **Affected content:** the pirate fighter's scene root, and the audit's
  verdict for that one row. **Resolving task:** #398 (F11-E) plus a production
  path that converts `ZBD/planes.zbd`; `AirframeRoster::audit` then reports
  `RootMissing` for it on its own.
- **`ZBD/planes.zbd` still does not convert.** `SceneGraph::build` refuses it at
  node 640 (`brigturret2 `, a trailing space in the stored name), and all eight
  world containers refuse with `InconsistentParentage`. This stage therefore
  **did not** "turn today's nine `NodeArrayUndecoded` verdicts into per-airframe
  mappings" in the sense of producing sockets: it turned nine container-level
  verdicts into **eleven per-airframe verdicts**, each naming the same blocker.
  **Affected content:** every airframe's socket mapping.
  **Resolving tasks:** the two follow-ups #392's findings file for the id
  grammar's name rule and the world containers' partial child lists.
- **The declaration table is caller data, and it is not committed.** It lives in
  the acceptance test with the measured offsets, which is what makes the claim
  auditable but also means the engine has no shipped idiom: a consumer other
  than the acceptance suite must supply the table itself. **Affected content:**
  the discovery's reachability. **Resolving task:** #398 (F11-E) plus a
  `cs-inspect` surface when the owner wants one.
- **`discover_airframe_roster` is a library path.** The acceptance suite and the
  evidence harness drive it; nothing in the running binary and no `cs-inspect`
  subcommand invokes it, so there is no reachability evidence beyond the tests.
  **Affected content:** the discovery's own reachability. **Resolving task:**
  #398 (F11-E).
- **`required_roles` is declared, not derived.** Only the cockpit requirement is
  evidence-backed here (by `support\util\planesurgery.gw`). Guns, rocket
  mounts, engines, control surfaces, camera anchors and damage zones are not
  declared, so the audit says nothing about them. `support\cockpit.gw` does
  name authored sub-nodes per airframe (`gungauge`, `missilegauge`,
  `ggindicator0`–`3`, `mgindicator0`–`7`, `rightwingdamage`,
  `leftwingdamage`, `taildamage`, `nosedamage`), and they are a measured lead
  for the part-binding stage — **not** used here, because a name is not a role.
  **Affected content:** every part binding of every airframe.
  **Resolving task:** the F29 zones/armor stage together with F11-C's name-path
  binding rules.
- **Evidence class.** The roster is derived from original bytes by production
  readers, which makes it `observed_tool`. `retail` is file access, not proof
  that the original executable ran; the loading-script semantics this stage
  reads (`set`, `source`, `NewObject3D`, `GameZWriteZBDFile`, `%NAME%`) are the
  corpus's own spelling and were not verified against the running engine. No
  original run happened and nothing claims `verified_original`.
- **Independent review is outstanding for this work.** The F11 format work
  (`#392`) and this roster discovery are both original-data semantics, which
  AGENTS.md asks a different agent instance or model with a fresh context to
  review. No agent review replaces the owner's approval, and no agent review
  awards more than `checked`.
- **Fixture scope.** The four unignored tests are synthetic and newly
  authored; only the `#[ignore]`d retail test reads original data. Nothing
  derived from it beyond ids, counts, offsets and digests is committed.
- **`MULTIPLAYERLOBBY_PLANE.SCRIPT` / `MULTIPLAYER_PLANEDEF.SCRIPT`** in
  `GOSDATA/ASSETS/crimson.rof` are a lead, not a source: read during research,
  they spell an eleven-entry multiplayer plane list whose contents are fetched
  from native callbacks (`callback($$E$$, 5029, …)`), so they corroborate the
  *count* and prove nothing about this roster's identities. **Not** used by any
  production path and **not** relied on here.

## Sources used

- `specs/F11-scene-hierarchy-aircraft-parts-sockets-and-lod.md` (deliverable,
  non-negotiable behaviors 1–5, AC01–AC04).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, provenance, explicit
  unknowns, exact lookup, no positional resolution).
- `docs/contracts/CLI-EVIDENCE.md` and `schemas/evidence.schema.json`.
- `docs/findings/2026-09-30-f11-d-private-roster-audit.md` (the instrument, the
  empty-roster limitation this stage addresses, and the audit's own semantics).
- `docs/findings/2026-10-02-gamez-node-array-layout.md` (#392: the node array
  reads, `planes.zbd` converts as records, the `brigturret2 ` build refusal and
  the world containers' `InconsistentParentage`).
- `docs/findings/2026-09-28-f07-d-retail-opcode-classes.md` (the loading-script
  container, its 98 scripts, and the precedent that an idiom is caller-supplied
  data rather than an embedded rule).
- `docs/findings/2026-09-29-f11-a-node-hierarchy-bindings.md` (the `scene_node`
  id scheme and the rule that an unspellable name crosses over verbatim).
- `crates/cs_formats::interp` (`decode_interp`, tokens as stored bytes),
  `crates/cs_formats::gamez` (`read_gamez_nodes`, `read_gamez_meshes`,
  `read_gamez_materials`), `cs_types::install::RelativePath`,
  `cs_content::catalog::baseline::install_file_key` and
  `cs_assets::install` (production discovery and the two fingerprints).
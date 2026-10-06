# #678: a mission's startup world actors and animation records, joined and refused

Date: 2026-10-05. Task: #678 (`M01-LC-ACTOR-ANIM-PLAYBACK`), the step
`VS-M01-RUNTIME` (#359) waits on. Feature sheet:
`specs/F20-object-animation-and-authored-destruction-states.md`, stage
`### F20-D` (non-negotiable behavior 2). Shared contract:
`docs/contracts/IDENTITY-CONTENT.md`. Capabilities used: **`retail`**
(read-only access to `$CS_GAME_DIR`) and ordinary build/test. `gpu` and
`audio` were available and **not used**: nothing is rendered or played and no
original run happened, so nothing here is `verified_original`.

## Files

- `crates/cs_app/src/animation/mission.rs` (new, an F20 owner path): the
  consumer — `bind_mission_animation`, `MissionAnimationBinding`,
  `MissionAnimationRun`, `StartupAnimation`, `join_startup_animation`,
  `AnimationRecordFacts`, `RecordSequence`, `RecordResolution`, `AnimationTarget`,
  `TargetSource`, `TargetResolution`, `PlayRefusal`, `WorldActorPlacement`,
  `CarrierFact`, `MissionAnimationError`, the four reason constants and the
  three claim ids — plus the in-module `accept_m01_lc_actor_anim_playback_`
  test that covers `run`, the one seam the integration file cannot reach
  without an installation (the binding's fields stay private).
- `crates/cs_app/src/animation/mod.rs` (wiring only): `pub mod mission;`, the
  re-exports and the module-documentation paragraph.
- `crates/cs_app/tests/accept_m01_lc_actor_anim_playback.rs` (new, an F20 owner
  path): nine `accept_m01_lc_actor_anim_playback_` tests, seven synthetic and
  two retail — ten with the in-module one.
- This file.

**No reader refusal was weakened.** `git diff` touches no line of
`crates/cs_formats`, `crates/cs_types`, `crates/cs_assets` or `cs_content`; the
`.zrd` grammar is the existing `cs_content::stunts::decode_zrd`, the member walk
the existing `cs_formats::script_raw::discover_container`, the record walk the
existing `cs_formats::zbd::AnimationPayload::records`, the identity join the
existing `cs_app::animation::carrier::bind_startup_identities`, and the world
container the existing `cs_app::world::retail`.

## What this stage changed

The three stages before it measured halves and stopped at each half's edge.
`programs` resolved a startup animation name to the member that declares it;
`carrier` bound the same name to one record of the mission or camera carrier;
`cs_formats::zbd` walked those records. **Nothing joined the two**, nothing asked
which world nodes a record addresses, and nothing decided — per animation, with
a source locator — whether it can be played. A mission had no consumer for any
of it.

This stage is that consumer. `bind_mission_animation(install_root, scope)` reads
one mission scope's three reader archives, its two animation carriers and its
world group container, joins them, and answers three questions with measured
facts: which member declares each startup animation and which carrier record
stores it; which world nodes those declarations and records address; and which
world actors the mission's own archive places at startup.

## The two name agreements, and the one disagreement

A record and a declaring member are paired by the identity rule #650 measured
(an `anim_name` equal, byte for byte, to exactly one record across the two
carriers). That rule alone would pair a record with **any** member whose name
matches. Two further agreements are what make the pair one animation, and both
are measurements, not assumptions:

1. the record's `object_name` is **one of** the declaration's object selectors;
2. the declaration's sequence names, in order, are the record's **ordinary**
   sequence block names, in order — an unnamed `.zrd` sequence agreeing with an
   empty stored name.

Census over M01's whole closure (`zbd/c1c/m01` + `zbd/c1c` + `zbd`, against
`zbd/c1c/m01/mis_anim.zbd` and `zbd/c1c/cam_anim.zbd`), measured with the
production readers:

| quantity | value |
| --- | ---: |
| definition sites read across the three archives | 896 |
| …declaring an `ANIMATION_NAME` | 395 |
| records in the two carriers | 880 (573 mission + 307 camera) |
| declaration/record pairs (exactly one record per name) | **280** |
| names with no record in either carrier | 115 |
| names with more than one record | **0** |
| sequence-name agreements | **280** |
| object-name agreements | **279** |
| `root_name == object_name` | **280** |

**The single object disagreement is `agyro_rotors`**: declared by
`zbd/zrdr.zbd::autogyro_bus.zrd` over `agyrobus`, stored over `autogyro` in the
camera carrier. It is reported by `PlayRefusal::ObjectNameDisagrees` with both
spellings side by side and is **never repaired** — picking between two spellings
the original stored is a guess, and a consumer that quietly repaired it would
hide the only interesting record in the closure. The census pins it by name.

`root_name == object_name` in all 280 pairs is kept as its own field rather than
folded into `object_name`, because the equality is a measurement and a future
record could break it.

## Why nothing is played, and why refusing is the answer

A record's sequence blocks are raw byte streams. No event layout is measured for
this installation, and the pinned MechWarrior 3 grammar is explicitly not
assumed to apply (F20 non-negotiable behavior 2). So there is no statement, no
tick and no pose to play: `StartupAnimation::is_playable()` is `false` for every
record the installation holds, and each one carries
`PlayRefusal::EventsNotDecoded` with `f20-anim.sequence-event-stream-not-decoded`
and the record's own `SourceSpan`.

This is the consumer's measured answer rather than a stub or a placeholder. The
mission's startup set **is** established — which member declares each animation,
which record stores it, which world nodes it addresses, which animations it calls
— and every record is refused by name, with the reason and the bytes to read. A
consumer that reported "played" would be reporting a duration and a pose nobody
measured, which is exactly the failure F20 behavior 2 exists to prevent. The
five tests that pin this are the ones a reviewer should check first, because the
alternative — a `play` that returns success — would be the tempting wrong turn.

M01's seven rows, all joined and all refused, measured by
`accept_m01_lc_actor_anim_playback_retail_m01_startup_animations_are_joined_and_refused`:

| event | animation | declaring member | carrier | record |
| --- | --- | --- | --- | ---: |
| `NEW_GAME_START` | `generic_intro` | `zbd/zrdr.zrd::generic_intro.zrd` | camera | 23 |
| `NEW_GAME_START` | `wv_hookup_state` | `zbd/c1c/m01/zrdr.zbd::wv_tailhook.zrd` | mission | 496 |
| `NEW_GAME_START` | `pzep_engines_start` | `zbd/zrdr.zbd::pirate_zep_nacelles.zrd` | mission | 36 |
| `NEW_GAME_START` | `wvzep_engines_start` | `zbd/zrdr.zbd::wv_zep_nacelles.zrd` | mission | 414 |
| `NEW_GAME_START` | `bszep_engines_start` | `zbd/zrdr.zbd::bswan_zep_nacelles.zrd` | mission | 230 |
| `NEW_GAME_START` | `call_add_jack` | `zbd/zrdr.zbd::passengers.zrd` | camera | 60 |
| `LOAD_GAME_START` | `player_setup` | `zbd/zrdr.zbd::player_setup.zrd` | camera | 115 |

Seven identities, seven resolving members, seven bound records, **zero**
unresolved and **zero** ambiguous, both name agreements holding for all seven,
and one refusal each — the event gap.

## What the records carry that the declarations do not

The payload half names objects the `.zrd` half does not, which is the part of
this stage with no predecessor:

* **Node-reference tables.** `generic_intro` (camera record 23) names thirteen
  nodes: `player`, `piratezep`, `world1`, `camera1`, `player_balmoral`,
  `player_warhawk`, `piratefighter`, `interior`, `front_door_left`,
  `front_door_right`, `healthy`, `cockpit1` (plus the empty zero entry). Every
  one resolves to a record of `ZBD/C1C/gamez.zbd`. The camera animation's
  driver sees the whole scene through these names, and no `.zrd` member states
  them.
* **Animation-reference tables.** `generic_intro` calls `apzep_engines_start`,
  `letterbox`, `gi_scene1`, `gi_scene2`, `gi_1stperson` and `gi_playerdrop` —
  the intro's call graph, by name, read from the payload rather than inferred
  from the declarations.
* **Sequence block lengths.** Every block's event-stream length is a measured
  number, and each record's `reset_time` is read verbatim (`-1.0` on M01's five
  non-camera records, `0.0` on `generic_intro` and `player_setup`). The **unit**
  of `reset_time` is unmeasured, so it is reported and never applied.

## The world-actor half

The mission's own archive declares three `ON_STARTUP` definitions in
`placezeps.zrd`, naming `piratezep`, `workersvoyagezep` and `blackswanzep`. Each
resolves to exactly one record of `ZBD/C1C/gamez.zbd`, so the actors'
**identity** is measured. Their **placement** is refused under
`f20-anim.placement-member-fields-undecoded`: the placement member's `node` /
`position` / `yaw` / `pitch` / `max_speed` / `max_accel` fields are F33-D's
undecoded carrier and the world's own unit is #436's open measurement.

The consumer deliberately takes placements from the **mission archive only**.
The world group and the shared root also declare `ON_STARTUP` definitions, but
those describe content placed elsewhere; #632 measured that scope choice, and a
consumer that widened it would report actors that are not this mission's.

## Design decisions

- **Two agreements, checked per row, not a census constant.** `join_startup_animation`
  compares the two names on every row it joins, so a corpus-wide 280/280 does not
  make a future disagreement invisible. The census test states the corpus figure;
  the join test states that a disagreement is refused.
- **A disagreement is a refusal with both lists, not a repair.** `agyro_rotors`
  keeps `["agyrobus"]` and `"autogyro"` in the same refusal value. Repairing
  either side would make the census's 279/280 impossible to observe.
- **An empty name is unreadable, not a zero match.** The first entry of every
  non-empty objects/nodes table is empty (measured over all 15 024 records by
  #650). The consumer keeps those entries with
  `TargetResolution::Unreadable { .. }` rather than counting them as selecting
  nothing, so "the zero entry" and "a name no world record carries" stay
  distinct — the same rule `WorldNodeNames` follows for a `None` count.
- **No world container means "unreadable", never "nothing there".** A target
  resolved with no container is `Unreadable`, so a caller without a world can
  never mistake "I could not look" for "nothing is there".
- **Four failures stay four.** `Undeclared`, `AmbiguousDeclaration`, `NoRecord`
  and the two name disagreements have distinct labels and distinct reasons. A
  consumer that collapsed "nobody declares it" into "no record stores it" would
  report a missing payload where the data says there is no declaration.
- **A refused carrier is a row, not an `Err`.** A missing carrier leaves its
  identities `NoRecord { UNBOUND_REASON_NOT_WALKED }` and its fact on the
  binding with a blocker. Only a failure to *read the installation at all*, a
  scope with no `startanims.zrd`, or an unreadable world container is an `Err`.
- **An absent startup event is measured content.** `MissionAnimationBinding::run`
  answers an undeclared event with an empty run, because 29 of 53 retail
  `LOAD_GAME_START` records name zero animations.

## Test inventory

| `accept_m01_lc_actor_anim_playback_` test | Covers | Fails when |
| --- | --- | --- |
| `a_pair_is_joint_only_when_both_names_agree` | **the observable failure**: the measured `pzep_engines_start` pair agreeing on both names and being refused for exactly the event gap; the measured `agyro_rotors` object disagreement reported with both spellings; a reordered sequence list reported side by side | a consumer pairs halves on the identity alone, repairs a disagreement, or reports a pair as playable |
| `an_unnamed_sequence_agrees_with_an_empty_stored_name` | `wv_hookup_state`'s measured shape: an unnamed `.zrd` sequence agreeing with an empty stored block name | an empty stored name is treated as "no name" and the pair breaks |
| `the_four_failures_stay_distinct` | each of undeclared, ambiguous declaration, unbound and ambiguous record refused with its own label, reason and match list; every match kept; only the event gap carrying a claim | failures collapse, a winner is picked, or a content mismatch is filed as a claim |
| `every_name_keeps_its_source_and_its_resolution` | all four `TargetSource` values in join order; the six collected names of one row; the three real names resolving to one record each; the empty zero entry kept as `Unreadable`; no container meaning `Unreadable`, never zero | a name loses its source, an entry is dropped, or "no container" becomes "nothing there" |
| `a_wildcard_reports_its_reach_and_a_narrowing_suffix_reports_none` | `lbroad*` counting three records; `lbroad*1` reported with **no** count; a literal counting one | a narrowing suffix is applied as a prefix and over-selects silently |
| `a_placement_names_its_world_actor_and_refuses_to_place_it` | the placement record's identity, member, definition index, three resolved selectors, its claim id and its refusal reason | a placement starts spawning, or its identity stops being measured |
| `the_rows_keep_their_events_in_stored_order` | the startup table read through the production reader; the joined rows' identities in stored order; refusal counts 1 / 2 / 2; the bound row's carrier and record index | identities lose their event order, or a joined row and an unjoined row are indistinguishable |
| `a_run_filters_one_event_and_reports_its_rows` (in-module) | `MissionAnimationBinding::run`: one event's rows in stored order, `playable`/`refused` split, an undeclared event an empty run; `startup_of`, `world_targets`, `playable_count`, `refused_count` | `run` drops its event filter, a refused row reports playable, or `world_targets` loses or re-pairs a target |
| `retail_m01_startup_animations_are_joined_and_refused` (retail) | M01's three archives, world container, both carriers walked with their declared counts, the seven identities and their exact member/carrier/record, seven single refusals, every record's span naming its own carrier, every world name resolving, `generic_intro`'s node and animation-reference tables and its six agreeing sequence names, the three placements | a binding stops resolving, a member or record moves, a refusal count changes, or a placement is lost |
| `retail_the_closure_has_one_object_disagreement` (retail) | 896 definition sites, 395 named declarations, 880 records, 280 pairs, 0 ambiguous, 280 sequence agreements, 279 object agreements and `agyro_rotors` named as the single disagreement | a measured count moves, or the disagreement is repaired into an agreement |

**Sensitivity.** Seven mutations were applied to `mission.rs` and the
non-retail selection (`--skip retail`) re-run with the source restored each
time. **All seven are killed by a test CI can run**, so none relies on the
`#[ignore]`d cases:

| mutation | killed by |
| --- | --- |
| the object agreement is not checked | `a_pair_is_joint_only_when_both_names_agree` |
| the sequence agreement is not checked | the same test's reorder arm |
| an unnamed sequence is compared against `None` rather than `""` | `an_unnamed_sequence_agrees_with_an_empty_stored_name` |
| the two name failures are collapsed into one label | `the_four_failures_stay_distinct` |
| the empty zero entry is dropped instead of kept | `every_name_keeps_its_source_and_its_resolution` |
| a target resolved with no world counts as zero | the same test's no-container arm |
| `is_playable()` ignores the refusals | `the_rows_keep_their_events_in_stored_order` |

The behaviours only the retail cases check are the corpus counts themselves
(896 / 395 / 880 / 280 / 279), M01's seven member-carrier-record rows, the two
specific refusals (`agyro_rotors`, and the three placements) and the byte
spans; every **rule** above is covered without the installation.

## Unknowns and limitations (recorded, not guessed)

- **No record can be played.** The event stream is undecoded, so there is no
  duration, no pose, no marker and no statement. **Affected content:** every
  claim that M01's startup animations move, show or call anything. **Resolving
  task:** an event-grammar task; F20-D's family validation, M01-B and
  `VS-M01-RUNTIME` all need it. Until then `is_playable()` is `false` for the
  whole installation and that is the honest reading.
- **`reset_time`, `max_health`, the flag word, the status / activation /
  priority bytes** are read and reported, with their meanings unmeasured. No
  duration, weight or ordering is read out of any of them.
- **`agyro_rotors` is unexplained.** `agyrobus` declared, `autogyro` stored:
  whether the original renamed the bus, stored a family name, or the record is
  simply mismatched in the installation is not established. **Affected content:**
  that one animation; the consumer reports it and plays nothing either way.
- **A record's table entries after their names are unmeasured.** A node entry is
  a 4-byte flags word and a 32-byte name; the object's 92-byte body beyond its
  name is not interpreted. The ids inside entries (equal for the same object
  across tables, looking like world-node ids) stay unmeasured.
- **Placement stays refused.** No spawn position, heading or motion limit is read
  from `placezeps.zrd` / `zeppelins.zrd`, and the world's unit is #436's
  measurement. **Affected content:** every placed actor's pose and route.
- **Only M01 is resolved by a test.** The closure census covers M01's group and
  the shared root, but the 52 other mission scopes are not bound row by row.
  **Affected content:** the per-mission bindings. **Resolving task:** F50.
- **The event byte lengths are not times.** A block's `event_bytes` is a length;
  nothing converts it into ticks.
- **Evidence class.** The layouts, counts and relations above are
  `ObservedTool` + measurement: read out of the original bytes by the production
  readers. Every **rule** carries its own claim id, and the three claim ids are
  `ClaimStatus::ObservedTool`, never `verified_original`. No original
  executable was run: `retail` is file access, not evidence of runtime
  behaviour.
- **Nothing derived from the original bytes is committed.** The numbers above
  are counts, dimensions and relations; no member payload, no name list beyond
  what a reviewer needs to check a binding, and no screenshot is in the
  repository.

## Follow-ups filed

One task is filed for the event grammar (the gap that blocks playing anything).
The remaining questions above name their resolving tasks and are recorded here
rather than filed, because filing them would duplicate work those tasks own.

## Sources used

- `crates/cs_app/src/animation/programs.rs` (`read_startup_animations`,
  `read_animation_definition_member`, `WorldActorProgramBinding`,
  `WorldNodeNames`, `SelectorMatch`) and
  `crates/cs_app/src/animation/carrier.rs` (`bind_animation_carrier`,
  `bind_startup_identities`, `StartupOutcome`, `UNBOUND_REASON_*`).
- `crates/cs_formats/src/zbd/anim.rs` (`read_animation_index`,
  `AnimationPayload::records`, `AnimationRecord`, `AnimationRecordSequence`) and
  `crates/cs_formats/src/script_raw/discovery.rs` (`discover_container`).
- `crates/cs_app/src/world/retail.rs` (`read_world_containers`,
  `RetailWorldContainer::nodes`) and `crates/cs_formats/src/gamez/nodes.rs`.
- `docs/findings/2026-10-04-m01-lc-world-actors.md` (the member → actor binding
  and the M01 closure this stage re-measures) and
  `docs/findings/2026-10-05-m01-lc-anim-records.md` (the record walk, the
  15 024-record census and the empty-zero-entry measurement).
- `docs/contracts/IDENTITY-CONTENT.md` (stable ids, `Provenance`, evidence
  classes).

## Commands run

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_m01_lc_actor_anim_playback_ --include-ignored
#   10 tests: 8 run and pass (one in-module), 2 retail run and pass (about 75
#   seconds; each does production discovery passes over the installation)
```
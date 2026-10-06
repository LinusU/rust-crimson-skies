# #690: the animation record event stream — the grammar, the census and the duration

Date: 2026-10-06. Task: #690 (`F20-EVENT-GRAMMAR`), the gap #650 and #678 named
as the reason no original animation record could be played. Feature sheet:
`specs/F20-object-animation-and-authored-destruction-states.md`, non-negotiable
behavior 2. Shared contract: `docs/contracts/IDENTITY-CONTENT.md`. Capabilities
used: **`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary
build/test. `gpu` and `audio` were available and **not used**: nothing was
rendered or played, no original executable was run, so nothing here is
`verified_original`, and `retail` is file access rather than evidence of how
the 2000 engine behaved.

## Files

- `crates/cs_app/src/animation/events.rs` (new, an F20 owner path): the grammar
  — `walk_event_stream`, `decode_event_stream`, `sequence_duration`,
  `RawEvent`, `DecodedEvent`, `EventStreamError`, `OpcodeInfo`,
  `STORED_OPCODES`, `EventClass`, `StartTimeEvidence`, `RunTimeEvidence`, the
  four claim ids and their reason texts.
- `crates/cs_app/src/animation/mission.rs` (an F20 owner path): the consumer —
  `SequenceEvents`, `PlaybackGap`, `RecordPlayback`, `PlaybackSequence`,
  `TickPose`, `PoseStatement`, `AnimationRecordFacts::playback`,
  `StartupAnimation::playback`, and the two new refusals
  (`PlayRefusal::EventOpcodeUndecoded`, `PlayRefusal::EventTimingUndecoded`)
  beside the existing `EventsNotDecoded`.
- `crates/cs_app/src/animation/mod.rs` (wiring only): `pub mod events;` and the
  re-exports.
- `crates/cs_app/tests/accept_m01_lc_actor_anim_playback.rs`: the nine
  `accept_f20_event_` tests (six synthetic, three retail — the third,
  `retail_every_opcode_spelling_is_read_from_a_declaration`, was added in
  review; see the review section below).
- `crates/cs_app/tests/accept_f20_d_validation.rs`: #690's evidence harness
  (`evidence_report_f20_event_grammar_writes_the_acceptance_report` and
  `f20e_census_json`), beside the harnesses #633 and #650 already used. The
  shared `M01lcReport` census callback gained a `&Path` (the game dir)
  parameter so a census can re-read the installation; the two earlier census
  functions ignore it, and nothing else of theirs changed.
- This file.

**No reader was weakened.** `cs_formats` is untouched: the walk of the records
(#650) still hands every block's bytes over raw, and every refusal this task
*lifts* was lifted by decoding those bytes rather than by relaxing a check.
Two #678 tests changed because their subject changed: the retail row set went
from "seven joined and refused" to "seven joined and played", and the retail
test was renamed `…_retail_m01_startup_animations_are_joined_and_played` to say
what it now checks. Its sister test `…_retail_the_closure_has_one_object_disagreement`
and all six synthetic join tests are unchanged, so the two name agreements are
still the checks they were.

## The layout (acceptance criterion 1)

A sequence block's event stream is a list of records, each

| field | bytes | measured as |
| --- | --- | --- |
| tag word | 4 | low byte = opcode; second byte = `1`, `2` or `3`; high half = 0 |
| length word | 4 | the record's **own** byte length, header included, a multiple of four and at least 8 |
| payload | `length - 8` | word `0` = `START_TIME`; last word = `RUN_TIME` where measured; everything else raw |

Stepping by `length` consumes the block exactly and lands on the next tag. That
single rule reproduces **all 56 994 blocks** of the installation's 61 carriers
(mission and camera), **242 391 events** and **16 132 948 bytes**, with zero
structural refusals — the same standard #650 applied to the record walk: a
recurrence confirmed over the whole corpus, not fitted to the first records.
`accept_f20_event_retail_every_event_stream_walks_and_is_censused` asserts
every one of those four numbers plus, per block, that the stepped lengths sum
to the block's stored size.

The stream is **not** assumed to be the MechWarrior 3 grammar: nothing here
comes from the pinned upstream source. What fixes the layout is that it walks
the retail corpus exactly, and what fixes the *meaning of an opcode* is the
next section.

## Each opcode is one authored statement (acceptance criterion 2)

A declaration's `SEQUENCE_DEFINITION` lists statements (`OBJECT_MOTION_FROM_TO`,
`CALL_ANIMATION`, `IF`, …) in stored order, and #678 already measured that a
declaration's sequence names, in order, are a bound record's ordinary sequence
block names, in order. Aligning the *statements* of one such sequence with the
*events* of its block gives a direct opcode → statement join, once the
declaration's leading `ACTIVATION` statement is dropped: **the payload does not
store it as an event** (it is the only statement that appears 1 119 times as a
first statement, and the only drop needed — 1 055 sequences align without it
and 385 align exactly after removing it).

Measured over **477 declaration/record pairs and 1 440 sequences**: every
aligned pair has the same length, and the resulting opcode → statement mapping
has **zero conflicts** — no opcode ever joins two spellings. (`OBJECT_MOTION_SI_SCRIPT`
is the one spelling with two opcodes, 12 and 47; that direction is fine and is
recorded as such.)

So an opcode's name is *the installation's own text*, not a MechWarrior name
and not a guess. The five classes the task asks for are **this project's
grouping** of those spellings, stated in `EventClass` so a reader can see the
rule rather than infer it:

| class | opcodes |
| --- | --- |
| motion | 7, 8, 9, 10, 11, 12, 14, 47 |
| state | 4, 5, 6, 15, 16, 20, 36, 42 |
| marker | 35, 41 |
| sound | 1, 2, 46 |
| control | 22, 23, 24, 25, 27, 30, 31, 32, 33, 34 |
| unknown | 13, 17, 26, 28 |

### The census, over all 242 391 events

| opcode | statement (the installation's spelling) | class | events | `START_TIME` | `RUN_TIME` |
| ---: | --- | --- | ---: | --- | --- |
| 1 | SOUND | sound | 6 398 | value match | absent |
| 2 | SOUND_NODE | sound | 1 244 | absent | absent |
| 4 | LIGHT_STATE | state | 1 468 | vocabulary | absent |
| 5 | LIGHT_ANIMATION | state | 535 | vocabulary | **unmatched** |
| 6 | OBJECT_ACTIVE_STATE | state | 68 109 | value match | absent |
| 7 | OBJECT_TRANSLATE_STATE | motion | 1 143 | absent | absent |
| 8 | OBJECT_SCALE_STATE | motion | 2 209 | absent | absent |
| 9 | OBJECT_ROTATE_STATE | motion | 6 610 | vocabulary | absent |
| 10 | OBJECT_MOTION | motion | 7 458 | value match | value match |
| 11 | OBJECT_MOTION_FROM_TO | motion | 9 421 | value match | value match |
| 12 | OBJECT_MOTION_SI_SCRIPT | motion | 845 | value match | absent |
| 13 | *(joins no statement)* | unknown | 340 | unmeasured | unmeasured |
| 14 | OBJECT_OPACITY_FROM_TO | motion | 9 917 | value match | value match |
| 15 | OBJECT_ADD_CHILD | state | 1 163 | absent | absent |
| 16 | OBJECT_DELETE_CHILD | state | 263 | absent | absent |
| 17 | *(joins no statement)* | unknown | 144 | unmeasured | unmeasured |
| 20 | CAMERA_STATE | state | 10 | absent | absent |
| 22 | CALL_SEQUENCE | control | 22 391 | vocabulary | absent |
| 23 | STOP_SEQUENCE | control | 1 143 | vocabulary | absent |
| 24 | CALL_ANIMATION | control | 56 750 | value match | absent |
| 25 | STOP_ANIMATION | control | 7 928 | vocabulary | absent |
| 26 | *(joins no statement)* | unknown | 11 | unmeasured | unmeasured |
| 27 | INVALIDATE_ANIMATION | control | 6 618 | value match | absent |
| 28 | *(joins no statement)* | unknown | 1 | unmeasured | unmeasured |
| 30 | LOOP | control | 3 475 | value match | absent |
| 31 | IF | control | 5 468 | absent | absent |
| 32 | ELSE | control | 4 680 | absent | absent |
| 33 | ELSEIF | control | 5 727 | absent | absent |
| 34 | ENDIF | control | 5 468 | absent | absent |
| 35 | CALLBACK | marker | 736 | vocabulary | absent |
| 36 | FBFX_COLOR_FROM_TO | state | 155 | value match | value match |
| 41 | DETONATE_WEAPON | marker | 3 | absent | absent |
| 42 | PUFFER_STATE | state | 4 535 | vocabulary | absent |
| 46 | SOUND_ADJUST | sound | 3 | absent | value match |
| 47 | OBJECT_MOTION_SI_SCRIPT | motion | 22 | absent | absent |

Class totals: control 119 648, state 76 238, motion 37 625, sound 7 645,
marker 739, **unknown 496**. Both totals are asserted by the retail test and
written into `docs/findings/evidence/F20-EVENT-GRAMMAR.json`.

### The four unknowns, by name (criterion 2's gap requirement)

* **Claim:** `f20-anim.event-opcode-not-measured`.
* **What is unknown:** opcodes **13, 17, 26 and 28** join no statement of any
  declaration in the installation. Their class, timing and effect are
  unmeasured; no name is borrowed for them.
* **Affected content:** 496 events in **389 of the 56 994 blocks**, inside
  **347 records** of 16 carriers (211 of the blocks are reset blocks, 178
  ordinary). Every record carrying one stays refused — including if it were a
  startup identity, which M01's seven are not.
* **Resolving task:** an original run that shows what such a record does, or
  the original's own source.

* **Claim:** `f20-anim.event-run-time-position-unmeasured`.
* **What is unknown:** opcode **5** (`LIGHT_ANIMATION`) states a `RUN_TIME` in
  340 of the installation's declarations, but no joined statement of that
  opcode exists, so the position of that word in its 104-byte payload was never
  value-matched. Reading it anyway would produce a *short* duration that looks
  measured.
* **Affected content:** 535 events in **182 blocks** and **158 records**
  (chiefly `c1/m05`'s boat-destruction set).
* **Resolving task:** the same — a joined statement, an original run, or the
  original's source.

Both are refused by `PlaybackGap`, so neither can reach a duration.

## The two timing fields

* **`START_TIME` is payload word 0.** Joined statements of ten opcodes (1, 6,
  10, 11, 12, 14, 24, 27, 30, 36) state a `START_TIME`, and word `0` equals it
  **70 of 70** times (values such as `0.1`, `1`, `5`, `15`). No joined statement
  *without* a `START_TIME` has a nonzero word `0` (1 114 joined statements,
  zero contradictions). Across the whole corpus, word `0` is nonzero for 18
  opcodes, and **every one of them** has a statement vocabulary that can carry
  `START_TIME`; the 17 opcodes that never store a nonzero word `0` store it
  only where the same vocabulary allows it — the one asymmetry is opcode 47,
  which shares `OBJECT_MOTION_SI_SCRIPT` with opcode 12 and still stores only
  zeros, so its evidence is reported as `absent` rather than `value_match`.
  That is a corpus-wide correlation, not a fit.
  The eight opcodes whose word `0` is nonzero but never value-matched (4, 5, 9,
  22, 23, 25, 35, 42) are reported as `vocabulary` evidence rather than
  `value_match`, so the difference is visible in the census above.
* **`RUN_TIME` is the payload's last word.** Joined statements of opcodes 10,
  11, 14, 36 and 46 state a `RUN_TIME`, and the last word equals it **109 of
  109** times, with no mismatch. An opcode whose statement vocabulary never
  states a `RUN_TIME` contributes zero — measured from the declaration side,
  not from the payload's last word (that word is a node index for opcode 6, for
  example, so it is never read as a time).
* **Duration** is therefore `max(start_time + run_time)` over a record's
  decoded events, in the **original's stored time unit**. The unit is
  unmeasured (seconds is plausible, unverified), so the value is reported and
  never multiplied into ticks by this module.

## Per M01 startup record: what its events do (acceptance criterion 5)

All seven are joined (that half is #678's), all seven decode, all seven are
**playable** with `MissionAnimationBinding::playable_count() == 7` and
`refused_count() == 0`. Times are the original's stored unit.

| event | animation | carrier · record | blocks | events | what its events do (measured spelling, count, start) | duration | still unknown |
| --- | --- | --- | --- | ---: | --- | ---: | --- |
| NEW_GAME_START | `pzep_engines_start` | mission · 36 | `call_eachengine` | 12 | 12 × `CALL_ANIMATION` at 0.0 — it calls the per-engine animations and moves nothing itself | 0.0 | what the called animations do; the target fields inside each payload |
| NEW_GAME_START | `wvzep_engines_start` | mission · 414 | `call_eachengine` | 18 | 18 × `CALL_ANIMATION` at 0.0 | 0.0 | as above |
| NEW_GAME_START | `bszep_engines_start` | mission · 230 | `call_eachengine` | 14 | 14 × `CALL_ANIMATION` at 0.0 | 0.0 | as above |
| NEW_GAME_START | `wv_hookup_state` | mission · 496 | one unnamed sequence | 3 | 3 × `CALL_ANIMATION`, two at 0.0 and one at 5.0 | 5.0 | which nodes the later call addresses |
| NEW_GAME_START | `generic_intro` | camera · 23 | reset + `callback_sequence`, `start_script`, `check_balmoral`, `check_warhawk`, `drop_planes` | 46 | 8 × `CALLBACK`, 7 × `CALL_ANIMATION`, 5 × `CALL_SEQUENCE`, 6 × `OBJECT_ACTIVE_STATE`, 5 × `OBJECT_DELETE_CHILD`, 3 × `OBJECT_ADD_CHILD`, 4 × `STOP_ANIMATION` at **1.95, 9.9, 12.4, 15.9**, 2 × `OBJECT_ROTATE_STATE`, 2 × `IF`, 2 × `ELSE`, 2 × `ENDIF` — all the rest at 0.0 | **15.9** | what `IF` evaluates against, what each state selects |
| NEW_GAME_START | `call_add_jack` | camera · 60 | `add_j` | 1 | 1 × `CALL_ANIMATION` at 0.2 | 0.2 | the target |
| LOAD_GAME_START | `player_setup` | camera · 115 | reset + `callback_sequence` | 11 | 8 × `CALLBACK`, 2 × `OBJECT_ACTIVE_STATE`, 1 × `CALL_ANIMATION`, all at 0.0 | 0.0 | what the states select |

Reading that table is the answer to #678's open question: **M01's seven startup
"animations" are almost entirely call graphs.** Six of them call other
animations (the zeppelin engine sets are one `CALL_ANIMATION` per engine); only
`generic_intro` does state work of its own, and its whole visible timeline is
four `STOP_ANIMATION` statements at 1.95, 9.9, 12.4 and 15.9 — which is why its
duration is 15.9 and why the camera intro's length comes from stops rather than
from motion.

Reset and damage blocks are decoded with the same grammar (their opcodes are
the same vocabulary); they are not separately named in the table because none
of the seven except `generic_intro` and `player_setup` carries one.

## What is still not decoded

* **Payload bodies beyond the two timing fields.** No name field, node index,
  colour or vector is read out of an event. Consequently **no transform pose is
  produced from an event**: `PoseSample` needs metres and a quaternion, the
  stored unit is #436's open measurement, the original's animation tick rate is
  `f20-anim.tick-rate-unmeasured`, and a motion statement's interpolation is
  unmeasured. The per-tick report therefore says *which statements the record
  has started by that tick*, with the installation's own spelling and its
  measured span — not what the scene looks like. That is stated on
  `RecordPlayback::poses` itself so no caller can mistake it.
* **The tag's second byte** (`1`, `2`, `3`) is measured as a value and
  unmeasured as a meaning; it travels on every `DecodedEvent` as `group`.
* **The time unit** of `START_TIME` / `RUN_TIME` is unmeasured, so a duration
  is in the original's own unit and `poses(ticks_per_second)` takes the
  caller's rate as the caller's statement rather than a measurement.
* **What the original did** with any of this: no original executable ran, so
  every claim is `ObservedTool` and none is `verified_original`.

## Design decisions

- **One table, one answer.** `STORED_OPCODES` is the single source for a
  opcode's spelling, class and timing evidence; `opcode_info` searches it, so a
  census and a decode cannot disagree. The four unknowns are *in* the table
  (a census must see them) and are refused in `read_event` (a record must not
  play them).
- **Refuse, do not truncate.** `decode_event_stream` walks the whole block and
  refuses on the first unjoined opcode; a block that half-decodes is
  `SequenceEvents::Refused` with its byte offset, never `Decoded` with a
  hole. `AnimationRecordFacts::playback` then refuses the *record* if any of
  its blocks did, so partial decoding can never become a partial playback.
- **The unmatched `RUN_TIME` is a refusal, not a zero.** Opcode 5's payload
  ends in a plausible nonzero float, so reading it would have produced a
  convincing wrong duration; `RunTimeEvidence::Unmatched` refuses instead.
- **Two gap kinds, two labels, two claims.** `event_opcode_not_decoded` (4
  opcodes, nobody joins them) and `event_timing_not_decoded` (opcode 5, the
  position is unmeasured) stay separate so a report can tell "we do not know
  what this is" from "we know what this is but not when it ends".
- **A refusal keeps its bytes.** `PlaybackGap::offset` and both new
  `PlayRefusal` variants carry the offset inside the block, plus the claim id,
  so F20 behavior 2's source locator holds for the new refusals too.
- **The consumer reads the events it was given.** Facts built without bytes (a
  synthetic row) keep `SequenceEvents::Absent` and the original
  `EVENTS_NOT_DECODED_REASON`; the earlier synthetic join tests therefore
  still assert exactly what they asserted before.

## Test inventory

| `accept_f20_event_` test | Covers | Fails when |
| --- | --- | --- |
| `a_stream_walks_into_tag_and_length_records` | the two-word header, offsets, lengths, the measured `START_TIME`/`RUN_TIME` reads, and all five stream refusals with their codes, offsets and claims | the header layout changes, a timing field is read from the wrong word, or a structural failure stops being named |
| `a_decoded_duration_is_read_from_the_payloads` *(the guess detector)* | 5.5 = 2.0 + 3.5 from one stream, 9.0 = 2.0 + 7.0 from the same stream with one word changed, 0.0 and 2.5 from two single-event streams, and `sequence_duration` | a decoded duration is replaced by a constant or any guess |
| `an_undecoded_opcode_refuses_the_whole_record` | one good event then opcode 13: the block refuses at its byte, `playback()` is `None`, `is_playable()` is `false`, and the refusal carries opcode, offset and claim | an undecoded opcode is truncated away, or a decoded half reaches a playback |
| `an_unmatched_run_time_refuses_the_whole_record` | opcode 5 walks, then refuses with `event_timing_not_decoded` and its claim | the unmatched `RUN_TIME` is read as a number |
| `every_stored_opcode_is_classed_and_the_unknowns_named` | 35 stored opcodes, the four unknowns by code, every class present, the measured spellings, and `None` for codes no carrier stores | an opcode or a class vanishes from the table |
| `the_per_tick_report_follows_the_measured_timing` | the report at 2 Hz over a 2.0 record: which rows hold which statements, and a zero rate sampling nothing | the report ignores its timing or invents a timeline |
| `retail_every_event_stream_walks_and_is_censused` *(retail)* | 61 carriers, 56 994 blocks, 242 391 events, 16 132 948 bytes, the full opcode census and the six class totals, and 389 refused blocks | any number moves, or an unknown opcode is dropped from the census |
| `retail_every_opcode_spelling_is_read_from_a_declaration` *(retail, added in review)* | the naming claim itself: every `zrdr.zbd` declaration and every carrier record read through production readers, 477 unique-identity pairs, 1 440 aligned sequences, 5 340 events whose opcode's table spelling is asserted **against the declaration's own statement kind**, zero conflicts, and the joined set equal to the table's 31 joined entries | a spelling in `STORED_OPCODES` stops matching the installation's text, or the `ACTIVATION` alignment rule stops holding |
| `retail_m01_startup_records_play_with_measured_durations` *(retail)* | `playable_count() == 7`, each row's measured duration (15.9, 5.0, 0.2 and four zeros), each event's spelling and class, and `call_add_jack`'s first statement appearing only at tick 2 of a 10 Hz report | a duration becomes a guess, a row stops playing, or an unknown opcode reaches a playback |

## Sensitivity

Three mutations were applied to the implementation and the non-retail
selection re-run with the source restored afterwards. Every one is killed by a
test CI can run:

| mutation | killed by |
| --- | --- |
| the decoded duration replaced by a constant (`RecordPlayback::new` returns `0.0`) | `a_decoded_duration_is_read_from_the_payloads`, `the_per_tick_report_follows_the_measured_timing` |
| the unjoined-opcode check removed from `read_event`, so opcode 13 decodes | `an_undecoded_opcode_refuses_the_whole_record`, `a_stream_walks_into_tag_and_length_records` |
| `RUN_TIME` read from word `0` instead of the last word | `a_decoded_duration_is_read_from_the_payloads`, `a_stream_walks_into_tag_and_length_records` |

The three retail cases add what only the installation can check: the corpus
counts, the declaration join behind every opcode's spelling, and the seven
durations, all re-measured from `$CS_GAME_DIR` on every run rather than copied
from this document.

## Evidence classes

| claim | class | why |
| --- | --- | --- |
| the two-word event header, the walk, the corpus counts | `ObservedTool` | derived and checked over all 56 994 retail blocks by this repository |
| the opcode → statement join and the class mapping's inputs | `ObservedTool` | 477 declaration/record pairs, 1 440 sequences, zero conflicts; the statements' spellings are the installation's own text |
| `START_TIME` at word 0, `RUN_TIME` at the last word | `ObservedTool` | 70 and 109 joined statements with exact value matches, zero contradictions, plus the corpus-wide correlations above |
| the four unjoined opcodes, opcode 5's position, payload bodies, the tag's second byte, the time unit | **unknown** | named above with their claim ids and affected content |
| what the original engine did with any of it | **unknown** | no original run |

## Review (2026-10-06)

Reviewer: **bunny-alpha-2/bunny-alpha-2 — the same agent identity as the
implementer**, in a session that started from the implementer's hand-over
summary and read the branch diff from there. The context was therefore **not
fresh**, this is a self-review rather than independent evidence, and `checked`
is its ceiling; it cannot stand in for a fresh-context agent or for the owner's
human approval. What the review changed:

* **The naming claim became a test.** The opcode → statement join was measured
  during implementation (477 pairs) but no committed test re-derived it, so
  `STORED_OPCODES`' spellings were only ever asserted against themselves. The
  reviewer first reproduced the join independently from the implementer's
  private dump (1 055 sequences aligning directly, 385 after dropping the
  leading `ACTIVATION`, zero conflicts, the same 31 opcodes with the same
  spellings), then committed the measurement as
  `accept_f20_event_retail_every_opcode_spelling_is_read_from_a_declaration`,
  which reads the installation through the production readers and asserts each
  table spelling against the declaration's own statement kind — 477 pairs,
  1 440 sequences, 5 340 events. The finding's central claim is now checked
  rather than reported.
* **Three documentation statements corrected**, none of them a behaviour
  change: `TickPose::statements` said a row holds "every statement whose span
  covers this tick" while the measured behaviour (and its test) is "started by
  that tick"; `StartupAnimation::playback` said `None` "exactly when a refusal
  says why", which is not true for a row refused for a *name* disagreement;
  and `carrier` / `AnimationRecordFacts::animation_refs` still said the event
  streams and the call statements were not decoded, which #690 ended.
* **Evidence regenerated** on the reviewed commit: the report now lists nine
  `accept_f20_event_` tests and this review's identity and method.

## Commands run

```sh
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --workspace --locked
cargo test --workspace --locked -- accept_f20_event_ --include-ignored
#   8 tests: 6 synthetic run unignored, 2 retail run with CS_GAME_DIR
#   (about 75 seconds; the corpus walk re-measures every count from $CS_GAME_DIR)
python3 tools/validate_evidence.py private/evidence/F20-EVENT-GRAMMAR/acceptance.json \
  --artifact-root private/evidence/F20-EVENT-GRAMMAR --require-pass
```

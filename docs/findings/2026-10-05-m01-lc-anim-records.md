# M01-LC-ANIM-RECORDS: the animation records behind the payload header

Date: 2026-10-05. Task: #650 (`M01-LC-ANIM-RECORDS`), the follow-up to #633
(`docs/findings/2026-10-04-m01-lc-anim-carriers.md`). Capability used:
**`retail`** (read-only access to `$CS_GAME_DIR`) and ordinary build/test. No
original run happened; nothing here is `verified_original`, and nothing says
what the 2000 engine did with any of it.

#633 stopped at the payload header because no rule fixed a record's length.
This task derives one, checks it over every retail container, and reads what the
records carry. The walk is **measured and checked**; what lies *inside* the
records is still mostly not decoded, and that is listed below rather than
implied away.

## Files

- `crates/cs_formats/src/zbd/anim.rs`: `AnimationPayload::records` ->
  `AnimationRecords` (record *n* by index, `by_anim_name`, the trailing region),
  `AnimationRecord`, `AnimationRecordCounts`, `AnimationRecordPointers`,
  `AnimationRecordTable(Kind)`, `AnimationRecordSequence(Kind)`,
  `AnimationRecordError`, `POINTERS_UNRESOLVED_REASON`. `RECORDS_NOT_DECODED_REASON`
  now says what is *still* open (it was false once the walk existed: it claimed
  the layout was unmeasured). It was rewritten to state the remaining gaps, not
  shortened. Wiring: `crates/cs_formats/src/zbd/mod.rs` (re-exports).
- `crates/cs_app/src/animation/carrier.rs`: `PayloadFacts::records` /
  `RecordFacts` (every record's `anim_name`, in carrier order),
  `BindingBlocker::RecordsRefused`, and `bind_startup_identities` /
  `AnimationBindingSurvey::startup_bindings`. Wiring:
  `crates/cs_app/src/animation/mod.rs`.
- `crates/cs_app/tests/accept_f20_d_validation.rs`: the
  `accept_m01_lc_anim_records_` section (5 synthetic tests, 2 retail). It stays
  in F20-D's existing test binary for the runner-disk reason #633 recorded
  (task #637).

## Where the layout comes from

The pinned mech3ax v0.6.0 source (`crates/mech3ax-anim/src/anim_def.rs`, commit
`d3521a9721be731d365504568ddcd78e3f9846bb`) documents only the MechWarrior 3
`AnimDefC` (316 bytes, a 64-byte reset block *always* present, 40-byte nodes).
The Crimson Skies records do not match it, but they match the **Pirate's Moon**
`AnimDefC` in the upstream project's *later* source (`main`, not the pinned
commit): the same field offsets for `flags` (+156), `status`/`activation`/
`priority` (+160..+162), the counts (+216..) and the pointers (+204..), the
92-byte object and 44-byte node references, 44-byte lights/puffers/sounds,
48-byte activation prerequisites, 72-byte animation references, and "the reset
block is read only when its pointer is nonzero". That source is **not pinned and
not an authority here**: it told this task where to look, and every offset and
size below was then *measured in the retail files* (so the code labels them
`ClaimStatus::ObservedTool`). Where Crimson Skies differs from it, the
difference is a finding below.

## The derivation (acceptance criterion 1)

A record is `272` fixed bytes followed by, in this order and each only when
present:

| part | bytes | present when |
| --- | --- | --- |
| fixed part | 272 | always |
| unknowns table | `u32 at +36` x 36 | the count is nonzero |
| objects | `byte +217` x 92 | |
| nodes | `byte +218` x 44 | |
| lights | `byte +219` x 44 | |
| puffers | `byte +220` x 44 | |
| dynamic sounds | `byte +221` x 44 | |
| static sounds | `byte +222` x 40 | |
| activation prerequisites | `byte +224` x 48 (8-byte header + 40) | |
| animation refs | `byte +226` x 72 (64-byte name + 2 words) | |
| index words | `byte +227` x 4 | |
| reset block | 64 + `u32 at info+60` | the u32 at `+208` is nonzero |
| damage block | 64 + `u32 at info+60` | the u32 at `+212` is nonzero |
| ordinary sequences | `byte +216` x (64 + `u32 at info+60`) | |

`len = 272 + Σ count x entry + Σ (64 + events)`, and record *n + 1* starts at
`start(n) + len(n)`; record 0 starts at payload `+108` (68 header bytes + the
40 measured zero bytes) and is exactly 272 bytes (every count zero). The
effect-table count (`+223`) is zero in all 15 024 records; a record that set it
is refused (`AnimationRecordError::UnmeasuredEffectTable`) rather than skipped.

**The check**, run over all 61 retail containers by
`accept_m01_lc_anim_records_retail_*`:

* the walk reaches the header's declared record count in **61 of 61**
  containers, 15 024 records in total — the figure #633 reported as
  "15 024 declared records, unread";
* it never reads past a payload, and every record start equals the previous
  start plus the previous derived length (asserted per record);
* **30 of 61 containers end exactly at their last record** (all eight `ia1`
  scopes, the `mp*` scopes, `c4/m05`); the other 31 carry a region after the last
  record of 29 690 .. 1 361 762 bytes (10 231 693 in total), see "Trailing
  region" below;
* every reset block is named `RESET_SEQUENCE` and every damage block
  `DAMAGE_SEQUENCE`; the reset (`+208`) and damage (`+212`) words are nonzero
  exactly when that block is present; an ordinary-sequence count (`+216`) always
  equals the number of ordinary blocks; the 56 994 blocks all have flags `0` or
  `0x303`, a nonzero pointer word, **no empty event stream** and zeros in info
  bytes `36..56`; the first (zero) entry of every objects and nodes table has an
  empty name.

The recurrence was not fitted to the first records: record starts were first
located independently of any length (the priority/`2` bytes at `+162`/`+163`
beside a printable name), the gaps between them were compared with the counts
and with the `RESET_SEQUENCE`/size words found inside them, and the resulting
formula was then confirmed on all 15 024 records. M01's `placepiratezep` (572 bytes beyond the fixed part's 272 is two
objects, two nodes, one 64-byte block and 64 bytes of events: `272 + 2x92 +
2x44 + 64 + 64 = 672`) was the first record checked by hand.

### Where Crimson Skies differs from the Pirate's Moon layout

* the fixed part is **272** bytes (PM: 268): offset `+268` is a word that is
  zero in all 14 963 non-zero records, and `+227` (PM: a zero pad) is the count
  of an *index-word table*, nonzero in 254 records;
* the word at `+212` (PM `unknown_seq_ptr`, asserted null) is nonzero in 1 544
  records, and a named `DAMAGE_SEQUENCE` block follows the tables (the reset
  block, `+208`, is present in 5 171 records);
* the words at `+32`/`+36` (PM: asserted null/zero) are an **unknowns table**
  (36-byte entries) in 10 457 of the 14 963 records, placed *before* the
  objects;
* static sounds are 40 bytes (MW: 36);
* the execution priority at `+162` takes the values `1`, `4`, `5`, `6` (MW and
  PM assert `4`), and the activation byte `0`, `2`, `3`, `4`;
* a node entry's 4-byte flags/root-index prefix is the *first* 4 bytes of the
  44-byte entry, so node names sit at entry `+4`.

## The record reader (acceptance criterion 2)

`AnimationPayload::records` returns record *n* by index with: the 32-byte
`anim_name` (+0), `object_name` (+40), `root_name` (+76), `flags` (+156),
`status`, `activation`, `execution_priority`, the `+163` byte (`2` in every
record), `reset_time`, `max_health`, the counts, the raw pointer words, every
present table (entries verbatim, names where a fixed name field exists) and
every sequence block (name, flags, pointer word, raw events).

Names are read as the text before the first NUL of their 32-byte field and
never further: the bytes after the NUL are left-over memory (`ode_name` after
`n` was overwritten by `piratezep`; the default text is `invalid_node_name`).
All fixed-part fields are `ClaimStatus::ObservedTool`. Still **unknown** and
named as such in the code and here: the meaning of the flag word bit by bit;
what the *unknowns* and *index-word* tables are for; the small id words inside
table entries (`0x205`, `0x869`, ...: equal for the same object across tables,
looking like world-node ids, unmeasured); every byte of the object entries after
the name (92 bytes: an affine matrix is plausible by analogy, unmeasured);
the activation and priority values' meanings.

## The pointers (acceptance criterion 3)

**Unresolved, with evidence, and never used as an offset.** Over the 13 pointer
words of every non-zero record, the smallest nonzero value is `0x01fadcf8`
(33.4 MB) and the largest `0x04f7fe60`; the largest container is 2 019 493
bytes (counted in the retail test), so no pointer can be an offset into any
container. They are heap addresses of the original engine's image. The walk
never reads one, and `POINTERS_UNRESOLVED_REASON` travels with every record.
The two small id words at `+72` and `+108` are *not* pointers in this sense (0
.. `0x3dad`); they are equal in 12 051 of 14 963 records. Nothing is decided
about what they index.

## The events

Each sequence block's event stream is kept as raw bytes (length from the block's
own size word). No event layout is measured here; the pinned source's MechWarrior
event grammar is not assumed to apply. So `VS-M01-RUNTIME` still cannot *play*
an animation from this: it can now say **which animation records a carrier
holds, by name, in order, with their object references and their sequence
blocks' sizes**. That is the question the task was opened for; running them is
the next one.

## Trailing region (not walked)

30 of 61 containers have nothing after the last record. In the other 31 the
record area ends inside the payload and 29 690 .. 1 361 762 bytes follow
(M01's mission carrier: 543 041 bytes at payload offset 1 468 528; c1c's camera
carrier: 405 135 bytes). It looks like a table of 32-byte rows of mixed pointer
words and small counts (`0x82c, 1, 0xc, 0x7b0, ptr, ptr, ptr`). **Unmeasured**;
the reader returns it as `AnimationRecords::trailing()` and does not interpret it.
The camera carriers of `c1b`, `c1c`, `c2b` and `c3` all end in the same 405 135 bytes,
which suggests the tail is a function of shared content, not of the carrier's
own records; not investigated further.

## Mission carrier or camera carrier, and the startup identities (criterion 4)

`startanims.zrd` names animation identities; an identity is a record's
`anim_name`. #633 found that, by raw byte search, M01's identities lived in
different carriers and one seemed to be in both. **With the walk, every one of
M01's seven identities is exactly one record**, and none is in both:

| identity | carrier | record |
| --- | --- | --- |
| `pzep_engines_start` | mission (`zbd/c1c/m01`) | 36 |
| `bszep_engines_start` | mission | 230 |
| `wvzep_engines_start` | mission | 414 |
| `wv_hookup_state` | mission | 496 |
| `generic_intro` | camera (`zbd/c1c`) | 23 |
| `call_add_jack` | camera | 60 |
| `player_setup` | camera | 115 |

(The `pzep_engines_start` "occurs in both" of #633 was a byte occurrence inside
other data, not a second record. `NEW_GAME_START` / `LOAD_GAME_START` are keys,
not identities, and occur in neither.)

The rule `bind_startup_identities` applies: **an identity binds only when its
text equals exactly one record's `anim_name` across the scope's mission carrier
and its world group's camera carrier**, byte for byte. Measured over all 53
mission scopes and 195 identities:

| outcome | identities |
| --- | --- |
| bound to a record of the scope's mission carrier | 117 |
| bound to a record of its group's camera carrier | 68 |
| ambiguous (several records) | 0 |
| matches no record of either carrier | 10 |

So the mission/camera split — "the first thing to settle" — is: **both carriers
are searched, and neither alone is enough** (117 vs 68). The rule is a
*description of the retail files*; it says nothing about how the original
engine looked an identity up, which is unmeasured (no original run exists).

The 10 that do not bind stay open with `UNBOUND_REASON_NO_RECORD` and are named:
`pure_panic` (`c1/ia1`, `c1/m04`, `c1/mp1`; four identities),
`deactivate_bmhookup_node` (`c4/m01`, `c4/m02`, `c4/m03`), `fueltrlight1`
(`c3/m01`), `black_chimneysmoke` (`c4/m04`) and `dtzep_engines_start`
(`c5/m04`). Three of those names *are* `anim_name`s elsewhere —
`pure_panic` in `c1/m02`'s carrier, `fueltrlight1` in `c1`'s camera carrier and
`c2/m02`'s, `deactivate_bmhookup_node` in `c4/m04`'s — i.e. in **another scope**,
while `black_chimneysmoke` and `dtzep_engines_start` are in no carrier at all.
Whether the original resolved them across scopes, ignored them, or logged an
error is unmeasured; a cross-scope lookup would make these numbers prettier and
is a guess, so it is not applied. **Affected content**: those 10 identities in
`c1/ia1`, `c1/m04`, `c1/mp1`, `c3/m01`, `c4/m01..m04`, `c5/m04`; M01 is
unaffected.

## Evidence classes

| claim | class | why |
| --- | --- | --- |
| the signature/version/count words, the 84-byte member row | `Documented` | pinned mech3ax v0.6.0 (#633) |
| the 272-byte fixed part, every field offset, every table entry size, the reset/damage/sequence block shape, the walk | `ObservedTool` | derived and checked over 15 024 retail records by this repository; the layout coincides with an *unpinned* later upstream layout, which is a hint and not evidence |
| names (`anim_name`, object, root, sequence) | `ObservedTool` | read verbatim, NUL-terminated ASCII in all 15 024 records |
| pointer words are not container offsets | `ObservedTool` | smallest nonzero `0x01fadcf8` > 2 019 493 |
| the startup-identity rule and its 117/68/0/10 census | `ObservedTool` | a description of the files, not of the engine |
| event layout, flag-word bits, unknowns/index-word tables' purpose, object-entry body, the trailing region | **unknown** | measured sizes only, no measured meaning |
| how the original resolved the 10 open identities, or the 8 open definition-file references of #633 | **unknown** | no original run |

## Limits, and what stays open

* **Events are not decoded** (the biggest gap): a record's sequences are blocks of
  bytes. `VS-M01-RUNTIME` (#359), F20-D family validation and M01-B need an event
  grammar before an animation can *play*; that is a separate task, and it needs
  evidence the pinned source does not provide (no Crimson Skies event
  documentation exists in `docs/research`).
* **The trailing region of 31 containers** is not walked.
* **10 startup identities** do not bind (above).
* **Nothing is `verified_original`.** No original executable was run; `gpu` and
  `audio` were available and unused.
* The retail tests take ~40 s each (the whole 61-carrier census); the pinned
  counts are re-measured from `$CS_GAME_DIR` on every run, not copied from here.

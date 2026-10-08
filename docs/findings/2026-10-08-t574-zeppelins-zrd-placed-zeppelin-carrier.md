# Task #574: the mission-scoped `zeppelins.zrd` placed-zeppelin carrier, decoded

Date: 2026-10-08. Task #574 "Decode the mission-scoped zeppelins.zrd
placed-traffic carrier", the follow-up F33-D (#136) filed from its census
(`docs/findings/2026-10-03-f33-d-neutral-traffic-population-and-retail-census.md`).
Capabilities used: `retail` (read-only, `$CS_GAME_DIR`) plus ordinary
build/test. Test prefix: `accept_t574_`. Evidence:
`docs/findings/evidence/T574.json` (with unresolved `unknowns`, so validate
without `--require-pass`). **Nothing here is `verified_original`**; no
original run happened. A reviewer with a fresh context should check this
format work.

## Sources

- **Retail installation**: install fingerprint
  `c14a876f4457d8710dee7986333ab636122c9549cf72b646fd69cbe7e72c5352`, content
  fingerprint `148a24b7b0506812e8f1ee13d8d3137a05926abebbe10161994e8c4cd300c35e`
  (production `cs_assets::install::discover` + `fingerprint` /
  `content_fingerprint`; the same installation T692's evidence records).
  The per-mission table (archive spelling, SHA-256, carrier presence, record
  count, decoded `node`/`team` spellings) is the private artifact
  `zeppelin-carrier.json`, referenced by digest in the report.
- **F33-D's census** established the carrier counts without decoding the
  member: 50 of the 53 mission `ZBD/<group>/<mission>/zrdr.zbd` archives
  carry `zeppelins.zrd`, `c1/m02`, `c2/m01` and `c5/mp2` do not, and no
  installation-scope archive carries it. This task's census re-measures all
  of that *and decodes*; the counts agree exactly.
- **The `.zrd` node grammar**: F09's rule, already used by `cs_content`'s
  general `.zrd` decoder (`stunts::decode_zrd`) and by the `dzones.zrd`
  decode (#513, `docs/findings/2026-10-05-t513-dzones-framing.md`): a list
  word `N` holds `N - 1` children. Read with that rule, **all 50 members
  decode with no byte left over** — the member is a `.zrd` file of the same
  family, not a bespoke layout.

## The measured grammar

Every word is a little-endian `u32`. A node is one of:

| Tag | Kind | Payload |
| --- | --- | --- |
| `1` | int | one `u32` |
| `2` | float | one `f32` bit pattern |
| `3` | string | `u32` byte length + that many UTF-8 bytes |
| `4` | list | `u32` word `N`, then `N - 1` child nodes |

The member's frame is **one list holding one list of records**: the root
list's only child is the record list; every record is a keyed list — an
even number of children alternating a text key and that key's value, the
value always a list. The 12 empty members are `list(list())`: tag `4`,
word `2`, tag `4`, word `1` — 16 bytes, the smallest member on the
installation.

## The census: 50 members, 58 records

One row per `ZBD/<group>/<mission>/zrdr.zbd`, decoded through
`cs_formats::zbd::zeppelins::read_zeppelins_member` (production code, not a
research copy). `records` is the decoded record count, `bytes` the member
size.

| Mission | records | bytes | | Mission | records | bytes |
| --- | --- | --- | --- | --- | --- | --- |
| c1/ia1 | 1 | 3206 | | c3/m02 | 1 | 2524 |
| c1/m04 | 1 | 2558 | | c3/m03 | 1 | 2524 |
| c1/m05 | 1 | 2524 | | c3/m04 | 1 | 2524 |
| c1/mp1 | 0 | 16 | | c3/m05 | 1 | 2524 |
| c1/mp2 | 0 | 16 | | c3/mp1 | 0 | 16 |
| c1/mp3 | 2 | 6614 | | c3/mp2 | 0 | 16 |
| c1b/ia1 | 1 | 3212 | | c3/mp3 | 2 | 6614 |
| c1b/m03 | 2 | 4986 | | c4/ia1 | 1 | 3206 |
| c1b/mp1 | 0 | 16 | | c4/m01 | 1 | 2524 |
| c1b/mp3 | 2 | 6614 | | c4/m02 | 1 | 2524 |
| c1c/ia1 | 1 | 3206 | | c4/m03 | 1 | 2524 |
| c1c/m01 | 3 | 6766 | | c4/m04 | 1 | 2524 |
| c1c/mp1 | 0 | 16 | | c4/m05 | 3 | 5204 |
| c1c/mp3 | 2 | 6614 | | c4/mp1 | 0 | 16 |
| c2/ia1 | 1 | 3206 | | c4/mp2 | 0 | 16 |
| c2/m02 | 1 | 2524 | | c4/mp3 | 2 | 6614 |
| c2/m03 | 1 | 2559 | | c5/ia1 | 1 | 3209 |
| c2/m05 | 1 | 1343 | | c5/m01 | 1 | 2105 |
| c2/mp1 | 0 | 16 | | c5/m02 | 1 | 1797 |
| c2/mp2 | 0 | 16 | | c5/m03 | 4 | 5632 |
| c2/mp3 | 2 | 6614 | | c5/m04 | 3 | 7535 |
| c2b/ia1 | 1 | 3206 | | c5/mp1 | 0 | 16 |
| c2b/m04 | 2 | 5104 | | c5/mp3 | 2 | 6614 |
| c2b/mp1 | 0 | 16 | | | | |
| c2b/mp3 | 2 | 6614 | | | | |

Histogram: 12 members carry zero records (every `c*/mp1` and `c*/mp2`
present — `c5/mp2` is one of the three omitted archives), 23 carry one, 11
carry two, three carry three (`c1c/m01`, `c4/m05`, `c5/m04`) and one
carries four (`c5/m03`). 58 records in all. The eight `mp3` members each
carry the same two-record shape — `multiplayer1zep` and `multiplayer2zep`,
6 614 bytes each — with per-mission position and tuning values (the
members differ byte for byte at that size).

The three omissions — `c1/m02`, `c2/m01`, `c5/mp2` — are *absent members*,
distinct from the mp1/mp2 state of *present but empty*, and the census
keeps them distinct: `carrier_present` vs `record_count`. No
`ZBD/zrdr.zbd` or `ZBD/<group>/zrdr.zbd` carries the member, so no
installation-scope population can fall back on it either.

## What a record is

One record is **one placed zeppelin**: a `node` binding into the mission's
world, a pose, motion tuning, a `net` name, gasbag/engine/cannon bindings
and the targets the authored cannons name. 26 keys total; 16 are stated by
every one of the 58 records.

| Key | Value shape | Records | Measured vocabulary |
| --- | --- | --- | --- |
| `node` | one-text list | 58 (required) | 13 distinct spellings: `piratezep`, `cargozep1..3`, `vostokzep`, `beowulfzep`, `blackhatzep`, `blackswanzep`, `dantezep`, `geminizep`, `workersvoyagezep`, `multiplayer1zep`, `multiplayer2zep`; unique within a member |
| `position` | three-float list | 58 (required) | x −18 006.0 .. −1 015.2, y 100.0 .. 2 400.0, z −18 053.0 .. 4 767.6 (mission space, unit unmeasured) |
| `yaw` | one-float list | 58 (required) | −180.0 .. 340.0 |
| `pitch` | one-float list | 58 (required) | `0.0` on all 58 |
| `max_speed` | one-float list | 58 (required) | 5.0 .. 30.0 |
| `max_accel` | one-float list | 58 (required) | `4.47` on all 58 |
| `accel_pitch` | one-float list | 58 (required) | `0.5` on all 58 |
| `accel_yaw` | one-float list | 58 (required) | 0.5 .. 2.0 |
| `max_rate_yaw` | one-float list | 58 (required) | 5.0 .. 15.0 |
| `max_rate_pitch` | one-float list | 58 (required) | `5.0` on all 58 |
| `min_pitch` | one-float list | 58 (required) | `−30.0` on all 58 |
| `max_pitch` | one-float list | 58 (required) | `30.0` on all 58 |
| `net` | one-text list | 58 (required) | a net-name spelling per record |
| `healthy` | list of two-text rows | 58 (required) | 316 rows; every row's second text is `panels` |
| `num_healthy_required` | one-int list | 58 (required) | `2` ×1, `3` ×36, `4` ×18, `5` ×3 |
| `engines` | list of texts | 58 (required) | 11, 14 or 18 engine node names per record |
| `deactivated` | one-int list | 9 | `1` ×7, `0` ×2 |
| `team` | one-text list | 16 | `ally` ×12, `enemy` ×4 |
| `cannon_fire_delay` | one-float list | 48 | 10.0 .. 20.0 |
| `cannon_fire_range` | one-float list | 48 | 500.0 .. 15 000.0 |
| `cannon_inaccuracy` | one-float list | 3 | 6.0 .. 10.0 |
| `left_cannons` | list of three-text rows | 48 | `lbroad<N>` node + `deploy_*` + `retract_*` animation spellings |
| `right_cannons` | list of three-text rows | 48 | `rbroad<N>` + `deploy_*` + `retract_*` |
| `targets` | list of texts | 47 | `player` or a sibling record's `node`; empty on the 8 instant-action records |
| `gasbags` | list of `[text, float, one-text list, optional text]` rows | 57 | 310 rows; float `120.0` or `400.0`; `*_gasbagtorpedo<N>` in the list; `panels` as the fourth text on the 100 multiplayer rows |
| `cannon_health` | list of `[4 texts, float, one-text list, two float/text pairs]` rows | 24 | 144 rows; texts are cannon node, `gunback`, `frame`, gasbag node; float `200.0`; `destroy_*` list; pairs `0.6`/`60_*`, `0.3`/`30_*` |

432 `left_cannons`/`right_cannons` rows in all. Non-`player` `targets`
spellings are `blackswanzep`, `cargozep2`, `dantezep`, `geminizep`,
`multiplayer1zep`, `multiplayer2zep`, `piratezep`, `vostokzep` — every one
resolves to a record's `node` in the *same* member (asserted by
`accept_t574_retail_decoded_records_hold_the_measured_vocabulary`), so the
cannon targets are an intra-member reference, not a global actor name.
Records spell their keys in 16 distinct orders; `node` leads everywhere
except the nine `deactivated` records, which lead with `deactivated`.

## The F33 link is a measured negative

The task was filed to connect F33's declared neutral-traffic vocabulary to
original data. The connection does not exist in this member — measured,
not assumed: `cs_content::pilots::neutral_traffic_support` evaluates a
decoded record per [`NeutralTrafficField`] and reports

| Field | Support | Why |
| --- | --- | --- |
| `Traffic` | `Absent` | the encoding carries no traffic index |
| `Pilot` | `Absent` | no pilot id or spelling anywhere |
| `Airframe` | `Nearby` (`node`) | `node` is a placed instance's world-node binding, not an airframe catalog id; which airframe it names is unmeasured |
| `Faction` | `Nearby` (`team`) or `Absent` | `team` is mission vocabulary (`ally`/`enemy`), not a faction catalog id |
| `Survivability` | `Absent` | no survival policy in the encoding |

`can_lower()` is `false` for all 58 records, asserted over the whole corpus
by `accept_t574_retail_carrier_census_decodes_all_50_members`. A lowering
could invent an ordinal or a default, but that is designed data — the same
class the declared synthetic fixture authors — not a measurement of the
original. The `Nearby` variants name the keys a naive wiring would grab,
so the temptation is documented rather than silently coded.

What the member *is*, then: per-mission placed-zeppelin declarations —
world actor placements with pose, motion, armament and an intra-member
target graph — which is why the dependent task is #772
(`M01-LC-WORLD-ACTOR-SPAWN`, a measured world-actor spawn), not a roster
task.

## What is still unknown

The grammar and per-key shapes are measured; **what the original does with
any of it is not** (`KeyMeaning::Unknown` for all 26 keys,
`ClaimStatus::Unknown`):

- whether a record is spawned as an actor at all, and under what runtime
  name;
- which records — if any — the original treats as neutral traffic;
- what `team`, `deactivated`, `num_healthy_required`, the `healthy`,
  `gasbags` and `cannon_health` row spellings and the tuning floats mean at
  runtime;
- how a record maps to a pilot, an airframe catalog id or a faction;
- whether the `node` spellings resolve to entries in the mission's world
  container (measured plausible — they are authored node names — but not
  cross-checked in this task's owner paths);
- the `yaw`/`pitch` unit and axis convention, and the `position` unit
  (values up to ~18 000 suggest mission-scale units, unmeasured).

## What changed

- `crates/cs_formats/src/zbd/zeppelins.rs` (new):
  `read_zeppelins_member`, `ZeppelinMember`, `ZeppelinRecord`,
  `ZeppelinKey`, `MeasuredTeam`, `KeyMeaning`, `ZeppelinsError` and the row
  types (`HealthyBinding`, `CannonBinding`, `GasbagRow`,
  `CannonHealthRow`). Every refusal is named with its byte offset:
  `Truncated`, `UndefinedTag`, `LengthDoesNotFit`, `ZeroListWord`,
  `InvalidText`, `DepthExceeded`, `TrailingBytes`, `NotAMember`,
  `UnknownKey`, `DuplicateKey`, `MissingKey`, `WrongValueShape`.
- `crates/cs_content/src/pilots.rs`: the measured tail —
  `NeutralTrafficField`, `CarrierFieldSupport`, `NeutralTrafficSupport`,
  `neutral_traffic_support`, plus the retail census
  `survey_retail_zeppelin_carrier` / `RetailZeppelinCarrierCensus` /
  `RetailZeppelinCarrier` / `CarrierCensusError` over production
  `install::discover` and `script_raw::discover_container`.
- `crates/cs_formats/tests/zbd/zeppelins.rs` (new): 6 synthetic
  `accept_t574_zeppelins_*` tests over independently authored bytes —
  decode, the empty member, every named refusal, the record frame refusals,
  the nested row shapes and the all-`Unknown` meaning table.
- `crates/cs_content/tests/accept_t574_zeppelin_carrier.rs` (new): 6
  unignored tests over authored installations (the three presence states,
  an undecodable member, a non-reader archive, an undiscoverable install,
  the support negative) plus 2 `#[ignore]`d retail tests asserting the
  measured corpus.
- `crates/cs_content/tests/evidence_report_t574.rs` (new): the evidence
  harness (not an acceptance test), writing `acceptance.json` plus the
  `zeppelin-carrier.json` second production observation.
- Wiring: `crates/cs_formats/src/zbd/mod.rs` (module + doc),
  `crates/cs_formats/tests/zbd/main.rs` (test module).

## Mutation check

The decoder is falsifiable three ways: (1) deleting
`read_zeppelins_member`'s grammar fails all 6 synthetic tests and both
retail tests; (2) loosening a shape — e.g. letting `position` take one
float, or `targets` take non-texts — fails
`accept_t574_zeppelins_nested_row_shapes_are_enforced` and the
`WrongValueShape` cases of `..._a_record_outside_the_measured_shape_is_refused`;
(3) weakening the census to skip an undecodable member fails
`accept_t574_census_refuses_an_undecodable_member` and
`..._refuses_a_non_reader_archive`. The support evaluation cannot go quiet
either: `can_lower()` returning `true` for any record would flip the
retail assertion immediately.

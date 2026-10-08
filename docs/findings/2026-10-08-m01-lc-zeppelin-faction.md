# Task #793: the faction of M01's placed zeppelins is measured absent

Date: 2026-10-08. Task #793 `M01-LC-ZEPPELIN-FACTION`, filed by #359
(`VS-M01-RUNTIME`). Capability: `retail` (read-only `$CS_GAME_DIR`).
Test prefix: `accept_m01_lc_zeppelin_faction_`. Nothing here is
`verified_original`; no original run happened.

## Question

`cs --mission M01` refused the launch under `f34-world.zeppelin-faction-unmeasured`
because no source mapped a `zeppelins.zrd` record's `team` spelling to a faction.

## Measurement

Archive `ZBD/C1C/M01/zrdr.zbd` (logical key `zbd/c1c/m01/zrdr.zbd`), read through
production `install::discover`, `discover_container` and
`read_zeppelins_member`:

| Member | Bytes | Finding |
| --- | --- | --- |
| `zeppelins.zrd` | 6766 | 3 records (`piratezep`, `workersvoyagezep`, `blackswanzep`). **None states `team`** (`ZeppelinKey::Team` absent from all three key lists). `blackswanzep` states `deactivated = 1`; the others state neither. |
| `net.zrd` | 336 | Decodes under the `.zrd` grammar with no byte left over. Holds 8 lists of floats and **no text node at all**: no spelling, hence no net→faction table. |

The record `net` value (`PirateZep1`, `WVZep1`, `SwanZep1`) is an instance name
spelling, not a faction. Of the 26 record keys (#574), `team` is the only one
whose vocabulary is side-like.

## Verdict

Option (b): for M01's three records the faction is **measured absent in the
record** — no `team`, and no mission `net.zrd` table that could supply one. No
decoder for a mapping was written, because there is no mapping to decode in this
scope. Across the whole corpus 16 of 58 records state `team` (`ally` x12,
`enemy` x4, #574); those, in other missions, still have no source mapping the
spelling to a faction id and stay under `f34-world.zeppelin-faction-unmeasured`.

## Binding

`crates/cs_app/src/mission_world_actors.rs`: a record without `team` binds
`faction` as `Resolved::Unknown` under the new `f34-world.zeppelin-faction-absent`
claim (its reason states the evidence); a record with `team` keeps
`FACTION_UNKNOWN_CLAIM`, which now covers only what is still unanswered.
`DeclaredWorldActor::faction` is `Resolved<ContentId>` and cannot express an
absence, so the field is still not `Known` and the lowering still refuses it.
Resolving that needs a type change in `cs_content::world_actors` (optional
faction) or a measured source of the zeppelin's allegiance; see the follow-up.

## Still unknown

- Where (if anywhere) the original takes a zeppelin's allegiance from:
  candidates not measured here are `aiv.zrd` (9869 bytes), `objectives.zrd` and
  `placezeps.zrd` in the same archive.
- The meaning of `ally`/`enemy` in the missions that state it.
- The unit and role of the `net.zrd` float lists (8 x 3 floats), untouched here.

## Tests

`crates/cs_app/tests/accept_m01_lc_zeppelin_faction.rs` (retail, `#[ignore]`):
asserts no `team` key, no text in `net.zrd`, full decode, and the production
binding's `faction` open fields carry the absent claim. Unit test in
`mission_world_actors.rs` covers absent versus unmapped. The
`vs_m01_runtime` expectation of the old claim was updated to the absent claim.

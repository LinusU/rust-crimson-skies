# Does the hit dispatcher consult a team relation? (#2102)

Date: 2026-10-25. Prerequisite for #2099 (can the player's gun damage the neutral
`hydrogentank1..4`?). Capability: `retail` plus the owner's decrypted image read
statically (`$CS_ENGINE_IMAGE`; disassembly stays in `private/`). Nothing here ran the
original executable; nothing is `verified_original`. Cited rows: `[[hit_dispatch_fact]]`
in `data/owner-static/engine-facts.toml` (claim `OWNER-STATIC-2026-10-25`, provenance
`static`). Tests: prefix `accept_m01_hit_dispatch_team_` in
`crates/cs_app/tests/campaign/m01_hit_dispatch_team.rs`.

## Result

**No team, side or shooter-versus-target compare exists between a round's impact and the
call of a node's registered weapon-hit handler.** What admits a hit is the node, not the
relation of the two teams.

## The path (gun round: `0x5acf60`, dispatcher `0x5abcf0`)

1. `0x5acf60` sets the database-query node-flag mask `[0x71e8b4]` to `0x40000`
   (`0x5acf7d`, setter `0x4c7620`), runs the ray query `0x4c8ec0`, and `0x5ad330` picks
   the nearest contact. The mask is tested against node flag words at `0x4c9a24` and
   `0x4cad27`; `0x5ad330` additionally skips a contact whose node has `[node+0x20] == 1`
   (`0x5ad3ed`). These are node attributes. Row `hit.gun_round_mask`,
   `hit.best_contact_filter`.
2. When `[node+0xbc]` (the registered handler) is non-zero (`0x5acfe6..0x5acfee`) the
   damage pair is built from the weapon alone (`0x5ad1c0`: weapon `+0x4c/+0x50/+0x70/+0x34`,
   weapon flag `0x4`, a per-round already-hit bit) and `0x5abcf0` is called (`0x5ad01b`).
   Rows `hit.gun_round_dispatch`, `hit.damage_pair_weapon_only`.
3. `0x5abcf0` returns 0 when the hit node or its handler is null (`0x5abd1c..0x5abd35`);
   otherwise it calls `[handler+4]` once (`0x5abe68`). The hook `[0xa1e190]` is
   called before it but its result is discarded. The only owner-like value it loads,
   `[record+0x20][+0x20]` (`0x5abdef`), indexes the weapon's per-surface effect table
   (`[weapon+0x15c]`, stride 100, `0x5abf16..0x5abf26`) for the impact effect and is not
   compared with anything. Rows `hit.dispatch_entry`, `hit.dispatch_call`,
   `hit.surface_index_is_not_team`.
4. The other callers (`0x44323d`, `0x48d55b`, `0x5ac4cd`, `0x5ac80f`, `0x5acc24`,
   `0x5b13d8`, `0x5b14bd`) were read at the call site: each passes a weapon record, a
   record and a damage pair, and none is preceded by a team test. Vehicle contact
   (`0x48d55b`) is separate and already documented in
   `2026-10-21-m01-contact-response-static-locate.md`.

## Answers

| Question | Answer |
| --- | --- |
| Does `0x5abcf0` or the gun path compare a shooter team with the node's team (0 = neutral)? | **No.** No team value is an argument or is read. |
| Does a player round that hits a team-0 animated node (`hydrogentank1..4`) reach `0x4e7220`? | **Not refused by any relation.** It reaches `0x4e7220` if the ray query admits the node (flag mask `0x40000`, `[node+0x20] != 1`) and the `ANIMATION_DEFINITION` registered the handler on it (`0x5231e2`, bit 22 of `+0x9c` for children). |
| Likewise for AI rounds (Kestrels, pirates)? | The dispatcher receives no shooter, so it cannot differ by shooter kind. That the AI's fire uses the same `0x5acf60` / projectile callers was not traced per shooter kind. |

## Not measured

| Gap | Affects | Resolver |
| --- | --- | --- |
| Whether the `hydrogentank` nodes carry the `0x40000`-passing flag and no `[node+0x20] == 1`; i.e. whether the query returns them at all | the tanks' admission | read the node flags of the tank nodes in the mission geometry (node flag words) |
| Which weapon kinds reach each caller | rockets, flak | per-caller trace |
| Whether the AI fire path calls the same functions | AI rounds on neutral targets | trace the AI fire step |

## What this means for #2099

The designed `FriendlyFireRule::HostileOnly` (`cs_sim::weapons::guns`) refuses a neutral
relation that the engine path never reads. Nothing measured supports refusing the tanks
on a relation. #2099 can drop the "neutral = no damage" premise and decide the tanks by
node admission (above) instead, e.g. `NonFriendly` for the neutral ground targets. This
task did not change the rule: it is a per-gun rule shared by every pair, and the node
admission of the tanks, which decides whether a hit happens at all, is the part still
unmeasured.

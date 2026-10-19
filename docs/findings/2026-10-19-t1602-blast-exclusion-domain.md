# T1602 — the domain of the object `+0xbc` blast-exclusion key

Follow-up to #1318 Q3 (`2026-10-18-t1318-blast-unknowns-measured.md`), claim
`t1318.limit.exclusion_domain`. Everything below is **static analysis** of the
owner-supplied decrypted executable (`$CS_ENGINE_IMAGE`, base `0x400000`).
Claim level `checked`, never `verified_original`. The byte windows are pinned
by `accept_t1602_image_pins_the_exclusion_key_writers`
(`crates/cs_content/tests/accept_t1602_blast_exclusion_domain.rs`).

## Result

The key is **not** a damage group, an object id or a faction. For a world
*node* (the 0xd0-byte pooled object the projectile, actor and area query all
hold) `+0xbc` is a **pointer to a collision-callback record** (or one of two
small constants). Two nodes share a key exactly when they were registered
under the same record, and registration covers a node **and its whole child
subtree**. So the apply loop (`0x5acb0f`) skips every node in the same
registered tree as the directly hit one, not only the hit node.

Values of `node + 0xbc`:

| value        | meaning                                                                                         |
| ------------ | ----------------------------------------------------------------------------------------------- |
| `0`          | no record: nothing registered (the state of every fresh node)                                   |
| `1`          | a projectile node (the dispatcher takes an inline branch at `0x5abd35`, no callback)           |
| other        | pointer to a 0x10-byte record: two `(context, callback)` slots, callback at `+4` of a slot     |

## Writers of the node field (all pinned)

1. `0x4cc5ec` — node pool allocator `0x4cc4f0`: clears 0x34 dwords, then
   stores `0` (`eax` zeroed at `0x4cc5aa`).
2. `0x5abb13` — `0x5abad0(old, new, node)`: rewrites `+0xbc` to the new key
   on `node` if it equalled the old key, after recursing into the children
   (`+0x56` count, `+0x5c` array). Used by registration to hand the new record
   to the node and every child that carried the old key.
3. `0x5abcdc` — `0x5abca0(node, key)`: the same walk, storing `0` where the
   field equals `key` (unregistration).
4. `0x5aee2e` — projectile teardown: `[projectile node + 0xbc] = 0`.
5. `0x5af6a9` — projectile spawn: `[projectile node + 0xbc] = 1`.

The registry behind 2 and 3 is the list at `0x9c6c38`; `0x5abb20` allocates a
record (`0x5ab950`, 0x10 bytes), `0x5abaa0` fills a slot, `0x5abbf0` is the
public register call and `0x5abc50` the public unregister (which also frees
the record, `0x5ab9b0`).

Registration sites (callback constant → who):

* `0x4c061c` callback `0x4c05f0` (slot 0) and `0x4c0860` callback `0x4c0830`
  (slot 1);
* `0x4def0e` callback `0x4df420` (slot 1);
* `0x5232b7`/`0x5232ee` callback `0x4e7220`: the actor class whose node is
  `[actor + 0x6c]`, slot chosen by bit 22 of `[actor + 0x9c]`; the unregister
  `0x523472` releases that node. This is the aircraft/actor registration:
  the actor's node and its parts share one record.

## Consumers

* Hit dispatcher `0x5abcf0` (called from the impact and detonation paths,
  e.g. `0x5ac4cd`, `0x5ac80f`, `0x5acc24`, `0x5ad01b`, `0x5b13d8`): key `0`
  returns; key `1` decrements two clamped counters of the projectile; any
  other key calls the record's callback `[record + 4]` with context
  `[record]`. So a registered tree is also the thing that receives hits.
* Blast apply loop `0x5acac0`: skips a recipient whose `+0xbc` equals the
  fourth argument. Impact (`0x5b0741`) passes `[hit + 0x24]->+0xbc`, so the
  **hit node's whole registered tree** (aircraft body and parts) is excluded;
  if the hit node has key `1` (another projectile) every projectile is
  excluded; if it has key `0` every unregistered node is excluded.
  Timed/proximity detonations pass `0`: they exclude every recipient with no
  record. Whether the area query ever returns unregistered nodes is not
  traced (below).

## Modelling consequence for `cs_sim`

Model the exclusion as *same registered tree as the hit object*, not
"the hit object only": a producer that knows an actor's parts should exclude
all of them from an impact blast of that actor. A timed detonation's `0` key
is "recipients that carry a record are the ones that are hit"; do not copy
that until the query's candidate set is measured.

## Still unknown (recorded, not claimed)

* Other-layout hits: the image has 159 instructions with displacement
  `0xbc`. The 21 integer-store sites outside the five above were inspected
  and sit in initialisers of other structs (sibling fields `0xa4`–`0xc4`,
  float and copy sites), not the 0xd0-byte node; that is a spot check, not
  proof that no other code writes a node's `+0xbc` through an unrelated
  register. Claim `t1602.limit.other_layout_writers`.
* Whether the area query `0x4cb420` can return unregistered (key `0`) nodes,
  which decides whether a timed detonation excludes anything at all. Claim
  `t1602.limit.query_candidates`.
* What each of the four callbacks does with a hit. Claim
  `t1602.limit.callback_bodies`.

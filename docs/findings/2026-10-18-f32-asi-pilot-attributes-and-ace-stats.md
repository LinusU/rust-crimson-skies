# F32-ASI: the image's nine pilot attributes do not (yet) name the nine ace stat slots

Date: 2026-10-18. Task: #1344 `F32-ACE-STATS-IMAGE`. Follows
`2026-10-11-f32-as-ace-stat-slots.md`. Capability: `retail` plus
`$CS_ENGINE_IMAGE` (sha256 `43540fc9…6c37d75`, static reading only, provenance
`static`; no original run). Addresses are virtual addresses in the image
(file offset = VA − 0x400000 for `.data` and `.text`).

## Measured

**1. The nine pilot attributes, their order and their range.**

* The AI-vehicle node format comment (`.data` 0x622508) lists, after
  `signature_maneuvers … stickiness bait pilot`, the keys `dare_devil
  natural_touch sixth_sense dead_eye quick_draw steady_hand stun_recovery talker
  constitution`, then `pref_engage_alt`: nine attributes. The vehicle-descriptor
  key table (`.data` 0x6280fc…) spells the same nine in the same order, with
  `preferred_engagement_altitude` between `stun_recovery` and `talker`.
* A `pilot` key (0x47b169) looks a record up in `pilots.zrd` (object 0x71c280).
  The record holds the nine as consecutive ints at `+0x14..+0x34`, read by the
  one-instruction getters 0x428ab0, 0x486200 … 0x486270 in that order.
* The vehicle descriptor stores per-class defaults at `+0x1ec,+0x1f0,+0x1f4,
  +0x1f8,+0x1fc,+0x200,+0x204,(+0x208 altitude),+0x20c,+0x210` (0x47b21c–0x47b342).
* The AI-node spawn function at 0x47cdee takes, per attribute, the node's
  override (`node+0x7c,0x80,0x84,0x88,0x8c,0x90,0x94,(0x98 altitude),0x9c,0xa0`,
  `-1` = unset), else the pilot record, else the class default, then **clamps
  to `0..=9`** (`cmp eax,9` / `test eax,eax`, 0x47ce17–0x47ce33 and repeated per
  attribute) and maps it linearly between two global floats
  (0x71c490…0x71c4dc) into vehicle fields `+0x954..+0x97c`.

So order, count (9) and range (0..=9) of the *pilot attributes* are measured.

**2. The `ace_stats` side.**

* The scenario-descriptor parser (key `ace_stats`, the only occurrence in the
  image, 0x6255b0; code 0x45950c) accepts a list but
  requires a nine-element list (the list header test is against 0xa), and copies the
  elements, stride 8 in the list, into nine consecutive ints at
  `descriptor+0x68..+0x88`.
* The descriptor constructor (0x459064–0x45909c) sets the **defaults used when
  the key is absent**: `[5, 6, 6, 8, 9, 6, 7, 6, 9]`. Not in the retail
  descriptors (all 72 slots are 9), so this vector is the only other
  measured value of the slots.

## Not measured (still unknown)

* **No consumer of `descriptor+0x68..+0x88` was located.** The instant-action
  spawn path that turns an `ace_*` descriptor into an AI vehicle was not found
  by a read search over the image (no sequence loading those nine fields into
  the node override fields, a pilot record, or a vehicle). Nothing therefore
  binds slot *i* to pilot attribute *i*. Equal count (9) and equal range
  (0..=9) are consistent with the hypothesis but do not prove it, and the
  default vector `[5,6,6,8,9,6,7,6,9]` supplies no order evidence.
* What each attribute does to a vehicle field (the mapping floats at
  0x71c490…) is read but not interpreted here; no effect is claimed.

## Result

The correspondence is **still unknown**. `AceStatSlots` keeps nine unnamed slots
and `ORIGINAL_ACE_STAT_SLOTS_NAMED` stays `false`; a pilot attribute name is
refused as a slot name. A resolving task needs the spawn path from a
scenario descriptor to an AI vehicle (who reads `descriptor+0x68`), or an
original run showing an ace with a changed slot.

Tests: `accept_f32_asi_*` in `crates/cs_content/src/ai.rs` (one synthetic, one
`#[ignore = "requires CS_ENGINE_IMAGE"]` re-measuring the order, count and the
single `ace_stats` key).

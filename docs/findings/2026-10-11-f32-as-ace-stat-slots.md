# F32-AS: the ace's nine stat slots — no slot is named; the record stays unnamed

Date: 2026-10-11. Task: #568 `F32-ACE-STATS`. Follows
`2026-10-03-f32-d-original-ai-roles-and-difficulty.md` section 4. Capability:
`retail` (read access to the installation); no original run.

## Measured

* F32-D, unchanged: `ace_stats` is a nine-element integer list in all eight
  instant-action descriptors, every one of the 72 slots is 9.
* New: the engine's own identifier lists name no ace stat. In
  `ASSETS/SCRIPTS/DEBUGINFO.TXT` (20505 bytes, 1260 lines) and
  `ASSETS/SCRIPTS/RESOURCE.H` (29579 bytes, 646 lines), both read through the
  production ROF mount, **no identifier starts with `ace`**. The only
  `ace` substring in either is inside an unrelated word. Lines mentioning
  `stat` are interface-state and multiplayer-scoreboard identifiers, not ace
  slots. `skill`, `reaction`, `accura` and `aggress` occur in neither member.
* The complete descriptor key vocabulary contains exactly one key containing
  `stat`: `ace_stats` itself.

No display text is recorded or committed; only counts and the absence of a
prefix.

## Not measured (still unknown)

* What each of the nine slots is, and their order's meaning.
* What a value below 9 does; the installation ships no such value.
* Whether a damage or health slot exists. No measured key names one.
* The executable image was not searched for a slot table.

## What changed

`cs_content::ai`:

* `AceStatSlots` carries the list as ordered, unnamed `u32` slots, addressed by
  index only. `try_from_list` refuses a list that is not nine long
  (`AceStatSlotCount`) and an element outside `0..=9` (`AceStatOutOfRange`).
* `AceStatSlots::slot_named` always returns `AceStatSlotNameUnmeasured`, and
  `effect_of` always returns `AceStatEffectUnmeasured`. A later stage cannot add
  nine named fields through this type, and a mission that wants an ace to
  behave differently declares a `DeclaredAceProfile`.
* `ORIGINAL_ACE_STAT_SLOTS_NAMED = false`.
* The `f32.d.limit.ace_stats` entry in `UNRESOLVED` now says the declared half
  is closed and the meanings remain unknown.

`cs_sim::ai::combat` is unchanged: it has no ace stat vector.

Tests: `accept_f32_as_*` in `crates/cs_content/src/ai.rs` (two synthetic, one
retail, `#[ignore = "requires CS_GAME_DIR"]`).

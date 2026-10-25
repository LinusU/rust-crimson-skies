# #2077 Intro gate: the record writer, the chapter table and the saved block

Date: 2026-10-25. Static analysis of `$CS_ENGINE_IMAGE` (read-only, disassembled into `private/`, never committed) and
the installation's `ZBD/zrdr.zbd`; the original executable did not run, so every claim is `static`, nothing is
`verified_original`. Implementer: `sonnet-1` (Sonnet 5.5), no independent review yet. Addresses are image virtual
addresses. Follows `2026-10-24-intro-callback-2151-static.md`.

## 1. The record is a per-mission result, not a "passed" flag

The 168-byte records at `0x64cc50` are two 84-byte halves; callback 2151's words are each half's word `+4`
(`+0x04`, `+0x58`). The ordinary writer is the end-of-mission handler `0x4194e0`, called with the mission index
`[0x71c098]` (set at `0x41726e`, `0x41a88d`, `0x442b38`):

* `0x4194e0` zeroes the first 21 dwords of record `[0x71c098]` (`0x419511`), then writes `+0x00` (a bit mask:
  bit 0 when the mission-passed word of the campaign object, `0x71b480+0xc58`, is set (`0x463be0`); bits for the
  completed objectives; bits 18..30 from the `0x64fb60` collection) and calls `0x419630` (`0x419601`), which stores
  `+0x04` = the mission timer (`0x71b468`, `0x46c590`) scaled by `[0x603464]` and truncated (`0x41964b`), plus per-
  player stats at `+0x08..`.
* It runs for **any** single-player ending of the mission: `0x443090` (the mission teardown, from `0x46bac0`,
  `0x47e2ae`, `0x47e2d5`) calls it when `[0x71d508] == 0` and `[0x71bb80] != 3`; `3` calls `0x419700`
  (a different record, `0x64ad8c`) instead. A failed attempt writes the record too: only bit 0 of `+0x00` tells
  passed from failed. After four failed attempts at a mission (`0x419544`, counter `+0x14` of the table entry) a
  message box (`0x4af930`) offers to treat it as passed: yes sets the passed word and re-enters (`0x419583..0x419591`).
* So **`+0x04` is non-zero once the mission has a result on record** (it is the elapsed time, zero only if it rounds
  to zero), whatever the ending. The gate therefore means "this chapter's first mission has been flown to an ending",
  not "passed" and not "loadout set". Whether a quit from the pause map reaches `0x443090` is **not traced**.
* `+0x58` is the second half's word `+4`. No ordinary writer of the second half was found: the only references to
  `0x64cc50..0x64dc10` are `0x4194e0`/`0x419630` (first half), the new-pilot zeroing `0x411419`, the cheat advance
  of callback 2104 (`0x409f6f..0x40a01b`, sets `+0x00`, `+0x04`, `+0x54`, `+0x58`) and readers (`0x40bfb1`,
  `0x40bfbb`, `0x40c8b5` reads `+0x54`). Unknown: what the second half is for. It stays 0 in this build.

## 2. Mission-to-chapter table

The table at `0x71b480+0x704` (52-byte entries, 25 slots) is built by the loader at `0x46bf40` from
`cm_sequence.zrd` (`0x6270c0`; the `seq`, `desc`, `campaign`, `mission`, `area`, `wingman` fields). Entry `+0x2c` is the
`area` string compared at `0x46c036..0x46c0b4` against `HAWAII` 3, `NORTHWEST` 1, `HOLLYWOOD` 2, `COLORADO` 4,
`MANHATTAN` 5; `0x419ce0` then maps the code through the jump table `0x419d38` to the chapter: 1 → 2, 2 → 3, 3 → 1,
4 → 4, 5 → 5. So **Hawaii = chapter 1, Northwest 2, Hollywood 3, Colorado 4, Manhattan 5**, which agrees with the
owner's fresh-pilot capture (`chap1`). `cm_sequence.zrd` (`zrdr.zbd`, 5192 bytes) lists 24 missions, five per area in
that order, Manhattan four; slot 25 is empty (chapter 0). Tests re-read the image and the archive
(`accept_parity5_intro_gate_2077_*`, ignored without `CS_ENGINE_IMAGE` / `CS_GAME_DIR`).

## 3. The saved block

The block `0x64b340` (`0x2a44` bytes) is the save chunk **`UIData`** (`0x61f704`): `0x416be0` writes it, `0x416c00`
reads it, registered at `0x416b8d` (flags `0x65`/2) and again as `InMenu` (`0x416ba8`, `0x416c20`/`0x416c50`). The
records are therefore saved with the rest of the UI state; whether that is the pilot's own file or a game save is not
established, and the layout of the original's pilot files remains unknown.

## Wired

* `cs_app::ui::front_end::intro`: `chapter_of_area`, `chapter_of_mission`, `AREA_CHAPTERS`, `CAMPAIGN_MISSIONS`;
  `IntroMovie::path_for` (chapter movie `chap<N>.mpg` beside the configured one).
* `pilot_progress`: `current_mission` (first mission without a `completed` counter: this build's reading, the
  original's advance rule is untraced), `gate_flags` (`word_04` set when the chapter's first mission has any counted
  attempt), `intro_chapter`. Our own profile counters, no claim about the original's bytes.
* `window.rs`: the pilot-page → cabin entry asks `intro_chapter` and plays `chap<N>` or nothing.

## Not established

* Whether Esc-quit writes the record (counted as an attempt here).
* The meaning and writer of the second half (`+0x58`).
* Whether the original advances the current mission only on a pass.
* The hearing and look of chapters 2..5 (`human_review`).

# M24-B-FU1 — M24's operand declarations audited against the installation

Rally #1271, follow-up of M24-B (#328; `2026-10-10-m24-b-compatibility-gaps.md`).
Tests: `crates/cs_app/tests/campaign/m24_b_fu1.rs` (`accept_m24_b_fu1_*`, retail,
`#[ignore = "requires CS_GAME_DIR"]`). Nothing here is `verified_original`; no
original executable was run.

## Method

The M17-B method. One `.zrd` text index of the whole installation is built
through production discovery (`cs_assets::install::discover`,
`cs_formats::script_raw::discover_container`, `cs_content::stunts::decode_zrd`):
16 040 distinct texts, each mapped to the `(container, member)` pairs that spell
it. The control member `ZBD/C5/M04/zrdr.zbd#objectives.zrd` spells 226 distinct
texts. Each is looked up outside that member. Texts nothing else declares get a
raw-byte scan of every installation file.

## Result

* **Declared** (223 of 226): directive keys and `OBJECTIVE<N>` addresses (other
  missions' control members; grammar words, not declarations), the `M4*` nets
  (`ZBD/C5/zrdr.zbd#neindex.zrd`, M24-B), and:

| Operand class | Declared by |
| --- | --- |
| `bhatwarhawk_5_1…6`, `stihellhound_5_1…8`, `bsfury_5_1…3`, `devastator_1/2`, `wingman_1/2/3` (22 names) | `ZBD/C5/M04/zrdr.zbd#aiv.zrd` (M24's own) |
| 45 `snd_c5-MN-m4_*`, `snd_MN4*`, `snd_c3-HW-m4_Sparks_33` names | `ZBD/zrdr.zbd#sounds.zrd` |
| `call_bombs_away`, `milesdrop` | M24's own `glidebomb.zrd`, `miles_drop.zrd` |
| `killdtzep`, `all_dtzep_gasbags` | `ZBD/zrdr.zbd#dante_destruction.zrd`, `#dante_gasbags.zrd` |
| `rock_zeppelin` | `ZBD/zrdr.zbd#rock_zeppelin.zrd`, M24's `targets.zrd` |
| `MSG_OBJ_DESTROY` | M24's `targets.zrd` (and `strings.dll`) |

"Declared" means a name exists; what the original does with it is not read here.

## Gaps (not guessed)

1. **`stihellhound_5_eg0`** — five occurrences in M24's reader archive, in no
   other file; absent from `aiv.zrd` (which declares `stihellhound_5_1…8`). The
   original's resolution of this aircraft is unknown. Resolving task:
   **M24-C** (original reference run).
2. **`MSG_BRF_NYM4_OBJ1`** — no `.zrd` member declares it, but `strings.dll`
   spells it once, in an identifier table beside `MSG_BRF_NYM3_OBJ1` and
   `MSG_OBJ_DESTROY`. `strings.dll`'s format is not decoded by this stage, so
   the briefing text it resolves to is unknown. Resolving task: **M24-C**
   (original reference run); a decoder for `strings.dll` would also settle it.
3. **`SET_AI_`** — the truncated key M24-B already recorded (no other member
   declares it; `SET_AI_NET` and `SET_AI_TEAM` are declared elsewhere). Resolving
   task: **M24-C**.

The test file pins all three exactly, so a change in either direction fails.

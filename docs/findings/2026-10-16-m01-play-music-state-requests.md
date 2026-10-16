# M01-PLAY-MUSIC: what M01's music state requests resolve to, and what still needs an owner ruling

Date: 2026-10-16. Task: `M01-PLAY-MUSIC` (#1463), follow-up of `M01-PLAY-RADIO`
(#1438). Capabilities used: `retail` (read-only `$CS_GAME_DIR`, through
production readers) and ordinary build/test over the owner's decrypted engine
image. Test prefix: `accept_m01_music_state_requests_` (2 tests,
`crates/cs_app/tests/accept_m01_music_state_requests.rs`). Evidence report:
none produced yet — see "What is still open", item 3.

**Everything below about the original program is static code evidence**: a
read-only disassembly of the owner's executable plus a read-only read of the
installation. The original program was never run for this task, nothing here is
`verified_original` (`AGENTS.md` rule 8), and every semantic the code does not
settle is recorded as **unknown**, not guessed (`AGENTS.md` rule 4).

## Provenance

| Item | Value |
| --- | --- |
| Engine file | `$CS_ENGINE_IMAGE` |
| Format | PE32, image base `0x400000`, 2 580 480 bytes |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` (identical to `M01-LC-DIRECTIVE-D`'s, so both tasks read one binary) |
| Address convention | virtual address (VA); every section of this image has `PointerToRawData == VirtualAddress`, so `VA = file offset + 0x400000` — re-measured here by walking the section table with `cs_formats::pe_resources::read_pe_layout` |
| Disassembly tools | `r2` (`aaa`, then `pdf`/`pd`), `python3` with `capstone` 5.0.7 linear sweeps, plus raw little-endian byte searches for each absolute address (so a sweep's desync cannot hide a reference) |
| Retail readers | production `cs_app::mission_session::{plan-side recovery, declared_engine_objectives, declared_sound_groups, MissionRadio::read}` over `zbd/c1c/m01` and `sounds.zrd` |
| Cross-check | `cs_formats::pe_resources::read_pe_layout` + `map_rva` inside the task's own image test; a plain byte scan of `ZBD/soundsl.zbd`/`ZBD/soundsh.zbd` names (auxiliary, below) |

## What a music state request is (measured before this task)

A `WAKEUP_SOUND_GROUP` or `COMPLETED_SOUND_GROUP` naming one of the executable's
seven built-in `music_*_sg` groups is a **music state request**, not an ordinary
cue: `docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`
measured that the engine's own string table holds exactly those seven names and
that dispatcher `0x5954f0` selects a music state by name.
`docs/findings/2026-10-15-m01-play-radio-dialogue.md` (#1438) measured the same
thing from the data side and reports such a cue as
`RadioLineOutcome::Music`, requesting nothing.

M01 spells **five of the seven** over **six** `WAKEUP_SOUND_GROUP` sites:
`music_battlesuccess_sg`, `music_missionsuccess_sg`, `music_prebattle_sg`,
`music_primaryobj_sg`, `music_secondaryobj_sg`. It never spells
`music_tertiaryobj_sg` or `music_battle_sg`.

## 1. The retail half: what `sounds.zrd` expands each state to

Measured through production code (`MissionRadio::read` over the installation;
reproduced by `accept_m01_music_state_requests_resolve_to_their_tracks`):

| State | Tracks `sounds.zrd` reaches, in spelled order |
| --- | --- |
| `music_battlesuccess_sg` | `music_battlesuccess1..6.wav` (6) |
| `music_missionsuccess_sg` | `music_missionsuccess1..6.wav` (6) |
| `music_tertiaryobj_sg` | `music_tertiaryobj1.wav`, `music_tertiaryobj2.wav` (2) |
| `music_primaryobj_sg` | `music_primaryobj1.wav`, `music_primaryobj2.wav` (2) |
| `music_secondaryobj_sg` | `music_secondaryobj1.wav`, `music_secondaryobj2.wav` (2) |
| `music_prebattle_sg` | `music_prebattle1..6.wav` (6) |
| `music_battle_sg` | `music_battle1..6.wav` (6) |

Every one of the seven expands with `music == true` (the `MUSIC` leaf) and with
the property word `DYNAMIC_WEIGHTS` named as an unresolved child — the group
records spell `["MUSIC", "DYNAMIC_WEIGHTS", 0.5, [tracks…]]`, and the property
is named rather than dropped.

**Measured:** the group's children are a *candidate set of tracks*, in the
spelled order, and the prepare step never decodes them: `MissionRadio::read`
adds members only when `!expansion.music`, so no music track becomes a radio
line (pinned by the same test).

## 2. The image half: the dispatcher and its seven track tables

`0x5954f0` is one `strcmp` chain. Each matched branch stores a runtime handle
into a single global and returns a `const char*` from a static table:

| State | Name at | Static table at | Slots | Runtime table at | Index the branch uses |
| --- | --- | --- | --- | --- | --- |
| `music_battle_sg` | `0x63a530` | `0x63a040` | 6 | `0x9ad61c` | `[0x9ad684] % 6` (also sets byte `[0x9ad480]=1`) |
| `music_prebattle_sg` | `0x63a51c` | `0x63a070` | 6 | `0x9ad64c` | `[0x9ad684] % 6` (also sets byte `[0x9ad480]=1`) |
| `music_secondaryobj_sg` | `0x63a504` | `0x63a030` | 2 | `0x9ad60c` | `[0x9ad6c0] % 2`, then `[0x9ad6c0]++` |
| `music_primaryobj_sg` | `0x63a4f0` | `0x63a028` | 2 | `0x9ad604` | handle: `[0x9ad6c0] % 2`; returned name: `[0x9ad6c8] % 2`, then `[0x9ad6c8]++` |
| `music_tertiaryobj_sg` | `0x63a4d8` | `0x63a038` | 2 | `0x9ad614` | handle: **raw `[0x9ad6c0]`** (no mask); returned name: `[0x9ad6c4] % 2`, then `[0x9ad6c4]++` |
| `music_missionsuccess_sg` | `0x63a4c0` | `0x63a058` | 6 | `0x9ad634` | `[0x9ad684] % 6` |
| `music_battlesuccess_sg` | `0x63a4a8` | `0x63a088` | 6 | `0x9ad664` | `[0x9ad684] % 6` |

The chain then falls through four direct clips — `music_spicyairtales.wav`
(`0x63a0e0`), `music_airtales.wav` (`0x63a0f8`), `music_instantaction.wav`
(`0x63a0c8`), `music_splash.wav` (`0x63a0b4`) — each of which stores a
pre-held handle and returns the name itself, and finally returns the name
unchanged when nothing matches.

**The returned name is discarded.** `0x5954f0` has exactly one call site
(measured by scanning every `E8 rel32` in `.text`): `0x59525b`, inside the
music routine `0x595140`. Immediately after the call the caller does
`mov eax, [0x9ad484]` (`0x595260`) — overwriting the returned `const char*`
with the runtime handle the branch stored — and then returns without playing
anything when that handle is `0`. So the **static name tables have no consumer
on this path**: what plays is the runtime table's entry.

Static table contents (each entry is a pointer to a NUL-terminated name),
re-read by `accept_m01_music_state_requests_match_the_executable_track_tables`:

| State | Slots, as the image spells them |
| --- | --- |
| `music_battle_sg` | `music_battle1..6.wav` |
| `music_prebattle_sg` | `music_prebattle1.wav`, `2`, `3`, **`1`, `2`, `3` again** |
| `music_missionsuccess_sg` | `music_missionsuccess1..6.wav` |
| `music_battlesuccess_sg` | `music_battlesuccess1..6.wav` |
| `music_primaryobj_sg` | `music_primaryobj1.wav`, `music_primaryobj2.wav` |
| `music_secondaryobj_sg` | `music_secondaryobj1.wav`, `music_secondaryobj2.wav` |
| `music_tertiaryobj_sg` | `music_tertiaryobj1.wav`, `music_tertiaryobj2.wav` |

## 3. Open question 1: how a music state maps to the tracks it plays

**Measured: a weighted random draw per request for the four six-slot states,
and an alternating counter for the three objective states. Not a shuffle, not a
fixed sequence, not one clip.**

The chain, all inside the request path:

1. `fcn.0059ab60` (the music request routine, reached from `fcn.0059ab40`
   and `fcn.0059ad20`) first calls `fcn.0059a3e0` at `0x59ac5b`. That routine
   takes the group object at `record+0x50`; when the group declares children
   (`[group+0x20] > 0`) it calls **`fcn.0059a440`** (`0x59a40f`), and stores the
   chosen child at `record+0x40`.
2. **`fcn.0059a440` is the selector.** Measured, step by step:
   * `[0x9ad730] = 0` at entry (`0x59a44b`); a group with exactly one child
     returns with that zero — index 0;
   * it sums the `float` weight at `+8` of each `0x1c`-byte child record whose
     word at `+2` is non-zero (`0x59a47f`…`0x59a493`);
   * it calls **`MSVCRT!rand`** (`0x59a4a2`), normalises the draw against that
     weighted total (scale constants at `0x6081bc = 0.001` and `0x609430`) and
     walks the children accumulating weights until the accumulated weight
     passes the draw — at `0x59a4ea`, on that single edge, it stores the child
     index: `mov [0x9ad730], ecx` (`0x59a594`);
   * afterwards it adjusts the weights (clamping a weight below `0.001` up to
     `0.001` at `0x59a536`, then rescaling each by the inverse total at
     `0x59a574`) — the mechanism the retail record's `DYNAMIC_WEIGHTS` property
     names.
3. `fcn.0059ab60` then copies that index onto the dispatcher's selector:
   `mov ecx, [0x9ad730]; mov [0x9ad684], ecx` (`0x59ac84`, `0x59ac8a`), and
   calls the music routine `0x595140` (`0x59ac9e`) with the handle.
4. `0x595140` copies the handle record onto the stack and calls the dispatcher
   `0x5954f0(name)` (`0x59525b`), which indexes the six-slot tables with
   `[0x9ad684] % 6` and writes the runtime handle into `[0x9ad484]`; the caller
   plays from `[0x9ad484]` and drops the returned name.

`[0x9ad730]` is written **only** at `0x59a44b` (zero) and `0x59a594` (chosen
index), and read only at `0x59ac86` — so for `music_battle_sg`,
`music_prebattle_sg`, `music_missionsuccess_sg` and `music_battlesuccess_sg`
the slot is a **per-request weighted-random draw over the group's own dynamic
weights**, `rand()`-based, with a uniform fallback to slot 0 for a one-child
group or a draw that passes no child.

The three objective states hold their own counters instead
(`[0x9ad6c0]`, `[0x9ad6c4]`, `[0x9ad6c8]` — `0x9ad6c0` is incremented by the
secondary branch only, `0x9ad6c4`/`0x9ad6c8` by their own branches), so their
two-slot selection advances by one per request rather than drawing. Note the
measured oddity: the *tertiary* branch indexes its runtime table with the raw
`[0x9ad6c0]` while the other two mask it to `mod 2`; whether the counter is
kept in range elsewhere was not traced (unknown #6).

**Still unknown here:** which runtime slot holds which track. The loader
`fcn.00596ea0` fills the runtime tables by computing
`slot = handle − handle_of("music_<state>.wav")` in a switch keyed on the
member name's length (`0x596ea0`…`0x59708b`), so slot order follows the order
handles are assigned at load — an order no shipped file states and this task
did not measure. The candidate *set* is the retail expansion of §1; the
slot ↔ track binding is not.

## 4. The path a mission's music request takes

The previous finding measured the mission side up to the engine's play entry
point `0x593590` (the routine the mission-level woken/completed channels wrap).
This task measured what `0x593590` does with the handle:

* if the handle record's first dword is `1` → `call fcn.0059ab40` (`0x5935b6`),
  i.e. the `fcn.0059ab60` request path of §3 — the branch that runs the
  weighted selection;
* otherwise, if the handle's name at `+4` starts with `mu` — or the handle's
  `+0xf0` flag is already set — it sets `[handle+0xf0] = 1` (`0x5935e0`) and
  calls the music routine directly: `0x595140(handle, (int)volume, 0)`
  (`0x5935f3`);
* otherwise it is an ordinary sound and goes to `0x593b80` (`0x593607`).

**Measured consequence:** a handle whose name begins `mu` — which every
`music_*_sg` name does — is routed into the music routine by name, and the
first such play marks the handle so every later play of it takes that branch
without re-testing the name.

**Unknown:** which of the two music branches a music *group* handle takes at
runtime. That depends on the record type dword `0x596120` hands back for these
names, which this task did not measure; the difference matters, because only
the `dword == 1` branch runs the weighted selection of §3. `0x595140` has four
call sites in total (`0x5935f3`, `0x59388a`, `0x5959b6`, `0x59ac9e`); the three
others were not traced here.

## 5. Cross-check: image against installation

| State | Image table | `sounds.zrd` | Agree? |
| --- | --- | --- | --- |
| battle | `music_battle1..6.wav` | same 6, same order | yes |
| battlesuccess | `music_battlesuccess1..6.wav` | same 6, same order | yes |
| missionsuccess | `music_missionsuccess1..6.wav` | same 6, same order | yes |
| primaryobj | `music_primaryobj1..2.wav` | same 2, same order | yes |
| secondaryobj | `music_secondaryobj1..2.wav` | same 2, same order | yes |
| tertiaryobj | `music_tertiaryobj1..2.wav` | same 2, same order | yes |
| **prebattle** | `music_prebattle1..3.wav` **twice** (6 slots, 3 distinct) | `music_prebattle1..6.wav` (6 distinct) | **no** |

The image carries **no string at all** for `music_prebattle4/5/6.wav` (a string
sweep over the whole image finds `music_prebattle1..3` only), while `sounds.zrd`
names six prebattle tracks and a plain byte scan of `ZBD/soundsl.zbd` and
`ZBD/soundsh.zbd` finds all six member names in both archives. That scan is an
auxiliary observation, not a production decode: the prepare step never decodes
music members (§1), so no production reader has read those six recordings for
this task. Because the returned static name is discarded (§2), the discrepancy
does not change what this task claims the engine plays — but it is the one
place where the two sources disagree, and both readings are pinned by the two
tests so neither can drift into the other.

## 6. What this does **not** settle

1. **Open question 1 — the selection rule** is answered above (§3) for the
   seven states, with the slot ↔ track binding left unknown, and with the
   branch question of §4 open. What remains unmeasured is *audible* behaviour:
   no original run, so "draws a weighted random track" is a code path, not
   observed play.
2. **Open question 2 — the `music` content identity — is unchanged and needs
   the owner.** `MusicDirector::request` refuses an id whose kind is not
   `ContentKind::Music`; F14-D.7 measured that no container family states a cue
   class and its own mutation probe fails a `music` row minted from a `music_`
   member-name prefix; and no measured derivation turns a *group name* (or a
   track name) into a music content id. Two candidate rules exist and both are
   **designed conventions, not measurements**:
   * **(a) state-level**: one `Music` id per music state (the cue names the
     state, `MusicDirector`'s `Started/Transition/Unchanged` follow state
     changes, and the track choice stays inside playback). Needs a spelling for
     the id: nothing in F14 keys a row on a `sounds.zrd` group name.
   * **(b) track-level**: one `Music` id per selected track, over F14-D.7's
     `install_file_key("<container>/<member>")` with
     `ContentKind::Music` — the exact shape of the dialogue rule the owner
     ruled on #1465, but for a different kind and with the container choice
     (which the original opens is unmeasured, F14-D.7) still open.

   Until the owner picks one (and says which container, or that the choice is
   labelled designed), **this task mints no id**, and the acceptance clause
   "M01's music cues reach `MusicDirector` under the owner-ruled identity" is
   unimplementable without guessing.
3. **Open question 3 — playback.** Measured: `AudioSession::request_music` has
   no production caller (only `crates/cs_app/tests/accept_f41_c_radio_device_loss.rs`
   calls it), no mixer or device routine reads `MusicOutcome`, and
   `MissionRadio::plan` answers a music cue with `RadioLineOutcome::Music` and
   requests nothing. Wiring a music cue to a device is the same gap
   `M01-RADIO-DEVICE` names for radio and stays outside this slice.
4. **Evidence report.** This task reads `retail`, so under
   `docs/contracts/CLI-EVIDENCE.md` a passing `acceptance.json` is expected
   before merge. It must be generated on the finished tree by the session that
   completes the routing half, and is deliberately not written by hand here.

## 7. What landed on this branch

* `crates/cs_app/src/mission_session/radio.rs` — one read-only accessor,
  `MissionRadio::resolved(group) -> Option<&ResolvedGroup>`, returning exactly
  the expansion `MissionRadio::plan` already answers music cues from. No
  behaviour change: `read`, `plan`, `enqueue` and the host's sound step are
  untouched.
* `crates/cs_app/tests/accept_m01_music_state_requests.rs` — prefix
  `accept_m01_music_state_requests_`, two members, both `#[ignore]`d with their
  capability named so CI (which has neither) skips them and a local run without
  the variable fails loudly:
  * `…_resolve_to_their_tracks` (`CS_GAME_DIR`) — production chain end to end:
    launch plan → retail objective recovery → engine lifecycle model →
    `declared_sound_groups` (asserts M01's own program cues exactly the five
    measured states and neither of the two it does not spell) →
    `MissionRadio::read` (asserts each of the seven states resolves with the
    `MUSIC` leaf, the exact track list of §1, and `DYNAMIC_WEIGHTS` named) and
    that no music track was prepared as a line;
  * `…_match_the_executable_track_tables` (`CS_ENGINE_IMAGE`) — reads the
    image through `cs_formats::pe_resources`, asserts each state's name at its
    measured VA, each table slot's pointer and the name it spells, that each
    table's address is referenced by code in `.text`, and that the four direct
    clips' names exist.
* `docs/findings/2026-10-16-m01-play-music-state-requests.md` — this document.

Commands run on this branch (exit codes recorded in the hand-over):

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_m01_music_state_requests --include-ignored` | 0 (2 tests, both pass) |

## 8. Unknowns, each with its evidence

1. **Slot ↔ track binding in the runtime tables.** Slots are filled by
   `handle − handle_of("music_<state>.wav")` in `fcn.00596ea0`; the order
   handles are assigned at load is not measured (§3).
2. **Which entry branch a music *group* handle takes** in `0x593590`
   (`dword == 1` → weighted selection, or name-prefix → direct), which decides
   whether a given request draws or reuses the last index (§4).
3. **The music content identity.** No owner ruling exists; §6 item 2 lists the
   two designed candidates. Nothing in this finding mints one.
4. **Playback.** No consumer of `MusicOutcome` and no caller of
   `AudioSession::request_music` in production (§6 item 3).
5. **Audibility.** No original executable ran; no person heard anything; the
   selection rule is static code evidence only.
6. **`[0x9ad6c0]`'s range.** It is incremented by the secondary branch and read
   unmasked by the tertiary branch (§2); whether anything keeps it in range is
   untraced.
7. **The three other `0x595140` call sites** (`0x59388a`, `0x5959b6`, plus the
   routine around `0x59388a`) were not traced (§4).
8. **Which container's music tracks the original opens** — F14-D.7's unknown,
   unchanged: `soundsl`/`soundsh` both hold the names, no state reads which one
   is chosen.
9. **`music_airtales.wav`** exists as an image string but no `music_` name
   sweep of either sound archive finds it; its status is unknown and outside
   this task's seven states.

## 9. What the owner must decide to unblock #1463

The acceptance clause *"a test (prefix `accept_m01_music_`) that M01's music
cues reach `MusicDirector` under the owner-ruled identity — never a guessed id"*
cannot be written yet: the identity has no ruling (unknown #3). Concretely:

* rule **(a)** state-level or **(b)** track-level from §6 item 2 (or a third
  spelling of your own), including the container handling for (b);
* confirm the selection rule of §3 as the behaviour to reproduce (weighted
  random draw per request for the six-slot states, alternating counters for the
  objective states), or say that it stays recorded-but-unimplemented until an
  original run can confirm it;
* say whether #1463 stays evidence-bound for the finishing session (§6 item 4).

Nothing in the repository was guessed to get past this: no `Music` id exists on
this branch, `MusicDirector` is unchanged, and the two tests assert only what is
measured above.

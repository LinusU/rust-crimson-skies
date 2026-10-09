# RECORD-OBJECTIVES-SOUND: the two OBJECTIVES_*_SOUND record keys, measured in the retail executable

Date: 2026-10-09. Task: `RECORD-OBJECTIVES-SOUND` "Admit the two
OBJECTIVES_*_SOUND record keys to the record sound vocabulary" (Rally #808,
follow-up to `M02-B-FU2` #801). Shared contract:
`docs/contracts/SCRIPT-MISSION.md`. Capabilities used: `retail`
(`$CS_GAME_DIR` read-only, never written) and the owner-supplied decrypted
engine image (`$CS_ENGINE_IMAGE`, read-only, never committed). Evidence
report: `private/evidence/RECORD-OBJECTIVES-SOUND/acceptance.json` (validated
with `tools/validate_evidence.py --require-pass`), committed as
`docs/findings/evidence/RECORD-OBJECTIVES-SOUND.json`; its artifacts are the
acceptance log and `record-objectives-sound-census.json`, both under
`private/`.

**Everything about the original program below is static code evidence** from
disassembling the owner's decrypted executable. The original program was
never run for this task. Nothing here is `verified_original` runtime
behaviour (AGENTS rule 8), and where a semantic could not be reduced from the
code alone it is recorded as **unknown**, not guessed (AGENTS rule 4).

This is the same method, and the same image, as the M01-LC directive family
(`2026-10-06-m01-lc-directive-{a,b,c,d}-*.md`) and M02-B-FU2
(`2026-10-09-m02-b-fu2-record-sound-keys.md`). Finding D first located the
seven mission-level `*_SOUND` record keys; #801 measured and admitted the
five M02 spells. This task measures and admits the **other two** —
`OBJECTIVES_WON_SOUND` and `OBJECTIVES_LOST_SOUND` — completing the known
parser vocabulary.

## Provenance

| Item | Value |
| --- | --- |
| File | `$CS_ENGINE_IMAGE` (`crimson.decrypted.exe`), outside `$CS_GAME_DIR` since Rally #798 |
| Format | PE32, image base `0x400000` |
| Size | 2 580 480 bytes |
| SHA-256 | `43540fc97347210d6f4c10b77edbd4cdab1f03d57554d638223c2430a6c37d75` — byte-identical to the directive family's and M02-B-FU2's image, so all readings describe one binary, and equal to `cs_content::coordinates::ORIGINAL_IMAGE_SHA256`, which the acceptance test re-hashes on every run |
| Address convention | virtual address (VA); `.text` at `VA 0x401000` = file `0x1000` and `.data` at `VA 0x219000` = file `0x219000`, so for both sections this note reads, `file offset = VA − 0x400000` |
| Parser | the mission-file parse block of `0x466b70` (`mov ebx, ecx` = mission object, `xor edi, edi`), the same routine finding A measured |
| Name → handle | `0x596120(name)` — the runtime name-to-handle lookup M02-B-FU2 re-read |
| Record lookup | `0x57a090(name, container)` — the named-child lookup the parse calls for every record key |
| Outcome block | `CZMission::Update`, `this` in ebx; the end-of-tick block at `0x46af76`, the ended-flag gate at `0x46a9b7` |
| Mission end | `0x463c30(mission, ended, arg)`, `this` in ecx — the same routine M02-B-FU2 measured; here `ended` is `1` and `arg` is a float selected per branch |
| Sound play | `0x46caf0` through the mission sound manager global `0x71b438` |
| Retail census | production `cs_app::mission_control::survey_mission_control_programs` over the whole installation |
| Tools | `python3` for the section-table walk, the string and 32-bit-immediate-reference searches, the call-site census and the byte assertions the acceptance test now performs |

## The two keys, measured

| Key | String VA | Parse site | Field | Consumer site | Consumer |
| --- | --- | --- | --- | --- | --- |
| `OBJECTIVES_WON_SOUND` | `0x626010` | `0x466d74` | `+0xc70` | `0x46afd2` | end-of-tick outcome block, won branch |
| `OBJECTIVES_LOST_SOUND` | `0x626028` | `0x466da1` | `+0xc74` | `0x46af9f` | end-of-tick outcome block, lost branch |

Each string is referenced **exactly once** in the image, by the `push imm32`
in its own parse block — no second reader of either key's name, and
(searching the four-byte displacement over `.text`) no reader of either field
beyond the parse's own two writes and the outcome block's load. The parse
sites sit **between** `TERTIARY_COMPLETE_SOUND`'s (`0x466d47`, field
`+0xc6c`) and `MISSION_WON_SOUND`'s (`0x466dce`, field `+0xc78`), which is
why the vocabulary lists them fourth and fifth: the order is the parser's
own, read off the blocks' `jz`-to-next-key chain, not this task's choice.

## The parse blocks, byte for byte

Both keys are parsed by the same seven-instruction shape M02-B-FU2 measured
for its five (`push` at `+0x0`, recorded offsets from it):

```
0x466d74  68 10 60 62 00        push 0x626010        ; "OBJECTIVES_WON_SOUND"
0x466d79  55                    push ebp             ; the record node
0x466d7a  89 bb 70 0c 00 00     mov  [ebx+0xc70], edi; zero default
0x466d80  e8 0b 33 11 00        call 0x57a090        ; named-child lookup
0x466d85  83 c4 08              add  esp, 8
0x466d88  3b c7                 cmp  eax, edi
0x466d8a  74 15                 je   0x466da1        ; not spelled → next block
0x466d8c  8b 48 04              mov  ecx, [eax+4]
0x466d8f  8b 51 0c              mov  edx, [ecx+0xc]  ; the child's text
0x466d92  52                    push edx
0x466d93  e8 88 f3 12 00        call 0x596120        ; name → handle
0x466d98  83 c4 04              add  esp, 4
0x466d9b  89 83 70 0c 00 00     mov  [ebx+0xc70], eax; store the handle
0x466da1  68 28 60 62 00        push 0x626028        ; "OBJECTIVES_LOST_SOUND"
           …                    identical shape, field +0xc74,
                              its not-found `je` lands on 0x466dce — the
                              MISSION_WON_SOUND block's head
```

So the chain of `je +0x15` targets *is* the parser's key order: tertiary →
won → lost → mission-won. A record that does not spell a key keeps the zero
default; a spelled one stores the runtime handle `0x596120(text)` returns.
No tag check guards the text child — finding D's parse fragility, carried as
unknown rather than refused, exactly as M02-B-FU2 ruled.

## The consumer: `CZMission::Update`'s end-of-tick outcome block

The outcome block starts at `0x46af76`. `Update`'s head has already tested
the ended flag (`call 0x463c00` at `0x46a9b7`, `jnz` past the body), so the
block runs **at most once**, on the tick that settles the outcome:

```
0x46af76  8b cb              mov ecx, ebx           ; this
0x46af78  33 f6              xor esi, esi
0x46af7a  e8 71 8c ff ff     call 0x463bf0          ; lost flag = [this+0xc5c]
0x46af7f  85 c0              test eax, eax
0x46af81  74 28              je   0x46afab          ; lost clear → won test
; ---- lost branch ----
0x46af83…0x46af96            select a float (0.1f or 3.0f) on a stack flag
0x46af96  6a 01              push 1
0x46af98  8b cb              mov ecx, ebx
0x46af9a  e8 91 8c ff ff     call 0x463c30          ; end the mission
0x46af9f  8b 83 74 0c 00 00  mov eax, [ebx+0xc74]   ; OBJECTIVES_LOST_SOUND
0x46afa5  85 c0              test eax, eax
0x46afa7  74 47              je   0x46aff0          ; null handle → past play
0x46afa9  eb 31              jmp  0x46afdc          ; join the play block
; ---- won branch (0x46afab) ----
0x46afad  e8 2e 8c ff ff     call 0x463be0          ; won flag = [this+0xc58]
0x46afb2  85 c0              test eax, eax
0x46afb4  74 49              je   0x46afff          ; won clear → past all
0x46afb6…0x46afc9            the same float select, push 1
0x46afcd  e8 5e 8c ff ff     call 0x463c30          ; end the mission
0x46afd2  8b 83 70 0c 00 00  mov eax, [ebx+0xc70]   ; OBJECTIVES_WON_SOUND
0x46afd8  85 c0              test eax, eax
0x46afda  74 14              je   0x46aff0          ; null handle → past play
; ---- shared play block (0x46afdc) ----
0x46afdc  6a 00              push 0
0x46afde  6a 01              push 1
0x46afe0  68 00 00 80 3f     push 1.0f
0x46afe5  50                 push eax               ; the handle
0x46afe6  b9 38 b4 71 00     mov ecx, 0x71b438      ; mission sound manager
0x46afeb  e8 00 1b 00 00     call 0x46caf0          ; play
0x46aff0  b9 68 b4 71 00     mov ecx, 0x71b468      ; a second manager
0x46aff5  e8 c6 15 00 00     call 0x46c5c0
0x46affa  be 01 00 00 00     mov esi, 1
0x46afff  …
```

The three flag accessors are bare stubs, all measured:

```
0x463be0  mov eax, [ecx+0xc58]; ret   ; won flag
0x463bf0  mov eax, [ecx+0xc5c]; ret   ; lost flag
0x463c00  mov eax, [ecx+0xc54]; ret   ; ended flag
```

**Measured:**

- The **lost** flag is tested first. Its set branch runs `0x463c30` and then
  reads `+0xc74`; a non-null handle joins the shared play call.
- Only when the lost flag is **clear** is the won flag tested. Its set
  branch runs the same `0x463c30` and reads `+0xc70`; a non-null handle
  falls through into the same play call.
- A tick with neither flag jumps the whole block (`0x46afff`); a null handle
  skips only the play call (`0x46aff0`), not the second manager call or the
  `esi = 1` that follows.
- The mission-end call `0x463c30` is the routine that sets `+0xc54`
  (measured in M02-B-FU2), and `Update`'s head gate means a second tick
  cannot re-enter the block — the sound is a once-only edge, not a
  while-flag-is-set poll.
- The float pushed to `0x463c30` (0.1f or 3.0f, selected on stack flag
  bytes) and the `0x71b468`/`0x46c5c0` call after the outcome are context,
  not part of this measurement — see Unknowns.
- `0x46caf0` is the same sound-play routine the objective-completion chain
  and mission end use (findings D and M02-B-FU2); the arguments measured
  here are `(0, 1, 1.0f, handle)`.

**Which branch played is not the recorded result.** Finding F37-D-FU2
measured that the flags' writers and the recorded outcome are separate
latches — the same warning M02-B-FU2 carried for `MISSION_LOST_SOUND`
applies here with the opposite sign: when both flags are set this block
takes the *lost* branch (it is tested first), and the recorded result is
unaffected by which handle played.

## The retail census: no control member spells either key

`survey_mission_control_programs` walks every mission-scoped reader archive
in the owner's installation (53 rows, 40 with a measurable control member)
and measures the chosen member's record. Over that census:

- **zero** measured rows report either key in `record_sounds()`;
- **zero** rows report either spelling in `unclassified_record_keys()` —
  which, after admission, they cannot anyway;
- an independent re-walk (`discover_container` → `decode_zrd` →
  `zrd_flat_fields` over every measured row's control member) spells neither
  key at record level in any member either;
- the walk is not vacuous: the same corpus spells the five M02-B-FU2 keys
  (M02 carries all five).

So the admission completes the parser's **known** vocabulary — seven keys —
without changing any retail mission's classified content: a record that
spells one of these keys would be classified and measured the day it ships,
and today none does.

## What RECORD-OBJECTIVES-SOUND adds, in owner paths

- `crates/cs_content/src/mission_control.rs` —
  `CONTROL_RECORD_SOUND_KEY_VOCABULARY` grows from five to seven (in the
  parser's own order, measured off the `je`-to-next-key chain);
  `ControlRecordSound` gains `ObjectivesWon` and `ObjectivesLost`;
  `RecordSoundConsumer` gains `ObjectivesOutcome { won }` for the
  end-of-tick block; `record_sound_disposition` gains both measured
  dispositions with consumer, field offset, parse site, consumer site,
  summary, evidence and residual unknowns. `ControlRecordField::Sound`
  classifies both keys automatically through `ControlRecordSound::from_key`,
  and `record_sounds`/`record_sound_shapes` count them the day a record
  spells one. No `MeasuredControlRecord` field is added; the classification
  changes nothing for a record that spells neither key.
- `crates/cs_app/tests/campaign/record_objectives_sound.rs` — this task's
  three `accept_record_objectives_sound_*` tests, below.
- `crates/cs_app/tests/campaign/m02_b.rs`,
  `crates/cs_app/tests/campaign/m02_b_fu2.rs` — the assertions whose premise
  changed ("the sound vocabulary is M02's five"): the retail vocabulary test
  now asserts M02 spells five **of seven** by membership rather than by
  count against the constant, the image test's base-register check gains the
  new consumer (ebx, like the class chain's), and the partition test's
  unadmitted arm moves to a key no original spells while the consumers now
  partition 3 classes + 2 mission ends + 2 objectives outcomes.
- `crates/cs_app/tests/campaign/evidence.rs` — the test lists and
  `evidence_report_record_objectives_sound_writes_the_acceptance_report`,
  which writes `private/evidence/RECORD-OBJECTIVES-SOUND/{cargo-test.log,
  record-objectives-sound-census.json, acceptance.json}` and is validated
  with `tools/validate_evidence.py --require-pass`; the committed copy is
  `docs/findings/evidence/RECORD-OBJECTIVES-SOUND.json`. One text clause of
  M02-B-FU2's own harness text is corrected (its report said the two keys
  "stay outside every vocabulary"; after this task that is no longer true,
  so the clause now names #808), in the same way #801 corrected M02-B's.
- Wiring only (AGENTS rule 1): `crates/cs_app/tests/campaign/main.rs` (one
  `mod record_objectives_sound;` and a doc paragraph).
- No `Cargo.toml` or `Cargo.lock` change; `missions/bindings/` is unchanged.

## Test inventory (`accept_record_objectives_sound_*`, 3 tests)

| Test | Kind | What it pins |
| --- | --- | --- |
| `accept_record_objectives_sound_the_two_keys_are_measured_with_their_outcome_consumer` | synthetic (CI) | the vocabulary is the parser's seven keys in parse order and is disjoint from the field vocabulary; `ALL` matches it; `key`/`from_key` round-trip both keys; `ControlRecordField::from_key` classifies both as `Sound`; both dispositions are `Measured` with `ObjectivesOutcome { won }`, `objectives_outcome` code, no class, the four finding citations, unknowns and nonzero addresses; the field offsets are `+0xc70`/`+0xc74` beside `MISSION_WON_SOUND`'s `+0xc78` |
| `accept_record_objectives_sound_the_image_gates_and_consumes_both_keys_at_end_of_tick` | engine image (`CS_ENGINE_IMAGE`) | the image hashes to the recorded digest; each parse site pushes its own NUL-terminated string, writes the zero default to `field_offset`, calls `0x57a090` and `0x596120` and stores the handle to `field_offset`; the `je` chain links won → lost → `MISSION_WON_SOUND`; the three flag accessors read `+0xc5c`/`+0xc58`/`+0xc54`; `Update`'s head gate calls the ended accessor; the block tests lost first, its clear arm lands on the won test, each branch calls `0x463c30`, the `consumer_site` loads are `mov eax,[ebx+field]`, each null-handle `je` skips the play call, the lost arm joins it, the won-clear arm leaves entirely, and the play block pushes `(0, 1, 1.0f, handle)` through `0x71b438` → `0x46caf0` |
| `accept_record_objectives_sound_no_retail_control_member_spells_either_key` | retail (`CS_GAME_DIR`) | the production census measures ≥ 30 rows; no measured row's `record_sounds()` carries either variant and none names either key unclassified; the corpus spells sound keys at all (M02); and an independent member re-walk through `discover_container`/`decode_zrd`/`zrd_flat_fields` spells neither key in any control member |

Every test calls production code (`record_sound_disposition`,
`ControlRecordSound`, `ControlRecordField::from_key`,
`survey_mission_control_programs`, `discover_container`, `decode_zrd`,
`zrd_flat_fields`). The retail member is
`#[ignore = "requires CS_GAME_DIR"]`, the image member
`#[ignore = "requires CS_ENGINE_IMAGE"]`; CI runs the synthetic one. Each
fails if its half of the implementation is removed: the synthetic test panics
on `None`/`Refused` dispositions, the image test reads the recorded
addresses, and the census test depends on `record_sounds()` classifying
either key.

## Mutation probes

Applied one at a time against the final design, each reverted before the
next; the tree carried none of them afterwards. Selection:
`cargo test --locked --test campaign -- accept_record_objectives_sound_
--include-ignored`.

| Mutation | Observed result |
| --- | --- |
| `CONTROL_RECORD_SOUND_KEY_VOCABULARY` drops the two keys | **the whole `campaign` test binary fails to compile**: the synthetic member pins the constant to a 7-element literal, so the admission is load-bearing before any assertion runs |
| `record_sound_disposition` answers `Refused` for the two keys | **2 of 3 fail** — the synthetic and image tests at *"OBJECTIVES_WON_SOUND is in the vocabulary and measured"* — **plus 2 of 3 of `accept_m02_b_fu2_*`** (its vocabulary test now sees a vocabulary key answer `Refused`, and its image test unwraps the disposition). The census member survives: `record_sounds()` classifies through `from_key`, not the table — correct, since the census asks what a record *spells* |
| `ObjectivesWon`'s `consumer_site` moved one byte (`0x46afd2` → `0x46afd3`) | **1 fails**: the image test at *"OBJECTIVES_WON_SOUND's consumer site is the field load in the outcome block"* (`left: 4632531` = `0x46afd3`, `right: 4632530` = `0x46afd2`) — the recorded site must point at the `mov`, not inside it |
| `ObjectivesOutcome { won }` flags swapped between the two dispositions | **1 fails**: the synthetic test's consumer equality — `won` is the branch selector, not an address, so it is the table-level lane that catches it |

## Checks

| Command | Exit |
| --- | --- |
| `cargo fmt --all -- --check` | 0 |
| `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings` | 0 |
| `cargo test --workspace --locked` | 0 |
| `cargo test --workspace --locked -- accept_record_objectives_sound_ --include-ignored` | 0 (3 tests) |
| `python3 tools/validate_evidence.py private/evidence/RECORD-OBJECTIVES-SOUND/acceptance.json --artifact-root private/evidence/RECORD-OBJECTIVES-SOUND --require-pass` | 0 (`structurally_valid: true`) |

## Recorded unknowns (not guessed)

- **What sound a name resolves to.** Both keys are name → handle lookups
  whose result is runtime state; no shipped file declares a name's sound,
  and — since no retail record spells either key — even the *names* that
  would be resolved are unmeasured for these two. Resolved by: an
  ordinary-play reference capture on a mission that spells them, which is
  blocked on the owner.
- **Which branch ran is not the outcome.** The flags' writers — the outcome
  tally over `+0x554`'s WON/LOST classes and the instant outcome classes —
  belong to findings A/B and F37-D-FU2; the sound selection does not decide
  or record the result (when both flags are set the lost branch runs while
  the recorded result is still success).
- **The `0x463c30` arguments and the second manager call.** Each branch
  passes a flag byte and selects a float (0.1f or 3.0f) for the mission-end
  routine, and `0x46c5c0` runs on manager `0x71b468` whenever either flag was
  set. Their roles are context — the block's *sound* effect is measured —
  but what they do is outside this measurement.
- **A non-text value beside one of these keys is not a refusal.** The
  original's parse reads the child's payload without a tag check (finding
  D's parse fragility). Refusing on shape would invent a rule the original
  does not have, so a non-`[text]` value would be *counted with its measured
  shape* instead; no retail record measured here spells one.
- **No engine operation plays these keys.** `Measured` is a statement about
  the original, not support: nothing in this change lowers or emits a sound.
- **No original run, and nothing `verified_original`.** Every claim above is
  static code evidence from one byte sequence (`43540fc9…`), read on
  2026-10-06 by the directive family, on 2026-10-09 by M02-B-FU2 and again
  here.

## Sources

`$CS_GAME_DIR` read-only through `cs_assets::install::discover` and
`cs_app::mission_control::survey_mission_control_programs`, with the
independent member walk through `cs_formats::script_raw::discover_container`,
`cs_content::stunts::decode_zrd`, `objective_record` and `zrd_flat_fields`;
`$CS_ENGINE_IMAGE` read-only (section table, string and
32-bit-immediate-reference searches, call-site census of
`0x463be0`/`0x463bf0`/`0x463c00`/`0x463c30`, byte disassembly of
`0x466d40`…`0x466e10`, `0x463be0`…`0x463c40`, `0x46a9a8`…`0x46a9d8` and
`0x46af76`…`0x46b010`); `missions/M02.md`;
`docs/contracts/SCRIPT-MISSION.md`; `docs/contracts/CLI-EVIDENCE.md`;
`docs/findings/2026-10-09-m02-b-fu2-record-sound-keys.md`;
`docs/findings/2026-10-06-m01-lc-directive-d-sound-help-timer-directives.md`;
`docs/findings/2026-10-06-m01-lc-directive-b-objective-lifecycle-target-semantics.md`;
`docs/findings/2026-10-06-m01-lc-directive-a-objective-directive-parser.md`;
`docs/findings/2026-10-07-f37-d-fu2-mission-terminal-precedence-and-tick-ordering.md`;
`crates/cs_content/src/mission_control.rs`;
`crates/cs_app/src/mission_control.rs`.

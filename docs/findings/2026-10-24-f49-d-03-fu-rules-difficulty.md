# F49-D.03-FU-RULES-DIFF (#1640): how an original Instant Action scenario meets a difficulty

Date: 2026-10-24. Task: #1640. Follows `2026-10-03-f32-d-original-ai-roles-and-difficulty.md`
(no `ia.zrd` key names a difficulty; the option is a three-step selector) and
`2026-10-18-f49-d-03-fu-faction-survivability.md` (the spawn loop sets
`GameIntensity` per group). Capabilities: `retail` and `CS_ENGINE_IMAGE`, read
statically. Nothing was run, rendered or played. Provenance of every row below:
`static`, never `verified_original`. Virtual addresses are in the decrypted image
(`image_va_base 0x400000`). No disassembly or code is committed; the rows are
facts and addresses.

## Answer

**The scenario does not bind one difficulty.** The engine has one global the
difficulty option lives in — the console variable `GameIntensity` — and the
Instant Action spawn routine *overwrites it around each spawn call*, so the
player's option value is not what an Instant Action actor is created under:

| actors | value `GameIntensity` holds while they are created | where it comes from |
| --- | --- | --- |
| the ace | `0` for `novice`, `1` for `veteran`, `2` for `ace` | the descriptor key `ace_skill` (label strings `0x62562c` / `0x625634` / `0x62563c`), stored in the scenario record at `+0xd0` (`0x718da8`, record `this = 0x718cd8`), mapped 0/1/2 -> 0/1/2 at `0x45acbe..0x45acfc` |
| each of the four enemy groups | `1` | the group record's last dword (`+0x5c`), set to `1` by the group initialiser (`0x458ddd`), written by no reader |
| the player and the wingmen | not measured | not traced |

The `enemy_skill` key that all eight retail `ia.zrd` descriptors carry in their
group records is **not read by the executable**: the string `enemy_skill` occurs
nowhere in the image (the option name `EnemySkill`, `0x623824`, is a different
console variable). The group reader (`0x458e00`) asks for `num_enemies`,
`enemy_name`, `enemy_plane`, `enemy_accentID` only. So the retail group skill
labels are authored data the original does not consume (measured absence), and
the per-scenario difficulty an Instant Action player experiences is: *ace
follows `ace_skill`, groups are always `1`*. No scenario-level difficulty
exists to put in `ScenarioParameters::difficulty`.

## Measurement

| fact | VA / bytes |
| --- | --- |
| the option: `GameIntensity` registered through `0x5bde90`, pointer kept at `0x64f720` | `push "GameIntensity"` at `0x43ff27` (`68 cc 36 62 00`), `mov [0x64f720], eax` at `0x43ff36`; name at `0x6236cc` |
| getter / setter | `0x440710` reads `*[0x64f720]`, `0x440700` writes it; the setter is called with `0` at start-up (`0x43ff3e`) |
| saved and restored around a launch | `0x418d48` reads it into `0x64b648` (with `0x71dacc`); `0x419185` writes `0x64b648` back |
| Instant Action ace spawn | `0x45acbe` read old value, `0x45acc5` `mov eax, [0x718da8]`, map `0->0, 2->2, else 1`, `0x45ace3` set, `0x45acf6` `call 0x47c210`, `0x45acfc` restore |
| Instant Action group spawn | `0x45b100` read old value, `0x45b107` `mov eax, [ebp+0x3c]` (group `+0x5c`), same map, `0x45b120` set, `0x45b133` `call 0x47c210`, `0x45b139` restore |
| `ace_skill` reader | `0x45972b` looks up `ace_skill` (`0x625620`); `0x459750..0x459794` compare the text with `novice`/`veteran`/`ace` and store `0`/`1`/`2` at `this+0xd0` |
| the consumer of the value | `0x47c210` (the spawn call), at `0x47cb32` / `0x47cb42` |

What the consumer does with it (`0x47c210`, only for an object whose `+0x8`
differs from the result of `0x4830c0` with both non-zero — the test is not
interpreted here; the other branch jumps over the read at `0x47cb30`):

* `GameIntensity` `0` -> `-2`, `2` -> `+2`, `1` -> `0`, held as a step `s`.
* If `s != 0`: two object fields (`+0x2c4`/`+0x2c8`, `+0x2cc`/`+0x2d0`) are
  multiplied by `1 + s * 0.125` (the `float` at `0x60802c` is `0.125`, the one at
  `0x6032dc` is `1.0`), and six more paired fields named by the strings at
  `0x6283b4..0x6283ec` are scaled by the same factor.
* The object's AI skill level (a descriptor value or a random draw, `0..9`) has
  `s` added and is clamped to `0..9` (`0x47ce17..0x47ce2c`); each AI skill
  parameter is then `lo + level * (hi - lo) * (1/9)` (the `float` at `0x608028`
  is `1/9`) from the pair table at `0x71c490..0x71c4dc`, whose defaults are
  immediates at `0x47473d..0x4747dc` and whose overrides come from the
  `ai_skill_parameters` record (`0x627a04`) read at `0x47473d..0x4749d6`.

So the option moves the skill level by `+-2` of nine steps and some paired
fields by `+-25 %` — as read, not run. This task does **not** lower any of that
into a `DifficultyProfile` (see "What changed in code").

## What changed in code

`cs_app::ui::instant_action::DIFFICULTY_UNMEASURED_REASON` replaces the old
reason ("whose binding to a scenario is unmeasured") with this measurement. A
retail preset's `difficulty` stays `Declared::Unmeasured` under claim
`f49.d.03.unmeasured.difficulty`, so `lower_preset` still refuses it by name.

It stays unmeasured, not set, because (a) there is no scenario-level value (the
ace and the groups differ and the player/wingmen are untraced), (b) a
`DifficultyTier` would be a project design (F32-E: tier knobs are *designed
alternatives*), and (c) what the `+-2` step does in the original is read but not
exercised. Lowering the ace/group split needs per-actor intensity in the
scenario schema (a separate task), not a preset-level profile.

Tests: `accept_f49_d_03_fu_rules_difficulty_*`
(`crates/cs_app/tests/accept_f49_d_03_fu_rules_difficulty.rs`). The image test
re-reads the bytes above independently of the production constants.

## Unknowns that remain

| unknown | note |
| --- | --- |
| what the player and wingmen are created under | the player/wingman spawn loops were not traced for a `GameIntensity` write |
| the default of the ace record's `+0xd0` when `ace_skill` is missing | the scenario record initialiser was not read; all eight retail descriptors carry `ace_skill`, so no retail preset reaches it |
| why `0x47c210` applies the step only when `+0x8` and `0x4830c0()` differ | looks like an own-side/other-side test; not interpreted |
| how options ids 109..111 write `GameIntensity` | the option-screen binding was not traced; the name `GameIntensity` and the three-step selector are an inference, not a measured link |
| the second consumer pair `0x418d52` (`EnemySkill` variable at `0x64f774`, mapped 6/7/other) | game-start code outside the Instant Action setup; unread |
| what `IDS_IA_DIFFICULTY` (3695) is bound to | a label; its binding to the ace/group values is not measured |

Sources: the decrypted image at the VAs above; the F32-D, F32-E and F49-D.03
follow-up findings.

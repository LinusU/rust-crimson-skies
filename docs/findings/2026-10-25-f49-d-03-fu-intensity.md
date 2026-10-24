# F49-D.03-FU-INTENSITY (#2055): the per-actor `GameIntensity` of an Instant Action actor

Date: 2026-10-25. Task: #2055. Follows `2026-10-24-f49-d-03-fu-rules-difficulty.md`
(#1640). Capabilities: `retail` and `CS_ENGINE_IMAGE`, read statically. Nothing was
run, rendered or played. Provenance of every row: `static`, never `verified_original`.
Addresses are virtual addresses in the decrypted image (`image_va_base 0x400000`).
No disassembly is committed; the rows are facts and addresses.

## Answer

The scenario spawn routine `0x45a390` creates three kinds of actor through the spawn
call `0x47c210`. Only two are bracketed by a `GameIntensity` write:

| actors | spawn call | `GameIntensity` while they are created |
| --- | --- | --- |
| the wingmen (loop over the count `[0x718ce0]`, plane `[0x718ce4]`) | `0x45a9a1` | **no write around the call**: the ambient value, i.e. whatever the player's option holds |
| the ace | `0x45acf6` | `0`/`1`/`2` from `ace_skill` (read `0x45acbe`, set `0x45ace3`, restore `0x45acfc`) |
| each enemy group | `0x45b133` | `1` (read `0x45b100`, set `0x45b120`, restore `0x45b139`) |

Why the wingmen run under the player's option: the only callers of the setter `0x440700`
are `0x419185` (restore after launch), `0x43ff3e` (start-up, `0`), `0x4404a2` (a
read-then-write sequence in the settings code), and the four ace/group sites above; the
getter `0x440710` is called from `0x418d48`, `0x44049c`, the two bracket reads and the
consumer `0x47cb32`/`0x47cb42`. No call lies between the routine's entry `0x45a390` and
the wingman spawn call `0x45a9a1`, and the ace and group brackets restore the old value,
so the value the wingmen are created under is the one the option screen left in the
variable. The scenario carries no value for them.

`0x47c210` has further callers outside this routine (`0x451f15`, `0x47531e`, `0x47b74a`,
`0x47ff32`); they were not read. The Instant Action *player's* own aircraft is not created
by `0x45a390`'s three sites, so it is **not traced**; no `GameIntensity` write exists for it outside the sites
above, so if it is created through `0x47c210` at all it is also under the ambient value,
but that inference is not measured.

## What changed in code

* `cs_content::instant_action::GameIntensity` (a value `0..=2`) and `ActorIntensity`
  (`Set(GameIntensity)` or `PlayerOption`), carried by `ScenarioActorSpec::with_intensity`.
* `Declared::PerActor`: a preset whose difficulty is carried per actor.
* The retail catalog gives every wingman `PlayerOption`, every enemy group `Set(1)`,
  the ace `Set(0|1|2)` from `ace_skill` (novice/veteran/ace). The player carries none.
  `lower_preset` no longer refuses on difficulty; the lowered scenario's difficulty is
  `LoweredDifficulty::PerActor` and each lowered actor carries its `intensity`. A custom
  scenario keeps its scenario-level tier; the retail per-actor values do not apply
  under it.

What the value does downstream (skill level step -2/0/+2, paired fields scaled by
1 + step * 0.125) is still read, not run, and is not modelled; no `DifficultyTier` is
derived from it.

## Unknowns that remain

| unknown | note |
| --- | --- |
| the Instant Action player aircraft's creation | not traced (see above) |
| `ace_skill` default when absent | no retail descriptor lacks it; an ace without it keeps no intensity and refuses by name |
| how option ids 109..111 write `GameIntensity` | unchanged from #1640 |
| the model of the value's effect | unchanged from #1640 |

Tests: `accept_f49_d_03_fu_intensity_*` (`crates/cs_app/tests/accept_f49_d_03_fu_intensity.rs`).

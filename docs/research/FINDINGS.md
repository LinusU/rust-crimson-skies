# Findings, confidence and research boundaries

## What is already useful

The inspected MM2 project supplies a good workflow baseline: pure parsers, a content pipeline, pre-commit quality gates, evidence classifications and deterministic capture support. Its configured Bevy/Avian pair is 0.19/0.7. Its large evolving Ralph plan should **not** be copied as one huge Bonsai prompt. This pack instead gives one task packet at a time. [S01](SOURCES.md#s01-mm2-repository-baseline)

Crimson Skies extraction work points to shared aircraft geometry in `ZBD/PLANES.ZBD`, world-specific `gamez.zbd`, texture containers and `GOSDATA/ASSETS/crimson.rof`. The exporter explicitly visits `c1`, `c1b`, `c1c`, `c2`, `c2b`, `c3`, `c4`, `c5`. These are **eight storage/world groups**, not eight campaigns or eight missions. Runtime discovery, namespacing and closure checks supersede a hardcoded directory loop. [S03, S04](SOURCES.md)

The most important version trap is mech3ax: CS GameZ support exists in the legacy 0.6 line and was removed in the 0.7.0-rc1 history. The working Blender pipeline pins 0.6.0. The resolved commit is `d3521a9721be731d365504568ddcd78e3f9846bb`. Do not replace it with latest and assume feature equivalence. The changelog year conflicts with the commit date; use identity, not chronology, for the oracle lock. [S02, S03, S17](SOURCES.md)

The Blender scripts merge textures by basename, substitute textures, drop some problematic faces and apply Blender-specific axis conversion. These are practical export choices, not permissions to lose original scene information in the engine. Preserve origin namespaces, topology and raw metadata. [S03, S08](SOURCES.md)

## Original-product discovery cues

The original manual describes campaign/Instant Action/multiplayer, construction and loadout selection, cabin/scrapbook progression, docking, flight instruments, targeting/spyglass and nitro control. The mission guide provides a 24-mission discovery sequence. The companion independent guide is based on German v1.02 at hard difficulty. These sources identify required systems but do not supply a verified program, object-id map, exact timer table or damage equations. Mission sheets intentionally omit invented numerical details. [S13-S15](SOURCES.md)

The airframe-prefix helper covers HOPLITE, BALMORAL, BLOODHAWK, BRIGAND, DEVASTATOR, FIREBRAND, FURY, HELLHOUND, KESTREL, PEACEMAKER and WARHAWK. This is a useful discovery list; menu availability and forced mission configurations must be resolved separately. Livery construction is layered and faction-aware, not just loading a single texture per plane. [S09, S10](SOURCES.md)

## Critical unknowns

| Unknown | Why it matters | Closure owner |
|---|---|---|
| Actual install/patch/locale hashes and full inventory | Fixes the denominator and compatibility profile | F02/F14 |
| ROF compressed length-field interpretation | Avoids reading adjacent entries or truncating streams | F05 |
| Complete CS GameZ variants, material flags and collision roles | Prevents visible/collision omissions and false materials | F10/F11/F18 |
| Coordinate handedness, scale and angle units | A visually plausible world can have entirely wrong motion | F16/F26 |
| Mission programs, opcodes, host behavior and timing | Required for an actual campaign rather than generic fights | F13/F37/F38/F39 |
| Camera/mission animation layouts | Controls docking, capture, cinematic and world transitions | F20/F40 |
| Flight/damage/weapon tuning and difficulty semantics | Required for faithful handling and combat | F24-F32 |
| Full IA/multiplayer catalogs and rule variants | Cannot infer from the Xbox sequel or a trial | F49/F56 |
| Complete media/codecs/strings/fonts for the chosen locale | Prevents silent story/UI/audio gaps | F12/F40/F41/F51 |
| Original save/custom-plane structures | Read-only compatibility, never guess-and-overwrite | F64 |

## Avoid misleading reference claims

The separate static-recompilation repository is a research lead, not the requested architecture and not proof that original gameplay is solved. Its own status leaves asset/gameplay work pending. Its statements about GOS/GW/ROF/media are not sufficient to assert a mission ABI. Do not incorporate lifted game source or DRM code. [S12](SOURCES.md)

`INTERP.ZBD` is demonstrably a loading-script container in the inspected extractor interface. Its structured tokens may lead to other resources; this does not establish a universal mission VM. The concrete observed subset is documented in FORMAT-NOTES.md. [S06, S07](SOURCES.md)

No original data was supplied or processed here. All supplied binary fixtures are newly constructed synthetic data. No archived manual, external source implementation or retail asset is redistributed.

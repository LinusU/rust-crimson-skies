# Research sources

Accessed 2026-09-21. Source observations are not retail verification. Links point to external reference material; none of that source material is bundled. File blob ids identify inspected content when the branch URL is mutable.

## S01: MM2 repository baseline

https://github.com/LinusU/rust-mm2/tree/5e0751a17fc46bf751f04aa6984a947cf30ff1fb

Inspected AGENTS.md, Cargo.toml, docs/ralph/PLAN.md and directory metadata. Reuse workflow principles, not MM2 game formats.

**Evidence:** Repository snapshot; code/configuration observation.

## S02: mech3ax project and compatibility history

https://github.com/TerranMechworks/mech3ax

Current README/CHANGELOG plus legacy v0.6.0 source. CS support varies by subcommand and version. EUPL-1.2; do not assume permissive code reuse.

**Evidence:** Source implementation and author-maintained history; supported-corpus limits apply.

## S03: Crimson Skies to Blender pipeline

https://github.com/rozab/crimsonskies2blend/blob/main/everything2blend.py

File blob 32f936506045088f3b080690bcf1858a25864308. Observed v0.6.0 extractor invocation, world-group list, PLANES.ZBD and texture merging.

**Evidence:** Working community source; not a complete engine or parity oracle.

## S04: Crimson Skies Blender project documentation

https://github.com/rozab/crimsonskies2blend

Explains installation-based extraction and reported broken/missing geometry/material cases.

**Evidence:** Community author report; validate locally.

## S05: ROF extractor source

https://github.com/rozab/crimsonskies2blend/blob/main/extract_rof.py

File blob 3cd197de1fc6ff00d18cd832160316ec79cd5d8f. Directory structure, flags and zlib use observed; compressed length semantics not resolved.

**Evidence:** Source observation; no supplied retail corpus was tested.

## S06: mech3ax CLI and supported families

https://github.com/TerranMechworks/mech3ax/blob/main/README.md

Reader, sounds, interp, textures and other family-specific subcommands. Output schemas are explicitly unstable.

**Evidence:** Project documentation; do not assume every game supports every command.

## S07: Legacy INTERP parser

https://github.com/TerranMechworks/mech3ax/blob/d3521a9721be731d365504568ddcd78e3f9846bb/crates/mech3ax-interp/src/interp.rs

File blob 2f6ecb6c2021c076e3d896efe3470952acee50f8. Header, index and NUL-separated line representation inspected.

**Evidence:** Exact pinned source; still requires CS input verification.

## S08: Plane mesh/material conversion reference

https://github.com/rozab/crimsonskies2blend/blob/main/plane2blend.py

File blob 0a17bdbf79ac1876a78d00fffc53fa764096a7e8. Scene/mesh/material fields and Blender-specific conversion observed.

**Evidence:** Exporter contains simplifications and omissions; not a runtime fidelity specification.

## S09: BM layer extractor

https://github.com/rozab/crimsonskies2blend/blob/main/extract_bm.py

File blob ec196de05f532cc3c286ccf8fead363bf76e5c63. Height-first dimensions and RGB/masks/RGBA planes.

**Evidence:** Observed subset; extra variants remain unverified.

## S10: Livery composition helper

https://github.com/rozab/crimsonskies2blend/blob/main/set_paintjob.py

File blob ccf7c4ea065c17a44d354e8d703561cdc94dd518. Airframe prefixes, faction masks and composition behavior.

**Evidence:** Tool behavior, not proof of original palettes or blending precision.

## S11: Avian API documentation

https://docs.rs/avian3d/latest/avian3d/

Official crate documentation consulted. Use the exact version resolved during F00; MM2 baseline is Bevy 0.19 / Avian3d 0.7.

**Evidence:** Mutable official API documentation; bootstrap compilation remains mandatory.

## S12: Separate static-recompilation project

https://github.com/sp00nznet/crimsonskies

README blob 722a04012718574b99c3a8a77df6207a01901d2a. Asset/GOS/program references are investigation leads; its own status lists gameplay and asset work pending.

**Evidence:** Author report, not a completed game or verified mission-format specification. Do not import generated game code.

## S13: Original PC manual, HTML rehost

https://manualmachine.com/gamespc/crimsonskies/1119420-user-manual/

Original authored manual; use printed sections on campaign, construction, briefing, flight, instruments, targeting and multiplayer. Read in HTML, not bundled.

**Evidence:** Primary game documentation rehosted by a third party; exact target-edition behavior still needs observation.

## S14: Aaron Cloutier firsthand mission guide

https://gamefaqs.gamespot.com/pc/914280-crimson-skies/faqs/9275

Version 0.95; page updated 2001-01-16. Used for mission discovery labels and coverage cues, not authoritative scripts, timings or counts.

**Evidence:** Firsthand observations with acknowledged uncertainty and occasional spelling errors.

## S15: Independent German firsthand walkthrough

https://www.kultloesungen.de/doku.php/crimsonskies

Author reports German v1.02, hard difficulty. Useful independent branch/behavior checks.

**Evidence:** Different language/difficulty; do not silently merge its observations with another edition.

## S16: PrismML model/server documentation

https://docs.prismml.com/models/bonsai-27b

Official documentation describes OpenAI-compatible tool calls. Exact Bonsai 2 model id, weights, template and server build must be selected and probed locally.

**Evidence:** General current family documentation; not a benchmark or guarantee about the users chosen quantization.

## S17: Resolved legacy extractor release

https://github.com/TerranMechworks/mech3ax/commit/d3521a9721be731d365504568ddcd78e3f9846bb

Tag v0.6.0 resolved through the commits API to d3521a9721be731d365504568ddcd78e3f9846bb. Commit date is 2024-02-10; CHANGELOG prints a conflicting year. Pin the commit, not the date.

**Evidence:** Git commit identity independently resolved; no reference binary bundled.

## S18: Bonsai 2 official release announcement

https://prismml.com/news/bonsai-2-27b

Published 2026-09-17. Identifies Bonsai 2 as the Qwen3.8-based model, distinct from the earlier Bonsai 27B. Used for exact family identification, not as evidence that this coding project will succeed.

**Evidence:** Official release announcement; performance claims were not locally benchmarked.

## S19: Official Bonsai demo native tool-calling documentation

https://github.com/PrismML-Eng/Bonsai-demo/blob/main/TOOLS.md

Documents standard OpenAI tools/tool_calls round trips, the llama-server Jinja template and native MLX tool-call support. The associated README describes Bonsai 2 as the current default and its model-format/fork requirements.

**Evidence:** Official serving documentation; mutable branch, exact local server/model/template must still be probed.

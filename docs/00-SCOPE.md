# Scope and completion contract

## Product identity

The target is an independent reimplementation of **Crimson Skies for Windows (2000)** using an owner-provided original installation as the content source. It is not a wrapper around the original executable, a static recompilation, a byte-matching decompilation, the tabletop game or the Xbox sequel. No original executable or DLL is executed by the new runtime.

The engine should reproduce the authored playable game: campaign and its conclusion, original worlds and aircraft, combat and mission interactions, construction/loadouts, progression/scrapbook, original presentation/media, Instant Action and the original PC multiplayer content over a new networking layer. Improving resolution, usability, stability and optional handling does not excuse changing mission semantics.

## Meaning of "all the data"

Every source file and every parsed record is accounted for. Not every byte is meant to become a visible object: installers, platform DLLs, debug leftovers and unused resources can be classified with evidence. Every resource **reachable from supported original playable content** must be parsed, normalized, validated and consumed correctly. An unused classification requires a reason and a reference/reachability report. Unknown is not unused.

Use a fixed baseline inventory. A report that parses 20 missions, filters out four failures and then claims 20/20 is invalid. A world model without collision, sounds, mission program and transitions is not a playable world. A supported decoder without a runtime consumer is not a completed feature.

## Three product milestones

**Vertical slice:** one real original mission with its actual world, aircraft, combat, audio, objectives, success/failure and restart. It is not a release and cannot hide unsupported assets behind developer placeholders.

**Complete single-player candidate:** the entire campaign, ending, IA, construction, records, media and save/navigation flows work. Multiplayer progress is reported separately. This milestone must not be advertised as the complete Ultimate Edition while required multiplayer work remains.

**Complete-playable Ultimate release:** all required feature and content inventories pass, multiplayer works over the new protocol, supported platforms are tested, private-original dependencies are diagnosed, no proprietary assets are distributed, and the owner approves the candidate based on real play, visual and audible evidence.

## Designed improvements

Modern resolution/FOV, antialiasing, optional visual enhancements, remapping, controller/mouse choices, subtitles/UI scale, device recovery, diagnostics and opt-in mods are explicit enhancements. A more forgiving or refined flight profile is a named option; the reference-comparison profile remains separate. Neither profile claims original source-equation identity until measured.

Texture upscaling/generation, new campaigns, new fiction, voice replacement, open-world redesign, mandatory online services, VR, split-screen, sequel mechanics and legacy network wire compatibility are not assumed requirements. Existing old-save import is an optional compatibility extension; original custom-plane data needed by the supported content is not optional.

## Evidence limits at authoring

Research used the linked MM2 repository, public source code, the original manual as an HTML rehost, firsthand walkthroughs and official engine/model documentation. No user retail files were supplied. No original Crimson Skies executable was run. No Rust game was compiled by the pack author. The game-format references are valuable but incomplete. In particular, original mission program semantics and some archive details remain discovery gates.

This pack specifies how to close those gaps and what must fail until then. It does not falsely fill them with plausible constants, scene coordinates, opcode tables or mission scripts.

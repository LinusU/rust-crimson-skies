# Architecture and ownership

All statements here are **project design**, except dependency baseline observations cited in research.

## Workspace

| Crate | Owns | Allowed project dependencies |
|---|---|---|
| `cs_types` | ids, source spans, units, immutable cross-boundary records | none |
| `cs_formats` | byte-level parsers, raw records and parse diagnostics | `cs_types` |
| `cs_assets` | read-only mounts, discovery, IO, cache | `cs_types`, `cs_formats` |
| `cs_content` | normalization, catalog, blueprints, localization, saves | `cs_types`, `cs_formats`; VFS interfaces from `cs_assets` only where needed |
| `cs_script` | engine-facing IR, validation, pure evaluator | `cs_types` |
| `cs_sim` | gameplay state, flight forces, combat, AI, objectives | `cs_types`, `cs_script`; consumes normalized records, never parses retail bytes |
| `cs_net` | protocol, codecs, connection/lobby state | `cs_types`; no renderer and no client authority over simulation |
| `cs_app` | Bevy composition, Avian adapter, rendering, input, audio, UI | all necessary lower crates |
| `cs_inspect` | command-line inspection, conversion diagnostics | formats/assets/content/types; simulation probes through explicit interfaces |
| `cs_xtask` | reproducible testing, coverage, packaging | appropriate pure libraries; launches app as subprocess for GPU/audio |

Dependency cycles are prohibited. `cs_sim` uses lightweight math/ECS dependencies only if justified; the chosen math version must match app adapters. Parsers never construct Bevy `Mesh`, `Image`, `Entity` or GPU handles. Save parsing does not depend on a running app. Network packets never serialize Bevy `Entity` values.

## Asset pipeline

`owner installation -> read-only mounts -> raw typed records + SourceSpan -> normalized content + provenance -> validated dependency closure -> session-ready bundle -> Bevy/Avian/audio/UI consumers`

A derived cache is a performance optimization, not the authoritative data source. Rebuilding it must preserve normalized hashes. A normal end user must not need Windows-only extraction, Python, Blender or manually merged PNG folders to play the completed engine. Temporary research-oracle workflows may use such tools privately.

## Stable identity

`ContentId` is a canonical namespace/key string with a validated length and grammar. Source numeric ids are retained separately. Never use catalog index as save identity. `SessionId` is a monotonically allocated opaque runtime generation. `ActorId` includes session plus a non-recycled generation-qualified local id. A render entity is a disposable binding to an ActorId.

An event includes `EventId(session, tick, producer, sequence)`. Sound, score, capture, destruction and reward consumers keep appropriate deduplication state. Dedupe retention is bounded by lifetime or persisted transaction identity, never an unbounded global set.

## Simulation schedule

The following is the initial designed ordering. F13/F38 may require an evidenced compatibility schedule variant; a one-tick difference that changes an original mission is not waved away.

1. **Input boundary:** collect quantized player/network inputs for tick T; handle session transitions and eligible prior-tick commands.
2. **Program pre-phase:** mature timers and consume queued events; validate and stage host effects, actor spawns and control ownership changes.
3. **Decision phase:** AI and player commands produce desired controls/fire intents; flight computes forces/torques; scripted actors produce kinematic trajectories.
4. **Physics phase:** Avian integrates each dynamic body exactly once. Custom ballistic projectiles use one clearly owned integrator plus swept queries, not a second Avian integration. Compute relative-motion trigger and hit candidates.
5. **Resolution phase:** stable ordering resolves hits, damage, destruction and interactions. Objective/script reactions run within a bounded declared microstep policy. Work exceeding the budget fails diagnostically rather than hanging.
6. **Commit phase:** publish terminal outcome, score/reward transactions, snapshots and presentation events. Late spawns/actions execute at the documented next boundary. No asynchronous IO callback can mutate simulation state here without a staged command.
7. **Presentation:** interpolate poses, render, mix audio and update UI. These consume state but cannot award damage, create mission success or advance simulation clocks.

The exact Avian schedule hooks must be taken from the pinned version's API and tested during bootstrap. Do not paste an old Bevy `FixedUpdate` example and assume it establishes ordering relative to physics.

## Body ownership

Dynamic aircraft: Avian owns pose and velocity integration; flight applies forces/torques. Scripted/cinematic aircraft: explicit kinematic mode. Docked actors: attachment transform owner. Player capture/swaps transfer ownership atomically. Only the transition system may change body ownership mode; tests assert there are no simultaneous owners.

Collision events are input to gameplay, not an excuse for renderer-side health changes. Projectiles are either custom swept particles or dynamic bodies, never both. Debris that has no gameplay effect is clearly cosmetic and cannot collect stunts or damage objectives.

## Application state and lifecycle

`Boot -> LocateInstall -> MainMenu -> Profile/CampaignCabin/InstantAction/Lobby -> Briefing/Construction/FlightCheck -> Loading -> InMission -> Results -> Cabin/Menu`

`Paused`, `Cinematic`, `LoadingFailed` and `Disconnecting` have explicit transitions. State entry/exit owns input contexts, asset tasks, music, active UI and session entities. A mission retry creates a fresh session generation and restores authored initial state. It does not recycle stale triggers, score ledgers, timers or aircraft references.

## Compatibility profiles

Keep four independent dimensions: installation edition/locale; original-rule compatibility; presentation settings; gameplay assists/mods. A setting cannot accidentally label a modded ruleset as stock because visuals look original. Every replay, net handshake and evidence record captures the relevant dimensions.

## Error policy

Library APIs return structured errors. User-facing diagnostics name the missing member, unsupported format/opcode and affected content. Developer mode may display missing meshes/materials conspicuously, but a retail content-readiness gate refuses to call them complete. Unknown optional cosmetic records can be tolerated only through an explicit scoped rule; unknown critical semantics cannot.

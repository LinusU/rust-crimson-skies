# Mission execution and source-adapter contract

## Deliberate distinction

Original program bytes, the original language/VM, new-engine mission IR and runtime host calls are different layers. F07 decodes the observed INTERP loading container; it does not establish the full mission language. F13 finds that language in the actual installation; F38 implements it. This pack supplies no invented original opcode numbers.

## Host interface

Expose a typed command interface, not direct ECS/OS access. Required command families, subject to corpus verification, are:

- Actor lifecycle and control: spawn group, remove, enable, disable, change owner/faction, assign route/AI role, control-lock, change player vehicle, attach/release.
- Combat/world state: read damage/status, configure vulnerability, enable mounts, set capital subsystem/door state, detach cargo, query actor existence and relationships.
- Interaction: authorize a specific docking/pickup/boarding, begin transfer, complete/abort transfer.
- Mission state: set/reveal/supersede objective, counters, variables, timers, terminal outcome and reward intent.
- Presentation cues: dialogue, sound/music, camera/cinematic, map markers and scrapbook/stunt events.

Every implemented binding has a source signature, argument domains, ownership checks, effect phase, repeatability policy, cancellation semantics, error result and tests. This family list is a design boundary, not evidence that these exact names exist in original scripts.

## IR requirements

Types include bool, checked integer, finite float, string/content id, ActorId, vector and typed optional actor reference. Functions/labels/variables retain source maps. Expressions have defined comparison/coercion/overflow behavior. Original semantics can require a compatibility numeric type; do not use whatever Rust coercion is convenient.

Control flow is explicit and bounded. Each tick has an instruction/action budget and recursion/stack limits. When exceeded, the diagnostic contains mission id, program locator, current instruction and a short call/event trace. Do not kill the whole process or continue with arbitrary skipped instructions.

Mutable runtime state contains variables, program positions, pending timers/subscriptions, RNG state, spawned ids and consumed execution keys. State snapshot/restore must preserve all gameplay-relevant pieces or declare mid-mission save unsupported. Ordinary post-mission save support must not depend on inventing a mid-mission format.

## Objective event ordering

Actions do not directly recurse into callbacks. Maintain ordered queues and define when a new event is eligible for observation. Stable ordering keys use session/tick/source/program sequence, not hash map or entity iteration order. Terminal success/failure precedence is a compatibility rule that must be measured for conflicting events. A designed conservative policy can be used for synthetic tests only until verified.

Conditions distinguish disabled, dead, captured, escaped, detached and despawned. An actor removed by a cinematic is not necessarily a kill. A protected actor destroyed after a success latch may or may not change the outcome; measure rather than assume.

## Source adapter acceptance

For each mission, export a private normalized program listing plus source spans and dependency closure. Count raw records, decoded instructions, unknown instructions, resolved host calls and dynamic lookups. Compare a normalized event trace against an original observation. For dynamic native behavior not directly visible in text, isolate one controlled condition and record the inference, contrary hypotheses and subsequent verification.

If the actual program is unavailable or cannot be decoded, the mission remains Unsupported. A handwritten declarative compatibility reconstruction is permitted only after the owner approves that method, it remains labeled reconstructed, and it reproduces all measured branches and source-derived data. A walkthrough alone cannot certify such a reconstruction.

## Program security

Original scripts are data, not trusted programs with system access. No host filesystem mutation, shell invocation, arbitrary network sockets or DLL loading. Validate names/ids/ranges and cap memory/time. Error messages must not leak unrelated owner files or huge copyrighted source dumps.

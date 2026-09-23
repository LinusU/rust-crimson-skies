# Identity, content and provenance contract

This is a normative **new-engine** API contract, not an original file layout.

```rust
pub struct ContentId(pub String); // validate at construction; no unchecked path join
pub struct SessionId(pub u64);
pub struct ActorId { pub session: SessionId, pub serial: u64 }
pub struct EventId { pub session: SessionId, pub tick: u64, pub producer: u32, pub sequence: u32 }
pub struct SourceSpan {
    pub install_sha256: String,
    pub container_path: String,
    pub member_key: Option<String>,
    pub offset: u64,
    pub length: u64,
    pub member_sha256: Option<String>,
}
pub enum EvidenceClass { Documented, ObservedTool, VerifiedOriginal, Inferred, Designed, Unknown, Contradicted }
pub struct Provenance { pub claim_id: String, pub class: EvidenceClass, pub source: Option<SourceSpan> }
pub struct Known<T> { pub value: T, pub provenance: Provenance }
pub enum Resolved<T> { Known(Known<T>), Unknown { claim_id: String, reason: String } }
```

These sketches define semantics; derive/serialization details belong to implementation. Hash strings must be canonical lowercase hexadecimal of the declared algorithm. Treat offsets as unsigned checked ranges. Normalize ids centrally and preserve original display names outside identity.

## Required catalog collections

Install files and container members; world groups and variants; scene roots and nodes; render meshes/materials/images; collision surfaces; airframes and exceptional control laws; engines/armor/guns/ammo/hardpoint equipment; blueprints and faction paint masks; pilot/voice/faction relations; campaign missions and dependencies; scripts/instructions/native bindings; animation/camera tracks; objectives/triggers/routes; sounds/music/dialogue/video; UI/font/string resources; stunts and scrapbook rewards; IA presets/options; multiplayer scenarios/rules; legacy custom-plane resources.

A catalog element has `id`, `kind`, `origin`, `dependencies`, `parse_state`, `normalize_state`, `runtime_consumers`, `readiness`, `unsupported_reasons` and `fingerprint`. Collections cannot exclude failed entries. An opaque unparsed member is still an inventory row.

## Dependency closure algorithm

Start with the selected mission/scenario id. Traverse all statically referenced edges plus conservative dynamic candidate sets discovered by the script adapter. Maintain visited ids, predecessor chains and per-edge provenance. Cycles in legitimate reference graphs may exist; cycles in ownership/parent hierarchies are invalid. A dynamic lookup that cannot be bounded is an unresolved dependency, not proof of no dependencies.

Validate each reached record's decoder, normalized fields, required handlers and runtime consumer. Emit a closure hash from sorted ids/hashes and compatibility options. Readiness is true only if every critical node is supported. Unreachable unknowns remain in the global accounting report and need an unused/optional classification before a full release.

## Lookup contract

`resolve(context, key) -> Result<ResolvedAsset, ResolveError>` returns exact origin and ordered attempts. No filename guessing loop that tries several random chapter archives until a texture happens to exist. Evidence-backed aliases are records with scope and test coverage. Multiple equal-priority candidates fail visibly.

## Numeric contract

Normalization creates meters, seconds, radians, kilograms or explicitly documented game-weight units. Original UI miles/hour and feet are conversion outputs, not simulation units. Unknown original units are `Resolved::Unknown`, not assumed SI. Integers represent money, ticks, counts, ammo and ids. Tuning float values must be finite and within measured/approved ranges.

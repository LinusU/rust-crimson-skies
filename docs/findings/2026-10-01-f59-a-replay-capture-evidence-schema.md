# F59-A: replay, capture and evidence schema — design notes and what stays open

Date: 2026-10-01. Task: F59-A "Define replay/capture/evidence schemas"
(`specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
`### F59-A`). Capabilities used: ordinary build/test only. All data is newly
authored synthetic design; nothing here is derived from the original game, and
no record in this module can award `verified_original`.

## Files and the observable failure

- `crates/cs_content/src/replay.rs` (new, the whole stage):
  - `ReplayRecord` + `BuildFingerprint`, `PlatformTag`, `BuildId`,
    `InitialState`, `ReplaySeeds`, `AuthoredChoices`, `OverrideLog`,
    `RunPurpose`, `ReplayField`;
  - `StateEnvelope` / `StateEntry` / `EnvelopeComparison` / `Divergence`,
    `InputStreamDigest`;
  - `ReplayRecord::compatibility_signature`, `differences_from`,
    `compatibility_with`, `CrossBuildPolicy`, `CompatibilityVerdict`,
    `CompatibilityDifference`, `verdict_against`;
  - `CaptureRecord`, `CaptureCamera`, `RenderConfig`, `TonemapKind`,
    `ArtifactMedia`, `PcmRole`, `ArtifactOrigin`, `CaptureDifference`,
    `CaptureError`;
  - `CapabilityClass`, `DeclaredCapabilities`, `EvidenceArtifact`,
    `ArtifactDescription`, `EvidenceBundle`, `CandidateBuild`, `TestCounts`,
    `CertificationReport`, `Certification`, `StaleReason`, `CapabilityGap`,
    `EvidenceRefusal`, `OrdinaryPlayRefusal`;
  - `encode` / `decode` / `encode_capture` / `decode_capture` (bounded,
    checksummed `CSREPLAY` / `CSCAPTURE` line documents) and the three
    `synthetic_*` fixtures.
- `crates/cs_content/tests/accept_f59_a_replay_schema.rs` (new, 56 tests).
- Wiring only: `pub mod replay;` plus a module paragraph in
  `crates/cs_content/src/lib.rs`. No other crate is touched.

Observable failure without the implementation: a changed content asset is
accepted as the same run and its old replay keeps replaying
(`accept_f59_a_a_changed_content_asset_rejects_the_old_replay`); a headless
machine can report a screenshot and an audible clip as verified
(`accept_f59_a_a_headless_machine_cannot_certify_visual_or_audible_evidence`);
a hand-drawn image attached to a fingerprinted installation passes
`EvidenceRecord::verifies_original`
(`accept_f59_a_an_authored_artifact_never_backs_an_original_claim`); a report
made on one tree certifies a different one
(`accept_f59_a_a_stale_report_cannot_certify_a_new_build`).

## Decisions

- **The content half only.** This stage is the engine-free record set plus its
  bounded document form, following the `cs_content::cameras` ↔
  `cs_app::camera` split every other contract uses. Nothing here simulates,
  renders, opens a device or touches the filesystem: the capture path is
  `cs_app::capture` (F59-B) and the commands are `cs-inspect` / `cs_xtask`
  (F59-C).
- **Three separate digests, not one.** `BuildFingerprint` keeps engine,
  content and rules apart so AC02's refusal can name *which* moved. A single
  hash over all three would answer "that it differs" and nothing more.
- **Field-wise differences, not just a signature.** `compatibility_signature`
  is the one-line identity; `differences_from` is the diagnostic list, and the
  two are deliberately different mechanisms (a signature match is a fact, a
  difference list is an explanation).
- **`BestEffort` is a verdict of its own, not a pass.**
  `CompatibilityVerdict::certifies_determinism` is true only for `Compatible`,
  and the `Display` text says "no determinism certified" so a log line cannot
  be read as a determinism claim.
- **The promised envelope excludes unknown fields.** `ReplayRecord::extra`
  preserves a newer minor's lines verbatim and re-emits them, but it is not in
  the signature: an unknown line describes a *document*, not a different run.
  Preserving is bounded, though: an `extra` entry is re-emitted as one
  `key=value` line, so `check_extra` refuses a key or value carrying a newline
  and a key the decoder already interprets. Without that, a field whose entire
  purpose is to be *unknown* could smuggle in a promised state hash, an input
  record or a second `subject=`, and a genuinely newer minor's line would be
  indistinguishable from an injection. `MAX_EXTRA_FIELDS` bounds the list.
- **The signature and the difference list are one contract, not two.** Every
  field `compatibility_signature` covers except `extra` has a
  `CompatibilityDifference` variant and is checked by `differences_from`. This
  is load-bearing rather than tidiness: a field that moves the signature but is
  absent from the difference list is a field `CrossBuildPolicy::Reject`
  silently accepts, which is precisely the shortcut AC02 exists to catch.
  `accept_f59_a_every_signature_field_is_compared_field_wise` states it as a
  property.
- **`purpose=` and `profile_write=` are required lines.** `RunPurpose::default()`
  is `OrdinaryPlay`, so defaulting a missing line would let a truncated or
  hand-edited *capture* document read as a player's ordinary session — the one
  conclusion non-negotiable 4 has to be able to refuse. A document that does not
  say what it is says nothing.
- **`encode` checks its own output against the decoder's bounds.** A record at
  `MAX_STREAM_RECORDS` is valid and encodes to ~13.5 MB, which the 4 MiB decoder
  bound refuses with `TooLarge`; encoding it would put a file on disk this build
  cannot read back. `encode` returns `DocumentTooLarge` / `DocumentTooManyLines`
  instead, using the decoder's own constants, so the two cannot drift.
- **A bundle's own record minimum is part of certification.** `certify` runs
  `EvidenceBundle::validate` first and reports `EvidenceRefusal::IncompleteRecord`.
  Without it a bundle with an empty `tool` and `test_command` certified cleanly,
  which is a report that cannot tell a reader what was run. Unresolved issues are
  explicitly *not* a refusal: the contract requires them to survive, so naming
  one may never remove a claim.
- **Authored choices carry their whole provenance into the canonical text** —
  the claim *and* its source span, not just the class — so the same value
  recorded as `inferred` and as `designed` is not silently the same run, and
  neither is a `observed_tool` claim that names where it was observed and one
  that names nothing. This is the F01 discipline applied to a replay field.
  The span is written to the document as
  `choice.<slot>=<class>|<claim>|<span>|<value>` with `-` for an absent span, so
  a round trip cannot strip a located observation. The decoder builds the
  provenance through `Provenance::new`, so a hand-edited
  `verified_original` choice with no span is refused by F01's own rule rather
  than smuggled in through the document form.
- **Capability table, not inference.** `ArtifactMedia::required_capabilities`
  is the whole of AC04 at this stage: a trace and a report need nothing, a
  screenshot needs `gpu`, a decode-only PCM capture needs nothing (it is a
  decode artifact, not audible review) and an audible capture needs `audio`
  **and** `human_review`, which an agent never has. Every check in the module
  is a *subtract* from the declared set, so there is no path from "we could not
  tell" to a certification.
- **Origin decides the observation method.**
  `EvidenceArtifact::evidence_record` maps `ArtifactOrigin::Authored` to
  `ObservationMethod::Authored`, which makes
  `EvidenceRecord::verifies_original` refuse it however it was fingerprinted.
  The contract's "a WAV written without a device is a decode artifact"
  limitation is produced by the type and travels with the record.
- **Private-relative artifact paths.** `EvidenceArtifact::new` refuses an
  absolute path, a drive letter and any `..` component, so a report cannot name
  original game data as its evidence.
- **The document seal is SHA-256, not FNV-1a.** `cs_content::save` uses
  FNV-1a for torn-write detection; a replay/capture file is a *claim* about
  determinism and a 64-bit checksum is too weak to be part of one, so this
  module hashes with the workspace's FIPS 180-4 `cs_assets::install::Sha256`.
  All the module's digests are length-prefixed and domain-separated
  (`cs.f59.replay.compatibility.v1`, `…state-chain.v1`, `…input-stream.v1`,
  `…seeds.v1`, `cs.f59.capture.digest.v1`) so no two field sequences can
  collide on a shared byte boundary.
- **Frame and choice separators are characters the grammar cannot produce.**
  An input tick is `input.<tick>=<edges>|<axes>` and a choice is
  `choice.<slot>=<class>|<claim>|<span>|<value>`. `|` appears in no action label,
  command label, claim id or claim status, and the choice *value* is the last
  part so a value containing `|` survives. A source span is
  `install@container[member]:offset+length#member_sha256`; because that
  decomposition depends on its delimiters, `check_source_span_keys` refuses a
  container path or member key containing `@ [ ] : + # |` or a newline on encode,
  and `decode_source_span` refuses the same set — a provenance that cannot be
  transported unambiguously is not provenance.
- **Floats never enter the replay document.** The render configuration is
  stored in thousandths (`exposure_milli`, `gamma_milli`), so a capture's
  settings are exact integer text. Only the camera pose is float, it is stored
  in canonical `cs_types::space` types, and it is validated finite and in range
  on both encode and decode.

## Sensitivity of the tests (mutations actually run)

- `ArtifactOrigin::Authored` mapped to `RuntimeObservation` →
  `accept_f59_a_an_authored_artifact_never_backs_an_original_claim` fails.
- `StateEnvelope::compare` always reporting no divergence → 3 tests fail
  (changed input sample, changed state hash, truncated envelope).
- `ArtifactMedia::Screenshot` requiring no capability → 2 tests fail (the
  AC04 test and the capability-table test).
- The `Blocked` branch of `CertificationReport::certification` inverted → 6
  tests fail.
- The decoder dropping `input.` lines → 5 tests fail, including the AC01 replay
  and the round trip.

### The review pass on this branch (2026-10-01, reviewer bunny-2)

Six defects were found in the first implementation and fixed in review; each
one is now covered by a test that fails when the fix is removed. The mutations
below were each run and reverted.

1. **`differences_from` did not cover the signature.** `compatibility_signature`
   hashes the schema, the subject, the initial-state *label* and the whole
   override log (purpose, named overrides, profile-write flag), but
   `differences_from` compared none of them. A replay of a different mission,
   a different run purpose, a debug-override run or a run that wrote a
   production profile all returned `CompatibilityVerdict::Compatible`, whose
   `certifies_determinism()` is `true` — the same false pass `Reject` exists to
   prevent. Fixed with four new variants (`Schema`, `Subject`, a
   field-carrying `InitialState`, `Overrides`).
   *Mutation:* deleting the `Overrides` check → 2 tests fail.
2. **The document form dropped `Provenance::source`.** A choice line carried
   only `<class>|<claim>|<value>`, so a record with an `observed_tool` provenance
   naming a source span decoded back to `source: None` — the round trip was not
   identity, and the *identity* comparison did not see the span either
   (`canonical()` omitted it), so two records differing only in whether they
   located their evidence compared equal. Fixed by writing the span, hashing it,
   and building the decoded provenance through `Provenance::new`.
   *Mutations:* dropping the span from `canonical()` → 1 test fails; writing `-`
   for every span → 1 test fails. The first mutation initially passed, which
   showed the test was only comparing different provenance *classes*; the test
   now holds class and value fixed so only the span differs.
3. **`encode` could write a document its own `decode` refuses.** A record at the
   declared `MAX_STREAM_RECORDS` bound validates, then encodes to 13.5 MB
   against a 4 MiB decoder bound. Fixed with a post-encode check against the
   decoder's own constants.
   *Mutation:* removing the guard → 1 test fails.
4. **A preserved unknown field could inject an interpreted line.** `extra` is
   re-emitted as one `key=value` line with no validation, so a value containing
   a newline could add an `envelope.<tick>=<hash>` line that the decoder read as
   a *promised state hash*, and a key like `subject` collided with a real field.
   A field whose whole purpose is to be unknown could therefore author the
   record. Fixed by `check_extra` in `validate`.
   *Mutation:* removing the `check_extra` call → 1 test fails.
5. **`EvidenceBundle::validate` was never called by `certify`.** A bundle with an
   empty `tool`, `tool_version` and `test_command` certified as `Checked` — a
   report that cannot tell a reader what was run. Fixed by running it in
   `certify` and reporting `EvidenceRefusal::IncompleteRecord`.
   *Mutation:* removing the `validate` call → 1 test fails.
6. **Missing `purpose=` / `profile_write=` lines defaulted to the strongest
   claim.** `RunPurpose::default()` is `OrdinaryPlay` and `profile_write`
   defaulted to `false`, so a capture document with those lines stripped read
   back as an ordinary player session — directly against non-negotiable 4.
   Fixed by making both required fields.
   *Mutation:* restoring the defaults → 1 test fails.

Also removed: an ambiguous-span check was added and is mutation-tested
(removing `check_source_span_keys` → 2 tests fail).

## Open / not claimed (resolving stages)

- **No runtime produces a state hash yet.** The per-tick
  [`StateEnvelope`] digests are supplied by a real session (F59-B); this
  module compares, chains, seals and transports them. The synthetic fixture
  digests *labelled fixture descriptions* and says so in
  `synthetic_state_digest`'s name and doc. AC01's runtime half — "replay the
  stream through the simulation and compare what the run produced" — is
  F59-B's; `ReplayRecord::verdict_against` is the one call it will make.
  Resolving: F59-B.
- **Nothing captures.** `CaptureRecord` is a typed record with a digest, a
  comparison and a document form; no GPU render, no screenshot writer, no PCM
  write and no offscreen path exist behind it. `RenderConfig`'s lowering to
  `cs_app::render::capture::ComparisonSettings` (same field set, same
  `none`/`filmic` tone-curve labels, but `f32` exposure/gamma against
  thousandths here) is F59-B's boundary. Resolving: F59-B.
- **The compatibility signature is over a synthetic content digest.** A real
  replay needs the canonical content digest the mission actually loaded, and
  the installation digest for a retail run; which producer computes it is
  F59-C's wiring, and the F02 `cs_assets` content fingerprint is the obvious
  source. Resolving: F59-C.
- **The freshness check takes the candidate build as an argument.** It does not
  run `git rev-parse HEAD^{tree}`, does not read the workspace, and does not
  re-derive the content hash: it is a decision function over values a caller
  supplies, so a stale report cannot certify a build it does not describe, but
  a caller that passes the wrong tree gets a wrong answer. The harness that
  reads the real tree hash and writes `acceptance.json` is F59-C/F59-D, and
  `tools/validate_evidence.py` remains the structural check.
- **No `cs-inspect` or `cs_xtask` command exists.** The two tools' owner paths
  are listed for this feature but nothing in F59-A needed them, so they are
  untouched. Resolving: F59-C.
- **`$CS_CAPABILITIES` is parsed, not read.** `DeclaredCapabilities::parse`
  implements the contract's spelling; no code here reads the environment
  variable. The tool that wires it up is F59-C.
- **Unmeasured and therefore unclaimed:** the original game's tick rate, its
  replay or capture file format (if it had one), what its evidence artifacts
  looked like, whether its frame pacing was deterministic, and any original
  value for a render setting. The compatibility signature and the render
  baseline are project design. The render bounds (`MAX_RENDER_DIMENSION`,
  gamma/exposure/MSAA ranges, the 640×480 comparison size) are declared design
  bounds chosen to bracket sane settings, not measurements.
- **A `Trace` artifact needs no capability class.** A headless run is not a
  capability-gated activity, so a trace is not blocked on a headless machine.
  If a later stage shows a trace can only be produced with a real renderer,
  this table is the one line to change.

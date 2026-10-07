# F53-B: Safe mounts, measured payloads and the compatibility signature

Date: 2026-10-07. Task: F53-B "Implement safe mounts and compatibility
signatures"
(`specs/F53-mod-mounts-custom-content-and-compatibility-signatures.md`,
section `### F53-B`). Shared contract: `docs/contracts/IDENTITY-CONTENT.md`.
Capabilities used: ordinary build/test only (no `CS_GAME_DIR` read, no
evidence report required).

## Files and the one observable failure (listed before editing)

- `crates/cs_assets/src/mods.rs` (new): the root join — `MOD_NAMESPACE`,
  `ModRoot::mount` / `ModRoot::resolve` / `ModRoot::read` /
  `ModRoot::digest` / `ModRoot::rejected` / `ModRoot::mount_record`,
  `ModMountError`.
- `crates/cs_content/src/mods/mount.rs` (new): the mount and the signature —
  `mount_mods`, `MountEnvironment` (roots, base fingerprint,
  `ProgramValidator`), `MountedMods` / `MountedPayload`, `MountError`,
  `ProgramValidator`, `compatibility_signature`.
- `crates/cs_content/src/mods/mod.rs`: `mod mount;`, the re-exports, the new
  `synthetic_mission_mod()` fixture and the module-header paragraph for the
  F53-B half.
- `crates/cs_assets/src/lib.rs`, `crates/cs_content/src/lib.rs`: wiring only
  (`pub mod mods;` and the crate-doc paragraphs).
- This file.

**One observable failure:** a mod whose declared source is a malicious
relative spelling (`../outside.png`, `art/..\..\outside.png`, `/etc/passwd`,
a drive-prefixed or NUL-carrying spelling) is refused *by name, as unsafe*,
by the production call that joins the spelling to the mod root — and a mod
whose declared source exists only behind a symbolic link inside its root is
refused as *not shipped*, with the content id it claimed. A mount that
resolved either of them, or that enabled a cyclic set after opening a root,
fails this.

Test count: 8 `accept_f53_b_*` tests — 3 unit tests in
`crates/cs_assets/src/mods.rs`, 5 in `crates/cs_content/src/mods/mount.rs` —
selected by
`cargo test --workspace --locked -- accept_f53_b_ --include-ignored`
(exit 0, 8 executed, all passing).

**Why unit tests inside the owner paths again** (as in F53-A): the owner
paths of this task are `crates/cs_content/src/mods/`,
`crates/cs_assets/src/mods.rs`, `crates/cs_app/src/ui/mods.rs`, a root
`tests/` and `docs/findings/`. `crates/cs_content/tests/` and
`crates/cs_assets/tests/` are not among them, so the tests live in
`#[cfg(test)]` modules inside the two owner files. A root `tests/` directory
would not be built by this virtual workspace (the root `Cargo.toml` declares
no package), so it was not created.

## What this stage implements

- **Plan first, IO second.** `mount_mods` computes `plan_mods` before it
  opens anything, so every plan problem — a dependency cycle above all — is
  reported as `MountError::Plan` and no root is walked. The cycle test
  proves the ordering: it declares *no* roots at all, so any attempt to
  mount before planning would surface `RootNotSupplied` instead.
- **The root join is a production function with a name.**
  `cs_assets::mods::ModRoot::mount` walks the mod root once with F04-B's
  `mount_directory` (sorted, read-only, symlinks never followed, every file
  hashed), as a [`PrecedenceClass::Mod`] mount whose `MountScope` carries
  the `ModId`, so only a context that opted into that mod is admitted.
  `ModRoot::resolve` re-validates the declared spelling **before** the
  lookup — F53-A validated spellings with no root attached and said F53-B
  owns the join — and reports a safe spelling the mod does not ship as
  `SourceNotMounted` rather than following a link to it. `ModRoot::read`
  re-checks length, link-ness and digest against what the walk indexed.
  The mount id is the mod id (both labels are the same validated
  `[a-z0-9._-]` vocabulary), and the container label — the provenance every
  `SourceSpan` records — is the mod id; the host path never enters a span.
- **Measured budgets, not declared ones.** F53-A's five limits were checked
  against *declared* bytes because that stage reads nothing. This stage
  re-checks `max_declared_bytes_per_mod` and
  `max_declared_bytes_total` against the sizes the walk indexed, so a
  manifest that declares 512 bytes and ships 4 KiB is refused
  (`MountError::MeasuredByteBudgetExceeded`). Measured sums saturate, as
  F53-A's did.
- **Mission and script payloads are fail-closed** (F53 non-negotiable 2).
  A winning payload whose target `classify_validation` calls
  `SandboxedProgram` is read from disk and handed to the host's
  `ProgramValidator`; with no validator the mount is **refused**
  (`MountError::UnvalidatedProgram`), and a validator's refusal is the
  mount's verdict (`MountError::ProgramRejected`, quoting the reason).
  Ordinary content is deliberately not decoded here: its bounded reader
  runs when the content is resolved and consumed, which is the original
  adapter path, and duplicating it would be a second implementation of
  every format. Only executable content needs a gate *before* enabling.
- **The compatibility signature.** `MountedMods::signature()` is SHA-256
  over the domain separator `cs-content-compat-signature-v1`, the base
  installation fingerprint the mount was asked under, `ModPlan::hash()`
  (the declared inputs) and, per mounted payload in content-id order, the
  winning mod, its load position, the measured length and the payload's own
  SHA-256. It is exactly what `ModPlan::hash` documents itself *not* to be:
  a hash over the resolved content bytes. Only winners enter it; a shadowed
  claim serves no content. F53-C folds it into the lobby handshake (AC03)
  and into save marking (AC04's input).

## Unknowns, limitations and where they are resolved

- **No measured decoder from mod-authored bytes into a mission program
  exists in this workspace.** `cs_script::ir::MissionProgram::validate`
  validates an IR that an adapter built; `cs_content::mission_control`
  still documents why its measured control record cannot lower to one yet.
  A mod-authored mission payload therefore has nothing to be validated
  *by*, so the mount refuses it unless the host supplies a validator. This
  is a missing capability blocking, never a pass: no sandboxed payload can
  reach runtime through this mount today, and no stub success exists that
  would let one through. Resolving it needs a decoder plus a host that
  passes the real validator; filed as a follow-up task (see below). A
  `ProgramValidator` is a capability, not a policy knob: it cannot accept a
  payload the bounded validator rejected, and it is never consulted for a
  non-sandboxed target.
- **No manifest *file* reader.** F53-A defined `ModManifest` as a typed
  record; this stage consumes manifests a caller already holds. Nothing in
  the F53 sheet specifies a manifest file format, and inventing one would
  be new vocabulary with no source behind it (F53 "Research boundary"), so
  no format was guessed. The crate doc sentence that had said "the typed
  `ModManifest` an F53-B manifest reader produces" was corrected to what
  this stage actually delivers; reading a manifest out of a mod root is
  filed as a follow-up for the stage that owns the producer side (F53-C
  wires the producer and consumer).
- **The original game's mod support remains unmeasured** (F53 "Research
  boundary"; F53-D). The `mod` namespace, the mount ids, the signature
  domain separator and every fixture are newly authored project design with
  `ClaimStatus::Designed` provenance under claim `f53a.synthetic-mod-set`.
  Nothing here is evidence about the original game, and no
  `verified_original` state is reachable from this module.
- **AC03 and AC04 are not claimed here.** AC03 (a tuning mod changes the
  compatibility hash and blocks a mismatched stock lobby) is F53-C's
  wire-up; AC04 (disable a mod, reopen a dependent save) is the blocked
  F53-A-FU1 / #468 follow-up. The signature this stage produces is AC03's
  input.
- **Hot reload boundaries** (non-negotiable 5) are runtime behaviour of the
  session that hosts these mounts; this stage keeps every mount immutable
  for the lifetime of the mount it returns and performs no mutation after
  the walk. The paused/developer boundary itself is F53-C's wiring.

## Checks run (all exit 0)

- `cargo fmt --all -- --check`
- `cargo clippy --workspace --all-targets --all-features --locked -- -D warnings`
- `cargo test --workspace --locked` — 430 test-result lines, 0 failures
- `cargo test --workspace --locked -- accept_f53_b_ --include-ignored`
  — 8 tests executed (3 in `cs_assets`, 5 in `cs_content`), all passing

## Sensitivity probes (run and reverted; none committed)

Each probe removes one behavior and re-runs the `accept_f53_b_` selection.

1. `ModRoot::resolve` no longer classifies an unsafe spelling as unsafe
   (it reports it as not shipped) → 1 failure
   (`…a_malicious_relative_path_is_refused_by_the_root_join`).
2. `mount_mods` stops reporting the plan's refusal (the plan error is
   remapped) → 1 failure
   (`…a_cyclic_dependency_is_rejected_before_any_root_is_opened`).
3. The fail-closed sandboxed-program gate removed (no validator ⇒ still
   mounts) → 1 failure
   (`…mission_content_mounts_only_through_a_bounded_validator`).
4. The base fingerprint dropped from the signature → 1 failure
   (`…the_compatibility_signature_covers_the_resolved_payload_bytes`).
5. The measured per-mod byte-budget check removed → 1 failure
   (`…measured_payload_bytes_are_held_to_the_mount_budget`).

After every probe the file was restored and the selection re-run green
(the final green run above is the run of record).

# F53-FU4: no on-disk manifest format (wontfix)

Date: 2026-10-17. Task: #755 / F53-FU4 "Decide whether mods need an on-disk
manifest format" — the decision `F53-C` deferred in
`docs/findings/2026-10-07-f53-c-selection-diagnostics-and-private-export.md`.
Synthetic only — no retail or original-behavior claim.

## The question

`ModSelection::offer(manifest, root)` takes a typed `ModManifest` the
caller builds by hand plus the directory the mod ships under. There is
deliberately no manifest *file* reader: the F53 sheet never specifies an
on-disk manifest format, and inventing one would be unmeasured vocabulary
(F53 "Research boundary" — no original-game mod format was ever measured,
because the original game shipped no discoverable mod surface).

Should the project design one anyway, so real mod directories can be
discovered without each caller building manifests by hand?

## The decision

**No — wontfix.** The boundary stays: `AvailableMod` is manifest-plus-root,
discovery is the caller's concern.

Recorded by the implementing agent without an in-session owner ruling; the
owner was asked and declined to decide in-session. This closes nothing the
owner cannot reopen at zero cost — the API is unchanged either way, and
reversing the decision later only *adds* a reader where none existed.

## Why

- **No consumer demands it.** Every current caller — the synthetic
  fixtures, the F53-D mount/save flows, the mods screen — already holds a
  typed `ModManifest` (`synthetic_blueprint_mod`, hand-built headers). A
  reader without a caller is a designed vocabulary with no proof it is the
  right one.
- **The manifest is substantial.** `ModManifest::try_new` validates a
  header (id, display name, `ModVersion`, `EngineRange`,
  `declared_cosmetic_only`, provenance) plus three relations —
  dependencies, declarative payloads, content overrides — each with its
  own grammar (`RelPath`, `VersionRange`, `DependencyStrength`, override
  targets). A file format that expresses all of that honestly is a real
  designed surface: name, location, field spellings, parse diagnostics,
  versioning. Designing it before a real mod exists to read it risks
  baking in the wrong spellings.
- **The research boundary stays honest.** Any format invented now is
  entirely designed — not measured from original data and not constrained
  by an original loader. Recording "no format" keeps that explicit instead
  of laundering a guess through a parser.
- **Wontfix is reversible.** Adding `ModSelection::discover(root)` later
  is purely additive: the offer API, `MountError` diagnostics and the
  mount path do not change shape.

## The resolving condition

Reopen this when a real consumer exists that *cannot* reasonably build
manifests by hand — e.g. a mods screen that must enumerate user-dropped
mod directories, or a shipped mod whose bytes the engine must self-describe.
At that point the format should be designed against the actual
`ModManifest::try_new` fields listed above, with `ManifestError`-adjacent
parse diagnostics, and read through a `ModSelection::discover`-style
production path — not guessed earlier.

## What is not claimed

- This records that no format was *chosen*, not that the original game had
  none — the original shipped no measured mod surface, so there is nothing
  to measure.
- Callers keep building manifests by hand; nothing in F53-D or the mods
  screen is blocked by the absence of a reader.

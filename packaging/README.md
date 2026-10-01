# Release packaging policy (F61)

What a release archive may contain is a **policy**, not a build detail, and the
policy is production code: [`cs_xtask::package`](../tools/cs_xtask/src/package.rs).
This directory holds the packaging inputs and the synthetic fixtures that
policy is exercised against. It holds no archive and no original content.

## What a release contains

| Member | Why |
| --- | --- |
| `crimson-skies` or `crimson-skies.exe` | the new engine (F61 deliverable) |
| `LICENSE` | the new engine code's own license |
| `THIRD-PARTY-NOTICES.md` | transitive dependency licensing (non-negotiable 2) |
| `REFERENCE-TOOLS.md` | reference-tool licensing decisions, kept separate from the engine license (non-negotiable 2) |
| `NOTICE.md` | provenance and the "not official Microsoft/Zipper software" statement (non-negotiable 3) |
| `COMPATIBILITY.md` | the versioned compatibility report (F61 deliverable) |
| `README.md` | user instructions (F61 deliverable) |
| `*.md`, `*.txt`, `*.html` | newly authored documentation |
| `*.sha256`, `SHA256SUMS` | a source hash manifest — digests of original content, **not** the content (non-negotiable 1) |

Everything else is refused. A member carrying game textures, audio, scripts,
fonts, an executable that is not the engine, a scanned manual, an extracted
archive or decompiled source fails the scan, and so does a member under an
original-content root (`original/`, `original-data/`, `crimson-skies-data/`,
`extracted/`) whatever its name says, and so does a member nothing in the
policy classifies at all.

## Candidate manifests

`verify-package` scans a *candidate manifest*: what a packaging step knows
before an archive exists.

```text
version: 0.1.0
member crimson-skies 12582912
member LICENSE 1073
```

One directive per line, `#` comments and blank lines ignored, `member <path>
<size-bytes>` with a path that holds no whitespace. A manifest that states no
version is refused: a release ships versioned compatibility reports, so the
version is part of what is being checked.

```sh
cargo run -p cs_xtask -- verify-package --manifest packaging/fixtures/candidate-clean.manifest
```

Run it from the workspace root, or name one with `--workspace-root <dir>`: like
every other `cs_xtask` gate, the command checks the root it was handed instead of
ignoring it. It exits 0 when the candidate is releasable, 1 when it is not (with
every finding on stderr) and 2 when the request itself is wrong.

## Fixtures

`fixtures/` holds newly authored synthetic candidate manifests. Every file
there is text written for this repository; none of it is original game content,
an extracted member, or a build artifact.

## What this does not prove yet

The scan classifies members by **path**, because a path is all a packaging step
has before the archive is written. That catches a release that was assembled
wrong; it does not prove the bytes in an allowed member are newly authored. The
strong check — every member's digest compared against the recorded
original-content digest set, plus running the extracted binary against a private
fixture installation — is F61-B and F61-D, and the limits are written down in
`docs/findings/2026-10-02-f61-a-release-contents-and-user-data-policy.md`.

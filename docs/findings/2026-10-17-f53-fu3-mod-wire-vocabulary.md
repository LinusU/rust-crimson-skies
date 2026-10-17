# F53-FU3: enabled mods are named by `ModId` on the wire

Date: 2026-10-17. Task: #754 / F53-FU3 "Decide how enabled mods are named on
the wire (`Compatibility.mods`)" — the decision `F53-C` deferred in
`docs/findings/2026-10-07-f53-c-selection-diagnostics-and-private-export.md`.
Synthetic fixture evidence only — no retail or original-behavior claim.

## The problem

`Compatibility.mods` was documented as "the enabled mods, each a catalog
id" (`ContentId`), and `evaluate_hello` compares it as a set after the
content-signature gate. But the field stayed empty: no `ContentKind` names
a mod, and F54's synthetic fixtures only used `ContentKind::Blueprint` as a
stand-in — explicitly a fixture convention, not a production vocabulary.

## The decision

**`Compatibility.mods` carries `ModId`s — the mod manifest's own identity
vocabulary** — encoded on the wire as the validated label text (bounded by
`codec::MAX_MOD_ID_WIRE_BYTES`, the id grammar's `MAX_LABEL_LEN`).

The alternatives, and why this one:

- **Add `ContentKind::Mod` and keep `Vec<ContentId>`.** A mod is not
  catalog content — it is a *set* of overrides and additions with its own
  manifest identity, plan and signature. Minting a `ContentId` per mod
  fabricates catalog ids for non-catalog things, and the change ripples
  through `cs_types`' `ContentKind::ALL`, the id grammar and every
  exhaustive `ContentKind` match for a field that is informational anyway.
- **Keep `ContentId` typed by an existing kind.** Exactly the mislabel the
  task warns against — it promotes F54's synthetic-fixture stand-in to
  production vocabulary.
- **`ModId` strings (chosen).** `ModId` already *is* the validated,
  stable mod-identity vocabulary: `MountedMods::plan().order()` yields it
  directly, the display and rejection vocabulary already uses it, and the
  wire field's only job — naming the differing mods in a `ModSetMismatch`
  — is better served by the id a user actually enables than by a minted
  catalog handle.

`lobby_compatibility` now populates `Compatibility.mods` from
`mounted.plan().order()`; stock sessions announce an empty list.

## Consequence discovered while migrating the codec

`MAX_MOD_DIFF_IDS = 8` exists because a `ModSetMismatch`'s named ids must
be encodable inside `MAX_PACKET_BYTES`: under the *catalog-id* worst case —
`MAX_MODS` maximum-length ids per side, no overlap — the untruncated diff
could not encode at all (F54-B/C finding, task
F54-C-followup-mods-diff-cap). Under `ModId` the arithmetic changes: the
widest wire entry is a 64-byte label, so 128 of them fit a packet whole.
The bound still holds and still truncates; its motivation has moved from
encodability to narration — a refusal names the first few differing ids and
counts the rest rather than reciting up to 64. `bounds.rs`,
`compat.rs::ModSetMismatch` and the F54-A worst-case test now pin the new
arithmetic explicitly.

## What is not claimed

- The wire form is **our** protocol's choice; the original game's
  DirectPlay compatibility exchange is unread, and nothing here is an
  original-behavior claim.
- The list remains informational: `content_sha256` (F53-B's
  domain-separated signature over plan and payload bytes) is still the
  launch gate — two sessions with different mounted sets refuse on
  `ContentMismatch` before their lists are ever compared. `ModSetMismatch`
  is reachable only between equal-signature offers whose *lists* differ,
  which `accept_f53_fu3_enabled_mods_are_named_by_mod_id_on_the_wire`
  proves.
- `ContentKind` gains no `Mod` variant: mods stay outside catalog content
  by design.

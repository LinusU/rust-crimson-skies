# Crate module-doc order

Every crate's `src/lib.rs` opens with crate-level `//!` documentation. Besides a
short crate introduction and the trailing link-definition list, that block
carries one paragraph per feature stage ("module-doc paragraph"). Several stage
branches add such a paragraph at once, and while they all appended at the same
anchor (immediately above the link definitions) every one of them conflicted
with every other. This is the rule that gives each paragraph a position
determined by the sheet it documents, instead of every branch racing for the
same anchor.

## The rule

**A crate-doc paragraph is inserted in ascending order of the feature-sheet id
it documents — the `Fnn` of the first `specs/Fnn-...` reference in the
paragraph.**

- Read the paragraph's own first `specs/Fnn-` reference (a paragraph about
  `specs/F36-...` is F36).
- Insert it after every paragraph whose sheet id is lower and before every
  paragraph whose sheet id is higher. If two paragraphs share a sheet id, order
  them by module name.
- The crate introduction (the `//!` paragraphs before the first `specs/Fnn-`
  mention) stays first. The trailing link-definition list (`//! [`name`]: path`)
  and any closing general note stay last. Neither is part of the sorted run.
- A stage that extends an existing module's paragraph edits that paragraph in
  place. It does not add a second paragraph for a sheet that already has one.

The sheet ids are fixed when the specs are written (F00–F64), so a new
paragraph's position does not depend on how many paragraphs happen to sit at the
end of the block already. Additions with well-separated sheet ids no longer
share an anchor. Two additions whose sheet ids fall between the same pair of
neighbouring paragraphs can still share a line and conflict, but that residual
case is narrow; the old append-at-the-end rule made it universal. The rule
applies to any crate-level `//!` block that carries per-stage paragraphs, and
the tests below enforce it for `cs_content`, `cs_formats`, `cs_app` and
`cs_sim`.

## Why feature-sheet order

The feature sheet is the only key every paragraph already carries. Naming a
single owning module is not enough: the crate-narrative paragraphs in
`cs_formats` describe a foundation and several formats and have no one
unambiguous module, but each still opens with a `specs/Fnn-` reference. Sheet
order is also stable: a spec is allocated once and never renumbered, so the key
cannot drift the way an append position does.

## Enforcement

`crates/cs_content/tests/accept_doclib_conflict.rs`,
`crates/cs_formats/tests/accept_doclib_conflict.rs`,
`crates/cs_app/tests/accept_doclib_conflict.rs` and
`crates/cs_sim/tests/accept_doclib_conflict.rs` read each crate's real
`src/lib.rs` and fail, with this file's path in the message, when a paragraph is
inserted out of order or when the `pub mod` list stops being alphabetical.

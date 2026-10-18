# M01-M06-ARCHIVE-FINDING-PROSE (#1568): attribution notes on pre-decision findings

Follow-up of #1565 (`2026-10-17-m01-archive-decision.md`). Every finding in that
document's census (the 44 "M01" findings plus the `m01-b-opc` script note) now
opens with an attribution note: measured archive `ZBD/C1C/M01/zrdr.zbd` is work
order **M06** "The Red Menace", not M01 "The Lost Treasure" (`ZBD/C3/M01`).
`m06-b-fu2` and `m06-b-fu4` carry a note that their measured archive,
`ZBD/C2/M01`, is **M12**'s. Notes are inserted under each title; no measurement
text was altered (the diff has insertions only). `owner-facts-table` and
`f47-d2` only had M01 rows/references, and their notes say so.

## Committed evidence reports (`docs/findings/evidence/M01-*.json`)

Decision: **left as committed, not annotated and not reissued here.** They are
machine-validated against `schemas/evidence.schema.json` and carry hashes tied
to their harness output; editing prose by hand would break "never write a
report by hand", and a reissue re-runs retail harnesses that belong to the
tasks that own them. Their task-era M01 wording is historical record; this
document and the #1565 census are the attribution key. Any reissue (through the
#1174-style harnesses) will derive from the corrected binding and cite the
right archives. Reports whose claims are fidelity-relevant should be reissued
when their owning task is next touched; this does not need to block anything now.

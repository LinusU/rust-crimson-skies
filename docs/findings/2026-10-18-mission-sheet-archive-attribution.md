# M01-SHEET-ARCHIVE-OWNER-NOTE (#1569): census and ready-to-apply note text

What each protected mission sheet asserts today, the corrected archive
attribution decided in #1565, and the proposed note text an owner-authorized
edit can apply verbatim. The sheets are protected paths and this task ran
with `allowProtectedChanges: false`, so nothing on them was changed; the
application decision is the owner's.

## Census: the sheets assert no archive today

`missions/M01.md`, `missions/M06.md` and `missions/M12.md` name no `ZBD/*`
path, world id or program id anywhere in their prose (grep for
`zbd|archive|world/|script/` over `missions/M*.md`, this commit). No sheet
states the old directory-digit attribution, so there is no wrong text to
correct — only an attribution note to add, or not add.

The residual risk the task description flags — checklist state implicitly
asserting the old archive — is already resolved: the scoped binding entries
measured on `zbd/c1c/m01` moved from M01's record to M06's and the
`zbd/c2/m01` unknown moved from M06's record to M12's in #1565
(`missions/bindings/*.json`). The pre-decision prose that does name the old
attribution lives in `docs/findings/` and `docs/findings/evidence/` and is
being annotated under #1568, not here.

## Corrected attribution (measured in #1565, not guessed)

| Sheet | Directory-digit binding (pre-#1565) | Corrected archive | Campaign position |
|---|---|---|---|
| M01 The Lost Treasure | `ZBD/C1C/M01` | `ZBD/C3/M01` (`world/c3`, `script/c3-m01-zrdr`) | `mission/ch1-m01` |
| M06 The Red Menace | `ZBD/C2/M01` | `ZBD/C1C/M01` (`world/c1c`, `script/c1c-m01-zrdr`) | `mission/ch2-m01` |
| M12 The Great Plane Robbery | `ZBD/C3/M02` | `ZBD/C2/M01` (`world/c2`, `script/c2-m01-zrdr`) | `mission/ch3-m02` |

Method and evidence: `docs/findings/2026-10-17-m01-archive-decision.md`
(sound-code census, localized title tables, briefing ids, scrapbook order,
content census). Every pre-#1565 `M01-*` measurement was taken on
`ZBD/C1C/M01` and is M06 evidence; `M06-B-FU2`/`M06-B-FU4` measured
`ZBD/C2/M01` and are M12 evidence.

The same re-measurement also repositioned M02–M05, M07, M08, M10 and M13
(e.g. M02's archive is now `ZBD/C3/M02`, formerly M12's bound path). None of
those sheets has findings or checklist state written against the wrong
archive, so notes there are optional; the task's minimal outcome covers
M01, M06 and M12.

## Proposed note text (ready to apply)

Placement: immediately after the `**Status:**` line of each sheet, matching
the existing "Owner ruling 2026-09-28" note style in `missions/M01.md`.

### missions/M01.md

> **Archive attribution (owner ruling 2026-10-17, [#1565](../docs/findings/2026-10-17-m01-archive-decision.md)):** this mission's original archive is `ZBD/C3/M01/zrdr.zbd` (`mission/ch1-m01`, `world/c3`, `script/c3-m01-zrdr`). The directory-digit binding used before that ruling assigned `ZBD/C1C/M01` to this work order; that archive is M06's. Every measurement, finding and evidence report produced under an `M01-*` label before 2026-10-17 was taken on `ZBD/C1C/M01` and belongs to M06 "The Red Menace"; M01's real archive carries only the #1549 content census, so no earlier `M01-*` result establishes coverage of this sheet.

### missions/M06.md

> **Archive attribution (owner ruling 2026-10-17, [#1565](../docs/findings/2026-10-17-m01-archive-decision.md)):** this mission's original archive is `ZBD/C1C/M01/zrdr.zbd` (`mission/ch2-m01`, `world/c1c`, `script/c1c-m01-zrdr`). All findings filed under `M01-*` labels before 2026-10-17 measured this archive and are M06 evidence. The measurements filed under `M06-B-FU2`/`M06-B-FU4` were taken on `ZBD/C2/M01`, which is M12's archive.

### missions/M12.md

> **Archive attribution (owner ruling 2026-10-17, [#1565](../docs/findings/2026-10-17-m01-archive-decision.md)):** this mission's original archive is `ZBD/C2/M01/zrdr.zbd` (`mission/ch3-m02`, `world/c2`, `script/c2-m01-zrdr`) — not `ZBD/C3/M02`, which the directory-digit binding had assigned and which is M02's archive. The passenger-identity and scoped-binding measurements filed under `M06-B-FU2`/`M06-B-FU4` were taken on this sheet's archive and belong to M12.

## What the owner needs to decide

1. Whether the sheets should carry these notes at all — the alternative is
   sheets staying generic, with attribution living only in
   `missions/bindings/`, the binding README and `docs/findings/`.
2. The wording and placement if yes (the text above is a proposal).
3. Who applies it: the owner directly, or a re-issued task with
   `allowProtectedChanges: true` naming exactly `missions/M01.md`,
   `missions/M06.md` and `missions/M12.md`.

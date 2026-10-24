# Pause map: button label alignment and the unresolved faces (#1989)

Task `PARITY3-PAUSE-MAP-GAP.06`. Provisional, provenance `static`; nothing here is `verified_original`.

## Alignment (fixed)

The Resume/Restart/Preferences/Quit labels were `Text` nodes given the plate's width with
`Justify::Center`, but Bevy 0.20 lays a glyph run at the left of a node wider than the run, so the
smoke frame showed each label starting at its plate's left edge. Each label is now a plate-wide
row (`JustifyContent::Center`) holding the text, centred on `button.at.x + LABEL offset.x` (66 of
the 132-wide plates, i.e. the plate's centre). Owner-command smoke of 2026-10-24
(`cs --cs-path <retail> --mission M01 --smoke-seconds 45 --capture-dir private/evidence/F1989`):
`frame_03956_pause_map.png` shows all four labels centred in their plates. (The same run reported
four `flak_*` check failures in the unrelated flak phase; none concern the pause map, whose
checks pass.)

## Faces (unresolved)

`fonts.zrd WINDOWS_FONTS` names `Trebuchet MS Bold` for the buttons and `Andy Bold` (italic) for
the note. They are Windows system faces; the installation holds no font file (see
`2026-10-21-pause-map-screen-data.md`, "Measured absences"), so the dialog data cannot be honoured
and Bevy's built-in default face (a monospace) is drawn. This is why the face differs: a fallback,
not a lookup bug. Resolving it needs a licensed substitute chosen by the owner; not done here.
The note title and list stay left-justified as the record's offsets give them.

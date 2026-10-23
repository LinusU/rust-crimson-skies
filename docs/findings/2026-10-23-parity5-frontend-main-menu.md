# PARITY5-FRONTEND-MAIN-MENU: the integration pass over the main menu and pilot page

Date: 2026-10-23. Task: #1748 (integration of FE-MENU-1..4). Test prefix
`accept_parity5_main_menu_`. Evidence: `docs/findings/evidence/PARITY5-MAIN-MENU.json`,
artifacts in `private/evidence/PARITY5-MAIN-MENU/` (git-ignored). Implemented by
`claude-1`; this is the implementer's record, not a review, and awards no
`verified_original`/`release_approved`. No agent ran `crimson.exe`.

## Frames beside the owner's captures

Owner's Drydock captures (retail 1.02, 800x600; fetched from the task's links
into `private/ref-drydock/{menu,pilot}/original.png`, git-ignored):

| Screen | Ours | Original |
| --- | --- | --- |
| Main menu | `private/evidence/PARITY5-MAIN-MENU/mainmenu.png` | `private/ref-drydock/menu/original.png` |
| Pilot page | `.../campaign-typed.png`, `.../campaign-list.png` | `private/ref-drydock/pilot/original.png` |

Main menu: logo, winged skull badge, six strips on the pipe frame and the
flag movie agree in placement and art; the flag's pose differs (a looped
movie, different frame).

Pilot page: **the first comparison found a gap.** Our Campaign section drew
on black with no logo, where the original keeps the flag movie and the logo
behind the page. `MAINMENU.SCRIPT` runs `deactivate(this)` on Campaign and
Cancel returns to the "still-loaded" menu
(`2026-10-22-parity5-start-flow-transitions.md`): the menu is hidden, not
ended. The capture shows what stays: the flag movie and `MM_LOGO`; the button
frame and the six buttons are gone. `MenuShell::carry_under` now draws
`MainMenu`'s `M` rows and `MM_LOGO` under the Campaign section; the frame and
buttons are not carried. After the change the pilot page matches the capture's
composition (logo, panel at 193,251, edit box with Continue, black list box,
Delete Player and Cancel).

## Remaining differences (not claimed as matching)

* Edit and list text use the engine's default face; the original's is a
  typewriter face (`FONT_CM_EDIT` is a GDI font id, not in the installation).
* The list's scroll bar art is not drawn (placement unread).
* Which rows the original keeps behind Campaign is read from one screenshot;
  Preferences/Credits/Instant Action are not given the carry-over (no capture;
  Preferences already holds its own movie row).
* Pilots persist in this project's own profile files; the original save layout
  (callbacks 2099/2100/2101/2106) is unread, so nothing is imported.
* Audio: see `2026-10-23-fe-menu-4-music-and-cues.md`; nobody listened.

## Reproduce

```sh
mkdir -p private/evidence/PARITY5-MAIN-MENU
CS_EVIDENCE_DIR=$PWD/private/evidence/PARITY5-MAIN-MENU \
CS_FE_MENU_SMOKE_OUT=$PWD/private/evidence/PARITY5-MAIN-MENU \
  cargo test --locked -p cs_app --test cs_app_tests_c -- accept_parity5_main_menu_ --include-ignored
```

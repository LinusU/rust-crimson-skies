# F1 "View Help" keymap overlay (#1739 PARITY3-F1-KEYMAP-OVERLAY)

`F1` (command 111, `MSG_CMD_KEYMAP_DISP`) opens and closes a translucent
panel over the flight view without pausing (`mission_session/keymap_overlay.rs`).
It reads the live `ControlsMap`, so rebinding is reflected.

## Source of each part

| Part | Source |
| --- | --- |
| Category title | `strings.dll` label id of the category (3005 Movement), via `MissionMessages` |
| Command labels | `strings.dll` label id per row (`OriginalDefaultBinding::label_id`) |
| Key names | Project spelling. `strings.dll`'s `MSG_KEY_*` strings are the typed character (`MSG_KEY_NUMPAD0` = `0`, `MSG_KEY_NUMPADENTER` = CR), **not** the printed name, so they are not used |
| `Num 0`, `Num Del` | Read off the owner's frame; other numpad names follow the pattern (designed) |

## Unknown / designed (never `verified_original`)

- Position, size, mint colour, opacity, font and column gap: not measured from
  the engine or data; designed to match the owner's frame by eye.
- Whether the original pages through the other six categories: unknown. `F1`
  opens and closes the first category only.
- Whether joystick/mouse buttons are printed on a line: unknown; keys only.
- The permanent bottom help strip is the developer overlay; the composed
  mission already hides it unless `--dev-overlay` is passed.

The owner-command smoke now presses `F1` after the rocket phase and
photographs `frame_*_keymap_help.png`.

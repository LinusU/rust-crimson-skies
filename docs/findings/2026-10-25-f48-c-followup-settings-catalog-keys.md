# F48-C follow-up: the engine settings catalog keys

Task: F48-C-FOLLOWUP-SETTINGS-CATALOG (#460). Code: `cs_content::settings::{ENGINE_SETTING_RULES, engine_catalog}`;
wiring in `cs_app::ui::front_end::flow` (`SessionSetup::new`) and `cs_app::ui::front_end::pilot_progress::open_session`.
Tests: `crates/cs_app/tests/accessibility/catalog.rs` (prefix `accept_f48_c_followup_`).

**Every key, range and default below is engine-designed (F52-A `Settings::designed`). None is an original-game
setting, and none is `verified_original`.**

## Declared keys

| Key | Rule | Default | Apply | Owner | Reason |
| --- | --- | --- | --- | --- | --- |
| `accessibility.ui_scale_percent` | integer 100..=300 | 100 | Live | F52 | UI layout is rebuilt from the scale each frame; no device is reopened. |
| `accessibility.subtitles` | false/true | false | Live | F52 | Read by the presentation per line. |
| `accessibility.colour_filter` | off/protanopia/deuteranopia/tritanopia/monochrome | off | Live | F52 | A final-image filter, switchable between frames. |
| `accessibility.reduce_shake` | false/true | false | Live | F52 | Filters cosmetic effects; never reaches the simulation. |
| `accessibility.reduce_flash` | false/true | false | Live | F52 | As above. |
| `display.width` | integer 320..=16384 | 1280 | RestartRequired | F17 (display policy), declared here by F52 | Surface recreation is not wired for a running session; a half-applied size must not happen. |
| `display.height` | integer 320..=16384 | 720 | RestartRequired | F17 | As above. |
| `modern.profile` | original-rules/modern-assist | original-rules | RestartRequired | F52 | It decides which rule set a run is fidelity-labelled with; changing it mid-mission would make the label lie. |
| `modern.mouse_flight` | false/true | false | Live | F52 | An assist, inert unless `modern.profile` is `modern-assist`. |
| `modern.controller_flight` | false/true | false | Live | F52 | As above. |

## Not declared (still unknown keys to a profile session: preserved, never interpreted)

- Per-bus audio levels: a fractional gain fits neither `Integer` nor `Choice`; needs a rule shape or minor-unit encoding.
- `fov_degrees`: optional integer (`none` or 40..=130); needs an optional-integer rule shape.
- F22 device bindings: owned by F22, not yet declared.
- Original option set: unknown (F52-A finding, limit 1).

## Note

The profile document keys are namespaced (`accessibility.*`, `display.*`, `modern.*`); the F52 `cs-settings` file keeps its
own flat keys. The profile catalog and `SettingsSession` file are separate stores; nothing yet copies one into the other.

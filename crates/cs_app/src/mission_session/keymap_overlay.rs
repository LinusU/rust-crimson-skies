//! The `F1` "View Help" keymap overlay (PARITY3-F1-KEYMAP-OVERLAY, #1739).
//!
//! In the original, `F1` (`MSG_CMD_KEYMAP_DISP`, command 111) toggles a
//! translucent mint-green panel over the flight view without pausing it:
//! white text in two columns, a category title and one line per command with
//! its key. This module builds that panel's content from the **live**
//! [`ControlsMap`] — so it follows rebinding — and draws it.
//!
//! * **Text** is the installation's own where `strings.dll` has it: the
//!   category title is the category's `MSG_*_CONTROLS` label and each command
//!   line its `label_id` string, read through [`MissionMessages`]. Key names
//!   are **not** in the catalog: its `MSG_KEY_*` strings are the character a
//!   key types (`MSG_KEY_NUMPAD0` is `0`, `MSG_KEY_DECIMAL` is `Num .`), not
//!   the name the overlay prints. Keys are therefore spelled by
//!   [`overlay_key_name`]: [`scancode_name`], with the numpad spelled `Num 0`
//!   and `Num Del` as in the owner's frame (the other numpad names are the
//!   same pattern, designed). With no catalog every label falls back to the
//!   project's own wording, never to blank text.
//! * **Shown**: the first category only ([`OriginalControlCategory::Movement`]),
//!   as in the owner's frame. Whether the original pages through the other six
//!   categories (another press of `F1`, a click) is **unknown**; `F1` here
//!   opens and closes the one page.
//! * **Look**: the rectangle's position, size, mint colour and opacity, the
//!   font and the column gap were read off no engine table and are **designed**
//!   (`verified_original = false`); they are not measured.
//! * **Inputs listed**: the two key slots only. Whether the original also
//!   prints the joystick or mouse button on a line is unknown.

use bevy::prelude::{
    BackgroundColor, Color, Commands, Component, FlexDirection, Node, PositionType, Query, Res,
    Text, TextColor, TextFont, UiRect, Val, Visibility, With, default,
};
use bevy::text::FontSize;
use cs_content::config::{MessageCatalog, StringLookup};
use cs_types::input::{ORIGINAL_DEFAULT_BINDINGS, OriginalControlCategory, OriginalKeyCode};
use cs_types::keymap::{ControlsMap, command_name, scancode_name};

use super::original_keys::{ControlsHelpOpen, ControlsHelpPanel, MissionControls};
use crate::ui::hud::MissionMessages;

/// The category the overlay shows.
pub const OVERLAY_CATEGORY: OriginalControlCategory = OriginalControlCategory::Movement;

/// The separator between a command's two keys (`,  / Num 0` in the owner's
/// frame).
pub const KEY_SEPARATOR: &str = " / ";

/// The numpad names, `(scancode, name)`: `Num 0` and `Num Del` are read off the
/// owner's frame; the rest follow the same pattern (designed).
const NUMPAD_NAMES: [(u8, &str); 15] = [
    (0x52, "Num 0"),
    (0x4f, "Num 1"),
    (0x50, "Num 2"),
    (0x51, "Num 3"),
    (0x4b, "Num 4"),
    (0x4c, "Num 5"),
    (0x4d, "Num 6"),
    (0x47, "Num 7"),
    (0x48, "Num 8"),
    (0x49, "Num 9"),
    (0x53, "Num Del"),
    (0x4e, "Num +"),
    (0x4a, "Num -"),
    (0x37, "Num *"),
    (0xb5, "Num /"),
];

/// Where the overlay's strings come from.
#[derive(Clone, Copy)]
pub struct LabelSource<'a> {
    /// The text of a `strings.dll` string id.
    pub by_id: &'a dyn Fn(u32) -> Option<String>,
}

impl<'a> LabelSource<'a> {
    /// No catalog: every label falls back to the project's own wording.
    pub const NONE: LabelSource<'static> = LabelSource { by_id: &|_| None };

    /// Reads the strings from the installation's catalog; the closures it
    /// needs are built by the caller (they borrow the catalog).
    pub fn text_of_id(catalog: &MessageCatalog, id: u32) -> Option<String> {
        match catalog.strings().resolve(id, None) {
            StringLookup::Found(row) => row.text.clone().filter(|text| !text.is_empty()),
            _ => None,
        }
    }
}

/// The overlay's content: a title and one `(command, keys)` pair per line.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct KeymapPage {
    /// The category title.
    pub title: String,
    /// `(command label, key names)` per command, in the controls screen's
    /// order.
    pub lines: Vec<(String, String)>,
}

/// A key code as the overlay spells it: `Ctrl+`/`Alt+`/`Shift+`, then the key's
/// name.
#[must_use]
pub fn overlay_key_name(code: OriginalKeyCode) -> String {
    let mut name = String::new();
    if code.has_ctrl() {
        name.push_str("Ctrl+");
    }
    if code.has_alt() {
        name.push_str("Alt+");
    }
    if code.has_shift() {
        name.push_str("Shift+");
    }
    let scancode = code.scancode() as u8;
    match NUMPAD_NAMES.iter().find(|(known, _)| *known == scancode) {
        Some((_, numpad)) => name.push_str(numpad),
        None => name.push_str(&scancode_name(scancode)),
    }
    name
}

/// The page for `category` under the live `controls`.
#[must_use]
pub fn keymap_page(
    controls: &ControlsMap,
    category: OriginalControlCategory,
    source: &LabelSource<'_>,
) -> KeymapPage {
    let title = (source.by_id)(category.label_id()).unwrap_or_else(|| category.label().to_owned());
    let lines = controls
        .rows()
        .iter()
        .filter(|row| row.category == category)
        .map(|row| {
            let label = ORIGINAL_DEFAULT_BINDINGS
                .iter()
                .find(|original| original.command_id == row.command_id)
                .and_then(|original| (source.by_id)(original.label_id))
                .unwrap_or_else(|| command_name(row.command_id).to_owned());
            let keys: Vec<String> = [row.key_a, row.key_b]
                .iter()
                .filter(|code| !code.is_none())
                .map(|code| overlay_key_name(*code))
                .collect();
            (label, keys.join(KEY_SEPARATOR))
        })
        .collect();
    KeymapPage { title, lines }
}

/// The page under an optional installation catalog.
#[must_use]
pub fn keymap_page_from_catalog(
    controls: &ControlsMap,
    category: OriginalControlCategory,
    catalog: Option<&MessageCatalog>,
) -> KeymapPage {
    match catalog {
        Some(catalog) => keymap_page(
            controls,
            category,
            &LabelSource {
                by_id: &|id| LabelSource::text_of_id(catalog, id),
            },
        ),
        None => keymap_page(controls, category, &LabelSource::NONE),
    }
}

/// The overlay's translucent rectangle (designed colour: mint green).
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KeymapOverlay;

/// The overlay's title text.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KeymapTitle;

/// The overlay's command-label column.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KeymapLabels;

/// The overlay's key column.
#[derive(Component, Clone, Copy, Debug, Default)]
pub struct KeymapKeys;

/// The designed fill: mint green at 55 % opacity.
const OVERLAY_FILL: Color = Color::srgba(0.42, 0.86, 0.66, 0.55);

pub(super) fn spawn_keymap_overlay(mut commands: Commands) {
    let font = TextFont {
        font_size: FontSize::Px(16.0),
        ..default()
    };
    commands
        .spawn((
            Node {
                position_type: PositionType::Absolute,
                top: Val::Percent(14.0),
                left: Val::Percent(8.0),
                flex_direction: FlexDirection::Column,
                row_gap: Val::Px(8.0),
                padding: UiRect::all(Val::Px(14.0)),
                ..default()
            },
            BackgroundColor(OVERLAY_FILL),
            Visibility::Hidden,
            KeymapOverlay,
            ControlsHelpPanel,
        ))
        .with_children(|root| {
            root.spawn((
                Text::new(""),
                font.clone(),
                TextColor(Color::WHITE),
                KeymapTitle,
            ));
            root.spawn(Node {
                column_gap: Val::Px(28.0),
                ..default()
            })
            .with_children(|row| {
                row.spawn((
                    Text::new(""),
                    font.clone(),
                    TextColor(Color::WHITE),
                    KeymapLabels,
                ));
                row.spawn((Text::new(""), font, TextColor(Color::WHITE), KeymapKeys));
            });
        });
}

/// One overlay text and which column it is.
type OverlayTextRow<'a> = (
    &'a mut Text,
    Option<&'static KeymapTitle>,
    Option<&'static KeymapLabels>,
    Option<&'static KeymapKeys>,
);

pub(super) fn update_keymap_overlay(
    controls: Res<MissionControls>,
    open: Res<ControlsHelpOpen>,
    messages: Option<Res<MissionMessages>>,
    mut roots: Query<&mut Visibility, With<KeymapOverlay>>,
    mut texts: Query<OverlayTextRow>,
) {
    let wanted = if open.0 {
        Visibility::Visible
    } else {
        Visibility::Hidden
    };
    for mut visibility in &mut roots {
        if *visibility != wanted {
            *visibility = wanted;
        }
    }
    if !open.0 {
        return;
    }
    let page = keymap_page_from_catalog(
        &controls.0,
        OVERLAY_CATEGORY,
        messages.as_deref().and_then(MissionMessages::catalog),
    );
    let labels = page
        .lines
        .iter()
        .map(|(label, _)| label.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let keys = page
        .lines
        .iter()
        .map(|(_, keys)| keys.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    for (mut text, title, label_column, key_column) in &mut texts {
        let body = if title.is_some() {
            &page.title
        } else if label_column.is_some() {
            &labels
        } else if key_column.is_some() {
            &keys
        } else {
            continue;
        };
        if text.0 != *body {
            text.0.clone_from(body);
        }
    }
}

#[cfg(test)]
mod tests {
    //! Acceptance for Rally #1739 `PARITY3-F1-KEYMAP-OVERLAY`. The strings in
    //! the fakes are newly authored; the retail test reads the installation.

    use super::*;
    use cs_types::keymap::ControlsMap;

    fn fake_by_id(id: u32) -> Option<String> {
        match id {
            3005 => Some("Fake Movement".to_owned()),
            11057 => Some("Fake Nose Down".to_owned()),
            _ => None,
        }
    }

    const FAKE: LabelSource<'static> = LabelSource { by_id: &fake_by_id };

    #[test]
    fn accept_parity3_f1_keymap_overlay_lists_the_live_bindings() {
        let controls = ControlsMap::original_default();
        let page = keymap_page(&controls, OVERLAY_CATEGORY, &FAKE);
        assert_eq!(page.title, "Fake Movement");
        // Pitch down is the first row, bound to the Up arrow; its label comes
        // from the catalog by id, the key from the project's own spelling.
        assert_eq!(
            page.lines[0],
            ("Fake Nose Down".to_owned(), "Up".to_owned())
        );
        // Turn left: `,` and the numpad 0.
        let turn_left = page
            .lines
            .iter()
            .find(|(_, keys)| keys.contains("Num 0"))
            .expect("a line bound to numpad 0");
        assert_eq!(turn_left.1, format!(",{KEY_SEPARATOR}Num 0"));
        // Level off carries its modifier.
        assert!(
            page.lines.iter().any(|(_, keys)| keys == "Shift+L"),
            "{page:?}"
        );
    }

    #[test]
    fn accept_parity3_f1_keymap_overlay_follows_rebinding() {
        use cs_types::input::OriginalKeyCode;
        use cs_types::keymap::{ControlInput, ControlSlot};
        let mut controls = ControlsMap::original_default();
        controls
            .assign(
                1,
                ControlSlot::KeyA,
                ControlInput::Key(OriginalKeyCode::new(0x200 | 0x1e)),
            )
            .expect("assignable");
        let page = keymap_page(&controls, OVERLAY_CATEGORY, &LabelSource::NONE);
        assert_eq!(page.lines[0].1, "Ctrl+A");
    }

    #[test]
    fn accept_parity3_f1_keymap_overlay_falls_back_without_a_catalog() {
        let page = keymap_page(
            &ControlsMap::original_default(),
            OVERLAY_CATEGORY,
            &LabelSource::NONE,
        );
        assert_eq!(page.title, "movement");
        assert!(page.lines.iter().all(|(label, _)| !label.is_empty()));
    }

    /// The installation's own strings: the owner's frame shows `Point Nose
    /// Down` / `Up` and `Turn Left` / `, / Num 0` under `Movement`.
    #[test]
    #[ignore = "requires CS_GAME_DIR"]
    fn accept_parity3_f1_keymap_overlay_reads_the_installation_strings() {
        let root = std::env::var("CS_GAME_DIR").expect("CS_GAME_DIR must be set");
        let messages = MissionMessages::read(
            std::path::Path::new(&root),
            cs_types::evidence::ContentHash::from_bytes([0; 32]),
        );
        let catalog = messages.catalog().expect("strings.dll reads");
        let page = keymap_page_from_catalog(
            &ControlsMap::original_default(),
            OVERLAY_CATEGORY,
            Some(catalog),
        );
        println!("{page:#?}");

        assert!(!page.title.is_empty());
        assert_eq!(page.lines[0].1, "Up");
        assert_eq!(page.lines[4].1, ", / Num 0");
        assert_eq!(page.lines[5].1, ". / Num Del");
        assert!(
            page.lines.iter().any(|(_, keys)| keys.contains(',')),
            "{page:?}"
        );
    }
}

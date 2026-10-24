//! The windowed drawing of the Plane Construction hub (task #2042): the `T`
//! texts, the tabs' captions, the plane-name edit box and the marked
//! not-built panel line over the `PlaneConstruction` section's panes and
//! buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names; the
//! original's faces are not in the installation's public data, so this is a
//! stand-in. The colours and geometry are the layout's.

use bevy::prelude::{Entity, World};

use cs_content::ui_layout::AspectFit;

use super::construction_hub::HubPage;
use super::construction_panels::ChoicePanel;
use super::construction_slots::SlotPanel;
use super::previous_missions_view::{CAPTION_Z, Canvas, DEFAULT_SIZE, Z, argb};
use super::shell::{SECTION_SURFACE, ShellRect};

/// The marked line that stands in for a panel nobody has built.
const PANEL_NOTE_COLOUR: u32 = 0xFFFF_FF00;
/// The cancel prompt's colour.
const PROMPT_COLOUR: u32 = 0xFFFF_FFFF;
/// Where the panel area is: the line sits in the middle of it.
const PANEL_AREA: ShellRect = ShellRect {
    x: 0,
    y: 230,
    width: 800,
    height: 24,
};
/// The cancel prompt's box, over the panel area.
const PROMPT_AREA: ShellRect = ShellRect {
    x: 100,
    y: 280,
    width: 600,
    height: 40,
};
const PANEL_NOTE_SIZE: f32 = 14.0;
/// The drop-down's ground, frame and highlight: unread, designed (the
/// control's own art is not drawn).
const GROUND: u32 = 0xFF00_0000;
const FRAME: u32 = 0xFF73_699C;
const HIGHLIGHT: u32 = 0xFF37_277A;
const ITEM_TEXT: u32 = 0xFFFF_FFFF;
/// The open list sits over the buttons.
const LIST_Z: f32 = 20.0;

/// Spawns the page's overlay and returns its entities. `buttons` are the
/// section's buttons' rects by upper-cased id, which the captions sit
/// centred on.
pub(super) fn spawn_page(
    world: &mut World,
    page: &HubPage,
    buttons: &[(String, ShellRect)],
    surface: (u32, u32),
) -> Vec<Entity> {
    let Some(fit) = AspectFit::new(SECTION_SURFACE, surface) else {
        return Vec::new();
    };
    let scale = fit.image_rect().width as f32 / SECTION_SURFACE.0 as f32;
    let mut canvas = Canvas {
        world,
        fit,
        surface,
        scale,
        out: Vec::new(),
    };
    for text in page.texts() {
        canvas.text(
            text.rect,
            &text.text,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    }
    for tab in page.tabs() {
        let Some((_, rect)) = buttons.iter().find(|(id, _)| *id == tab.button) else {
            continue;
        };
        let size = tab.size.map_or(DEFAULT_SIZE, |size| size as f32);
        // The selected tab is held down: the strip's depressed colour.
        let selected = tab.button.eq_ignore_ascii_case(page.selected().button());
        let color = tab.colors[if selected { 3 } else { 1 }];
        let line = ShellRect {
            x: rect.x,
            y: rect.y + (rect.height as f32 * 0.5 - size * 0.6) as i32,
            width: rect.width,
            height: rect.height,
        };
        canvas.text(line, &tab.text, size, argb(color), 1, CAPTION_Z);
    }
    let edit = page.edit();
    if let Some(frame) = edit.frame_color {
        canvas.frame(edit.rect, argb(frame), Z + 0.4);
    }
    canvas.text(
        edit.rect,
        &format!("{}|", page.text()),
        edit.rect.height as f32,
        argb(edit.text_color),
        0,
        Z + 0.5,
    );
    if let Some(prompt) = page.prompt() {
        canvas.text(
            PROMPT_AREA,
            prompt,
            PANEL_NOTE_SIZE,
            argb(PROMPT_COLOUR),
            1,
            Z + 0.6,
        );
        canvas.text(
            ShellRect {
                y: PROMPT_AREA.y + 24,
                ..PROMPT_AREA
            },
            "Y / Enter: cancel the build     N / Esc: keep building",
            DEFAULT_SIZE + 2.0,
            argb(PANEL_NOTE_COLOUR),
            1,
            Z + 0.6,
        );
    } else if let Some(panel) = page.choice_panel(page.selected()) {
        draw_choice_panel(&mut canvas, panel);
    } else if let Some(panel) = page.slot_panel(page.selected()) {
        draw_slot_panel(&mut canvas, panel);
    } else if page.panel_built(page.selected()) {
        draw_armor_paint_panel(&mut canvas, page);
    } else {
        let (notice, task) = page.selected().not_built();
        canvas.text(
            PANEL_AREA,
            &format!("Not implemented: {} (task #{task})", notice.what),
            PANEL_NOTE_SIZE,
            argb(PANEL_NOTE_COLOUR),
            1,
            Z + 0.6,
        );
    }
    canvas.out
}

/// The Airframe or Engine panel: its title, the chosen entry's name, the
/// closed box with the chosen entry and, while open, the list. The
/// description box stays empty (its text source is unread).
fn draw_choice_panel(canvas: &mut Canvas<'_>, panel: &ChoicePanel) {
    for text in panel.texts() {
        canvas.text(
            text.rect,
            &text.text,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    }
    let dropdown = panel.dropdown();
    let line = |rect: ShellRect| ShellRect {
        x: rect.x + 4,
        y: rect.y + 2,
        width: rect.width.saturating_sub(8),
        height: rect.height,
    };
    canvas.block(dropdown.rect, argb(GROUND), Z + 0.1);
    canvas.frame(dropdown.rect, argb(FRAME), Z + 0.3);
    if let Some(option) = panel.selected_option() {
        canvas.text(
            line(dropdown.rect),
            &option.name,
            DEFAULT_SIZE + 2.0,
            argb(ITEM_TEXT),
            0,
            Z + 0.2,
        );
    }
    if panel.is_open() {
        for (index, option) in panel.options().iter().enumerate() {
            let row = panel.item_rect(index);
            canvas.block(row, argb(GROUND), LIST_Z);
            if Some(index) == panel.rolled().or(panel.selected()) {
                canvas.block(row, argb(HIGHLIGHT), LIST_Z + 0.1);
            }
            canvas.text(
                line(row),
                &option.name,
                DEFAULT_SIZE + 2.0,
                argb(ITEM_TEXT),
                0,
                LIST_Z + 0.2,
            );
        }
        canvas.frame(
            ShellRect {
                x: dropdown.rect.x,
                y: dropdown.rect.y + dropdown.rect.height as i32,
                width: dropdown.rect.width,
                height: dropdown.rect.height * panel.options().len() as u32,
            },
            argb(FRAME),
            LIST_Z + 0.3,
        );
    }
}

/// The list's closed-box ground, the item picked and the frame (stand-ins:
/// the original's `GN_DROPUP`/`GN_DROPDOWN` art and colours are unread).
const AP_GROUND: u32 = 0xFFE6_DABE;
const AP_PICKED: u32 = 0xFF80_8080;
const AP_FRAME: u32 = 0xFF00_0000;
const AP_HOVER: u32 = 0xFF0D_21E1;
const AP_TEXT_SIZE: f32 = 11.0;

/// A built panel: its description box and lists.
fn draw_armor_paint_panel(canvas: &mut Canvas<'_>, page: &HubPage) {
    if let Some((desc, text)) = page.panel_description() {
        canvas.block(desc.rect, argb(desc.back), Z + 0.1);
        canvas.frame(desc.rect, argb(desc.border), Z + 0.2);
        canvas.text(
            ShellRect {
                x: desc.rect.x + 4,
                y: desc.rect.y + 4,
                width: desc.rect.width.saturating_sub(8),
                height: desc.rect.height,
            },
            text,
            AP_TEXT_SIZE,
            argb(desc.color),
            0,
            Z + 0.3,
        );
    }
    for list in page.panel_lists() {
        let rect = list.row().rect;
        canvas.block(rect, argb(AP_GROUND), Z + 0.1);
        canvas.frame(rect, argb(AP_FRAME), Z + 0.3);
        if let Some((item, _)) = list.selected() {
            draw_item(canvas, list, item, rect, Z + 0.4);
        }
        if list.is_open() {
            for item in 0..list.shown() {
                for cell in 0..list.cells() {
                    let at = list.item_rect(item, cell);
                    let hot = list.hover() == Some((item, cell));
                    canvas.block(at, argb(AP_GROUND), LIST_Z);
                    if list.selected() == Some((item, cell)) {
                        canvas.block(at, argb(AP_PICKED), LIST_Z + 0.1);
                    }
                    canvas.frame(
                        at,
                        argb(if hot { AP_HOVER } else { AP_FRAME }),
                        LIST_Z + 0.2,
                    );
                    draw_item(canvas, list, item, at, LIST_Z + 0.3);
                }
            }
        }
    }
}

/// An item's swatch or text inside `rect`. A decal's picture is not drawn.
fn draw_item(
    canvas: &mut Canvas<'_>,
    list: &super::construction_armor_paint::DropList,
    item: usize,
    rect: ShellRect,
    z: f32,
) {
    let Some(entry) = list.items().get(item) else {
        return;
    };
    if let Some(swatch) = entry.swatch {
        canvas.block(
            ShellRect {
                x: rect.x + 3,
                y: rect.y + 3,
                width: rect.width.saturating_sub(6),
                height: rect.height.saturating_sub(6),
            },
            argb(swatch),
            z,
        );
    } else if !entry.label.is_empty() {
        canvas.text(
            ShellRect {
                x: rect.x + 4,
                y: rect.y + 2,
                width: rect.width.saturating_sub(8),
                height: rect.height,
            },
            &entry.label,
            AP_TEXT_SIZE,
            argb(AP_FRAME),
            0,
            z,
        );
    }
}

/// The Guns or Hardpoints panel: its texts, every position's closed box with
/// the chosen entry and, while one is open, its list. The description box
/// stays empty (its text source is unread).
fn draw_slot_panel(canvas: &mut Canvas<'_>, panel: &SlotPanel) {
    for text in panel.texts() {
        canvas.text(
            text.rect,
            &text.text,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    }
    let line = |rect: ShellRect| ShellRect {
        x: rect.x + 4,
        y: rect.y + 2,
        width: rect.width.saturating_sub(8),
        height: rect.height,
    };
    for slot in 0..panel.kind().slots() {
        let Some(dropdown) = panel.dropdown(slot) else {
            continue;
        };
        canvas.block(dropdown.rect, argb(GROUND), Z + 0.1);
        canvas.frame(dropdown.rect, argb(FRAME), Z + 0.3);
        if let Some(option) = panel
            .selected(slot)
            .and_then(|index| panel.options().get(index))
        {
            canvas.text(
                line(dropdown.rect),
                &option.name,
                DEFAULT_SIZE + 2.0,
                argb(ITEM_TEXT),
                0,
                Z + 0.2,
            );
        }
    }
    let Some(slot) = panel.open_slot() else {
        return;
    };
    let Some(dropdown) = panel.dropdown(slot) else {
        return;
    };
    for (index, option) in panel.options().iter().enumerate() {
        let Some(row) = panel.item_rect(slot, index) else {
            continue;
        };
        canvas.block(row, argb(GROUND), LIST_Z);
        if Some(index) == panel.rolled().or(panel.selected(slot)) {
            canvas.block(row, argb(HIGHLIGHT), LIST_Z + 0.1);
        }
        canvas.text(
            line(row),
            &option.name,
            DEFAULT_SIZE + 2.0,
            argb(ITEM_TEXT),
            0,
            LIST_Z + 0.2,
        );
    }
    canvas.frame(
        ShellRect {
            x: dropdown.rect.x,
            y: dropdown.rect.y + dropdown.rect.height as i32,
            width: dropdown.rect.width,
            height: dropdown.rect.height * panel.options().len() as u32,
        },
        argb(FRAME),
        LIST_Z + 0.3,
    );
}

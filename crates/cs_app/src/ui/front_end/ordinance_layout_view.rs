//! The windowed drawing of the Instant Action `OrdinanceLayout` screen (task
//! #2068): the seven titles, the plane line and the four gun names (greyed
//! for no gun), the four ammunition and eight rocket boxes with the held row's
//! text, the open list with its rolled row, and the two description boxes.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names; the
//! original's faces are not in the installation's public data, so this is a
//! stand-in. The colours and geometry of the layout's rows are the layout's.
//! The accept and cancel buttons are the shell's own button sprites. Not
//! drawn, because unread: the description texts (`E 2032`, `E 2033`), the
//! plane diagrams and the drop-downs' art. The frames and the row highlight
//! are stand-in colours.

use bevy::prelude::{Entity, World};

use cs_content::ui_layout::AspectFit;

use super::ordinance_layout::{AMMO_BOXES, OrdinanceLayoutScreen, OrdinanceList, ROCKET_BOXES};
use super::previous_missions_view::{Canvas, DEFAULT_SIZE, Z, argb};
use super::shell::SECTION_SURFACE;

const GROUND: u32 = 0xFF00_0000;
const FRAME: u32 = 0xFF73_699C;
/// The frame of a box that holds nothing (no gun, no rocket slot).
const FRAME_OFF: u32 = 0xFF40_3C55;
const HIGHLIGHT: u32 = 0xFF37_277A;
const ITEM_TEXT: u32 = 0xFFFF_FFFF;
/// The open list sits over the other boxes.
const LIST_Z: f32 = 20.0;

/// Spawns the screen's overlay and returns its entities.
pub(super) fn spawn_screen(
    world: &mut World,
    screen: &OrdinanceLayoutScreen,
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
    for text in screen.texts() {
        canvas.text(
            text.rect,
            &text.text,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    }
    for rect in [
        screen.ammo_description_rect(),
        screen.rocket_description_rect(),
    ] {
        canvas.frame(rect, argb(FRAME), Z + 0.3);
    }
    let boxes = (0..AMMO_BOXES)
        .map(|slot| (OrdinanceList::Ammo, slot))
        .chain((0..ROCKET_BOXES).map(|slot| (OrdinanceList::Rocket, slot)));
    for (list, slot) in boxes {
        let (closed, active) = match list {
            OrdinanceList::Ammo => (screen.ammo_box(slot), screen.ammo_active(slot)),
            OrdinanceList::Rocket => (screen.rocket_box(slot), screen.rocket_active(slot)),
        };
        let Some(closed) = closed.map(|row| row.rect) else {
            continue;
        };
        canvas.block(closed, argb(GROUND), Z + 0.1);
        canvas.frame(
            closed,
            argb(if active { FRAME } else { FRAME_OFF }),
            Z + 0.3,
        );
        if let Some(held) = screen.box_text(list, slot).filter(|_| active) {
            canvas.text(
                inset(closed),
                held,
                DEFAULT_SIZE + 2.0,
                argb(ITEM_TEXT),
                0,
                Z + 0.2,
            );
        }
    }
    if let Some((list, slot)) = screen.open_list() {
        let rows = screen.row_texts(list);
        let shown = rows.len().min(screen.displayed_rows(list, slot));
        for (index, row) in rows.iter().take(shown).enumerate() {
            let Some(rect) = screen.item_rect(list, slot, index) else {
                continue;
            };
            canvas.block(rect, argb(GROUND), LIST_Z);
            if screen.rolled() == Some(index) {
                canvas.block(rect, argb(HIGHLIGHT), LIST_Z + 0.1);
            }
            canvas.text(
                inset(rect),
                row,
                DEFAULT_SIZE + 2.0,
                argb(ITEM_TEXT),
                0,
                LIST_Z + 0.2,
            );
        }
        if let (Some(first), Some(last)) = (
            screen.item_rect(list, slot, 0),
            shown
                .checked_sub(1)
                .and_then(|index| screen.item_rect(list, slot, index)),
        ) {
            canvas.frame(
                super::shell::ShellRect {
                    height: last.y as u32 + last.height - first.y as u32,
                    ..first
                },
                argb(FRAME),
                LIST_Z + 0.3,
            );
        }
    }
    canvas.out
}

fn inset(rect: super::shell::ShellRect) -> super::shell::ShellRect {
    super::shell::ShellRect {
        x: rect.x + 4,
        y: rect.y + 1,
        width: rect.width.saturating_sub(8),
        height: rect.height,
    }
}

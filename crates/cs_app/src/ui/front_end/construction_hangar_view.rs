//! The windowed drawing of the Sell hangar (task #2063): the title and prompt
//! texts, the 25-row list, the selected plane's name and value lines and the
//! yes/no or OK box over the `Hangar` section's panes and buttons.
//!
//! Text uses the engine's default face at the size the `[FONT]` tag names; the
//! original's faces are not in the installation's public data, so this is a
//! stand-in. The colours and geometry of the layout's rows are the layout's.
//! The list's row text is unread (the drop-down's callback): a row shows the
//! plane's name as a **stand-in**, and the drop-down's own art is not drawn.
//! The four picture slots are not drawn: their art is not resolved here.

use bevy::prelude::{Entity, World};

use cs_content::ui_layout::AspectFit;

use super::construction_hangar::HangarPage;
use super::previous_missions_view::{CAPTION_Z, Canvas, DEFAULT_SIZE, Z, argb};
use super::shell::{SECTION_SURFACE, ShellRect};

/// The Sell and Export captions' colour columns carry an alpha byte of 0x1D
/// and 0x76 (`0x1D110C92`): drawn as such they vanish, so the colour is taken
/// opaque. What the alpha byte means for these rows is unread.
const OPAQUE: u32 = 0xFF00_0000;
const GROUND: u32 = 0xFF00_0000;
const FRAME: u32 = 0xFF73_699C;
const HIGHLIGHT: u32 = 0xFF37_277A;
const ITEM_TEXT: u32 = 0xFFFF_FFFF;
const NOTE_COLOUR: u32 = 0xFFFF_FF00;
/// The open list sits over the buttons.
const LIST_Z: f32 = 20.0;
/// The yes/no or OK box, over the list area.
const BOX_AREA: ShellRect = ShellRect {
    x: 100,
    y: 280,
    width: 600,
    height: 40,
};
const BOX_Z: f32 = 30.0;

/// Spawns the page's overlay and returns its entities. `note` is the OK box's
/// text while one is up.
pub(super) fn spawn_page(
    world: &mut World,
    page: &HangarPage,
    buttons: &[(String, ShellRect)],
    note: Option<&str>,
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
    let mut put = |text: &super::construction_slots::SlotText, line: &str| {
        canvas.text(
            text.rect,
            line,
            text.size.map_or(DEFAULT_SIZE, |size| size as f32),
            argb(text.color),
            text.justify,
            Z + 0.5,
        );
    };
    put(page.title(), &page.title().text);
    put(page.prompt(), &page.prompt().text);
    if let Some((text, line)) = page.plane_text() {
        put(text, &plain(&line));
    }
    if let Some((text, line)) = page.value_text() {
        put(text, &plain(&line));
    }
    for caption in page.captions() {
        let Some((_, rect)) = buttons.iter().find(|(id, _)| *id == caption.button) else {
            continue;
        };
        let size = caption.size.map_or(DEFAULT_SIZE, |size| size as f32);
        let line = ShellRect {
            y: rect.y + (rect.height as f32 * 0.5 - size * 0.6) as i32,
            ..*rect
        };
        canvas.text(
            line,
            &caption.text,
            size,
            argb(caption.colors[1] | OPAQUE),
            1,
            CAPTION_Z,
        );
    }
    let line = |rect: ShellRect| ShellRect {
        x: rect.x + 4,
        y: rect.y + 1,
        width: rect.width.saturating_sub(8),
        height: rect.height,
    };
    let closed = page.list().rect;
    canvas.block(closed, argb(GROUND), Z + 0.1);
    canvas.frame(closed, argb(FRAME), Z + 0.3);
    if let Some(plane) = page.selected_plane() {
        canvas.text(
            line(closed),
            &plane.name,
            DEFAULT_SIZE + 2.0,
            argb(ITEM_TEXT),
            0,
            Z + 0.2,
        );
    }
    if page.list_open() {
        let rows = page.row_count();
        for row in 0..rows {
            let rect = page.item_rect(row);
            canvas.block(rect, argb(GROUND), LIST_Z);
            if page.selected_index() == Some(row) {
                canvas.block(rect, argb(HIGHLIGHT), LIST_Z + 0.1);
            }
            if let Some(plane) = page.plane_at(row) {
                canvas.text(
                    line(rect),
                    &plane.name,
                    DEFAULT_SIZE + 2.0,
                    argb(ITEM_TEXT),
                    0,
                    LIST_Z + 0.2,
                );
            }
        }
        canvas.frame(
            ShellRect {
                x: closed.x,
                y: closed.y + closed.height as i32,
                width: closed.width,
                height: closed.height * rows as u32,
            },
            argb(FRAME),
            LIST_Z + 0.3,
        );
    }
    if let Some(asking) = page.asking() {
        draw_box(&mut canvas, asking, "Y / Enter: yes     N / Esc: no");
    } else if let Some(note) = note {
        draw_box(&mut canvas, note, "Enter / click: OK");
    }
    canvas.out
}

/// The strings mark bold with `<B>` and `<b>`, which the text drawing does not
/// read: they are left out.
fn plain(text: &str) -> String {
    text.replace("<B>", "").replace("<b>", "")
}

fn draw_box(canvas: &mut Canvas<'_>, text: &str, keys: &str) {
    canvas.block(
        ShellRect {
            height: BOX_AREA.height + 24,
            ..BOX_AREA
        },
        argb(GROUND),
        BOX_Z,
    );
    canvas.text(
        BOX_AREA,
        &plain(text),
        12.0,
        argb(ITEM_TEXT),
        1,
        BOX_Z + 0.1,
    );
    canvas.text(
        ShellRect {
            y: BOX_AREA.y + 32,
            ..BOX_AREA
        },
        keys,
        DEFAULT_SIZE + 2.0,
        argb(NOTE_COLOUR),
        1,
        BOX_Z + 0.1,
    );
}

//! The window's face of the `OrdinanceLayout` screen over the Flight Check
//! (task #2070): the layout's background pane, the seven titles, the plane line
//! and gun names, the four ammunition and eight rocket boxes with the held
//! row's text, the open list with its rolled row, and the accept and cancel
//! buttons. The model and its rules are
//! [`OrdinanceLayoutScreen`](crate::ui::front_end::ordinance_layout::OrdinanceLayoutScreen);
//! this file only draws it, in the Flight Check's own UI-node style.
//!
//! As in the shell window's presenter (#2068) the face is the default one at
//! the `[FONT]` size, the boxes' frames and the row highlight are stand-in
//! colours, and the description texts, plane diagrams and drop-down art are
//! not drawn: they are unread.

use bevy::prelude::{
    BackgroundColor, BorderColor, Color, Commands, Component, Entity, GlobalZIndex, ImageNode,
    Local, Node, PositionType, Query, Res, ResMut, Text, TextColor, TextFont, UiRect, Val, Window,
    With, default,
};
use bevy::text::{FontSize, Justify, TextLayout};
use cs_content::ui_layout::{AspectFit, Rect};

use super::flight_check::{
    FlightCheckArtwork, FlightCheckPhase, Logical, LogicalFont, PlacedPart, PreMissionStage, argb,
    to_image,
};
use crate::ui::front_end::BUTTON_STATE_FRAMES;
use crate::ui::front_end::SECTION_SURFACE;
use crate::ui::front_end::ShellRect;
use crate::ui::front_end::ordinance_layout::{
    AMMO_BOXES, OrdinanceList, OrdinanceSide, ROCKET_BOXES,
};

/// Everything the screen spawns.
#[derive(Component)]
pub(super) struct OrdPart;

const GROUND: u32 = 0xFF00_0000;
const FRAME: u32 = 0xFF73_699C;
/// The frame of a box that holds nothing (no gun, no rocket slot).
const FRAME_OFF: u32 = 0xFF40_3C55;
const HIGHLIGHT: u32 = 0xFF37_277A;
const ITEM_TEXT: u32 = 0xFFFF_FFFF;
/// The `[FONT]` size a row without a tag is drawn at.
const DEFAULT_SIZE: f32 = 15.0;

// Roots of one global index are stacked in no stated order, so every layer
// that must be above another has an index of its own.
const COVER: i32 = 2000;
const PANE: i32 = 2001;
const BOX_GROUND: i32 = 2002;
const BOX_FACE: i32 = 2003;
const LIST_GROUND: i32 = 2004;
const LIST_HIGHLIGHT: i32 = 2005;
const LIST_FACE: i32 = 2006;

fn logical(rect: ShellRect) -> Rect {
    Rect {
        x: rect.x.max(0) as u32,
        y: rect.y.max(0) as u32,
        width: rect.width,
        height: rect.height,
    }
}

fn inset(rect: ShellRect) -> ShellRect {
    ShellRect {
        x: rect.x + 4,
        y: rect.y + 1,
        width: rect.width.saturating_sub(8),
        height: rect.height,
    }
}

/// Spawns and despawns the screen as the phase opens and leaves it, and
/// rebuilds it when the model moved.
pub(super) fn sync_ordinance_screen(
    mut commands: Commands,
    phase: Res<FlightCheckPhase>,
    artwork: Res<FlightCheckArtwork>,
    mut images: ResMut<bevy::prelude::Assets<bevy::prelude::Image>>,
    mut shown: Local<Option<(OrdinanceSide, u64)>>,
    parts: Query<Entity, With<OrdPart>>,
) {
    let wanted = phase
        .ordinance_stage()
        .map(|stage| (stage.side(), stage.version()));
    if wanted == *shown {
        return;
    }
    for part in &parts {
        commands.entity(part).despawn();
    }
    *shown = wanted;
    let (Some(stage), Some(setup)) = (phase.ordinance_stage(), phase.ordinance()) else {
        return;
    };
    let screen = stage.screen();
    let block = |commands: &mut Commands, rect: ShellRect, color: u32, layer: i32| {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            BackgroundColor(argb(color)),
            Logical(logical(rect)),
            GlobalZIndex(layer),
            OrdPart,
        ));
    };
    let frame = |commands: &mut Commands, rect: ShellRect, color: u32, layer: i32| {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                border: UiRect::all(Val::Px(1.0)),
                ..default()
            },
            BorderColor::all(argb(color)),
            Logical(logical(rect)),
            GlobalZIndex(layer),
            OrdPart,
        ));
    };
    let text = |commands: &mut Commands,
                rect: ShellRect,
                line: &str,
                size: f32,
                color: u32,
                justify: u32,
                layer: i32| {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            Text::new(line.to_owned()),
            TextFont {
                font_size: FontSize::Px(size),
                ..default()
            },
            TextColor(argb(color)),
            TextLayout::justify(match justify {
                1 => Justify::Center,
                2 => Justify::Right,
                _ => Justify::Left,
            }),
            Logical(logical(rect)),
            LogicalFont(size),
            GlobalZIndex(layer),
            OrdPart,
        ));
    };

    commands.spawn((
        Node {
            position_type: PositionType::Absolute,
            left: Val::Px(0.0),
            top: Val::Px(0.0),
            width: Val::Percent(100.0),
            height: Val::Percent(100.0),
            ..default()
        },
        BackgroundColor(Color::BLACK),
        GlobalZIndex(COVER),
        OrdPart,
    ));
    if let Some(art) = artwork.get(setup.background()) {
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            ImageNode::new(images.add(to_image(art))),
            Logical(Rect {
                x: 0,
                y: 0,
                width: SECTION_SURFACE.0,
                height: SECTION_SURFACE.1,
            }),
            GlobalZIndex(PANE),
            OrdPart,
        ));
    }
    for line in screen.texts() {
        text(
            &mut commands,
            line.rect,
            &line.text,
            line.size.map_or(DEFAULT_SIZE, |size| size as f32),
            line.color,
            line.justify,
            BOX_FACE,
        );
    }
    for rect in [
        screen.ammo_description_rect(),
        screen.rocket_description_rect(),
    ] {
        frame(&mut commands, rect, FRAME, BOX_FACE);
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
        block(&mut commands, closed, GROUND, BOX_GROUND);
        frame(
            &mut commands,
            closed,
            if active { FRAME } else { FRAME_OFF },
            BOX_FACE,
        );
        if let Some(held) = screen.box_text(list, slot).filter(|_| active) {
            text(
                &mut commands,
                inset(closed),
                held,
                DEFAULT_SIZE + 2.0,
                ITEM_TEXT,
                0,
                BOX_FACE,
            );
        }
    }
    if let Some((list, slot)) = screen.open_list() {
        let rows = screen.row_texts(list);
        let shown_rows = rows.len().min(screen.displayed_rows(list, slot));
        for (index, row) in rows.iter().take(shown_rows).enumerate() {
            let Some(rect) = screen.item_rect(list, slot, index) else {
                continue;
            };
            block(&mut commands, rect, GROUND, LIST_GROUND);
            if screen.rolled() == Some(index) {
                block(&mut commands, rect, HIGHLIGHT, LIST_HIGHLIGHT);
            }
            text(
                &mut commands,
                inset(rect),
                row,
                DEFAULT_SIZE + 2.0,
                ITEM_TEXT,
                0,
                LIST_FACE,
            );
        }
        if let (Some(first), Some(last)) = (
            screen.item_rect(list, slot, 0),
            shown_rows
                .checked_sub(1)
                .and_then(|index| screen.item_rect(list, slot, index)),
        ) {
            frame(
                &mut commands,
                ShellRect {
                    height: last.y.max(0) as u32 + last.height - first.y.max(0) as u32,
                    ..first
                },
                FRAME,
                LIST_FACE,
            );
        }
    }
    for button in setup.buttons() {
        let Some(art) = artwork.get(&button.art) else {
            continue;
        };
        // The first state frame of the strip, the one at rest.
        let frame_height = art.height() / BUTTON_STATE_FRAMES;
        let mut node = ImageNode::new(images.add(to_image(art)));
        let (right, lower) = art.presented_texel(art.width() as f32, frame_height as f32);
        node.rect = Some(bevy::math::Rect::new(0.0, 0.0, right, lower));
        commands.spawn((
            Node {
                position_type: PositionType::Absolute,
                ..default()
            },
            node,
            Logical(button.rect),
            GlobalZIndex(BOX_FACE),
            OrdPart,
        ));
    }
}

/// Places the screen's parts on the aspect-fit window.
pub(super) fn fit_ordinance_screen(
    phase: Res<FlightCheckPhase>,
    windows: Query<&Window>,
    mut parts: Query<PlacedPart, With<OrdPart>>,
) {
    if phase.stage() != PreMissionStage::Ordinance {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let surface = (window.width() as u32, window.height() as u32);
    let Some(fit) = AspectFit::new(SECTION_SURFACE, surface) else {
        return;
    };
    let scale = fit.image_rect().width as f32 / SECTION_SURFACE.0 as f32;
    for (logical, mut node, font) in &mut parts {
        let rect = fit.map_rect(logical.0);
        node.left = Val::Px(rect.x as f32);
        node.top = Val::Px(rect.y as f32);
        node.width = Val::Px(rect.width as f32);
        if font.is_none() {
            node.height = Val::Px(rect.height as f32);
        }
        if let Some((designed, mut text_font)) = font {
            text_font.font_size = FontSize::Px(designed.0 * scale);
        }
    }
}

/// Feeds the pointer to the screen: the row under it is rolled over while a
/// list is open.
pub(super) fn ordinance_hover(mut phase: ResMut<FlightCheckPhase>, windows: Query<&Window>) {
    if phase.stage() != PreMissionStage::Ordinance {
        return;
    }
    let Ok(window) = windows.single() else { return };
    let surface = (window.width() as u32, window.height() as u32);
    let at = window
        .cursor_position()
        .map(|at| (at.x.max(0.0) as u32, at.y.max(0.0) as u32));
    phase.ordinance_hover(surface, at);
}

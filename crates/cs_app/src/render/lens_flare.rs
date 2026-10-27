//! The lens flare's placement and fade rule (PARITY4-LENS-FLARE-B, #1859).
//!
//! The rule is the engine's flare draw `0x57ca80` and the fade of `0x57cc00`,
//! recovered statically in `docs/findings/2026-10-21-parity4-lens-flare-engine-
//! rule.md` §2–3 and `docs/findings/2026-10-22-parity4-lens-flare-a-sun-source-
//! and-blend.md`. Everything here is pure: a projected light position, the
//! viewport extent and the fade in, four screen-space quads out. The consumer
//! that projects the light and draws the quads is
//! `crate::mission_session::lens_flare`.
//!
//! With `P` the light's projected screen position, `C` the viewport centre,
//! `W` the viewport width and `f` the fade, the quads are:
//!
//! | quad | texture | centre | half-extent |
//! | --- | --- | --- | --- |
//! | 0 | `lflare1` | `P` | `W·f·0.0625` |
//! | 1 | `lflare2` | `C + 0.5·(P − C)` | `W·f·0.0625` |
//! | 2 | `lflare3` | `C + 0.1·(P − C)` | `W·f·0.0625·0.5` |
//! | 3 | `lflare4` | `C − (P − C)` | `W·f·0.0625·1.5` |
//!
//! The engine's sprite routine returns without drawing a quad whose half-extent
//! is below one pixel; so does [`flare_quads`](crate::render::lens_flare::flare_quads).
//! Provenance `static`; nothing
//! here is `verified_original`.

/// The fraction of the viewport width that is the glow's half-extent at full
/// fade (`0.0625`, image VA `0x6093d4`).
pub const HALF_EXTENT_OF_WIDTH: f32 = 0.0625;
/// Where quad 1 sits on the line from the viewport centre to the light
/// (`0.5`, VA `0x6032e0`).
pub const RING_POSITION: f32 = 0.5;
/// Where quad 2 sits on that line (`0.1`, VA `0x6034a8`).
pub const SMALL_RING_POSITION: f32 = 0.1;
/// Quad 2's size relative to the glow (`0.5`).
pub const SMALL_RING_SCALE: f32 = 0.5;
/// Quad 3's size relative to the glow (`1.5`, VA `0x603460`).
pub const BIG_RING_SCALE: f32 = 1.5;
/// Quads whose half-extent is below this many pixels are not drawn.
pub const MIN_HALF_EXTENT_PX: f32 = 1.0;

/// One flare quad in viewport pixels (origin at the top left, y down).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlareQuad {
    /// Which `lflare` texture (0 = `lflare1` .. 3 = `lflare4`).
    pub texture: usize,
    /// The quad centre in pixels.
    pub centre: [f32; 2],
    /// The half-extent in pixels; the quad is square.
    pub half_extent: f32,
}

/// The flare's fade for a light at `depth` (the projected depth in the
/// viewing direction), given the light's stored fade pair.
///
/// `t = 1/depth`; `t >= max` draws nothing (`None`), `t < min` is full (`1`),
/// between it is `(max − t) / (max − min)`. A depth that is not positive has no
/// flare (`None`), as the record queue drops it.
#[must_use]
pub fn flare_fade(depth: f32, min: f32, max: f32) -> Option<f32> {
    if depth <= 0.0 || depth.is_nan() {
        return None;
    }
    let t = 1.0 / depth;
    if t >= max {
        None
    } else if t < min {
        Some(1.0)
    } else {
        Some((max - t) / (max - min))
    }
}

/// The quads of one flare, in the engine's draw order.
///
/// `viewport` is the viewport extent in pixels, `projected` the light's
/// projected position in the same pixels and `fade` the value from
/// [`flare_fade`]. Quads under [`MIN_HALF_EXTENT_PX`] are left out.
#[must_use]
pub fn flare_quads(viewport: [f32; 2], projected: [f32; 2], fade: f32) -> Vec<FlareQuad> {
    let centre = [viewport[0] * 0.5, viewport[1] * 0.5];
    let along = |position: f32| {
        [
            centre[0] + position * (projected[0] - centre[0]),
            centre[1] + position * (projected[1] - centre[1]),
        ]
    };
    let half = viewport[0] * fade * HALF_EXTENT_OF_WIDTH;
    [
        (projected, half),
        (along(RING_POSITION), half),
        (along(SMALL_RING_POSITION), half * SMALL_RING_SCALE),
        (along(-1.0), half * BIG_RING_SCALE),
    ]
    .into_iter()
    .enumerate()
    .filter(|(_, (_, half_extent))| *half_extent >= MIN_HALF_EXTENT_PX)
    .map(|(texture, (centre, half_extent))| FlareQuad {
        texture,
        centre,
        half_extent,
    })
    .collect()
}

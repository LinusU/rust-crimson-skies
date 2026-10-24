//! Where the engine draws a script pane that has `scale`, `rotate` and
//! `origin` applied (task #2039, finding
//! `docs/findings/2026-10-24-cabin-memento-pane-transform.md`).
//!
//! `CTL.SCRIPT`'s picture object (`ZJ`) runs `scale(YH) = GK, HK, IK` and
//! `rotate(YH) = 0, 0, JK` and then `render YH, AI.x, AI.y` every frame. The
//! statement handlers and the pane draw were read in the decrypted engine
//! image (provenance `static`, never `verified_original`):
//!
//! * `scale` (handler `0x5d94fd`, setter `0x5e4e40`): the first two values
//!   times `0.01` (`0x6032d8`) become the pane's x and y factors at pane
//!   `+0x10` / `+0x14`; the third value is parsed and dropped.
//! * `rotate` (handler `0x5d96b1`, setter `0x5e4e20`): each value times
//!   `1/360` (`0x60b894`) times `6.28` (`0x60b890`) is a radian angle at pane
//!   `+0x30` (x), `+0x34` (y) and `+0x38` (z): the unit is degrees.
//! * `origin` (handler `0x5d9741`, setter `0x5e4e60`): three integers as
//!   floats at pane `+0x18` / `+0x1c` / `+0x20`.
//! * the draw (`0x5e4710`): each of an element's four corners, in the pane's
//!   own pixels with the picture's top left at 0,0, becomes
//!   `(corner - origin) * scale`, then is rotated, then (perspective with the
//!   constant `277.0` at `0x60ee6c`; a no-op while z stays 0) added to the
//!   position the `render` statement gave. Origin is therefore both the
//!   pivot and the point of the picture that lands on that position.
//! * the z rotation (builder `0x5eaa20`, transform `0x5eaa40`) is
//!   `x' = x cos + y sin`, `y' = -x sin + y cos`: with y pointing down a
//!   positive angle turns the picture counter-clockwise on screen.
//!
//! Not decoded, so not modelled: the composition order of x and y rotations
//! (only z is applied; a nonzero x or y gives `None`), and the half-pixel
//! bias the draw subtracts from the position when `0x5ce440(2, 0, 0, 0)`
//! returns 0 ([`HALF_PIXEL_BIAS`], not applied here).

/// `scale` values are percent: the engine's float at `0x6032d8`.
pub const SCALE_UNIT: f32 = 0.01;
/// `rotate` values are degrees, converted as `v * (1/360)` (`0x60b894`) times
/// `6.28` (`0x60b890`), not as `v * pi / 180`.
pub const DEGREE_TO_TURN: f32 = 1.0 / 360.0;
/// The engine's "two pi" for `rotate`.
/// (The float `6.28`, which is not tau.)
pub const TURN_RADIANS: f32 = f32::from_bits(0x40c8_f5c3);
/// The perspective constant added to z (`0x60ee6c`).
pub const PERSPECTIVE_DISTANCE: f32 = 277.0;
/// What the draw subtracts from the render position on one path, whose
/// condition (`0x5ce440(2, 0, 0, 0)` returns 0) is not decoded.
pub const HALF_PIXEL_BIAS: f32 = 0.5;

/// A pane's `scale`, `rotate` and `origin` as the script wrote them.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct PaneTransform {
    /// `scale(pane) = x, y, z`, percent (the z is dropped by the engine).
    pub scale: (i32, i32, i32),
    /// `rotate(pane) = x, y, z`, degrees.
    pub rotate: (i32, i32, i32),
    /// `origin(pane) = x, y, z`, pane pixels.
    pub origin: (i32, i32, i32),
}

impl PaneTransform {
    /// A new pane: scale 1 (the engine stores 1.0, not 100), no rotation,
    /// origin 0 (`0x5e41e0`).
    pub const NEUTRAL: Self = Self {
        scale: (100, 100, 100),
        rotate: (0, 0, 0),
        origin: (0, 0, 0),
    };

    /// The x and y scale factors as the engine stores them.
    pub fn scale_factors(&self) -> (f32, f32) {
        (
            (f64::from(self.scale.0) * f64::from(SCALE_UNIT)) as f32,
            (f64::from(self.scale.1) * f64::from(SCALE_UNIT)) as f32,
        )
    }

    /// The z rotation in radians as the engine stores it.
    pub fn rotate_z_radians(&self) -> f32 {
        radians(self.rotate.2)
    }

    /// The four corners of a `width` x `height` picture (top left, top
    /// right, bottom left, bottom right; its own pixels from 0,0) as screen
    /// positions when drawn with `render pane, x, y`. `None` while an x or y
    /// rotation is set: their composition is not decoded.
    pub fn corners(&self, size: (u32, u32), render: (f32, f32)) -> Option<[(f32, f32); 4]> {
        if self.rotate.0 != 0 || self.rotate.1 != 0 {
            return None;
        }
        let (sx, sy) = self.scale_factors();
        let angle = self.rotate_z_radians();
        let (sin, cos) = (f64::from(angle).sin() as f32, f64::from(angle).cos() as f32);
        let (w, h) = (size.0 as f32, size.1 as f32);
        let w_persp = 1.0 / PERSPECTIVE_DISTANCE;
        let place = |x: f32, y: f32| {
            let x = (x - self.origin.0 as f32) * sx;
            let y = (y - self.origin.1 as f32) * sy;
            let (x, y) = if angle == 0.0 {
                (x, y)
            } else {
                (x * cos + y * sin, -x * sin + y * cos)
            };
            (
                render.0 + x * w_persp * PERSPECTIVE_DISTANCE,
                render.1 + y * w_persp * PERSPECTIVE_DISTANCE,
            )
        };
        Some([place(0.0, 0.0), place(w, 0.0), place(0.0, h), place(w, h)])
    }

    /// The axis-aligned box around [`corners`](Self::corners): left, top,
    /// right, bottom.
    pub fn bounds(&self, size: (u32, u32), render: (f32, f32)) -> Option<(f32, f32, f32, f32)> {
        let corners = self.corners(size, render)?;
        let fold = |pick: fn(&(f32, f32)) -> f32, f: fn(f32, f32) -> f32| {
            corners.iter().map(pick).fold(pick(&corners[0]), f)
        };
        Some((
            fold(|c| c.0, f32::min),
            fold(|c| c.1, f32::min),
            fold(|c| c.0, f32::max),
            fold(|c| c.1, f32::max),
        ))
    }
}

/// `v` degrees as the `rotate` handler computes them.
pub fn radians(degrees: i32) -> f32 {
    (f64::from(degrees) * f64::from(DEGREE_TO_TURN) * f64::from(TURN_RADIANS)) as f32
}

/// The cabin picture frame's `PC_MEMENTO` pane: `GK = HK = 20`, `IK = 1`,
/// `JK = 3` (`PASSENGERCABIN.SCRIPT`), rows' origin untouched.
pub const CABIN_MEMENTO: PaneTransform = PaneTransform {
    scale: (20, 20, 1),
    rotate: (0, 0, 3),
    origin: (0, 0, 0),
};
/// Where its layout row puts the pane (`PC_MEMENTO =P,,169,325,...`).
pub const CABIN_MEMENTO_POSITION: (i32, i32) = (169, 325);

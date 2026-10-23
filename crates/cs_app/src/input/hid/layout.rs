//! Reading a joystick's input reports into [`DeviceEvent::JoystickFrame`] terms
//! (SIDEWINDER-FFB2, #1968).
//!
//! Every control is found by its HID usage in the device's own report descriptor,
//! never by an assumed offset:
//!
//! | Index | Control | Usages looked for, in order |
//! | --- | --- | --- |
//! | [`AXIS_X`] | stick left/right | Generic Desktop X |
//! | [`AXIS_Y`] | stick forward/back | Generic Desktop Y |
//! | [`AXIS_TWIST`] | twist (rudder) | Rz, else Rx |
//! | [`AXIS_THROTTLE`] | throttle lever | Slider, else Z, else Dial |
//!
//! Axes are reported in `[-1, 1]` in the device's own sign: a signed field maps its
//! zero to exactly `0.0` and each end to `±1.0`, an unsigned one maps its range
//! linearly. Which end of the throttle lever is "full" is a binding's business
//! ([`cs_types::input::BindingSource::JoystickAxis`]'s `inverted` flag), not the
//! decoder's.
//!
//! Buttons keep their HID number: button `n` is index `n - 1`, the numbering the
//! original control table's joystick slot uses
//! ([`cs_types::keymap::ControlsMap::to_action_map`]). The point-of-view hat is
//! reported as four virtual buttons, [`HAT_UP`], [`HAT_RIGHT`], [`HAT_DOWN`] and
//! [`HAT_LEFT`], so a diagonal holds two of them and every hat direction can be
//! bound through the existing `JoystickButton` source.
//!
//! Ported from the owner's drydock86 project (`drydock-joystick/src/input.rs`).

use cs_types::input::DeviceId;

use super::descriptor::{
    Descriptor, Field, PAGE_BUTTON, PAGE_GENERIC_DESKTOP, Report, ReportKind, usage,
};
use crate::input::devices::DeviceEvent;

pub use cs_types::input::{
    JOYSTICK_AXIS_THROTTLE as AXIS_THROTTLE, JOYSTICK_AXIS_TWIST as AXIS_TWIST,
    JOYSTICK_AXIS_X as AXIS_X, JOYSTICK_AXIS_Y as AXIS_Y, JOYSTICK_HAT_DOWN as HAT_DOWN,
    JOYSTICK_HAT_LEFT as HAT_LEFT, JOYSTICK_HAT_RIGHT as HAT_RIGHT, JOYSTICK_HAT_UP as HAT_UP,
};

/// The highest HID button number the decoder reads.
const MAX_BUTTONS: u32 = 32;

/// A hat direction: one of eight, clockwise from up.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum HatDirection {
    /// Pushed up.
    Up,
    /// Pushed up and right.
    UpRight,
    /// Pushed right.
    Right,
    /// Pushed down and right.
    DownRight,
    /// Pushed down.
    Down,
    /// Pushed down and left.
    DownLeft,
    /// Pushed left.
    Left,
    /// Pushed up and left.
    UpLeft,
}

impl HatDirection {
    /// The eight directions, clockwise from up.
    pub const ALL: [Self; 8] = [
        Self::Up,
        Self::UpRight,
        Self::Right,
        Self::DownRight,
        Self::Down,
        Self::DownLeft,
        Self::Left,
        Self::UpLeft,
    ];

    /// The virtual buttons this direction holds (two for a diagonal).
    #[must_use]
    pub const fn buttons(self) -> &'static [u16] {
        match self {
            Self::Up => &[HAT_UP],
            Self::UpRight => &[HAT_UP, HAT_RIGHT],
            Self::Right => &[HAT_RIGHT],
            Self::DownRight => &[HAT_DOWN, HAT_RIGHT],
            Self::Down => &[HAT_DOWN],
            Self::DownLeft => &[HAT_DOWN, HAT_LEFT],
            Self::Left => &[HAT_LEFT],
            Self::UpLeft => &[HAT_UP, HAT_LEFT],
        }
    }

    /// A short diagnostic spelling.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Up => "up",
            Self::UpRight => "up-right",
            Self::Right => "right",
            Self::DownRight => "down-right",
            Self::Down => "down",
            Self::DownLeft => "down-left",
            Self::Left => "left",
            Self::UpLeft => "up-left",
        }
    }
}

/// The controls of one joystick as last reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct JoystickState {
    /// Each axis in `[-1, 1]`, by axis index; `None` until a report carried it (a
    /// stick reports only changes, and the SideWinder stalls requests for its
    /// current state, so a throttle that has not moved is not known yet).
    pub axes: [Option<f32>; 4],
    /// The raw logical value behind each axis, for the probe.
    pub raw_axes: [Option<i32>; 4],
    /// Bit `n` is HID button `n + 1`.
    pub buttons: u32,
    /// The hat, `None` when centred.
    pub hat: Option<HatDirection>,
}

impl JoystickState {
    /// The button indices down, with the hat as its virtual buttons.
    #[must_use]
    pub fn pressed(&self) -> Vec<u16> {
        let mut pressed: Vec<u16> = (0..MAX_BUTTONS as u16)
            .filter(|bit| self.buttons & (1 << bit) != 0)
            .collect();
        if let Some(hat) = self.hat {
            pressed.extend_from_slice(hat.buttons());
        }
        pressed
    }

    /// The state as the session's joystick report.
    #[must_use]
    pub fn to_event(&self, device: DeviceId) -> DeviceEvent {
        DeviceEvent::JoystickFrame {
            device,
            buttons: self.pressed(),
            axes: self
                .axes
                .iter()
                .enumerate()
                .filter_map(|(index, value)| value.map(|v| (index as u16, v)))
                .collect(),
        }
    }
}

/// Where the controls are in a joystick's input reports.
#[derive(Debug, Clone)]
pub struct Layout {
    axes: [Option<(Field, usize)>; 4],
    hat: Option<(Field, usize)>,
    buttons: Vec<(Field, usize, u32)>,
}

const fn gd(id: u32) -> u32 {
    usage(PAGE_GENERIC_DESKTOP, id)
}

/// A logical axis value in `[-1, 1]`: a signed range keeps its zero at `0.0`.
fn normalize(field: &Field, value: i32) -> f32 {
    let (min, max) = (i64::from(field.logical_min), i64::from(field.logical_max));
    let value = i64::from(value).clamp(min.min(max), min.max(max));
    if max <= min {
        return 0.0;
    }
    let unit = if min < 0 && max > 0 {
        if value < 0 {
            value as f64 / -min as f64
        } else {
            value as f64 / max as f64
        }
    } else {
        (value - min) as f64 * 2.0 / (max - min) as f64 - 1.0
    };
    unit.clamp(-1.0, 1.0) as f32
}

impl Layout {
    /// Find the controls in a descriptor.
    #[must_use]
    pub fn new(d: &Descriptor) -> Self {
        let find = |ids: &[u32]| {
            ids.iter()
                .find_map(|&id| d.find(ReportKind::Input, 0, gd(id)))
                .map(|(f, i)| (f.clone(), i))
        };
        let axes = [
            find(&[0x30]),
            find(&[0x31]),
            find(&[0x35, 0x33]),
            find(&[0x36, 0x32, 0x37]),
        ];
        let hat = find(&[0x39]);
        let mut buttons = Vec::new();
        for f in d
            .fields
            .iter()
            .filter(|f| f.kind == ReportKind::Input && !f.is_constant() && !f.is_array())
        {
            for i in 0..f.elements() {
                if let Some(u) = f.usage(i)
                    && u >> 16 == PAGE_BUTTON
                    && (1..=MAX_BUTTONS).contains(&(u & 0xffff))
                {
                    buttons.push((f.clone(), i, (u & 0xffff) - 1));
                }
            }
        }
        Self { axes, hat, buttons }
    }

    /// Which of the four axes the device has, by axis index.
    #[must_use]
    pub fn has_axis(&self, index: u16) -> bool {
        self.axes
            .get(usize::from(index))
            .is_some_and(Option::is_some)
    }

    /// How many axes the device has.
    #[must_use]
    pub fn axis_count(&self) -> usize {
        self.axes.iter().flatten().count()
    }

    /// How many buttons the device has.
    #[must_use]
    pub fn button_count(&self) -> usize {
        self.buttons.len()
    }

    /// Whether the device has a point-of-view hat.
    #[must_use]
    pub const fn has_hat(&self) -> bool {
        self.hat.is_some()
    }

    /// Update `st` from one input report; fields of other reports keep their values.
    pub fn decode(&self, r: &Report, st: &mut JoystickState) {
        let id = r.id();
        for (k, a) in self.axes.iter().enumerate() {
            if let Some((f, i)) = a
                && f.report_id == id
            {
                let raw = r.get(f, *i);
                st.raw_axes[k] = Some(raw);
                st.axes[k] = Some(normalize(f, raw));
            }
        }
        if let Some((f, i)) = &self.hat
            && f.report_id == id
        {
            let v = r.get(f, *i);
            st.hat = if v < f.logical_min || v > f.logical_max {
                None
            } else {
                // The hat's logical range covers the full circle in equal steps.
                let steps = i64::from(f.logical_max) - i64::from(f.logical_min) + 1;
                let index = (i64::from(v) - i64::from(f.logical_min)) * 8 / steps;
                HatDirection::ALL.get(index as usize).copied()
            };
        }
        for (f, i, bit) in &self.buttons {
            if f.report_id == id {
                if r.get_raw(f, *i) != 0 {
                    st.buttons |= 1 << bit;
                } else {
                    st.buttons &= !(1 << bit);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::input::hid::descriptor::sidewinder_ff2;

    #[test]
    fn sidewinder_report() {
        let d = sidewinder_ff2();
        let l = Layout::new(&d);
        assert_eq!(
            (l.axis_count(), l.button_count(), l.has_hat()),
            (4, 8, true)
        );
        // X full left (-512), Y centre (0), twist full right (31), throttle 127,
        // hat 2 (right), buttons 1 and 8.
        let mut bytes = vec![
            1u8,
            0x00,
            0x02,
            0x00,
            0x00,
            31,
            127,
            0x02,
            0b1000_0001,
            0,
            0,
            0,
            0,
        ];
        let mut st = JoystickState::default();
        l.decode(&Report::from_bytes(true, bytes.clone()), &mut st);
        assert_eq!(st.axes, [Some(-1.0), Some(0.0), Some(1.0), Some(1.0)]);
        assert_eq!(st.raw_axes, [Some(-512), Some(0), Some(31), Some(127)]);
        assert_eq!(st.hat, Some(HatDirection::Right));
        assert_eq!(st.pressed(), [0, 7, HAT_RIGHT]);
        // Hat out of range is centred; the PID state report changes nothing.
        bytes[7] = 0x08;
        l.decode(&Report::from_bytes(true, bytes), &mut st);
        assert_eq!(st.hat, None);
        let before = st.clone();
        l.decode(&Report::from_bytes(true, vec![2, 0x03, 0x85]), &mut st);
        assert_eq!(st, before);
    }
}

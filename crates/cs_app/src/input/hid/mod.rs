//! Host joysticks read and driven through raw HID: input for every HID joystick,
//! and USB PID force feedback for the Microsoft SideWinder Force Feedback 2 and
//! other PID devices (SIDEWINDER-FFB2, #1968).
//!
//! Bevy's gamepad input (gilrs) presents a joystick as a gamepad, which has no
//! twist or throttle-lever axes of its own and no force feedback at all, and on
//! macOS the OS offers no force-feedback API for a USB PID stick. This module
//! therefore reads and drives the device itself, through its HID report
//! descriptor.

pub mod descriptor;

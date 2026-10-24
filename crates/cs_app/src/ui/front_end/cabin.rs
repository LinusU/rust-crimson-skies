//! The cabin hub's composition over the `PassengerCabin` section
//! (PARITY5-CABIN-INTRO.04, task #1840).
//!
//! The shell draws the layout's rows as written; `PASSENGERCABIN.SCRIPT`
//! then changes five things when it creates the screen, and this applies the
//! ones the data states (findings: `docs/findings/2026-10-23-parity5-cabin-hub.md`):
//!
//! * `deactivate(PMA)` on `PC_B_SAVE`: the Save button is not shown and not
//!   pressable (the owner's frame has no Save button; `deactivate` hid the
//!   main menu behind the pilot page the same way);
//! * `HMA.AK = "assets\graphics\pc_p_hangar" N ".jpg"` on `PC_PLANE`: the
//!   hangar window shows the pilot's plane, drawn at the row's `X,Y` at the
//!   jpg's own extent (the layout's `PC_PlaneTemp.png` is a stand-in);
//! * one `PC_PIN<R>` pane per pin, `JMA[R].CK = R`: pin `R` of `PC_mappins.png`
//!   (five 20-row frames) is drawn on `PC_PIN<R>`; the other pin panes are
//!   never created;
//! * `focus(@ctl@)`: the script focuses the control host, not a button, so no
//!   button is focused on entry (the owner's frame draws Change Memento in its
//!   resting gold, not the focus glow) and the first `Down` focuses the first;
//! * the memento picture, `callback($$E$$, 2150, 0, -1, …)`: the pilot's
//!   memento, the one the Change Memento screen ([`super::memento`], #1957)
//!   shows and stores, drawn by [`show_memento`] on `PC_MEMENTO` behind
//!   `PC_FRAME` with the pane transform `KX.GK/HK/IK/JK` (20, 20, 1, 3) gives
//!   it ([`super::pane_transform::CABIN_MEMENTO`], #2039): 20 % of the
//!   picture, turned 3 degrees counter-clockwise about its top left at the
//!   row's 169,325. The decoded box lands 11 px left of and 5 px above where
//!   the project's own cabin shot measured the frame's key patch
//!   (`docs/findings/2026-10-24-cabin-memento-pane-transform.md`); the decode
//!   is drawn as it is, not nudged to the patch.
//!
//! What the original's `N` (callback 2010) and pin count (callback 7) are for
//! a pilot is engine state this project has not read. [`CabinHub::NEW_PILOT`]
//! is what the owner's capture of a new pilot's cabin shows (hangar 5: the
//! jpg matches the capture's window with a mean difference of 3 against
//! 14-22 for the other ten; one pin: one pin on the map), not an original
//! table. Nothing here is `verified_original`.

use std::collections::BTreeMap;

use super::memento::picture_art;
use super::pane_transform::{CABIN_MEMENTO, CABIN_MEMENTO_POSITION};
use super::shell::{MenuShell, ShellPane, ShellRect};

/// The layout section this composes.
pub const CABIN_SECTION: &str = "PassengerCabin";
/// The button `deactivate(PMA)` hides.
pub const SAVE_BUTTON: &str = "PC_B_SAVE";
/// The hangar window pane.
pub const PLANE_PANE: &str = "PC_PLANE";
/// The cabin photo frame's pane: the layout row `PC_FRAME` (Z 5).
pub const FRAME_PANE: &str = "PC_FRAME";
/// The pane the script builds the memento picture on (Z 2, behind the frame).
pub const MEMENTO_PANE: &str = "PC_MEMENTO";
/// The map-pin panes, `PC_PIN0` .. `PC_PIN4`.
pub const PIN_PANES: [&str; 5] = ["PC_PIN0", "PC_PIN1", "PC_PIN2", "PC_PIN3", "PC_PIN4"];
/// Hangar jpgs the installation holds: `pc_p_hangar0.jpg` .. `pc_p_hangar10.jpg`.
pub const HANGAR_COUNT: u32 = 11;

/// What the cabin shows of a pilot.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CabinHub {
    /// The `N` of `pc_p_hangar<N>.jpg`.
    pub hangar: u32,
    /// The number of map pins.
    pub pins: u32,
}

/// What [`CabinHub::apply`] changed on the section.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CabinApplied {
    /// The Save button was removed.
    pub save_hidden: bool,
    /// The art `PC_PLANE` now draws.
    pub plane_art: String,
    /// The pin panes kept, by id.
    pub pins: Vec<String>,
}

/// Why the hub could not be applied.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum CabinError {
    /// The layout has no `PassengerCabin` section.
    NoSection,
    /// The section has no `PC_PLANE` pane.
    NoPlanePane,
    /// The hangar index names no art the installation holds.
    NoHangar {
        /// The art name tried.
        art: String,
    },
}

impl std::fmt::Display for CabinError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NoSection => write!(f, "the layout has no {CABIN_SECTION} section"),
            Self::NoPlanePane => write!(f, "{CABIN_SECTION} has no {PLANE_PANE} pane"),
            Self::NoHangar { art } => write!(f, "the installation has no readable {art}"),
        }
    }
}

impl std::error::Error for CabinError {}

impl CabinHub {
    /// A new pilot's cabin as the owner's capture shows it (see the module
    /// docs): hangar 5 and one pin.
    pub const NEW_PILOT: Self = Self { hangar: 5, pins: 1 };

    /// The hangar art's name.
    #[must_use]
    pub fn plane_art(&self) -> String {
        format!("pc_p_hangar{}.jpg", self.hangar)
    }

    /// Applies the hub to the shell's `PassengerCabin` section, measuring the
    /// hangar art through `extent`.
    ///
    /// # Errors
    ///
    /// [`CabinError`] when the section, the plane pane or the hangar art is
    /// missing; the section is left as the layout wrote it.
    pub fn apply(
        &self,
        shell: &mut MenuShell,
        extent: &dyn Fn(&str) -> Option<(u32, u32)>,
    ) -> Result<CabinApplied, CabinError> {
        let section = shell
            .section_mut(CABIN_SECTION)
            .ok_or(CabinError::NoSection)?;
        let plane = section
            .panes
            .iter()
            .position(|pane| pane.id.eq_ignore_ascii_case(PLANE_PANE))
            .ok_or(CabinError::NoPlanePane)?;
        let art = self.plane_art();
        let (width, height) = extent(&art)
            .filter(|(w, h)| *w > 0 && *h > 0)
            .ok_or_else(|| CabinError::NoHangar { art: art.clone() })?;
        let pane = &mut section.panes[plane];
        pane.art.clone_from(&art);
        pane.rect = ShellRect {
            width,
            height,
            ..pane.rect
        };
        pane.frames = 1;
        pane.frame = 0;

        let pins = self.pins;
        let mut kept = Vec::new();
        section.panes.retain_mut(|pane| {
            let Some(pin) = PIN_PANES
                .iter()
                .position(|id| pane.id.eq_ignore_ascii_case(id))
            else {
                return true;
            };
            let shown =
                u32::try_from(pin).is_ok_and(|pin| pin < pins) && pin < pane.frames as usize;
            if shown {
                pane.frame = pin as u32;
                kept.push(pane.id.clone());
            }
            shown
        });

        section.focus_on_entry = false;
        let before = section.buttons.len();
        section
            .buttons
            .retain(|button| !button.id.eq_ignore_ascii_case(SAVE_BUTTON));
        Ok(CabinApplied {
            save_hidden: section.buttons.len() != before,
            plane_art: art,
            pins: kept,
        })
    }
}

/// Draws the pilot's memento `name` on the cabin's `PC_MEMENTO` pane: the
/// picture at `CABIN_MEMENTO`'s scale and turn, its top left at the row's
/// position, in the pane list just before `PC_FRAME` (Z 2 under Z 5; the
/// layout row names no art, so the shell skipped it). `extents` holds the
/// pictures' sizes by lower-case layout art name. Returns whether the pane
/// changed; `false` too when the section, the frame or the picture's extent
/// is missing.
pub fn show_memento(
    shell: &mut MenuShell,
    name: &str,
    extents: &BTreeMap<String, (u32, u32)>,
) -> bool {
    let art = picture_art(name);
    let Some(&(width, height)) = extents.get(&art.to_ascii_lowercase()) else {
        return false;
    };
    let Some(section) = shell.section_mut(CABIN_SECTION) else {
        return false;
    };
    if let Some(pane) = section
        .panes
        .iter_mut()
        .find(|pane| pane.id.eq_ignore_ascii_case(MEMENTO_PANE))
    {
        if pane.art == art {
            return false;
        }
        pane.art = art;
        pane.rect.width = width;
        pane.rect.height = height;
        return true;
    }
    let Some(frame) = section
        .panes
        .iter()
        .position(|pane| pane.id.eq_ignore_ascii_case(FRAME_PANE))
    else {
        return false;
    };
    section.panes.insert(
        frame,
        ShellPane {
            id: MEMENTO_PANE.to_owned(),
            art,
            rect: ShellRect {
                x: CABIN_MEMENTO_POSITION.0,
                y: CABIN_MEMENTO_POSITION.1,
                width,
                height,
            },
            frames: 1,
            frame: 0,
            is_region: false,
            color_keyed: false,
            transform: Some(CABIN_MEMENTO),
        },
    );
    true
}

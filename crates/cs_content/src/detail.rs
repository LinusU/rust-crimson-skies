//! The video-options detail settings: the surface that owns the original's
//! `TextureMemory_HW` and `TextureMemory_SW` members and the texture dropdown
//! of the video-options panel (task #688, `F08-C-renderer-texture-settings`).
//!
//! Spec: `specs/F08-texture-archives-and-conventional-image-decoding.md`. The
//! measured rule this
//! surface feeds is in
//! `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`.
//!
//! # Measured anchors
//!
//! The original stores its detail settings in the `detail.zrd` member of
//! `ZBD/zrdr.zbd`; that file is **not bound here** — reading it is a measured
//! follow-up task, so everything this module authors is
//! [`DETAIL_SURFACE_STATUS`] (`Designed`), never a claim about bytes. What *is*
//! measured, and what this surface reproduces, is the behaviour around the two
//! members:
//!
//! * `detail.zrd` defaults: `TextureMemory_HW` is `TEXMEM_MAX` on every
//!   machine, and `TextureMemory_SW` is picked from the machine's RAM —
//!   [`TextureMemory::software_default_for_ram_kb`].
//! * The video panel's texture dropdown (`VIDEO.SCRIPT` `vp_d_texture`) holds
//!   the three [`TextureDetailRow`]s. Loading it maps the member to a row
//!   (`0x418f1a`, [`TextureDetailRow::for_setting`]); applying it writes the
//!   row's setting (`0x419392`, [`TextureDetailRow::setting`]) to
//!   `TextureMemory_HW` when the hardware device is selected and to
//!   `TextureMemory_SW` otherwise (`0x64f6a8`/`0x64f704`/`0x64f708`).
//! * The settings' **only** consumer is the world texture-load descriptor
//!   (`0x530fe0`, [`WorldTextureLoad`]): software rendering reads
//!   `TextureMemory_SW`, hardware without a DirectDraw object reads
//!   `TextureMemory_HW`, and hardware with a device total ignores both.
//!
//! The settings are members, not rows: a value outside the dropdown's three
//! rows — reachable only from a hand-edited profile — is held as written and
//! shown by the measured [`TextureDetailRow::for_setting`] mapping rather than
//! clamped.

use cs_types::content::Provenance;
use cs_types::evidence::{ClaimId, ClaimStatus};

use crate::textures::{RendererMode, TextureDetailRow, TextureMemory, WorldTextureLoad};

/// The claim the authored surface is filed under.
pub const DETAIL_SURFACE_CLAIM: &str = "f08-c.detail-settings";

/// How well this surface's values are known.
///
/// [`ClaimStatus::Designed`], honestly: `detail.zrd` is measured to exist and
/// its defaults are known, but no task has bound the member yet, so every
/// default and every stored value here is the project's own — the same class
/// `PRECEDENCE_ORDER_STATUS` carries for an unmeasured rule.
pub const DETAIL_SURFACE_STATUS: ClaimStatus = ClaimStatus::Designed;

/// Which device the video panel's device switcher names.
///
/// The measured apply dispatches on it: the picked row's setting is written to
/// `TextureMemory_HW` for [`Self::Hardware`] and to `TextureMemory_SW` for
/// [`Self::Software`] (`0x64f6a8`). This is the panel's own choice, not a
/// renderer mode — the renderer that draws is `RendererDetail`'s, in
/// `cs_app::render::detail`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DetailDevice {
    /// The hardware device: reads and writes `TextureMemory_HW`.
    Hardware,
    /// Anything else: reads and writes `TextureMemory_SW`.
    Software,
}

impl DetailDevice {
    /// Both devices the switcher can name.
    pub const ALL: [Self; 2] = [Self::Hardware, Self::Software];
}

/// The original's texture detail settings.
///
/// Two stored members plus the measured panel behaviour on top of them. The
/// members are `pub`-equivalent through the accessors — they are not public
/// fields because every write the panel makes is the measured
/// `index → setting` conversion, and a member written directly is only for the
/// file reader that binds `detail.zrd`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct DetailSettings {
    texture_memory_hw: TextureMemory,
    texture_memory_sw: TextureMemory,
}

impl DetailSettings {
    /// The surface as this project ships it: both members `TEXMEM_MAX`.
    ///
    /// That is `detail.zrd`'s hardware default always, and its software default
    /// for a machine with at least 256 000 KB of RAM — the designed assumption
    /// until a memory query exists (see the finding's design notes).
    pub const fn designed() -> Self {
        Self {
            texture_memory_hw: TextureMemory::MAX,
            texture_memory_sw: TextureMemory::MAX,
        }
    }

    /// `detail.zrd`'s own defaults for a machine with `ram_kb` of RAM:
    /// `TextureMemory_HW` is always `TEXMEM_MAX`; `TextureMemory_SW` follows
    /// the measured RAM bands.
    ///
    /// **Designed** here only in that nothing measures this host's RAM — the
    /// band table itself is measured.
    pub const fn designed_for_ram(ram_kb: u64) -> Self {
        Self {
            texture_memory_hw: TextureMemory::MAX,
            texture_memory_sw: TextureMemory::software_default_for_ram_kb(ram_kb),
        }
    }

    /// The `TextureMemory_HW` member, exactly as stored.
    pub const fn texture_memory_hardware(&self) -> TextureMemory {
        self.texture_memory_hw
    }

    /// The `TextureMemory_SW` member, exactly as stored.
    pub const fn texture_memory_software(&self) -> TextureMemory {
        self.texture_memory_sw
    }

    /// The member `device` holds.
    pub const fn texture_memory(&self, device: DetailDevice) -> TextureMemory {
        match device {
            DetailDevice::Hardware => self.texture_memory_hw,
            DetailDevice::Software => self.texture_memory_sw,
        }
    }

    /// Writes `device`'s member, for the file reader that binds `detail.zrd`.
    /// The panel never calls it — it goes through [`Self::select_texture_row`].
    pub fn set_texture_memory(&mut self, device: DetailDevice, setting: TextureMemory) {
        match device {
            DetailDevice::Hardware => self.texture_memory_hw = setting,
            DetailDevice::Software => self.texture_memory_sw = setting,
        }
    }

    /// The row the dropdown shows for `device`'s member when the panel loads
    /// (`0x418f1a`): the measured [`TextureDetailRow::for_setting`] mapping.
    pub const fn texture_row(&self, device: DetailDevice) -> TextureDetailRow {
        TextureDetailRow::for_setting(self.texture_memory(device))
    }

    /// The index the panel stores for `device`'s member
    /// ([`TextureDetailRow::index`]).
    pub const fn texture_index(&self, device: DetailDevice) -> u32 {
        self.texture_row(device).index()
    }

    /// The panel's apply (`0x419392`, dispatched by `0x64f6a8`): the picked
    /// row's measured [`TextureDetailRow::setting`] is written to `device`'s
    /// member.
    pub fn select_texture_row(&mut self, device: DetailDevice, row: TextureDetailRow) {
        self.set_texture_memory(device, row.setting());
    }

    /// The descriptor a world load registers with these settings under
    /// `renderer` (`0x530fe0`): the renderer decides which member — if any —
    /// the texture budget reads.
    pub const fn world_load(&self, renderer: RendererMode) -> WorldTextureLoad {
        WorldTextureLoad {
            renderer,
            hardware_memory: self.texture_memory_hw,
            software_memory: self.texture_memory_sw,
        }
    }

    /// The provenance of every value this surface produced: [`DETAIL_SURFACE_STATUS`],
    /// no source span — a designed surface locates no bytes.
    pub fn provenance() -> Provenance {
        Provenance::new(
            ClaimId::new(DETAIL_SURFACE_CLAIM).expect("the claim id is valid"),
            DETAIL_SURFACE_STATUS,
            None,
        )
        .expect("a designed provenance needs no source span")
    }
}

impl Default for DetailSettings {
    fn default() -> Self {
        Self::designed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::textures::{PROJECT_HARDWARE_TEXTURE_MIB, TextureDetailRow, texture_budget};

    /// **The dropdown round-trips through the surface.** Loading the panel maps
    /// each member to the measured row; picking a row writes its measured
    /// setting to the member the device switcher names — and leaves the other
    /// member alone.
    #[test]
    fn accept_f08_c_renderer_the_detail_settings_round_trip_the_dropdown_rows() {
        let mut settings = DetailSettings::designed();
        for device in DetailDevice::ALL {
            for row in TextureDetailRow::ALL {
                settings.select_texture_row(device, row);
                assert_eq!(
                    settings.texture_memory(device),
                    row.setting(),
                    "{device:?}: the member takes the row's measured setting"
                );
                assert_eq!(
                    settings.texture_row(device),
                    row,
                    "{device:?}: the shown row is the measured setting→row mapping"
                );
                assert_eq!(
                    settings.texture_index(device),
                    row.index(),
                    "{device:?}: the stored index is the row's own"
                );
            }
        }
    }

    /// The dispatcher keeps the two members apart: applying a row for the
    /// hardware device never touches `TextureMemory_SW`, and vice versa — the
    /// measured `0x64f6a8` split.
    #[test]
    fn accept_f08_c_renderer_the_dropdown_writes_only_the_selected_devices_member() {
        let mut settings = DetailSettings::designed();
        settings.select_texture_row(DetailDevice::Hardware, TextureDetailRow::Low);
        assert_eq!(settings.texture_memory_hardware(), TextureMemory::SIX_MB);
        assert_eq!(settings.texture_memory_software(), TextureMemory::MAX);
        settings.select_texture_row(DetailDevice::Software, TextureDetailRow::Middle);
        assert_eq!(settings.texture_memory_hardware(), TextureMemory::SIX_MB);
        assert_eq!(settings.texture_memory_software(), TextureMemory::EIGHT_MB);
    }

    /// `detail.zrd`'s defaults: hardware is `TEXMEM_MAX` on every machine;
    /// software follows the measured RAM bands.
    #[test]
    fn accept_f08_c_renderer_the_detail_defaults_are_the_measured_ones() {
        assert_eq!(
            DetailSettings::designed().texture_memory_hardware(),
            TextureMemory::MAX
        );
        assert_eq!(
            DetailSettings::designed().texture_memory_software(),
            TextureMemory::MAX
        );
        for (ram_kb, expected) in [
            (300_000, TextureMemory::MAX),
            (200_000, TextureMemory::EIGHT_MB),
            (100_000, TextureMemory::FOUR_MB),
            (32_000, TextureMemory::TWO_MB),
        ] {
            assert_eq!(
                DetailSettings::designed_for_ram(ram_kb).texture_memory_software(),
                expected,
                "{ram_kb} KB of RAM"
            );
        }
    }

    /// The world-load descriptor carries both members; `texture_budget` reads
    /// the one the renderer mode says — `TextureMemory_SW` for software,
    /// `TextureMemory_HW` for hardware with no device total — and a device
    /// total outranks both.
    #[test]
    fn accept_f08_c_renderer_world_load_registers_the_member_the_renderer_reads() {
        let mut settings = DetailSettings::designed();
        settings.select_texture_row(DetailDevice::Software, TextureDetailRow::Low);
        settings.select_texture_row(DetailDevice::Hardware, TextureDetailRow::Middle);

        let software = settings.world_load(RendererMode::Software);
        assert_eq!(
            texture_budget(&software).mib,
            6,
            "software reads TextureMemory_SW"
        );
        assert!(!texture_budget(&software).reduced_prefix_first);

        let no_device = settings.world_load(RendererMode::Hardware {
            total_texture_mib: None,
        });
        assert_eq!(
            texture_budget(&no_device).mib,
            8,
            "no DirectDraw reads TextureMemory_HW"
        );
        assert!(!texture_budget(&no_device).reduced_prefix_first);

        let device = settings.world_load(RendererMode::Hardware {
            total_texture_mib: Some(PROJECT_HARDWARE_TEXTURE_MIB),
        });
        assert_eq!(
            texture_budget(&device).mib,
            16,
            "the device total outranks the setting"
        );
        assert!(texture_budget(&device).reduced_prefix_first);
    }

    /// The surface's evidence class is the honest one: designed, with no source
    /// span, until `detail.zrd` is bound by a measured task.
    #[test]
    fn accept_f08_c_renderer_the_surface_is_filed_as_designed() {
        let provenance = DetailSettings::provenance();
        assert_eq!(provenance.class, ClaimStatus::Designed);
        assert!(provenance.source.is_none());
        assert_eq!(provenance.claim_id.as_str(), DETAIL_SURFACE_CLAIM);
    }
}

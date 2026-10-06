//! The renderer side of the texture-archive rule (task #688,
//! `F08-C-renderer-texture-settings`): which renderer the run drives, what the
//! display device's texture memory is, and the [`WorldTextureLoad`] a world
//! load registers from the two plus the detail settings.
//!
//! Spec: `specs/F08-texture-archives-and-conventional-image-decoding.md`; the
//! measured rule is in
//! `docs/findings/2026-10-05-t352-texture-archive-selection-rule.md`. The
//! settings this reads are [`cs_content::detail::DetailSettings`]; the measured
//! half is [`cs_content::textures`]' `texture_budget`.
//!
//! # Designed, not measured
//!
//! The original asks DirectDraw for the device's *total* texture memory
//! (`IDirectDraw::GetAvailableVidMem(DDSCAPS_TEXTURE)`, `0x5a0ae0`). This
//! project's renderer has no device query — there is no DirectDraw and no
//! equivalent yet — so [`RendererDetail::project`] reports
//! [`PROJECT_HARDWARE_TEXTURE_MIB`] as the total, the designed stand-in the
//! finding records. That choice is honest about what it buys: every world
//! selects its top `rtexture*` tier, which is what the original does on a card
//! with at least that much texture memory.

use cs_content::detail::DetailSettings;
use cs_content::textures::{
    PROJECT_HARDWARE_TEXTURE_MIB, RendererMode, TextureBudget, WorldTextureLoad, texture_budget,
};

/// What the renderer is and what its device reports.
///
/// This is the renderer's own fact, kept apart from the settings: the panel's
/// device switcher ([`cs_content::detail::DetailDevice`]) decides which
/// *member* a pick writes, while this mode decides which member — if any — the
/// texture budget reads. The two are not the same axis: a hardware device total
/// outranks the setting whichever member the panel last wrote.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum RendererDetail {
    /// The software renderer. The texture budget always reads
    /// `TextureMemory_SW`, so the dropdown always moves the archive.
    Software,
    /// The hardware renderer. `device_texture_mib` is what a device query
    /// would report as *total* texture memory — the `0x5a0ae0` return — and
    /// `None` when no device answered at all, which is when the budget reads
    /// `TextureMemory_HW` instead.
    Hardware {
        /// The device's total texture memory in MiB, or `None`.
        device_texture_mib: Option<u32>,
    },
}

impl RendererDetail {
    /// This project's renderer: hardware, reporting the designed
    /// [`PROJECT_HARDWARE_TEXTURE_MIB`] total until a device query exists.
    ///
    /// Reporting it through `mode` keeps the constant exactly where the
    /// measured rule consumes it — a DirectDraw total — so the day a real query
    /// exists, only what feeds this field changes.
    pub const fn project() -> Self {
        Self::Hardware {
            device_texture_mib: Some(PROJECT_HARDWARE_TEXTURE_MIB),
        }
    }

    /// The hardware renderer over a device that reported `total_texture_mib`,
    /// or over no device at all (`None` — the original's missing-DirectDraw
    /// case, which reads the `TextureMemory_HW` setting).
    pub const fn hardware(device_texture_mib: Option<u32>) -> Self {
        Self::Hardware { device_texture_mib }
    }

    /// The software renderer.
    pub const fn software() -> Self {
        Self::Software
    }

    /// The measured [`RendererMode`] this detail maps to — the field
    /// `texture_budget` dispatches on.
    pub const fn mode(&self) -> RendererMode {
        match self {
            Self::Software => RendererMode::Software,
            Self::Hardware { device_texture_mib } => RendererMode::Hardware {
                total_texture_mib: *device_texture_mib,
            },
        }
    }

    /// The texture-memory total the device reported, when one did.
    /// [`Self::project`] reports [`PROJECT_HARDWARE_TEXTURE_MIB`].
    pub const fn device_texture_mib(&self) -> Option<u32> {
        match self {
            Self::Software => None,
            Self::Hardware { device_texture_mib } => *device_texture_mib,
        }
    }

    /// The descriptor one world load registers under this renderer and
    /// `settings` — both detail members plus this renderer's mode, the three
    /// inputs `0x530fe0` reads.
    pub const fn world_load(&self, settings: &DetailSettings) -> WorldTextureLoad {
        settings.world_load(self.mode())
    }

    /// The budget [`texture_budget`] computes for that descriptor: which
    /// archive tier a world load under these settings and this renderer walks
    /// down from.
    pub fn budget(&self, settings: &DetailSettings) -> TextureBudget {
        texture_budget(&self.world_load(settings))
    }
}

#[cfg(test)]
mod tests {
    use cs_content::detail::DetailDevice;
    use cs_content::textures::{TextureDetailRow, TextureMemory};

    use super::*;

    /// The project's renderer reports the designed 16 MiB total, and the load
    /// it registers reads the device total — not the dropdown's member.
    #[test]
    fn accept_f08_c_renderer_the_project_renderer_reports_the_designed_total() {
        let renderer = RendererDetail::project();
        assert_eq!(
            renderer.device_texture_mib(),
            Some(PROJECT_HARDWARE_TEXTURE_MIB)
        );
        let settings = DetailSettings::designed();
        let budget = renderer.budget(&settings);
        assert_eq!(budget.mib, PROJECT_HARDWARE_TEXTURE_MIB);
        assert!(
            budget.reduced_prefix_first,
            "a device total sets the r-flag"
        );
    }

    /// The renderer mode, not the panel's device switcher, decides which member
    /// the budget reads: the software renderer reads `TextureMemory_SW` even
    /// when the panel last wrote `TextureMemory_HW`, and a hardware device
    /// total reads neither.
    #[test]
    fn accept_f08_c_renderer_the_mode_picks_the_member_not_the_panels_device() {
        let mut settings = DetailSettings::designed();
        settings.select_texture_row(DetailDevice::Software, TextureDetailRow::Low);
        assert_eq!(settings.texture_memory_software(), TextureMemory::SIX_MB);
        assert_eq!(settings.texture_memory_hardware(), TextureMemory::MAX);

        // The software renderer reads TextureMemory_SW: the row the software
        // member holds is the budget it gets.
        assert_eq!(RendererDetail::software().budget(&settings).mib, 6);
        // A hardware renderer with no device total reads TextureMemory_HW —
        // untouched by the software pick.
        assert_eq!(RendererDetail::hardware(None).budget(&settings).mib, 0);
        // A hardware device total outranks both members.
        assert_eq!(RendererDetail::hardware(Some(4)).budget(&settings).mib, 4);
    }
}

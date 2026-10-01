//! Reduced motion (F52-A, AC03).
//!
//! Non-negotiable behavior 4: reduced motion disables cosmetic shake and flash
//! without suppressing a required damage or target notification.
//! [`filter_effects`] is the one place the two settings are read; it takes the
//! presentation only, so it cannot see a gameplay input, and it returns every
//! [`Effect::DamageNotice`] and [`Effect::TargetNotice`] unchanged and in order.

use cs_content::settings::Presentation;

/// A presentation effect a frame asks for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Effect {
    /// Cosmetic camera shake of this amplitude.
    CameraShake(f32),
    /// Cosmetic full-screen flash of this intensity.
    ScreenFlash(f32),
    /// A required damage notification.
    DamageNotice,
    /// A required target notification.
    TargetNotice,
}

impl Effect {
    /// Whether the player needs this effect to play correctly.
    #[must_use]
    pub fn is_required(self) -> bool {
        matches!(self, Self::DamageNotice | Self::TargetNotice)
    }
}

/// The effects to present under `presentation`.
#[must_use]
pub fn filter_effects(presentation: &Presentation, effects: &[Effect]) -> Vec<Effect> {
    effects
        .iter()
        .copied()
        .filter(|effect| match effect {
            Effect::CameraShake(_) => !presentation.reduce_shake,
            Effect::ScreenFlash(_) => !presentation.reduce_flash,
            Effect::DamageNotice | Effect::TargetNotice => true,
        })
        .collect()
}

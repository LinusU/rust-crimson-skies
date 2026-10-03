//! The one place a pinned render configuration becomes renderer settings
//! (F59-B).
//!
//! Spec: `specs/F59-replays-captures-probes-and-acceptance-evidence.md`,
//! stage `### F59-B`.
//!
//! `cs_content::replay::RenderConfig` is the documented, exact form a capture
//! record carries: `u32` thousandths for exposure and gamma, so the document
//! text is an integer and a round trip cannot lose a decimal place.
//! `cs_app::render::capture::ComparisonSettings` is what the renderer is
//! actually configured with: `f32` exposure and gamma, plus a tone curve, a
//! sample count, shadows and an internal render resolution. Two vocabularies,
//! one subject, and this module is the only code that crosses between them —
//! which is what keeps them from drifting into a state where a record's pinned
//! settings describe a frame that was rendered differently.
//!
//! Three properties are load-bearing:
//!
//! * **The comparison baseline is the same value on both sides.**
//!   [`RenderConfig::comparison`] stores `1000`/`2200`/`1` thousandths and no
//!   tone curve; [`ComparisonSettings::comparison`] is `1.0`/`2.2`/`1` with no
//!   tone curve. [`settings_for`] on one and [`render_for`] on the other return
//!   the other verbatim, so the fixed set F17 pins and the fixed set F59 pins
//!   are one set.
//! * **The tone-curve vocabularies are the same words.**
//!   [`replay::TonemapKind`] and [`Tonemap`] both spell the curves
//!   `none`/`filmic`, and [`tonemap_for`]/[`tonemap_label`] are the only two
//!   places that translate, so a record cannot name a curve the renderer has
//!   no name for.
//! * **A setting one side cannot express is refused, not dropped.**
//!   [`ComparisonSettings`] carries an internal render resolution that
//!   [`RenderConfig`] has no field for, and it carries the *fixed comparison*
//!   exposure and gamma only, while [`RenderConfig`] stores a designed range
//!   wider than that. A capture taken under a resolution override, or pinning an
//!   exposure or gamma the renderer cannot be configured with, is not the frame
//!   the record would describe: [`render_for`] refuses the first with
//!   [`CaptureError::RenderSettingUnpinned`] and [`settings_for`] the second
//!   with [`CaptureError::RenderSettingUnsupported`], rather than writing a
//!   record that claims a frame this configuration does not describe.

use cs_content::replay::{
    COMPARISON_EXPOSURE_MILLI, COMPARISON_GAMMA_MILLI, CaptureError, RenderConfig, TonemapKind,
};

use crate::render::capture::{ComparisonSettings, Tonemap};

/// The exposure and gamma a pinned configuration stores as thousandths.
const MILLI: f32 = 1000.0;

/// The renderer's tone curve for a record's tone curve.
///
/// The two enums carry the same two curves under the same two labels, and this
/// is the only place that says so; a third curve added to one side is a
/// compile error here rather than a record that names a curve nothing renders.
#[must_use]
pub const fn tonemap_for(kind: TonemapKind) -> Tonemap {
    match kind {
        TonemapKind::None => Tonemap::None,
        TonemapKind::Filmic => Tonemap::Filmic,
    }
}

/// The record's tone curve for a renderer's tone curve, `None` for a curve the
/// record's vocabulary does not name.
#[must_use]
pub const fn tonemap_label(tonemap: Tonemap) -> Option<TonemapKind> {
    match tonemap {
        Tonemap::None => Some(TonemapKind::None),
        Tonemap::Filmic => Some(TonemapKind::Filmic),
    }
}

/// The renderer settings a pinned configuration means.
///
/// The framebuffer size is deliberately *not* part of
/// [`ComparisonSettings`]: it sizes the target, not the settings, and
/// [`RenderConfig`] carries it as the capture's own width and height.
///
/// The exposure and gamma are checked rather than lowered, because
/// [`ComparisonSettings`] can only be built with the fixed comparison pair:
/// silently handing the renderer `1.0`/`2.2` for a record that pins anything
/// else would render the frame under settings the record does not claim, which
/// is the drift this module exists to prevent.
///
/// # Errors
///
/// Every error of [`RenderConfig::validate`], so a configuration outside its
/// declared ranges is refused here exactly as it would be at capture time, and
/// [`CaptureError::RenderSettingUnsupported`] for a pinned exposure or gamma the
/// renderer has no setting for.
pub fn settings_for(render: &RenderConfig) -> Result<ComparisonSettings, CaptureError> {
    render.validate()?;
    for (field, pinned, supported) in [
        (
            "exposure_milli",
            render.exposure_milli,
            COMPARISON_EXPOSURE_MILLI,
        ),
        ("gamma_milli", render.gamma_milli, COMPARISON_GAMMA_MILLI),
    ] {
        if pinned != supported {
            return Err(CaptureError::RenderSettingUnsupported {
                field,
                value: pinned,
            });
        }
    }
    Ok(ComparisonSettings::for_presentation(
        tonemap_for(render.tonemap),
        render.msaa_samples,
        render.shadows,
        None,
    ))
}

/// The pinned configuration a renderer setting means, at `width` × `height`.
///
/// # Errors
///
/// [`CaptureError::RenderSettingUnpinned`] when `settings` carries an internal
/// render resolution, which the pinned configuration has no field for, and
/// every error of [`RenderConfig::validate`] for the lowered thousandths.
pub fn render_for(
    settings: &ComparisonSettings,
    width: u32,
    height: u32,
) -> Result<RenderConfig, CaptureError> {
    if settings.render_resolution().is_some() {
        return Err(CaptureError::RenderSettingUnpinned {
            field: "render_resolution",
        });
    }
    let Some(tonemap) = tonemap_label(settings.tonemap()) else {
        return Err(CaptureError::RenderSettingUnpinned { field: "tonemap" });
    };
    let render = RenderConfig {
        width,
        height,
        exposure_milli: to_milli(settings.exposure()),
        gamma_milli: to_milli(settings.gamma()),
        tonemap,
        msaa_samples: settings.msaa_samples(),
        shadows: settings.shadows(),
    };
    render.validate()?;
    Ok(render)
}

/// The exact thousandths of a normalized setting.
///
/// `2.2f32` is not `2.2`, and `(x * 1000.0).round()` is the conversion that
/// both F59's document form and the renderer's own constants agree on:
/// [`ComparisonSettings::comparison`] holds `2.2f32`, which lands on `2200`, the
/// value [`RenderConfig::comparison`] stores. A value below the rounding step
/// lands on `0` and is then refused by [`RenderConfig::validate`] rather than
/// silently becoming a different setting.
fn to_milli(value: f32) -> u32 {
    let scaled = (f64::from(value) * f64::from(MILLI)).round();
    if scaled <= 0.0 {
        0
    } else if scaled >= f64::from(u32::MAX) {
        u32::MAX
    } else {
        scaled as u32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_two_comparison_baselines_are_one_set() {
        let pinned = RenderConfig::comparison();
        let lowered = settings_for(&pinned).expect("the comparison baseline is in range");
        assert!(lowered.is_fixed(), "F59's baseline must be F17's fixed set");
        let raised =
            render_for(&lowered, pinned.width, pinned.height).expect("the fixed set is in range");
        assert_eq!(raised, pinned);
    }
}

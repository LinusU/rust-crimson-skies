//! Material texture cycles: the 16-frame `wtr00000..15` sea and every other
//! cycled material of a container (task #1981, `M01-SEA-CYCLE`).
//!
//! The engine's per-draw material update (`0x55b1a0`, finding
//! `docs/findings/2026-10-24-m01-sea-material-cycle-static.md`) binds the
//! texture `table[ftol(position) % count]`, then advances `position` by
//! `frame_delta * rate`, where `rate` is the stored cycle record's `unk12` word.
//! The unit of the rate word is therefore **frames per second**; the frame delta
//! is the real frame time in seconds, clamped to `0.125` s (`data/owner-static/
//! engine-facts.toml`, `[[material_cycle]]`).
//!
//! Nothing here is `verified_original`: the update was read statically and the
//! original executable did not run.

use bevy::prelude::*;

/// The most one frame may advance a cycle by (the engine's frame-delta clamp,
/// `[[material_cycle]] cycle_delta_max_seconds`).
pub const CYCLE_DELTA_MAX_SECONDS: f32 = 0.125;

/// The engine's frame delta for a real elapsed time: clamped to
/// `0 ..= `[`CYCLE_DELTA_MAX_SECONDS`] (the time scale is 1.0).
#[must_use]
pub fn cycle_delta(elapsed_seconds: f32) -> f32 {
    elapsed_seconds.clamp(0.0, CYCLE_DELTA_MAX_SECONDS)
}

/// The frame a position shows: its truncation modulo `count`, a negative
/// position showing frame 0 (the engine's `idiv`, `0x55b1f4`, and the sign test
/// of `0x564410`).
#[must_use]
pub fn cycle_frame(position: f32, count: usize) -> usize {
    if count == 0 || position < 0.0 {
        return 0;
    }
    (position as usize) % count
}

/// The position after one update (`0x55b201..0x55b2xx`): `position + delta *
/// rate`; a non-looping cycle stops on its last frame; a negative position is
/// lifted by `count * trunc(|rate|)`.
#[must_use]
pub fn cycle_advance(position: f32, rate: f32, delta: f32, count: usize, looping: bool) -> f32 {
    let mut next = delta.mul_add(rate, position);
    if !looping && count as f32 <= next {
        next = count.saturating_sub(1) as f32;
    }
    if next < 0.0 {
        next += (count * rate.abs() as usize) as f32;
    }
    next
}

/// One cycled material: its frames, rate and running position, and every
/// engine material that shows it.
#[derive(Clone, Debug)]
pub struct MaterialCycle {
    /// The materials whose base colour texture follows the cycle (the bound
    /// material, and copies made from it, such as the horizon sea).
    pub targets: Vec<Handle<StandardMaterial>>,
    /// The frames' images, in stored order.
    pub frames: Vec<Handle<Image>>,
    /// The stored rate word (`unk12`): frames per second.
    pub rate: f32,
    /// The stored `unk00` word is non-zero: the cycle wraps instead of stopping
    /// on its last frame.
    pub looping: bool,
    /// The running position, in frames.
    pub position: f32,
}

/// Every running material cycle.
#[derive(Resource, Clone, Debug, Default)]
pub struct MaterialCycles {
    cycles: Vec<MaterialCycle>,
}

impl MaterialCycles {
    /// Starts a cycle at position 0.
    pub fn add(
        &mut self,
        material: Handle<StandardMaterial>,
        frames: Vec<Handle<Image>>,
        rate: f32,
        looping: bool,
    ) {
        self.cycles.push(MaterialCycle {
            targets: vec![material],
            frames,
            rate,
            looping,
            position: 0.0,
        });
    }

    /// Makes `copy` follow the cycle `original` follows. `false` when
    /// `original` is not cycled.
    pub fn follow(
        &mut self,
        original: &Handle<StandardMaterial>,
        copy: Handle<StandardMaterial>,
    ) -> bool {
        match self
            .cycles
            .iter_mut()
            .find(|cycle| cycle.targets.contains(original))
        {
            Some(cycle) => {
                cycle.targets.push(copy);
                true
            }
            None => false,
        }
    }

    /// The running cycles.
    #[must_use]
    pub fn cycles(&self) -> &[MaterialCycle] {
        &self.cycles
    }

    /// The cycle `material` follows.
    #[must_use]
    pub fn cycle_of(&self, material: &Handle<StandardMaterial>) -> Option<&MaterialCycle> {
        self.cycles
            .iter()
            .find(|cycle| cycle.targets.contains(material))
    }
}

/// Plays the registered cycles every frame.
pub struct MaterialCyclePlugin;

impl Plugin for MaterialCyclePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<MaterialCycles>()
            .add_systems(Update, play_material_cycles);
    }
}

fn play_material_cycles(
    time: Res<Time>,
    mut cycles: ResMut<MaterialCycles>,
    mut materials: ResMut<Assets<StandardMaterial>>,
) {
    let delta = cycle_delta(time.delta_secs());
    for cycle in &mut cycles.cycles {
        let count = cycle.frames.len();
        if count == 0 {
            continue;
        }
        let frame = &cycle.frames[cycle_frame(cycle.position, count)];
        for target in &cycle.targets {
            let Some(material) = materials.get(target) else {
                continue;
            };
            if material.base_color_texture.as_ref() != Some(frame) {
                // Mutate only on a change so an unchanged frame does not
                // re-extract the material.
                if let Some(mut material) = materials.get_mut(target) {
                    material.base_color_texture = Some(frame.clone());
                }
            }
        }
        cycle.position = cycle_advance(cycle.position, cycle.rate, delta, count, cycle.looping);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rate_is_frames_per_second() {
        // Rate 10, one second in exact 0.125 s frames: ten frames pass.
        let mut position = 0.0;
        for _ in 0..8 {
            position = cycle_advance(position, 10.0, 0.125, 16, true);
        }
        assert_eq!(position, 10.0);
        assert_eq!(cycle_frame(position, 16), 10);
    }

    #[test]
    fn delta_is_clamped_to_an_eighth_of_a_second() {
        assert_eq!(cycle_delta(2.0), 0.125);
        assert_eq!(cycle_delta(-1.0), 0.0);
        assert_eq!(cycle_delta(0.016), 0.016);
    }

    #[test]
    fn looping_wraps_and_non_looping_stops_on_the_last_frame() {
        assert_eq!(cycle_frame(17.5, 16), 1);
        assert_eq!(cycle_advance(15.5, 10.0, 0.1, 16, false), 15.0);
        assert!(cycle_advance(15.5, 10.0, 0.1, 16, true) > 16.0);
    }
}

//! Swept spatial triggers: entry and exit from real movement segments.

use cs_script::ir::{ActorId, SymbolId};
use cs_types::Tick;

use crate::ai::navigation::{segment_hits_aabb, segment_hits_sphere};

/// A trigger volume, in meters.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Volume {
    Sphere { center_m: [f64; 3], radius_m: f64 },
    Aabb { min_m: [f64; 3], max_m: [f64; 3] },
}

impl Volume {
    /// Whether the point is inside (surface inclusive).
    #[must_use]
    pub fn contains(&self, p: [f64; 3]) -> bool {
        match *self {
            Self::Sphere { center_m, radius_m } => {
                let d: f64 = (0..3).map(|i| (p[i] - center_m[i]).powi(2)).sum();
                d <= radius_m * radius_m
            }
            Self::Aabb { min_m, max_m } => (0..3).all(|i| p[i] >= min_m[i] && p[i] <= max_m[i]),
        }
    }

    /// Whether the segment touches the volume.
    #[must_use]
    pub fn touched_by(&self, from: [f64; 3], to: [f64; 3]) -> bool {
        match *self {
            Self::Sphere { center_m, radius_m } => {
                segment_hits_sphere(from, to, center_m, radius_m)
            }
            Self::Aabb { min_m, max_m } => segment_hits_aabb(from, to, min_m, max_m),
        }
    }

    fn is_finite(&self) -> bool {
        match *self {
            Self::Sphere { center_m, radius_m } => {
                center_m.iter().all(|v| v.is_finite()) && radius_m.is_finite() && radius_m >= 0.0
            }
            Self::Aabb { min_m, max_m } => {
                (0..3).all(|i| min_m[i].is_finite() && max_m[i].is_finite() && min_m[i] <= max_m[i])
            }
        }
    }
}

/// How an actor got from its previous position to its new one this tick.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Movement {
    /// A continuous move: the whole segment is swept.
    Continuous { from_m: [f64; 3], to_m: [f64; 3] },
    /// A discontinuous move: only the destination is observed. No volume
    /// between the two points is entered or exited.
    Teleport { to_m: [f64; 3] },
}

impl Movement {
    /// Whether every position in this movement is finite. Public because a
    /// runtime that drives many triggers per tick validates a whole tick's
    /// movements *before* observing any of them, so a refused tick changes no
    /// trigger's state at all.
    #[must_use]
    pub fn is_finite(&self) -> bool {
        match self {
            Self::Continuous { from_m, to_m } => {
                from_m.iter().chain(to_m.iter()).all(|v| v.is_finite())
            }
            Self::Teleport { to_m } => to_m.iter().all(|v| v.is_finite()),
        }
    }

    const fn end(&self) -> [f64; 3] {
        match *self {
            Self::Continuous { to_m, .. } | Self::Teleport { to_m } => to_m,
        }
    }
}

/// Entry or exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CrossingKind {
    Entry,
    Exit,
}

/// One emitted crossing. Within a tick the entry precedes the exit.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerEvent {
    pub trigger: SymbolId,
    pub actor: ActorId,
    pub tick: Tick,
    pub kind: CrossingKind,
}

/// A refused update.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TriggerError {
    /// A volume or position was NaN/infinite or the volume was inverted.
    NonFinite,
    /// The tick is not after the last observed one.
    NotAdvancing { last: Tick, given: Tick },
}

impl std::fmt::Display for TriggerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NonFinite => f.write_str("a volume or position was not finite"),
            Self::NotAdvancing { last, given } => write!(
                f,
                "the last observed tick is {} and this observation is tick {}",
                last.0, given.0
            ),
        }
    }
}

impl std::error::Error for TriggerError {}

/// One trigger watching one actor. `inside` is the state as of the previous
/// observation, so every transition is emitted exactly once.
#[derive(Clone, Debug, PartialEq)]
pub struct SweptTrigger {
    id: SymbolId,
    actor: ActorId,
    volume: Volume,
    inside: bool,
    last_tick: Option<Tick>,
}

impl SweptTrigger {
    /// A trigger that starts with the actor outside.
    ///
    /// # Errors
    ///
    /// [`TriggerError::NonFinite`] for a non-finite or inverted volume.
    pub fn new(id: SymbolId, actor: ActorId, volume: Volume) -> Result<Self, TriggerError> {
        if !volume.is_finite() {
            return Err(TriggerError::NonFinite);
        }
        Ok(Self {
            id,
            actor,
            volume,
            inside: false,
            last_tick: None,
        })
    }

    #[must_use]
    pub const fn is_inside(&self) -> bool {
        self.inside
    }

    /// The trigger symbol this trigger reports under.
    #[must_use]
    pub const fn id(&self) -> SymbolId {
        self.id
    }

    /// The one actor this trigger watches.
    #[must_use]
    pub const fn actor(&self) -> ActorId {
        self.actor
    }

    /// The last tick this trigger observed, for a caller that validates a whole
    /// tick's movements before observing any of them.
    #[must_use]
    pub const fn last_tick(&self) -> Option<Tick> {
        self.last_tick
    }

    /// Observes one tick's movement and returns the crossings in order.
    ///
    /// A continuous segment that passes through the volume between two
    /// outside endpoints yields one entry then one exit; a teleport yields
    /// only what the destination implies.
    ///
    /// # Errors
    ///
    /// [`TriggerError`] on bad input; the trigger state is unchanged.
    pub fn observe(
        &mut self,
        tick: Tick,
        movement: Movement,
    ) -> Result<Vec<TriggerEvent>, TriggerError> {
        if !movement.is_finite() {
            return Err(TriggerError::NonFinite);
        }
        if let Some(last) = self.last_tick
            && tick <= last
        {
            return Err(TriggerError::NotAdvancing { last, given: tick });
        }
        self.last_tick = Some(tick);

        let ends_inside = self.volume.contains(movement.end());
        let passed_through = match movement {
            Movement::Continuous { from_m, to_m } => self.volume.touched_by(from_m, to_m),
            Movement::Teleport { .. } => false,
        };
        let mut kinds = Vec::new();
        match (self.inside, ends_inside) {
            (false, true) => kinds.push(CrossingKind::Entry),
            (false, false) if passed_through => {
                kinds.push(CrossingKind::Entry);
                kinds.push(CrossingKind::Exit);
            }
            (true, false) => kinds.push(CrossingKind::Exit),
            _ => {}
        }
        self.inside = ends_inside;
        Ok(kinds
            .into_iter()
            .map(|kind| TriggerEvent {
                trigger: self.id,
                actor: self.actor,
                tick,
                kind,
            })
            .collect())
    }
}

/// The synthetic fixture: a 2 m cube at the origin.
#[must_use]
pub fn synthetic_small_volume() -> Volume {
    Volume::Aabb {
        min_m: [-1.0, -1.0, -1.0],
        max_m: [1.0, 1.0, 1.0],
    }
}

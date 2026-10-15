//! The windowed capture's **scenario**: where the player starts and which
//! block the run waits for (M01-PLAY-TRIGGERS-WINDOWED, Rally #1486).
//!
//! M01 starts 5 311 m from `workersvoyagezep` against a 700 m trigger radius,
//! and flying the authored heading never passes inside 1 400 m, so a windowed
//! run that merely flies cannot fire the proximity trigger. An evidence run
//! can instead be asked to **start closer**, on a stated heading, and to keep
//! running until a named block completes:
//!
//! | Variable | Meaning |
//! | --- | --- |
//! | `CS_MISSION_START_M` | `x,y,z` metres, replaces the measured start position |
//! | `CS_MISSION_START_HEADING_RAD` | heading in radians, replaces the measured heading |
//! | `CS_MISSION_WATCH_BLOCK` | zero-based block index the run waits for |
//! | `CS_MISSION_WATCH_MAX_FRAMES` | rendered frames the wait may last |
//!
//! The start pair is all-or-nothing, the watch pair is all-or-nothing, and
//! none of them is honoured outside a capture (`CS_MISSION_CAPTURE=1`): a
//! half-stated scenario or one asked of an ordinary run is a named
//! [`CaptureRefusal::Scenario`], never a silent default. The overridden start
//! is a test placement, not the measured start, and the capture's header
//! says so.
//!
//! The watch does not fly the aircraft: the player is the production body
//! under the production flight law, and the block must complete from where
//! that body really is, observed by the production world observer.

use bevy::prelude::{Commands, Res};

use super::MissionStage;
use super::evidence::{CaptureHandle, CaptureRefusal, request_screenshot};
use super::host::{MissionHost, MissionHostReport};
use super::observation::PLAYER_MEMBER;
use crate::mission_start::StartPose;

/// `x,y,z` metres of the replacement start position.
pub const START_ENV: &str = "CS_MISSION_START_M";
/// Radians of the replacement start heading.
pub const START_HEADING_ENV: &str = "CS_MISSION_START_HEADING_RAD";
/// The zero-based block index the run waits for.
pub const WATCH_BLOCK_ENV: &str = "CS_MISSION_WATCH_BLOCK";
/// The rendered frames the wait may last.
pub const WATCH_MAX_FRAMES_ENV: &str = "CS_MISSION_WATCH_MAX_FRAMES";

/// A replacement start pose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct StartOverride {
    /// World position, metres.
    pub position_m: [f32; 3],
    /// Heading, radians, in the measured start pose's own convention.
    pub heading_rad: f32,
}

/// The block a run waits for.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TriggerWatch {
    /// Zero-based block index.
    pub block: u32,
    /// Rendered frames the wait may last before the run ends unfired.
    pub max_frames: u32,
}

/// What a capture run is asked to do beyond the fixed script.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Scenario {
    /// Replacement start pose, when asked for.
    pub start: Option<StartOverride>,
    /// The block to wait for, when asked for.
    pub watch: Option<TriggerWatch>,
}

fn refuse(reason: String) -> CaptureRefusal {
    CaptureRefusal::Scenario(reason)
}

impl Scenario {
    /// Whether the scenario asks for nothing.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.start.is_none() && self.watch.is_none()
    }

    /// Resolves the four environment values.
    ///
    /// # Errors
    ///
    /// [`CaptureRefusal::Scenario`] when only one of a pair is given, a value
    /// does not parse, or a number is not finite.
    pub fn resolve(
        start: Option<&str>,
        heading: Option<&str>,
        block: Option<&str>,
        max_frames: Option<&str>,
    ) -> Result<Self, CaptureRefusal> {
        let start = match (start, heading) {
            (None, None) => None,
            (Some(start), Some(heading)) => Some(parse_start(start, heading)?),
            (Some(_), None) => {
                return Err(refuse(format!(
                    "{START_ENV} is set but {START_HEADING_ENV} is not: a start override states \
                     the heading it flies, there is no default"
                )));
            }
            (None, Some(_)) => {
                return Err(refuse(format!(
                    "{START_HEADING_ENV} is set but {START_ENV} is not"
                )));
            }
        };
        let watch = match (block, max_frames) {
            (None, None) => None,
            (Some(block), Some(frames)) => Some(TriggerWatch {
                block: block.trim().parse().map_err(|_| {
                    refuse(format!(
                        "{WATCH_BLOCK_ENV} must be a block index, found {block:?}"
                    ))
                })?,
                max_frames: match frames.trim().parse::<u32>() {
                    Ok(frames) if frames > 0 => frames,
                    _ => {
                        return Err(refuse(format!(
                            "{WATCH_MAX_FRAMES_ENV} must be a positive frame count, found \
                             {frames:?}"
                        )));
                    }
                },
            }),
            (Some(_), None) => {
                return Err(refuse(format!(
                    "{WATCH_BLOCK_ENV} is set but {WATCH_MAX_FRAMES_ENV} is not: a wait states \
                     how long it may last"
                )));
            }
            (None, Some(_)) => {
                return Err(refuse(format!(
                    "{WATCH_MAX_FRAMES_ENV} is set but {WATCH_BLOCK_ENV} is not"
                )));
            }
        };
        Ok(Self { start, watch })
    }

    /// The scenario the process environment asks for.
    ///
    /// # Errors
    ///
    /// The [`CaptureRefusal`] of [`Self::resolve`].
    pub fn from_env() -> Result<Self, CaptureRefusal> {
        let var = |name: &str| std::env::var(name).ok();
        Self::resolve(
            var(START_ENV).as_deref(),
            var(START_HEADING_ENV).as_deref(),
            var(WATCH_BLOCK_ENV).as_deref(),
            var(WATCH_MAX_FRAMES_ENV).as_deref(),
        )
    }

    /// Applies the start override to the stage's measured start pose.
    pub fn apply(&self, start: &mut StartPose) {
        if let Some(over) = self.start {
            start.position = over.position_m;
            start.heading = over.heading_rad;
        }
    }

    /// The `scenario` trace record, so the evidence names the placement.
    #[must_use]
    pub fn trace_line(&self) -> String {
        let start = self.start.map_or_else(
            || "null".to_owned(),
            |s| {
                format!(
                    "{{\"position_m\":[{},{},{}],\"heading_rad\":{}}}",
                    s.position_m[0], s.position_m[1], s.position_m[2], s.heading_rad
                )
            },
        );
        let watch = self.watch.map_or_else(
            || "null".to_owned(),
            |w| format!("{{\"block\":{},\"max_frames\":{}}}", w.block, w.max_frames),
        );
        format!(
            "{{\"kind\":\"scenario\",\"start_override\":{start},\"watch\":{watch},\
             \"measured_start\":{}}}",
            self.start.is_none()
        )
    }
}

fn parse_start(position: &str, heading: &str) -> Result<StartOverride, CaptureRefusal> {
    let parts: Vec<&str> = position.split(',').map(str::trim).collect();
    let [x, y, z] = parts.as_slice() else {
        return Err(refuse(format!(
            "{START_ENV} must be x,y,z metres, found {position:?}"
        )));
    };
    let number = |text: &str, name: &str| -> Result<f32, CaptureRefusal> {
        text.parse::<f32>()
            .ok()
            .filter(|value| value.is_finite())
            .ok_or_else(|| refuse(format!("{name} needs a finite number, found {text:?}")))
    };
    Ok(StartOverride {
        position_m: [
            number(x, START_ENV)?,
            number(y, START_ENV)?,
            number(z, START_ENV)?,
        ],
        heading_rad: number(heading.trim(), START_HEADING_ENV)?,
    })
}

/// Records the tick a watched block completes on, from the real player
/// position, and asks for a frame of it.
///
/// Installed in `FixedPostUpdate` after the mission host's entry, because one
/// rendered frame runs two fixed ticks and the host's report keeps only the
/// last: the completion must be read on the tick it happens.
pub fn watch_trigger(
    handle: Res<CaptureHandle>,
    report: Option<Res<MissionHostReport>>,
    host: Option<Res<MissionHost>>,
    stage: Option<Res<MissionStage>>,
    mut commands: Commands,
) {
    let (Some(report), Some(host)) = (report, host) else {
        return;
    };
    let Ok(answer) = &report.answer else {
        return;
    };
    let mut capture = handle.lock();
    let Some(watch) = capture.watch else {
        return;
    };
    if capture.trigger_fired || !answer.lifecycles.completed.contains(&watch.block) {
        return;
    }
    capture.trigger_fired = true;

    let mut members = Vec::new();
    for (name, row) in host.world_facts().observation().members() {
        members.push(format!(
            "{{\"member\":{},\"in_play\":{},\"position_m\":[{},{},{}]}}",
            super::evidence::json_string(&name.join("/")),
            row.in_play,
            row.position[0],
            row.position[1],
            row.position[2],
        ));
    }
    // The record's own effects, read off the model right after the host step
    // that completed the block: the nap target, the wake and kill lists.
    let mut effects = Vec::new();
    if let Some(objectives) = stage
        .as_ref()
        .and_then(|stage| stage.host.engine_objectives.as_ref())
        .and_then(|engine| engine.get(watch.block as usize))
    {
        let declaration = &objectives.declaration;
        let mut targets: Vec<(&str, u32)> = Vec::new();
        if let Some(nap) = declaration.nap.and_then(|nap| nap.target) {
            targets.push(("nap", nap as u32));
        }
        targets.extend(
            declaration
                .wake
                .targets()
                .iter()
                .map(|t| ("wake", *t as u32)),
        );
        targets.extend(
            declaration
                .kill
                .targets()
                .iter()
                .map(|t| ("kill", *t as u32)),
        );
        for (role, index) in targets {
            let state = host
                .lifecycles()
                .state(index)
                .map_or_else(|| "null".to_owned(), |state| format!("\"{state:?}\""));
            effects.push(format!(
                "{{\"role\":\"{role}\",\"target\":{index},\"state\":{state}}}"
            ));
        }
    }
    let tick = host.last_tick().map_or(0, |tick| tick.0);
    let frame = capture.frame;
    let line = format!(
        "{{\"kind\":\"trigger\",\"block\":{},\"tick\":{tick},\"frame\":{frame},\
         \"player_member\":{},\"members\":[{}],\"effects\":[{}]}}",
        watch.block,
        super::evidence::json_string(PLAYER_MEMBER),
        members.join(","),
        effects.join(","),
    );
    capture.lines.push(line);
    let path = capture.dir.join(format!("frame_{frame:05}_trigger.png"));
    request_screenshot(&mut commands, path, "trigger");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accept_m01_play_triggers_windowed_scenario_resolves_pairs_or_refuses() {
        assert!(
            Scenario::resolve(None, None, None, None)
                .expect("empty")
                .is_empty()
        );
        let full = Scenario::resolve(Some("1, 2.5,-3"), Some("0.5"), Some("2"), Some("100"))
            .expect("complete");
        assert_eq!(
            full.start,
            Some(StartOverride {
                position_m: [1.0, 2.5, -3.0],
                heading_rad: 0.5
            })
        );
        assert_eq!(
            full.watch,
            Some(TriggerWatch {
                block: 2,
                max_frames: 100
            })
        );
        let mut pose = StartPose {
            position: [0.0; 3],
            heading: 9.0,
        };
        full.apply(&mut pose);
        assert_eq!((pose.position, pose.heading), ([1.0, 2.5, -3.0], 0.5));

        for (start, heading, block, frames) in [
            (Some("1,2,3"), None, None, None),
            (None, Some("1"), None, None),
            (None, None, Some("2"), None),
            (None, None, None, Some("10")),
            (Some("1,2"), Some("1"), None, None),
            (Some("1,2,nan"), Some("1"), None, None),
            (Some("1,2,3"), Some("inf"), None, None),
            (None, None, Some("x"), Some("10")),
            (None, None, Some("2"), Some("0")),
        ] {
            let refused = Scenario::resolve(start, heading, block, frames)
                .expect_err("an incomplete or malformed scenario is refused");
            assert!(matches!(refused, CaptureRefusal::Scenario(_)), "{refused}");
        }
    }

    #[test]
    fn accept_m01_play_triggers_windowed_scenario_needs_a_capture() {
        use super::super::evidence::CaptureSettings;
        let scenario =
            Scenario::resolve(Some("1,2,3"), Some("0.5"), Some("2"), Some("100")).expect("ok");
        let refused = CaptureSettings::gate(false, scenario, Some("gpu,audio"), Some("/tmp/x"))
            .expect_err("an ordinary run is not given a scenario");
        assert!(matches!(refused, CaptureRefusal::Scenario(_)), "{refused}");
        assert_eq!(
            CaptureSettings::gate(false, Scenario::default(), None, None).expect("ordinary"),
            None
        );
        let settings = CaptureSettings::gate(true, scenario, Some("gpu,audio"), Some("/tmp/x"))
            .expect("a capture takes the scenario")
            .expect("requested");
        assert_eq!(settings.scenario, scenario);
    }
}

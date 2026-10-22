//! The smoke's per-second trace of our playing voice set (Rally #1866).
//!
//! Once per second of mission time the smoke records every voice the audio
//! session holds: the member, its decoded 16-bit PCM size, whether it loops,
//! and the gain, pan and rate the mixer hands the device. The rows are shaped
//! like the owner's Drydock DirectSound log so the two can be set side by
//! side (`docs/findings/2026-10-22-m01-audio-timeline-smoke-trace.md`).
//!
//! What this cannot see is named in [`UNOBSERVED`]; a row never carries a
//! value the smoke did not read.

use std::collections::BTreeSet;

use bevy::prelude::*;
use cs_sim::audio_events::{AudioEmitterId, EmitterMix, spatialize};

use crate::audio::AudioEmitterBinding;
use crate::audio::device::DeviceSampleLibrary;
use crate::audio::engine::EngineVoices;
use crate::audio::loops::AudioSession;
use crate::audio::mixer::AudioSpatial;

/// What a mission second's trace cannot say, verbatim into the report.
pub const UNOBSERVED: &[&str] = &[
    "one-shot effects (gunfire, explosions, ricochets) are accepted by the router and \
     handed on without a held voice, so they never appear in a second's set",
    "music tracks start on a verdict the smoke does not read back as a voice",
    "radio lines appear only while the queue holds one active line",
];

/// One voice in one second's set.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceRow {
    /// The content id of the member the voice plays.
    pub member: String,
    /// The member's decoded 16-bit PCM size in bytes (`None`: not in the
    /// device library, so not measured).
    pub decoded_bytes: Option<u64>,
    /// Whether the voice repeats its member.
    pub looping: bool,
    /// Final linear gain: loop gain x engine gain x spatial gain.
    pub gain: f64,
    /// Stereo placement in `-1.0 ..= 1.0` (`None`: the emitter has no pose).
    pub pan: Option<f64>,
    /// The playback rate in Hz: the member's own rate times the pitch ratio
    /// (`None` with `decoded_bytes`).
    pub rate_hz: Option<f64>,
}

impl VoiceRow {
    /// The gain on the original engine's own volume scale: hundredths of a
    /// dB, 1000 per octave of gain, 0 at unity and -10000 at or below 1/1024
    /// (`docs/findings/2026-10-22-m01-audio-timeline-voice-laws.md`).
    #[must_use]
    pub fn gain_db(&self) -> f64 {
        gain_to_db(self.gain)
    }
}

/// The engine's gain law in dB: `0` at unity or above, `-100` at or below
/// 1/1024, else `10 * log2(g)`.
#[must_use]
pub fn gain_to_db(gain: f64) -> f64 {
    if !gain.is_finite() || gain >= 1.0 {
        0.0
    } else if gain <= 1.0 / 1024.0 {
        -100.0
    } else {
        10.0 * gain.log2()
    }
}

/// One whole second of the trace.
#[derive(Clone, Debug, PartialEq)]
pub struct VoiceSecond {
    /// Mission seconds since Fly Mission.
    pub second: u32,
    /// The voices playing, ordered by member, then size.
    pub voices: Vec<VoiceRow>,
}

impl VoiceSecond {
    /// The set's identity for change detection: member, size and looping.
    #[must_use]
    pub fn identity(&self) -> BTreeSet<(String, Option<u64>, bool)> {
        self.voices
            .iter()
            .map(|v| (v.member.clone(), v.decoded_bytes, v.looping))
            .collect()
    }
}

/// The seconds at which the set differs from the second before it (the first
/// second counts when it is not empty).
#[must_use]
pub fn set_changes(trace: &[VoiceSecond]) -> Vec<u32> {
    let mut changes = Vec::new();
    let mut previous = BTreeSet::new();
    for second in trace {
        let now = second.identity();
        if now != previous {
            changes.push(second.second);
        }
        previous = now;
    }
    changes
}

/// Reads the live audio state into one second's set.
#[allow(clippy::too_many_arguments)]
pub fn sample_second(
    second: u32,
    session: &AudioSession,
    spatial: Option<&AudioSpatial>,
    engine: &EngineVoices,
    library: Option<&DeviceSampleLibrary>,
    emitters: &Query<(&AudioEmitterBinding, Option<&GlobalTransform>)>,
) -> VoiceSecond {
    let poses: std::collections::BTreeMap<AudioEmitterId, [f64; 3]> = emitters
        .iter()
        .filter_map(|(binding, transform)| {
            transform.map(|t| (binding.emitter, t.translation().as_dvec3().to_array()))
        })
        .collect();
    let mut voices = Vec::new();
    for binding in session.router.active_loops() {
        let level = engine.level(&binding.emitter);
        let placed = spatial.zip(poses.get(&binding.emitter)).and_then(|(s, p)| {
            let mix = EmitterMix {
                emitter: binding.emitter,
                position_m: *p,
                level,
            };
            spatialize(&s.policy, &s.listener, mix.position_m).ok()
        });
        let pcm = library.and_then(|l| l.library().pcm(&binding.asset));
        voices.push(VoiceRow {
            member: binding.asset.to_string(),
            decoded_bytes: pcm.as_ref().map(|p| p.sample_count() * 2),
            looping: true,
            gain: binding.gain * level.gain * placed.map_or(1.0, |s| s.gain),
            pan: placed.map(|s| s.pan),
            rate_hz: pcm.as_ref().map(|p| f64::from(p.rate_hz()) * level.pitch),
        });
    }
    if let Some(line) = session.radio.active() {
        let pcm = library.and_then(|l| l.library().pcm(&line.asset));
        voices.push(VoiceRow {
            member: line.asset.to_string(),
            decoded_bytes: pcm.as_ref().map(|p| p.sample_count() * 2),
            looping: false,
            gain: 1.0,
            pan: Some(0.0),
            rate_hz: pcm.as_ref().map(|p| f64::from(p.rate_hz())),
        });
    }
    voices.sort_by(|a, b| (&a.member, a.decoded_bytes).cmp(&(&b.member, b.decoded_bytes)));
    VoiceSecond { second, voices }
}

/// Samples the trace once per mission second, from the second frame on.
#[allow(clippy::needless_pass_by_value)]
pub(super) fn smoke_voice_trace(
    mut data: ResMut<super::SmokeData>,
    audio: Option<Res<AudioSession>>,
    spatial: Option<Res<AudioSpatial>>,
    engine_voices: Res<EngineVoices>,
    library: Option<Res<DeviceSampleLibrary>>,
    emitters: Query<(&AudioEmitterBinding, Option<&GlobalTransform>)>,
) {
    let Some(session) = audio.as_deref() else {
        return;
    };
    if !data.frame.is_multiple_of(60) {
        return;
    }
    let second = data.frame / 60;
    if data.voice_trace.last().map(|s| s.second) == Some(second) {
        return;
    }
    let row = sample_second(
        second,
        session,
        spatial.as_deref(),
        &engine_voices,
        library.as_deref(),
        &emitters,
    );
    data.voice_trace.push(row);
}

/// The trace as a JSON array, one object per second.
#[must_use]
pub fn trace_json(trace: &[VoiceSecond]) -> String {
    fn opt(value: Option<f64>) -> String {
        value.map_or_else(|| "null".to_owned(), |v| format!("{v:.3}"))
    }
    let seconds: Vec<String> = trace
        .iter()
        .map(|s| {
            let rows: Vec<String> = s
                .voices
                .iter()
                .map(|v| {
                    format!(
                        "{{\"member\":{},\"decoded_bytes\":{},\"looping\":{},\
                         \"gain\":{:.4},\"gain_db\":{:.2},\"pan\":{},\"rate_hz\":{}}}",
                        super::json_string(&v.member),
                        v.decoded_bytes
                            .map_or_else(|| "null".to_owned(), |b| b.to_string()),
                        v.looping,
                        v.gain,
                        v.gain_db(),
                        opt(v.pan),
                        opt(v.rate_hz),
                    )
                })
                .collect();
            format!(
                "{{\"second\":{},\"voices\":[{}]}}",
                s.second,
                rows.join(",")
            )
        })
        .collect();
    format!("[{}]", seconds.join(","))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(member: &str, bytes: u64, looping: bool) -> VoiceRow {
        VoiceRow {
            member: member.to_owned(),
            decoded_bytes: Some(bytes),
            looping,
            gain: 0.5,
            pan: Some(0.0),
            rate_hz: Some(22_050.0),
        }
    }

    /// **`accept_parity6_m01_audio_06_` the engine's gain law and change detection.**
    #[test]
    fn accept_parity6_m01_audio_06_gain_law_and_set_changes() {
        assert!((gain_to_db(0.5) + 10.0).abs() < 1e-9);
        assert_eq!(gain_to_db(2.0), 0.0);
        assert_eq!(gain_to_db(0.0), -100.0);
        let second = |s, voices| VoiceSecond { second: s, voices };
        let trace = vec![
            second(0, vec![]),
            second(1, vec![row("a", 10, true)]),
            second(2, vec![row("a", 10, true)]),
            second(3, vec![row("a", 10, true), row("b", 4, false)]),
            second(4, vec![row("b", 4, false)]),
        ];
        assert_eq!(set_changes(&trace), vec![1, 3, 4]);
        let json = trace_json(&trace);
        assert!(json.starts_with("[{\"second\":0,\"voices\":[]}"));
        assert!(json.contains("\"gain_db\":-10.00"));
    }
}

//! A device-side tap on every one-shot voice the world's output is asked to
//! start (Rally #1941).
//!
//! One-shots end on their own, so a per-second look at the voices alive (the
//! smoke's `voice_trace`) misses any that start and finish between two
//! samples. [`TappingDevice`] wraps the world's [`AudioDevice`] and records
//! each [`AudioDevice::start_one_shot`] call into a shared [`OneShotTap`]
//! before handing it on unchanged, so what is played does not change.

use std::sync::{Arc, Mutex, PoisonError};

use cs_sim::audio_events::{
    AudioDevice, DeviceError, DeviceVoiceId, VoiceStart, VoiceStop, VoiceUpdate,
};

/// One one-shot start the device was asked for.
#[derive(Clone, Debug, PartialEq)]
pub struct TappedStart {
    /// What the mixer asked the device to start.
    pub start: VoiceStart,
    /// The device's refusal code, or `None` when it took the voice.
    pub refused: Option<String>,
}

/// The shared log of one-shot starts; cloning shares the log.
#[derive(Clone, Debug, Default)]
pub struct OneShotTap {
    log: Arc<Mutex<Vec<TappedStart>>>,
}

impl OneShotTap {
    /// Takes every start recorded since the last drain, oldest first.
    #[must_use]
    pub fn drain(&self) -> Vec<TappedStart> {
        std::mem::take(&mut *self.log.lock().unwrap_or_else(PoisonError::into_inner))
    }

    fn record(&self, entry: TappedStart) {
        self.log
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .push(entry);
    }
}

/// An [`AudioDevice`] that forwards every command to `inner` and records the
/// one-shot starts.
#[derive(Debug)]
pub struct TappingDevice {
    inner: Box<dyn AudioDevice>,
    tap: OneShotTap,
}

impl TappingDevice {
    /// Wraps `inner`, recording into `tap`.
    #[must_use]
    pub fn new(inner: Box<dyn AudioDevice>, tap: OneShotTap) -> Self {
        Self { inner, tap }
    }
}

impl AudioDevice for TappingDevice {
    fn is_open(&self) -> bool {
        self.inner.is_open()
    }

    fn open(&mut self) -> Result<(), DeviceError> {
        self.inner.open()
    }

    fn close(&mut self) {
        self.inner.close();
    }

    fn start_voice(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        self.inner.start_voice(start)
    }

    fn stop_voice(&mut self, stop: VoiceStop) -> Result<(), DeviceError> {
        self.inner.stop_voice(stop)
    }

    fn update_voice(
        &mut self,
        voice: DeviceVoiceId,
        update: VoiceUpdate,
    ) -> Result<(), DeviceError> {
        self.inner.update_voice(voice, update)
    }

    fn start_one_shot(&mut self, start: VoiceStart) -> Result<DeviceVoiceId, DeviceError> {
        let result = self.inner.start_one_shot(start.clone());
        self.tap.record(TappedStart {
            start,
            refused: result.as_ref().err().map(|e| e.code.to_owned()),
        });
        result
    }
}

//! CPAL/WASAPI capture backend.
//!
//! The capture callback writes resampled i16 samples into a bounded in-memory
//! channel and never blocks the audio thread. Overflow drops the current
//! chunk rather than stalling capture; `stop()` drains whatever reached the
//! consumer and returns an [`AudioBuffer`] at 16 kHz mono PCM16.

use std::sync::mpsc::{sync_channel, Receiver, SyncSender, TrySendError};
use std::sync::{
    atomic::{AtomicUsize, Ordering},
    Arc,
};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, SupportedStreamConfig};
use promptloom_core::{AudioBuffer, AudioSource, PromptLoomError};

use crate::resample::downmix_and_resample;
use crate::rms_level;

/// Target rate expected by the ASR stage.
const TARGET_RATE_HZ: u32 = 16_000;

/// Bounded capture channel capacity (chunks). ~5 s of audio for 10 ms chunks.
const CHANNEL_CAPACITY: usize = 512;

/// A snapshot of the most recent RMS level, shared with the UI thread.
#[derive(Default, Clone)]
pub struct LevelMeter {
    inner: Arc<AtomicUsize>,
}

impl LevelMeter {
    pub fn set(&self, level: f32) {
        let scaled = (level.clamp(0.0, 1.0) * 10_000.0) as usize;
        self.inner.store(scaled, Ordering::Relaxed);
    }
    pub fn get(&self) -> f32 {
        self.inner.load(Ordering::Relaxed) as f32 / 10_000.0
    }
}

/// Real microphone capture via CPAL on top of WASAPI.
pub struct WasapiAudioSource {
    device: Device,
    config: SupportedStreamConfig,
    stream: Option<Stream>,
    receiver: Option<Receiver<Vec<i16>>>,
    overflow_count: Arc<AtomicUsize>,
    level: LevelMeter,
}

impl WasapiAudioSource {
    pub fn new() -> Result<Self, PromptLoomError> {
        let host = cpal::default_host();
        let device = host
            .default_input_device()
            .ok_or(PromptLoomError::AudioUnavailable)?;
        let config = device
            .default_input_config()
            .map_err(|_| PromptLoomError::AudioUnavailable)?;
        Ok(Self {
            device,
            config,
            stream: None,
            receiver: None,
            overflow_count: Arc::new(AtomicUsize::new(0)),
            level: LevelMeter::default(),
        })
    }

    pub fn level_meter(&self) -> LevelMeter {
        self.level.clone()
    }

    pub fn overflow_count(&self) -> usize {
        self.overflow_count.load(Ordering::Relaxed)
    }

    fn build_stream(
        &self,
        tx: SyncSender<Vec<i16>>,
    ) -> Result<Stream, PromptLoomError> {
        let input_rate = self.config.sample_rate().0;
        let channels = self.config.channels() as usize;
        let overflow = self.overflow_count.clone();
        let level = self.level.clone();

        let err_fn = |err| tracing::warn!(?err, "audio input stream error");

        let stream = match self.config.sample_format() {
            SampleFormat::F32 => self.device.build_input_stream(
                &self.config.config(),
                move |data: &[f32], _| {
                    push_chunk(
                        data,
                        channels,
                        input_rate,
                        &tx,
                        &overflow,
                        &level,
                    );
                },
                err_fn,
                None,
            ),
            SampleFormat::I16 => {
                let dev = self.device.clone();
                dev.build_input_stream(
                    &self.config.config(),
                    move |data: &[i16], _| {
                        let as_f32: Vec<f32> = data
                            .iter()
                            .map(|s| *s as f32 / i16::MAX as f32)
                            .collect();
                        push_chunk(
                            &as_f32,
                            channels,
                            input_rate,
                            &tx,
                            &overflow,
                            &level,
                        );
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::U16 => {
                let dev = self.device.clone();
                dev.build_input_stream(
                    &self.config.config(),
                    move |data: &[u16], _| {
                        let as_f32: Vec<f32> = data
                            .iter()
                            .map(|s| (*s as f32 - 32_768.0) / 32_768.0)
                            .collect();
                        push_chunk(
                            &as_f32,
                            channels,
                            input_rate,
                            &tx,
                            &overflow,
                            &level,
                        );
                    },
                    err_fn,
                    None,
                )
            }
            _ => return Err(PromptLoomError::AudioUnavailable),
        };

        stream.map_err(|e| {
            tracing::warn!(?e, "could not build input stream");
            PromptLoomError::AudioUnavailable
        })
    }
}

fn push_chunk(
    data: &[f32],
    channels: usize,
    input_rate: u32,
    tx: &SyncSender<Vec<i16>>,
    overflow: &AtomicUsize,
    level: &LevelMeter,
) {
    let samples = downmix_and_resample(data, channels, input_rate, TARGET_RATE_HZ);
    if samples.is_empty() {
        return;
    }
    level.set(rms_level(&samples));
    match tx.try_send(samples) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            overflow.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

impl AudioSource for WasapiAudioSource {
    fn start(&mut self) -> Result<(), PromptLoomError> {
        if self.stream.is_some() {
            return Ok(());
        }
        let (tx, rx) = sync_channel::<Vec<i16>>(CHANNEL_CAPACITY);
        let stream = self.build_stream(tx)?;
        stream
            .play()
            .map_err(|_| PromptLoomError::AudioUnavailable)?;
        self.stream = Some(stream);
        self.receiver = Some(rx);
        self.overflow_count.store(0, Ordering::Relaxed);
        self.level.set(0.0);
        Ok(())
    }

    fn stop(&mut self) -> Result<AudioBuffer, PromptLoomError> {
        self.stream.take();
        let rx = self.receiver.take().ok_or(PromptLoomError::AudioUnavailable)?;
        let mut samples = Vec::new();
        while let Ok(chunk) = rx.try_recv() {
            samples.extend_from_slice(&chunk);
        }
        self.level.set(0.0);
        Ok(AudioBuffer {
            sample_rate_hz: TARGET_RATE_HZ,
            samples,
        })
    }

    fn cancel(&mut self) {
        self.stream.take();
        self.receiver.take();
        self.level.set(0.0);
    }
}

/// Safety: cpal `Stream` is `!Send` on some targets but on Windows it is held
/// and dropped from the thread that owns the audio graph. We never move the
/// stream between threads once it is `start`ed; `WasapiAudioSource` is only
/// touched from the orchestrator thread.
unsafe impl Send for WasapiAudioSource {}

//! Lightweight input-level probe for the first-run mic setup step.
//!
//! Independent from [`WasapiAudioSource`] so onboarding can run concurrently
//! with the orchestrator without competing for its stream. Discards audio
//! samples; the only output is a shared [`LevelMeter`].

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleFormat, Stream, SupportedStreamConfig};

use crate::resample::downmix_and_resample;
use crate::wasapi::LevelMeter;
use crate::{rms_level, TARGET_SAMPLE_RATE_HZ};

#[derive(Debug, thiserror::Error)]
pub enum ProbeError {
    #[error("no input device named {0:?}")]
    NoSuchDevice(String),
    #[error("default input device unavailable")]
    DefaultUnavailable,
    #[error("device supports no usable input config")]
    NoConfig,
    #[error("could not build input stream")]
    BuildFailed,
    #[error("stream.play() failed")]
    PlayFailed,
}

/// A short-lived capture that only computes a running RMS. Drop to stop.
pub struct MicProbe {
    _stream: Stream,
    level: LevelMeter,
}

impl MicProbe {
    pub fn level_meter(&self) -> LevelMeter {
        self.level.clone()
    }

    /// Start probing the named device, or the system default when `name` is None.
    pub fn start(name: Option<&str>) -> Result<Self, ProbeError> {
        let host = cpal::default_host();
        let device = match name {
            Some(n) => host
                .input_devices()
                .map_err(|_| ProbeError::DefaultUnavailable)?
                .find(|d| d.name().map(|dn| dn == n).unwrap_or(false))
                .ok_or_else(|| ProbeError::NoSuchDevice(n.to_string()))?,
            None => host
                .default_input_device()
                .ok_or(ProbeError::DefaultUnavailable)?,
        };
        let config = device
            .default_input_config()
            .map_err(|_| ProbeError::NoConfig)?;
        let level = LevelMeter::default();
        let stream = build_probe_stream(&device, &config, level.clone())?;
        stream.play().map_err(|_| ProbeError::PlayFailed)?;
        Ok(Self {
            _stream: stream,
            level,
        })
    }
}

/// Safety: see WasapiAudioSource — CPAL Stream is Send on Windows.
unsafe impl Send for MicProbe {}

fn build_probe_stream(
    device: &Device,
    config: &SupportedStreamConfig,
    level: LevelMeter,
) -> Result<Stream, ProbeError> {
    let channels = config.channels() as usize;
    let input_rate = config.sample_rate().0;
    let err_fn = |err| tracing::warn!(?err, "mic probe stream error");

    let built = match config.sample_format() {
        SampleFormat::F32 => device.build_input_stream(
            &config.config(),
            move |data: &[f32], _| {
                let samples =
                    downmix_and_resample(data, channels, input_rate, TARGET_SAMPLE_RATE_HZ);
                level.set(rms_level(&samples));
            },
            err_fn,
            None,
        ),
        SampleFormat::I16 => device.build_input_stream(
            &config.config(),
            move |data: &[i16], _| {
                let as_f32: Vec<f32> = data.iter().map(|s| *s as f32 / i16::MAX as f32).collect();
                let samples =
                    downmix_and_resample(&as_f32, channels, input_rate, TARGET_SAMPLE_RATE_HZ);
                level.set(rms_level(&samples));
            },
            err_fn,
            None,
        ),
        SampleFormat::U16 => device.build_input_stream(
            &config.config(),
            move |data: &[u16], _| {
                let as_f32: Vec<f32> = data
                    .iter()
                    .map(|s| (*s as f32 - 32_768.0) / 32_768.0)
                    .collect();
                let samples =
                    downmix_and_resample(&as_f32, channels, input_rate, TARGET_SAMPLE_RATE_HZ);
                level.set(rms_level(&samples));
            },
            err_fn,
            None,
        ),
        _ => return Err(ProbeError::NoConfig),
    };
    built.map_err(|e| {
        tracing::warn!(?e, "could not build probe stream");
        ProbeError::BuildFailed
    })
}

/// List input device names available via the default host.
pub fn enumerate_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    host.input_devices()
        .map(|iter| iter.filter_map(|d| d.name().ok()).collect())
        .unwrap_or_default()
}

/// Name of the default input device, if any.
pub fn default_input_device_name() -> Option<String> {
    cpal::default_host()
        .default_input_device()
        .and_then(|d| d.name().ok())
}

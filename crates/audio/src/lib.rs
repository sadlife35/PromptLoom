//! promptloom-audio: WASAPI/CPAL capture, resampling, level meter.

pub mod resample;
pub mod wasapi;

pub use wasapi::{LevelMeter, WasapiAudioSource};

use promptloom_core::{AudioBuffer, AudioSource, PromptLoomError};

/// Target sample rate the ASR expects.
pub const TARGET_SAMPLE_RATE_HZ: u32 = 16_000;

/// Compute mean RMS as a level-meter value in [0.0, 1.0]. Used by the HUD.
pub fn rms_level(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum_sq: f64 = samples.iter().map(|s| (*s as f64).powi(2)).sum();
    let mean = sum_sq / samples.len() as f64;
    (mean.sqrt() / i16::MAX as f64).clamp(0.0, 1.0) as f32
}

/// Trim leading and trailing silence below `threshold` RMS (per 10 ms frame).
pub fn trim_silence(samples: &[i16], sample_rate_hz: u32, threshold: f32) -> Vec<i16> {
    if samples.is_empty() {
        return Vec::new();
    }
    let frame = (sample_rate_hz as usize / 100).max(1); // 10 ms frames
    let mut start = 0usize;
    while start + frame <= samples.len() && rms_level(&samples[start..start + frame]) < threshold {
        start += frame;
    }
    let mut end = samples.len();
    while end > start + frame && rms_level(&samples[end - frame..end]) < threshold {
        end -= frame;
    }
    samples[start..end].to_vec()
}

/// Null AudioSource for tests that cannot bind a mic. Kept so unit tests in
/// downstream crates don't need audio hardware.
pub struct NullAudioSource;

impl AudioSource for NullAudioSource {
    fn start(&mut self) -> Result<(), PromptLoomError> {
        Err(PromptLoomError::AudioUnavailable)
    }
    fn stop(&mut self) -> Result<AudioBuffer, PromptLoomError> {
        Err(PromptLoomError::AudioUnavailable)
    }
    fn cancel(&mut self) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rms_zero_for_silence() {
        assert_eq!(rms_level(&vec![0i16; 1600]), 0.0);
    }

    #[test]
    fn rms_nonzero_for_signal() {
        let samples: Vec<i16> = (0..1600).map(|i| ((i % 200) as i16) * 100).collect();
        assert!(rms_level(&samples) > 0.0);
    }

    #[test]
    fn trim_silence_drops_leading_zeros() {
        let mut samples = vec![0i16; 1600]; // 100 ms of silence @ 16 kHz
        samples.extend((0..1600).map(|i| ((i % 100) as i16) * 200));
        let trimmed = trim_silence(&samples, 16_000, 0.001);
        assert!(trimmed.len() < samples.len());
    }

    #[test]
    fn trim_silence_keeps_everything_when_loud() {
        let samples: Vec<i16> = (0..1600).map(|_| 10_000).collect();
        let trimmed = trim_silence(&samples, 16_000, 0.001);
        assert_eq!(trimmed.len(), samples.len());
    }
}

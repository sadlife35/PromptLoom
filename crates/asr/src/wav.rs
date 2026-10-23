//! Load a WAV fixture into an [`AudioBuffer`] at 16 kHz mono PCM16.

use std::path::Path;

use promptloom_core::AudioBuffer;

#[derive(Debug, thiserror::Error)]
pub enum WavFixtureError {
    #[error("wav read: {0}")]
    Read(String),
    #[error("unsupported sample format")]
    UnsupportedFormat,
    #[error("empty file")]
    Empty,
}

/// Read a WAV file and normalize it to 16 kHz mono PCM16. Linear resample.
pub fn load_wav_fixture(path: &Path) -> Result<AudioBuffer, WavFixtureError> {
    let mut reader = hound::WavReader::open(path).map_err(|e| WavFixtureError::Read(e.to_string()))?;
    let spec = reader.spec();
    let channels = spec.channels as usize;
    let rate = spec.sample_rate;

    let samples_i16: Vec<i16> = match (spec.sample_format, spec.bits_per_sample) {
        (hound::SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .filter_map(|s| s.ok())
            .collect(),
        (hound::SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .filter_map(|s| s.ok())
            .map(|s| (s >> 16) as i16)
            .collect(),
        (hound::SampleFormat::Float, 32) => reader
            .samples::<f32>()
            .filter_map(|s| s.ok())
            .map(|s| (s.clamp(-1.0, 1.0) * i16::MAX as f32) as i16)
            .collect(),
        _ => return Err(WavFixtureError::UnsupportedFormat),
    };

    if samples_i16.is_empty() {
        return Err(WavFixtureError::Empty);
    }

    let mono = if channels == 1 {
        samples_i16
    } else {
        downmix_to_mono(&samples_i16, channels)
    };

    let resampled = if rate == 16_000 {
        mono
    } else {
        resample_linear_i16(&mono, rate, 16_000)
    };

    Ok(AudioBuffer {
        sample_rate_hz: 16_000,
        samples: resampled,
    })
}

fn downmix_to_mono(samples: &[i16], channels: usize) -> Vec<i16> {
    let frames = samples.len() / channels;
    let mut out = Vec::with_capacity(frames);
    for frame in 0..frames {
        let base = frame * channels;
        let mut sum: i32 = 0;
        for ch in 0..channels {
            sum += samples[base + ch] as i32;
        }
        out.push((sum / channels as i32) as i16);
    }
    out
}

fn resample_linear_i16(samples: &[i16], in_rate: u32, out_rate: u32) -> Vec<i16> {
    if samples.is_empty() {
        return Vec::new();
    }
    let in_rate = in_rate as f64;
    let out_rate = out_rate as f64;
    let out_len = ((samples.len() as f64) * out_rate / in_rate).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = (i as f64) * in_rate / out_rate;
        let idx = src_pos.floor() as usize;
        let frac = src_pos - idx as f64;
        let a = *samples.get(idx).unwrap_or(&0) as f64;
        let b = *samples.get(idx + 1).unwrap_or(&(a as i16)) as f64;
        out.push((a + (b - a) * frac) as i16);
    }
    out
}

/// Convert i16 samples to f32 in [-1.0, 1.0] (what whisper.cpp expects).
pub fn samples_i16_to_f32(samples: &[i16]) -> Vec<f32> {
    samples
        .iter()
        .map(|s| *s as f32 / i16::MAX as f32)
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use hound::{SampleFormat, WavSpec, WavWriter};
    use std::io::Cursor;

    fn write_wav(samples: &[i16], rate: u32, channels: u16) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut cursor = Cursor::new(&mut buf);
            let spec = WavSpec {
                channels,
                sample_rate: rate,
                bits_per_sample: 16,
                sample_format: SampleFormat::Int,
            };
            let mut w = WavWriter::new(&mut cursor, spec).unwrap();
            for s in samples {
                w.write_sample(*s).unwrap();
            }
            w.finalize().unwrap();
        }
        buf
    }

    #[test]
    fn loads_mono_16khz_passthrough() {
        let samples: Vec<i16> = (0..8000).map(|i| ((i % 100) as i16) * 100).collect();
        let bytes = write_wav(&samples, 16_000, 1);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bytes).unwrap();
        let buf = load_wav_fixture(tmp.path()).unwrap();
        assert_eq!(buf.sample_rate_hz, 16_000);
        assert_eq!(buf.samples.len(), 8000);
    }

    #[test]
    fn downmixes_stereo_to_mono() {
        let samples: Vec<i16> = (0..1600).flat_map(|_| vec![1000i16, -1000i16]).collect();
        let bytes = write_wav(&samples, 16_000, 2);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bytes).unwrap();
        let buf = load_wav_fixture(tmp.path()).unwrap();
        assert_eq!(buf.samples.len(), 1600);
        // Stereo L=1000, R=-1000 averages to 0.
        assert!(buf.samples.iter().all(|s| *s == 0));
    }

    #[test]
    fn resamples_from_44100_to_16000() {
        let samples: Vec<i16> = (0..44100).map(|i| (i as i16).wrapping_mul(10)).collect();
        let bytes = write_wav(&samples, 44_100, 1);
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), bytes).unwrap();
        let buf = load_wav_fixture(tmp.path()).unwrap();
        assert_eq!(buf.sample_rate_hz, 16_000);
        // 1 second at 44.1k → ~16k samples
        assert!(buf.samples.len() >= 15_900 && buf.samples.len() <= 16_100);
    }

    #[test]
    fn i16_to_f32_round_trip_bounds() {
        let v = samples_i16_to_f32(&[0, i16::MAX, -i16::MAX]);
        assert!((v[0]).abs() < 0.0001);
        assert!((v[1] - 1.0).abs() < 0.0001);
        assert!((v[2] + 1.0).abs() < 0.0001);
    }
}

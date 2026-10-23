//! Simple mono downmix + resampling to the ASR target rate.

/// Downmix interleaved f32 samples to mono, then resample linearly to
/// `target_rate_hz` and convert to i16. The input is assumed to be the
/// device's native rate; a one-shot pass is used per callback chunk.
///
/// Linear interpolation is good enough for speech recognition at 16 kHz;
/// higher-quality resampling (sinc, polyphase) is deferred.
pub fn downmix_and_resample(
    input: &[f32],
    input_channels: usize,
    input_rate_hz: u32,
    target_rate_hz: u32,
) -> Vec<i16> {
    if input.is_empty() || input_channels == 0 {
        return Vec::new();
    }

    // 1. Downmix to mono by averaging channels per frame.
    let frames = input.len() / input_channels;
    let mut mono = Vec::with_capacity(frames);
    for frame in 0..frames {
        let base = frame * input_channels;
        let mut sum = 0.0f32;
        for ch in 0..input_channels {
            sum += input[base + ch];
        }
        mono.push(sum / input_channels as f32);
    }

    if input_rate_hz == target_rate_hz {
        return mono.iter().map(f32_to_i16).collect();
    }

    // 2. Linear resample mono to target rate.
    let in_rate = input_rate_hz as f64;
    let out_rate = target_rate_hz as f64;
    let out_len = ((mono.len() as f64) * out_rate / in_rate).floor() as usize;
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let src_pos = (i as f64) * in_rate / out_rate;
        let idx = src_pos.floor() as usize;
        let frac = (src_pos - idx as f64) as f32;
        let a = *mono.get(idx).unwrap_or(&0.0);
        let b = *mono.get(idx + 1).unwrap_or(&a);
        let sample = a + (b - a) * frac;
        out.push(f32_to_i16(&sample));
    }
    out
}

fn f32_to_i16(s: &f32) -> i16 {
    let clamped = s.clamp(-1.0, 1.0);
    (clamped * i16::MAX as f32) as i16
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn passthrough_when_rate_matches() {
        let input = vec![0.0_f32, 0.5, -0.5, 1.0];
        let out = downmix_and_resample(&input, 1, 16_000, 16_000);
        assert_eq!(out.len(), 4);
        assert_eq!(out[0], 0);
        assert!((out[1] - 16_383).abs() <= 2);
        assert_eq!(out[3], i16::MAX);
    }

    #[test]
    fn downmix_stereo_to_mono() {
        // L=1.0, R=-1.0 average to 0.0
        let input = vec![1.0_f32, -1.0, 0.5, -0.5];
        let out = downmix_and_resample(&input, 2, 16_000, 16_000);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0], 0);
        assert_eq!(out[1], 0);
    }

    #[test]
    fn downsample_halves_length() {
        let input: Vec<f32> = (0..32).map(|i| (i as f32) / 32.0).collect();
        let out = downmix_and_resample(&input, 1, 32_000, 16_000);
        assert_eq!(out.len(), 16);
    }

    #[test]
    fn upsample_doubles_length() {
        let input: Vec<f32> = vec![0.0, 0.5, 1.0, 0.5];
        let out = downmix_and_resample(&input, 1, 8_000, 16_000);
        assert_eq!(out.len(), 8);
    }

    #[test]
    fn clamps_out_of_range_f32() {
        let input = vec![10.0_f32, -10.0];
        let out = downmix_and_resample(&input, 1, 16_000, 16_000);
        assert_eq!(out[0], i16::MAX);
        assert_eq!(out[1], -i16::MAX);
    }
}

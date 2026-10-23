//! Fakes for every interface so unit tests need neither microphone nor model.

use std::sync::{Arc, Mutex};

use promptloom_core::{
    AudioBuffer, AudioSource, DeliveryOutcome, OutputSink, PromptLoomError, TargetWindow,
    TextTransformer, Transcriber,
};

#[derive(Default)]
pub struct FakeAudioSource {
    pub next_buffer: Option<AudioBuffer>,
    pub next_error: Option<PromptLoomError>,
    pub cancelled: bool,
    pub started: bool,
}

impl AudioSource for FakeAudioSource {
    fn start(&mut self) -> Result<(), PromptLoomError> {
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        self.started = true;
        Ok(())
    }
    fn stop(&mut self) -> Result<AudioBuffer, PromptLoomError> {
        if let Some(e) = self.next_error.take() {
            return Err(e);
        }
        self.next_buffer
            .take()
            .ok_or(PromptLoomError::AudioUnavailable)
    }
    fn cancel(&mut self) {
        self.cancelled = true;
        self.next_buffer = None;
    }
}

#[derive(Default)]
pub struct FakeTranscriber {
    pub next: Option<Result<String, PromptLoomError>>,
    pub last_audio_duration_ms: Option<u128>,
}

impl Transcriber for FakeTranscriber {
    fn transcribe(&mut self, audio: &AudioBuffer) -> Result<String, PromptLoomError> {
        self.last_audio_duration_ms = Some(audio.duration().as_millis());
        self.next
            .take()
            .unwrap_or(Err(PromptLoomError::TranscriptionFailed))
    }
}

#[derive(Default)]
pub struct FakeTransformer {
    pub prefix: String,
    pub suffix: String,
}

impl TextTransformer for FakeTransformer {
    fn transform(&self, text: &str) -> String {
        format!("{}{}{}", self.prefix, text, self.suffix)
    }
}

#[derive(Default, Clone)]
pub struct FakeOutputSink {
    pub log: Arc<Mutex<Vec<(String, TargetWindow)>>>,
    pub outcome: Option<DeliveryOutcome>,
}

impl OutputSink for FakeOutputSink {
    fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome {
        self.log
            .lock()
            .unwrap()
            .push((text.to_string(), target.clone()));
        self.outcome.clone().unwrap_or(DeliveryOutcome::Pasted)
    }
}

pub fn fixture_target() -> TargetWindow {
    TargetWindow {
        hwnd: 1,
        process_id: 100,
        executable: "notepad.exe".into(),
    }
}

pub fn fixture_audio(ms: u64) -> AudioBuffer {
    let samples = vec![100i16; (16 * ms) as usize];
    AudioBuffer {
        sample_rate_hz: 16_000,
        samples,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_audio_returns_buffer_once() {
        let mut src = FakeAudioSource {
            next_buffer: Some(fixture_audio(500)),
            ..Default::default()
        };
        assert!(src.start().is_ok());
        let buf = src.stop().unwrap();
        assert_eq!(buf.sample_rate_hz, 16_000);
    }

    #[test]
    fn fake_transcriber_delivers_text() {
        let mut t = FakeTranscriber {
            next: Some(Ok("hello".into())),
            last_audio_duration_ms: None,
        };
        assert_eq!(t.transcribe(&fixture_audio(500)).unwrap(), "hello");
        assert_eq!(t.last_audio_duration_ms, Some(500));
    }

    #[test]
    fn fake_sink_logs_delivery() {
        let mut sink = FakeOutputSink::default();
        let outcome = sink.deliver("hi", &fixture_target());
        assert_eq!(outcome, DeliveryOutcome::Pasted);
        assert_eq!(sink.log.lock().unwrap().len(), 1);
    }
}

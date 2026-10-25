//! Orchestrator — drives [`Session`] against a concrete set of subsystems.
//!
//! Keeps the state machine's step/effect purity but hides the glue that
//! consumes effects: calling `start`/`stop` on the audio source, invoking
//! the transcriber, applying the text transformer, and dispatching to the
//! output sink. All of this stays on one thread; the orchestrator is
//! `!Sync` and callers must own it from a single executor.

use crate::{
    AudioBuffer, AudioSource, Effect, Event, Notice, OutputSink, PromptLoomError, Session, State,
    TargetWindow, TextTransformer, Transcriber, MIN_UTTERANCE,
};

pub struct Orchestrator<A, T, V, O>
where
    A: AudioSource,
    T: Transcriber,
    V: TextTransformer,
    O: OutputSink,
{
    session: Session,
    audio: A,
    transcriber: T,
    transformer: V,
    sink: O,
    /// Last recorded audio buffer, held between Finalizing and Transcribing.
    buffer: Option<AudioBuffer>,
    /// User-visible notices produced during the last `handle` call.
    notices: Vec<Notice>,
}

impl<A, T, V, O> Orchestrator<A, T, V, O>
where
    A: AudioSource,
    T: Transcriber,
    V: TextTransformer,
    O: OutputSink,
{
    pub fn new(audio: A, transcriber: T, transformer: V, sink: O) -> Self {
        Self {
            session: Session::new(),
            audio,
            transcriber,
            transformer,
            sink,
            buffer: None,
            notices: Vec::new(),
        }
    }

    pub fn state(&self) -> &State {
        self.session.state()
    }

    /// Drain user-visible notices produced since the last call.
    pub fn take_notices(&mut self) -> Vec<Notice> {
        std::mem::take(&mut self.notices)
    }

    pub fn press(&mut self, target: TargetWindow) {
        self.handle(Event::HotkeyPressed { target });
    }

    pub fn release(&mut self) {
        self.handle(Event::HotkeyReleased);
    }

    pub fn cancel(&mut self) {
        self.handle(Event::EscapePressed);
    }

    pub fn fail(&mut self, err: PromptLoomError) {
        self.handle(Event::Error(err));
    }

    /// Feed one event, run every effect it produces, and drive the follow-up
    /// events (StopCapture → AudioFinalized → Transcribe → …) in-line.
    pub fn handle(&mut self, event: Event) {
        let mut queue: Vec<Event> = vec![event];
        while let Some(evt) = queue.pop() {
            let effects = self.session.step(evt);
            for effect in effects {
                self.apply(effect, &mut queue);
            }
        }
    }

    fn apply(&mut self, effect: Effect, queue: &mut Vec<Event>) {
        match effect {
            Effect::StartCapture => {
                if let Err(err) = self.audio.start() {
                    queue.push(Event::Error(err));
                }
            }
            Effect::StopCapture => match self.audio.stop() {
                Ok(buffer) => {
                    if buffer.is_too_short(MIN_UTTERANCE) {
                        queue.push(Event::CaptureInvalid);
                    } else {
                        self.buffer = Some(buffer);
                        queue.push(Event::AudioFinalized);
                    }
                }
                Err(err) => queue.push(Event::Error(err)),
            },
            Effect::DiscardBuffer => {
                self.audio.cancel();
                self.buffer = None;
            }
            Effect::Transcribe => {
                let buffer = match self.buffer.take() {
                    Some(b) => b,
                    None => {
                        queue.push(Event::Error(PromptLoomError::TranscriptionFailed));
                        return;
                    }
                };
                match self.transcriber.transcribe(&buffer) {
                    Ok(text) => queue.push(Event::TranscriptReady(text)),
                    Err(err) => queue.push(Event::Error(err)),
                }
            }
            Effect::Correct(raw) => {
                let corrected = self.transformer.transform(&raw);
                queue.push(Event::CorrectionReady(corrected));
            }
            Effect::Deliver { target, text } => {
                let outcome = self.sink.deliver(&text, &target);
                queue.push(Event::DeliveryDone(outcome));
            }
            Effect::Notify(notice) => {
                self.notices.push(notice);
            }
        }
    }

    /// For diagnostics: how many samples the last recorded buffer held.
    pub fn last_buffer_samples(&self) -> Option<usize> {
        self.buffer.as_ref().map(|b| b.samples.len())
    }

    /// Expose the underlying subsystems for test introspection.
    pub fn components(&mut self) -> (&mut A, &mut T, &mut V, &mut O) {
        (
            &mut self.audio,
            &mut self.transcriber,
            &mut self.transformer,
            &mut self.sink,
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::DeliveryOutcome;
    use std::sync::{Arc, Mutex};

    /// Minimal fakes local to this module so the orchestrator can be exercised
    /// without pulling in `promptloom-test-support` (which would be a cycle).
    struct FakeAudio {
        buffer: Option<AudioBuffer>,
        start_err: Option<PromptLoomError>,
        cancelled: bool,
    }
    impl AudioSource for FakeAudio {
        fn start(&mut self) -> Result<(), PromptLoomError> {
            if let Some(e) = self.start_err.take() {
                return Err(e);
            }
            Ok(())
        }
        fn stop(&mut self) -> Result<AudioBuffer, PromptLoomError> {
            self.buffer.take().ok_or(PromptLoomError::AudioUnavailable)
        }
        fn cancel(&mut self) {
            self.cancelled = true;
            self.buffer = None;
        }
    }

    struct FakeTx {
        out: Result<String, PromptLoomError>,
    }
    impl Transcriber for FakeTx {
        fn transcribe(&mut self, _: &AudioBuffer) -> Result<String, PromptLoomError> {
            self.out.clone()
        }
    }

    struct FakeTransform(&'static str);
    impl TextTransformer for FakeTransform {
        fn transform(&self, s: &str) -> String {
            format!("{}{}", self.0, s)
        }
    }

    #[derive(Clone, Default)]
    struct FakeSink {
        delivered: Arc<Mutex<Vec<(String, TargetWindow)>>>,
        outcome: Option<DeliveryOutcome>,
    }
    impl OutputSink for FakeSink {
        fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome {
            self.delivered
                .lock()
                .unwrap()
                .push((text.to_string(), target.clone()));
            self.outcome.clone().unwrap_or(DeliveryOutcome::Pasted)
        }
    }

    fn target() -> TargetWindow {
        TargetWindow {
            hwnd: 1,
            process_id: 1,
            executable: "test".into(),
        }
    }

    fn audio_ms(ms: u64) -> AudioBuffer {
        AudioBuffer {
            sample_rate_hz: 16_000,
            samples: vec![100i16; (16 * ms) as usize],
        }
    }

    #[test]
    fn happy_path_drives_full_pipeline() {
        let mut orch = Orchestrator::new(
            FakeAudio {
                buffer: Some(audio_ms(500)),
                start_err: None,
                cancelled: false,
            },
            FakeTx {
                out: Ok("raw".into()),
            },
            FakeTransform("X:"),
            FakeSink::default(),
        );

        orch.press(target());
        orch.release();

        assert_eq!(orch.state(), &State::Idle);
        let (_, _, _, sink) = orch.components();
        let delivered = sink.delivered.lock().unwrap().clone();
        assert_eq!(delivered.len(), 1);
        assert_eq!(delivered[0].0, "X:raw");
    }

    #[test]
    fn too_short_capture_rejects_and_discards() {
        let mut orch = Orchestrator::new(
            FakeAudio {
                // 100 ms is below the 250 ms MIN_UTTERANCE
                buffer: Some(audio_ms(100)),
                start_err: None,
                cancelled: false,
            },
            FakeTx {
                out: Ok("ghost".into()),
            },
            FakeTransform(""),
            FakeSink::default(),
        );

        orch.press(target());
        orch.release();

        assert_eq!(orch.state(), &State::Idle);
        let notices = orch.take_notices();
        assert!(notices.contains(&Notice::TooShort));
        let (_, _, _, sink) = orch.components();
        assert!(sink.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn escape_mid_recording_cancels_and_clears_audio() {
        let mut orch = Orchestrator::new(
            FakeAudio {
                buffer: Some(audio_ms(500)),
                start_err: None,
                cancelled: false,
            },
            FakeTx {
                out: Ok("x".into()),
            },
            FakeTransform(""),
            FakeSink::default(),
        );

        orch.press(target());
        orch.cancel();

        assert_eq!(orch.state(), &State::Idle);
        let (a, _, _, sink) = orch.components();
        assert!(a.cancelled);
        assert!(sink.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn transcription_failure_returns_to_idle_without_delivery() {
        let mut orch = Orchestrator::new(
            FakeAudio {
                buffer: Some(audio_ms(500)),
                start_err: None,
                cancelled: false,
            },
            FakeTx {
                out: Err(PromptLoomError::TranscriptionFailed),
            },
            FakeTransform(""),
            FakeSink::default(),
        );

        orch.press(target());
        orch.release();

        assert_eq!(orch.state(), &State::Idle);
        let (_, _, _, sink) = orch.components();
        assert!(sink.delivered.lock().unwrap().is_empty());
    }

    #[test]
    fn audio_unavailable_at_start_surfaces_notice() {
        let mut orch = Orchestrator::new(
            FakeAudio {
                buffer: None,
                start_err: Some(PromptLoomError::AudioUnavailable),
                cancelled: false,
            },
            FakeTx { out: Ok("".into()) },
            FakeTransform(""),
            FakeSink::default(),
        );
        orch.press(target());
        assert_eq!(orch.state(), &State::Idle);
        let notices = orch.take_notices();
        assert_eq!(notices.len(), 1);
        assert!(matches!(notices[0], Notice::Error(_)));
    }
}

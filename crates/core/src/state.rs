//! Session state machine.
//!
//! Transitions permitted by the specification:
//!   Idle -> Recording -> Finalizing -> Transcribing -> Correcting -> Delivering -> Idle
//!
//! Escape, hotkey-release after an invalid capture, device loss, or any error
//! returns the session to Idle after clearing the audio buffer. Partial or
//! cancelled text is never delivered.

use crate::{DeliveryOutcome, PromptLoomError, TargetWindow};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum State {
    Idle,
    Recording { target: TargetWindow },
    Finalizing { target: TargetWindow },
    Transcribing { target: TargetWindow },
    Correcting { target: TargetWindow, raw: String },
    Delivering { target: TargetWindow, text: String },
}

/// Inputs the state machine reacts to. Pure data; no I/O.
#[derive(Debug, Clone)]
pub enum Event {
    HotkeyPressed { target: TargetWindow },
    HotkeyReleased,
    EscapePressed,
    CaptureInvalid,
    AudioFinalized,
    TranscriptReady(String),
    CorrectionReady(String),
    DeliveryDone(DeliveryOutcome),
    Error(PromptLoomError),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Effect {
    /// Begin audio capture.
    StartCapture,
    /// Stop capture and surface the recorded buffer.
    StopCapture,
    /// Discard any buffered samples and ensure nothing persists.
    DiscardBuffer,
    /// Run the transcriber over the finalized buffer.
    Transcribe,
    /// Apply vocabulary corrections to raw transcript.
    Correct(String),
    /// Deliver the final text through the output sink.
    Deliver { target: TargetWindow, text: String },
    /// User-visible notice.
    Notify(Notice),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Cancelled,
    TooShort,
    Delivered(DeliveryOutcome),
    Error(ErrorKind),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorKind {
    NoMicrophone,
    ModelMissing,
    TranscriptionFailed,
    TargetUnavailable,
    DeliveryFailed,
}

impl From<&PromptLoomError> for ErrorKind {
    fn from(e: &PromptLoomError) -> Self {
        match e {
            PromptLoomError::AudioUnavailable => ErrorKind::NoMicrophone,
            PromptLoomError::ModelUnavailable => ErrorKind::ModelMissing,
            PromptLoomError::TranscriptionFailed => ErrorKind::TranscriptionFailed,
            PromptLoomError::TargetUnavailable => ErrorKind::TargetUnavailable,
            PromptLoomError::DeliveryFailed => ErrorKind::DeliveryFailed,
            PromptLoomError::AudioTooShort | PromptLoomError::Cancelled => {
                ErrorKind::TranscriptionFailed
            }
        }
    }
}

pub struct Session {
    state: State,
}

impl Session {
    pub fn new() -> Self {
        Self { state: State::Idle }
    }

    pub fn state(&self) -> &State {
        &self.state
    }

    /// Advance the machine by one event, returning any effects the caller
    /// must execute. All error paths return to Idle after discarding buffer.
    pub fn step(&mut self, event: Event) -> Vec<Effect> {
        use Event::*;
        use State::*;

        match (&self.state, event) {
            // Idle -> Recording
            (Idle, HotkeyPressed { target }) => {
                self.state = Recording { target };
                vec![Effect::StartCapture]
            }

            // Recording -> Finalizing on release
            (Recording { target }, HotkeyReleased) => {
                let target = target.clone();
                self.state = Finalizing { target };
                vec![Effect::StopCapture]
            }

            // Cancellation paths from any active state
            (Recording { .. } | Finalizing { .. } | Transcribing { .. }, EscapePressed) => {
                self.state = Idle;
                vec![Effect::DiscardBuffer, Effect::Notify(Notice::Cancelled)]
            }

            // Hotkey release after an invalid capture (too short, bad device)
            (Recording { .. }, CaptureInvalid) => {
                self.state = Idle;
                vec![Effect::DiscardBuffer, Effect::Notify(Notice::TooShort)]
            }

            // Finalizing -> Transcribing
            (Finalizing { target }, AudioFinalized) => {
                let target = target.clone();
                self.state = Transcribing { target };
                vec![Effect::Transcribe]
            }

            // Transcribing -> Correcting
            (Transcribing { target }, TranscriptReady(raw)) => {
                let target = target.clone();
                let raw_for_effect = raw.clone();
                self.state = Correcting { target, raw };
                vec![Effect::Correct(raw_for_effect)]
            }

            // Correcting -> Delivering
            (Correcting { target, .. }, CorrectionReady(text)) => {
                let target = target.clone();
                let text_for_effect = text.clone();
                self.state = Delivering {
                    target: target.clone(),
                    text,
                };
                vec![Effect::Deliver {
                    target,
                    text: text_for_effect,
                }]
            }

            // Delivering -> Idle
            (Delivering { .. }, DeliveryDone(outcome)) => {
                self.state = Idle;
                vec![Effect::Notify(Notice::Delivered(outcome))]
            }

            // Any Error returns to Idle after discarding
            (_, Error(err)) => {
                let kind = ErrorKind::from(&err);
                self.state = Idle;
                vec![Effect::DiscardBuffer, Effect::Notify(Notice::Error(kind))]
            }

            // Everything else is ignored (defensive — no partial delivery).
            _ => Vec::new(),
        }
    }
}

impl Default for Session {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn target() -> TargetWindow {
        TargetWindow {
            hwnd: 0x1234,
            process_id: 42,
            executable: "notepad.exe".into(),
        }
    }

    fn drive(session: &mut Session, events: Vec<Event>) -> Vec<Effect> {
        let mut out = Vec::new();
        for e in events {
            out.extend(session.step(e));
        }
        out
    }

    #[test]
    fn happy_path_idle_to_delivered() {
        let mut s = Session::new();
        let effects = drive(
            &mut s,
            vec![
                Event::HotkeyPressed { target: target() },
                Event::HotkeyReleased,
                Event::AudioFinalized,
                Event::TranscriptReady("hello world".into()),
                Event::CorrectionReady("Hello world".into()),
                Event::DeliveryDone(DeliveryOutcome::Pasted),
            ],
        );
        assert_eq!(s.state(), &State::Idle);
        assert!(matches!(effects[0], Effect::StartCapture));
        assert!(matches!(effects[1], Effect::StopCapture));
        assert!(matches!(effects[2], Effect::Transcribe));
        assert!(matches!(effects[3], Effect::Correct(ref s) if s == "hello world"));
        assert!(matches!(
            &effects[4],
            Effect::Deliver { text, .. } if text == "Hello world"
        ));
        assert!(matches!(
            effects[5],
            Effect::Notify(Notice::Delivered(DeliveryOutcome::Pasted))
        ));
    }

    #[test]
    fn escape_during_recording_cancels_and_discards() {
        let mut s = Session::new();
        s.step(Event::HotkeyPressed { target: target() });
        let effects = s.step(Event::EscapePressed);
        assert_eq!(s.state(), &State::Idle);
        assert_eq!(effects[0], Effect::DiscardBuffer);
        assert_eq!(effects[1], Effect::Notify(Notice::Cancelled));
    }

    #[test]
    fn escape_during_transcribing_cancels() {
        let mut s = Session::new();
        s.step(Event::HotkeyPressed { target: target() });
        s.step(Event::HotkeyReleased);
        s.step(Event::AudioFinalized);
        assert!(matches!(s.state(), State::Transcribing { .. }));
        let effects = s.step(Event::EscapePressed);
        assert_eq!(s.state(), &State::Idle);
        assert_eq!(effects[0], Effect::DiscardBuffer);
    }

    #[test]
    fn capture_invalid_returns_to_idle() {
        let mut s = Session::new();
        s.step(Event::HotkeyPressed { target: target() });
        let effects = s.step(Event::CaptureInvalid);
        assert_eq!(s.state(), &State::Idle);
        assert_eq!(effects[1], Effect::Notify(Notice::TooShort));
    }

    #[test]
    fn error_from_any_state_returns_to_idle_and_discards() {
        for setup in [
            vec![Event::HotkeyPressed { target: target() }],
            vec![
                Event::HotkeyPressed { target: target() },
                Event::HotkeyReleased,
            ],
            vec![
                Event::HotkeyPressed { target: target() },
                Event::HotkeyReleased,
                Event::AudioFinalized,
            ],
        ] {
            let mut s = Session::new();
            for e in setup {
                s.step(e);
            }
            let effects = s.step(Event::Error(PromptLoomError::AudioUnavailable));
            assert_eq!(s.state(), &State::Idle);
            assert_eq!(effects[0], Effect::DiscardBuffer);
            assert!(matches!(effects[1], Effect::Notify(Notice::Error(_))));
        }
    }

    #[test]
    fn unknown_transitions_are_ignored() {
        let mut s = Session::new();
        // AudioFinalized while Idle should do nothing
        let effects = s.step(Event::AudioFinalized);
        assert!(effects.is_empty());
        assert_eq!(s.state(), &State::Idle);
    }

    #[test]
    fn cancelled_text_never_delivered() {
        let mut s = Session::new();
        s.step(Event::HotkeyPressed { target: target() });
        s.step(Event::HotkeyReleased);
        s.step(Event::AudioFinalized);
        s.step(Event::EscapePressed);
        // Any further transcript event must be ignored.
        let effects = s.step(Event::TranscriptReady("ghost".into()));
        assert!(effects.is_empty());
        assert_eq!(s.state(), &State::Idle);
    }
}

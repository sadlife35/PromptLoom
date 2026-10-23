//! promptloom-core: session state machine and orchestration contracts.
//!
//! This crate owns the authoritative session state (see [`state`]) and the
//! narrowly-typed interfaces every subsystem must implement:
//! [`AudioSource`], [`Transcriber`], [`TextTransformer`], [`OutputSink`].
//!
//! No audio, model, clipboard, or OS code lives here; subsystems plug in
//! through these interfaces so unit tests need neither microphone nor model.

pub mod orchestrator;
pub mod state;

use std::time::Duration;

use thiserror::Error;

pub use orchestrator::Orchestrator;
pub use state::{Effect, ErrorKind, Event, Notice, Session, State};

/// Audio samples flowing through the pipeline: mono PCM16 at 16 kHz.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioBuffer {
    pub sample_rate_hz: u32,
    pub samples: Vec<i16>,
}

impl AudioBuffer {
    pub fn duration(&self) -> Duration {
        if self.sample_rate_hz == 0 {
            return Duration::ZERO;
        }
        let secs = self.samples.len() as f64 / self.sample_rate_hz as f64;
        Duration::from_secs_f64(secs)
    }

    pub fn is_too_short(&self, min: Duration) -> bool {
        self.duration() < min
    }
}

/// Identity of the target window captured when recording started.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TargetWindow {
    pub hwnd: u64,
    pub process_id: u32,
    pub executable: String,
}

/// Outcome of a delivery attempt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DeliveryOutcome {
    /// Text was pasted into the originally captured target.
    Pasted,
    /// Target changed or was protected; text is on the clipboard only.
    CopyOnly { reason: &'static str },
    /// Delivery failed and clipboard was restored.
    Failed,
}

#[derive(Debug, Clone, Error)]
pub enum PromptLoomError {
    #[error("audio buffer too short")]
    AudioTooShort,
    #[error("audio device unavailable")]
    AudioUnavailable,
    #[error("model unavailable or hash mismatch")]
    ModelUnavailable,
    #[error("transcription failed")]
    TranscriptionFailed,
    #[error("target window unavailable")]
    TargetUnavailable,
    #[error("delivery failed")]
    DeliveryFailed,
    #[error("cancelled")]
    Cancelled,
}

/// Owns microphone capture. Implementations never block the OS audio thread
/// and never write audio to disk.
pub trait AudioSource: Send {
    fn start(&mut self) -> Result<(), PromptLoomError>;
    fn stop(&mut self) -> Result<AudioBuffer, PromptLoomError>;
    fn cancel(&mut self);
}

/// Converts an [`AudioBuffer`] into transcript text. No other component sees
/// raw samples.
pub trait Transcriber: Send {
    fn transcribe(&mut self, audio: &AudioBuffer) -> Result<String, PromptLoomError>;
}

/// Applies deterministic text corrections (project vocabulary).
pub trait TextTransformer: Send {
    fn transform(&self, text: &str) -> String;
}

/// Delivers the transformed text to the user's captured target window.
pub trait OutputSink: Send {
    fn deliver(&mut self, text: &str, target: &TargetWindow) -> DeliveryOutcome;
}

/// Minimum utterance length treated as a valid dictation. Shorter buffers are
/// rejected and no transcription is attempted.
pub const MIN_UTTERANCE: Duration = Duration::from_millis(250);

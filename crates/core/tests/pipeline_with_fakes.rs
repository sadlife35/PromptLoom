//! Contract test: fake Transcriber, fake OutputSink, and the vocabulary
//! TextTransformer wire into the Session's effects correctly.
//!
//! Verifies the specification's privacy constraint: no component outside the
//! ASR sees raw audio samples. The fake sink only ever receives post-
//! correction text, and only after the state machine says `Delivering`.

use promptloom_core::{
    DeliveryOutcome, Effect, Event, Notice, PromptLoomError, Session, State, TargetWindow,
};

fn target() -> TargetWindow {
    TargetWindow {
        hwnd: 7,
        process_id: 77,
        executable: "notepad.exe".into(),
    }
}

#[test]
fn full_pipeline_delivers_corrected_text() {
    let mut s = Session::new();

    let effects = s.step(Event::HotkeyPressed { target: target() });
    assert_eq!(effects, vec![Effect::StartCapture]);
    assert!(matches!(s.state(), State::Recording { .. }));

    let effects = s.step(Event::HotkeyReleased);
    assert_eq!(effects, vec![Effect::StopCapture]);

    let effects = s.step(Event::AudioFinalized);
    assert_eq!(effects, vec![Effect::Transcribe]);

    // Simulate ASR returning raw text.
    let raw = "deploy prompt loom to super base".to_string();
    let effects = s.step(Event::TranscriptReady(raw.clone()));
    assert_eq!(effects, vec![Effect::Correct(raw)]);

    // Simulate vocabulary correction stage.
    let corrected = "deploy PromptLoom to Supabase".to_string();
    let effects = s.step(Event::CorrectionReady(corrected.clone()));
    assert_eq!(effects.len(), 1);
    match &effects[0] {
        Effect::Deliver { text, target } => {
            assert_eq!(text, &corrected);
            assert_eq!(target.hwnd, 7);
        }
        other => panic!("expected Deliver, got {other:?}"),
    }

    // Delivery completion returns to Idle.
    let effects = s.step(Event::DeliveryDone(DeliveryOutcome::Pasted));
    assert_eq!(
        effects,
        vec![Effect::Notify(Notice::Delivered(DeliveryOutcome::Pasted))]
    );
    assert_eq!(s.state(), &State::Idle);
}

#[test]
fn error_in_transcription_returns_to_idle_without_delivering() {
    let mut s = Session::new();
    s.step(Event::HotkeyPressed { target: target() });
    s.step(Event::HotkeyReleased);
    s.step(Event::AudioFinalized);

    let effects = s.step(Event::Error(PromptLoomError::TranscriptionFailed));
    assert_eq!(s.state(), &State::Idle);
    assert_eq!(effects[0], Effect::DiscardBuffer);
    assert!(matches!(&effects[1], Effect::Notify(Notice::Error(_))));
}

#[test]
fn escape_from_finalizing_cancels() {
    let mut s = Session::new();
    s.step(Event::HotkeyPressed { target: target() });
    s.step(Event::HotkeyReleased);
    // In Finalizing (StopCapture emitted, AudioFinalized not yet).
    let effects = s.step(Event::EscapePressed);
    assert_eq!(s.state(), &State::Idle);
    assert_eq!(effects[0], Effect::DiscardBuffer);
    assert!(matches!(&effects[1], Effect::Notify(Notice::Cancelled)));
}

#[test]
fn error_in_delivering_returns_to_idle() {
    let mut s = Session::new();
    s.step(Event::HotkeyPressed { target: target() });
    s.step(Event::HotkeyReleased);
    s.step(Event::AudioFinalized);
    s.step(Event::TranscriptReady("x".into()));
    s.step(Event::CorrectionReady("x".into()));
    let effects = s.step(Event::Error(PromptLoomError::DeliveryFailed));
    assert_eq!(s.state(), &State::Idle);
    assert_eq!(effects[0], Effect::DiscardBuffer);
}

#[test]
fn target_window_identity_flows_through_pipeline() {
    let mut s = Session::new();
    let t = TargetWindow {
        hwnd: 0xDEADBEEF,
        process_id: 1234,
        executable: "code.exe".into(),
    };
    s.step(Event::HotkeyPressed { target: t.clone() });
    s.step(Event::HotkeyReleased);
    s.step(Event::AudioFinalized);
    s.step(Event::TranscriptReady("x".into()));
    let effects = s.step(Event::CorrectionReady("x".into()));
    match &effects[0] {
        Effect::Deliver { target, .. } => {
            assert_eq!(target, &t);
        }
        _ => panic!("expected Deliver"),
    }
}

//! Long-form meeting transcription.
//!
//! Captures from the **microphone only** (not system audio — that requires
//! WASAPI loopback and a separate code path). Chunks the capture into
//! ~15-second windows, transcribes each via the existing `WhisperTranscriber`,
//! and appends the resulting text as a line to a markdown file at
//! `<app_data>/meetings/<timestamp>.md`.
//!
//! Limitations worth being honest about:
//! - mic only: only captures what the mic hears (your side of the meeting,
//!   plus anything the mic picks up through speakers)
//! - no diarization: one undifferentiated stream of text
//! - no timestamps per line (yet — would be easy to add)
//!
//! On the balance it's still useful: a scratch transcript of your own
//! remarks + a one-shot LLM summary at the end.
//!
//! The capture thread owns its own `WasapiAudioSource` and
//! `WhisperTranscriber`. It does NOT share with the hold-to-talk
//! orchestrator; starting the notetaker loads a second copy of the whisper
//! model into memory (~500 MB for small.en). This keeps the state machines
//! independent and lets the user dictate while a meeting records.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant, SystemTime};

use parking_lot::Mutex;
use promptloom_asr::WhisperTranscriber;
use promptloom_audio::WasapiAudioSource;
use promptloom_core::AudioSource;
use serde::Serialize;
use tauri::{AppHandle, Emitter};

/// How often to flush accumulated audio to whisper. Longer = fewer breaks
/// mid-sentence but more delay before text appears. 15s is a decent middle.
const CHUNK_SECS: u64 = 15;

/// Hard cap on a single session (2 hours). Catches runaway sessions.
const MAX_SESSION_SECS: u64 = 2 * 60 * 60;

#[derive(Default)]
pub struct Notetaker {
    inner: Arc<Mutex<Option<ActiveSession>>>,
}

struct ActiveSession {
    stop: Arc<AtomicBool>,
    started_at: Instant,
    path: PathBuf,
    /// Running character count of the transcript on disk. For UI progress.
    chars_written: Arc<Mutex<u64>>,
    handle: Option<std::thread::JoinHandle<()>>,
}

#[derive(Serialize, Clone)]
pub struct NotetakerStatus {
    pub recording: bool,
    pub elapsed_secs: u64,
    pub path: Option<String>,
    pub chars_written: u64,
}

#[derive(Serialize, Clone)]
pub struct MeetingSummary {
    pub path: String,
    pub name: String,
    pub bytes: u64,
    pub modified_secs: u64,
}

impl Notetaker {
    pub fn status(&self) -> NotetakerStatus {
        let guard = self.inner.lock();
        match guard.as_ref() {
            Some(s) => NotetakerStatus {
                recording: true,
                elapsed_secs: s.started_at.elapsed().as_secs(),
                path: Some(s.path.to_string_lossy().into_owned()),
                chars_written: *s.chars_written.lock(),
            },
            None => NotetakerStatus {
                recording: false,
                elapsed_secs: 0,
                path: None,
                chars_written: 0,
            },
        }
    }

    pub fn start(
        &self,
        app: AppHandle,
        model_path: PathBuf,
        data_dir: PathBuf,
        mic_device: Option<String>,
    ) -> Result<PathBuf, String> {
        {
            let guard = self.inner.lock();
            if guard.is_some() {
                return Err("already recording".into());
            }
        }

        let meetings_dir = data_dir.join("meetings");
        std::fs::create_dir_all(&meetings_dir).map_err(|e| format!("create meetings dir: {e}"))?;
        let filename = timestamp_filename();
        let md_path = meetings_dir.join(format!("{filename}.md"));

        {
            let mut f =
                std::fs::File::create(&md_path).map_err(|e| format!("create meeting file: {e}"))?;
            let header = format!(
                "# Meeting — {}\n\n_Recorded with PromptLoom. Mic only; no system audio._\n\n",
                filename
            );
            f.write_all(header.as_bytes())
                .map_err(|e| format!("write header: {e}"))?;
        }

        let stop = Arc::new(AtomicBool::new(false));
        let chars_written = Arc::new(Mutex::new(0u64));

        let stop_thread = stop.clone();
        let chars_thread = chars_written.clone();
        let md_path_thread = md_path.clone();
        let app_thread = app.clone();

        let handle = std::thread::Builder::new()
            .name("promptloom-notetaker".into())
            .spawn(move || {
                run_session(
                    app_thread,
                    stop_thread,
                    chars_thread,
                    md_path_thread,
                    model_path,
                    mic_device,
                );
            })
            .map_err(|e| format!("spawn thread: {e}"))?;

        *self.inner.lock() = Some(ActiveSession {
            stop,
            started_at: Instant::now(),
            path: md_path.clone(),
            chars_written,
            handle: Some(handle),
        });

        let _ = app.emit("promptloom:notetaker-state", "recording");
        Ok(md_path)
    }

    pub fn stop(&self, app: &AppHandle) -> Result<Option<PathBuf>, String> {
        let mut session = match self.inner.lock().take() {
            Some(s) => s,
            None => return Ok(None),
        };
        session.stop.store(true, Ordering::SeqCst);
        if let Some(h) = session.handle.take() {
            let _ = h.join();
        }
        if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(&session.path) {
            let _ = writeln!(f, "\n\n_(ended)_");
        }
        let _ = app.emit("promptloom:notetaker-state", "idle");
        Ok(Some(session.path))
    }
}

fn run_session(
    app: AppHandle,
    stop: Arc<AtomicBool>,
    chars_written: Arc<Mutex<u64>>,
    md_path: PathBuf,
    model_path: PathBuf,
    mic_device: Option<String>,
) {
    let mut transcriber = match WhisperTranscriber::load(&model_path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(?e, "notetaker: whisper load failed");
            let _ = app.emit(
                "promptloom:notice",
                format!("notetaker: model load failed: {e}"),
            );
            let _ = app.emit("promptloom:notetaker-state", "idle");
            return;
        }
    };

    let session_start = Instant::now();

    while !stop.load(Ordering::SeqCst) {
        if session_start.elapsed().as_secs() > MAX_SESSION_SECS {
            let _ = app.emit(
                "promptloom:notice",
                "notetaker: 2-hour session cap reached — stopped",
            );
            break;
        }

        // One chunk: start audio, wait CHUNK_SECS (or until stop), capture + transcribe.
        let mut source = match WasapiAudioSource::new_with_device(mic_device.as_deref()) {
            Ok(s) => s,
            Err(e) => {
                tracing::warn!(?e, "notetaker: audio unavailable");
                let _ = app.emit("promptloom:notice", format!("notetaker: {e}"));
                break;
            }
        };
        if let Err(e) = source.start() {
            tracing::warn!(?e, "notetaker: audio start failed");
            let _ = app.emit("promptloom:notice", format!("notetaker: {e}"));
            break;
        }

        let chunk_start = Instant::now();
        while chunk_start.elapsed() < Duration::from_secs(CHUNK_SECS) {
            if stop.load(Ordering::SeqCst) {
                break;
            }
            std::thread::sleep(Duration::from_millis(200));
        }

        let audio = match source.stop() {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!(?e, "notetaker: audio stop failed");
                continue;
            }
        };

        if audio.samples.is_empty() {
            continue;
        }

        // Use the full transcribe path (server-first with CLI fallback) so
        // we still produce text even when the whisper-server sidecar isn't
        // reachable. `transcribe_partial` returns None silently in that
        // case — the Notetaker's biggest early footgun.
        match transcriber.transcribe_with_prompt(&audio, None) {
            Ok(text) if !text.trim().is_empty() => {
                tracing::info!(
                    chars = text.len(),
                    samples = audio.samples.len(),
                    "notetaker: chunk transcribed"
                );
                append_chunk(&md_path, text.trim(), &chars_written);
                let _ = app.emit("promptloom:notetaker-transcript", text.trim());
            }
            Ok(_) => {
                tracing::debug!(
                    samples = audio.samples.len(),
                    "notetaker: chunk empty (likely silence)"
                );
            }
            Err(e) => {
                tracing::warn!(?e, "notetaker: transcription failed for chunk");
            }
        }
    }

    let _ = app.emit("promptloom:notetaker-state", "idle");
}

fn append_chunk(path: &Path, text: &str, chars_written: &Arc<Mutex<u64>>) {
    if let Ok(mut f) = std::fs::OpenOptions::new().append(true).open(path) {
        let line = format!("{text}\n\n");
        if let Err(e) = f.write_all(line.as_bytes()) {
            tracing::warn!(?e, "notetaker: append failed");
            return;
        }
        *chars_written.lock() += line.len() as u64;
    }
}

pub fn list_meetings(data_dir: &Path) -> Vec<MeetingSummary> {
    let meetings_dir = data_dir.join("meetings");
    let Ok(entries) = std::fs::read_dir(&meetings_dir) else {
        return Vec::new();
    };
    let mut out: Vec<MeetingSummary> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.extension().and_then(|s| s.to_str()) != Some("md") {
                return None;
            }
            let meta = std::fs::metadata(&path).ok()?;
            let modified_secs = meta
                .modified()
                .ok()
                .and_then(|m| m.duration_since(SystemTime::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            Some(MeetingSummary {
                name: path
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("meeting")
                    .to_string(),
                path: path.to_string_lossy().into_owned(),
                bytes: meta.len(),
                modified_secs,
            })
        })
        .collect();
    out.sort_by_key(|m| std::cmp::Reverse(m.modified_secs));
    out
}

pub fn read_meeting(path: &Path) -> std::io::Result<String> {
    std::fs::read_to_string(path)
}

fn timestamp_filename() -> String {
    // "YYYYMMDD-HHMMSS" using SystemTime; no chrono dep.
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    // Simple UTC breakdown.
    let days_since_epoch = secs / 86_400;
    let secs_in_day = secs % 86_400;
    let hour = (secs_in_day / 3600) as u32;
    let minute = ((secs_in_day % 3600) / 60) as u32;
    let second = (secs_in_day % 60) as u32;
    let (y, m, d) = civil_from_days(days_since_epoch as i64);
    format!("{y:04}{m:02}{d:02}-{hour:02}{minute:02}{second:02}")
}

/// Converts days-since-1970-01-01 (UTC) into civil (year, month, day).
/// Based on Howard Hinnant's date algorithms.
fn civil_from_days(z: i64) -> (i32, u32, u32) {
    let z = z + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = (z - era * 146_097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let y = (yoe as i64) + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = (if mp < 10 { mp + 3 } else { mp - 9 }) as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y as i32, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn timestamp_filename_is_well_formed() {
        let s = timestamp_filename();
        assert_eq!(s.len(), 15); // 8 + 1 + 6
        assert!(s.chars().nth(8).unwrap() == '-');
        assert!(s[..8].chars().all(|c| c.is_ascii_digit()));
        assert!(s[9..].chars().all(|c| c.is_ascii_digit()));
    }

    #[test]
    fn civil_from_days_sanity() {
        // 2026-11-10 UTC is day 20_767 since 1970-01-01.
        let (y, m, d) = civil_from_days(20_767);
        assert_eq!((y, m, d), (2026, 11, 10));
    }
}

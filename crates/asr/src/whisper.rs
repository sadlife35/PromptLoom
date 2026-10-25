//! whisper.cpp sidecar transcriber.
//!
//! Shells out to a prebuilt `whisper-cli.exe` from the official whisper.cpp
//! release. One `WhisperTranscriber` instance pins the binary + model paths
//! and spawns a child per `transcribe()` call. The model file stays memory-
//! mapped by the OS across successive calls, so the second dictation hits
//! warm page cache (that is what the spec's "stays warm" test asserts).
//!
//! Privacy: the AudioBuffer is written to a scoped `tempfile::NamedTempFile`
//! inside the user's temp dir and deleted when `transcribe` returns — even
//! on panic (RAII drop). Nothing persists after the call.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use hound::{SampleFormat, WavSpec, WavWriter};
use promptloom_core::{AudioBuffer, PromptLoomError, Transcriber};
#[derive(Debug, thiserror::Error)]
pub enum WhisperError {
    #[error("whisper binary not found at {0:?}")]
    BinaryNotFound(PathBuf),
    #[error("whisper model missing at {0:?}")]
    ModelMissing(PathBuf),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("whisper exited non-zero (code {code:?}): {stderr}")]
    NonZero { code: Option<i32>, stderr: String },
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("wav write: {0}")]
    Wav(String),
    #[error("audio sample rate must be 16000 Hz, got {0}")]
    WrongSampleRate(u32),
}

impl From<WhisperError> for PromptLoomError {
    fn from(e: WhisperError) -> Self {
        tracing::warn!(?e, "whisper error");
        match e {
            WhisperError::BinaryNotFound(_) | WhisperError::ModelMissing(_) => {
                PromptLoomError::ModelUnavailable
            }
            _ => PromptLoomError::TranscriptionFailed,
        }
    }
}

/// Loaded whisper sidecar. Keeps the binary + model paths and a scratch
/// directory. The child process is spawned per-call, not held.
pub struct WhisperTranscriber {
    binary_path: PathBuf,
    model_path: PathBuf,
    scratch_dir: PathBuf,
    language: Option<String>,
    threads: Option<u32>,
}

impl WhisperTranscriber {
    /// Load the sidecar. Verifies both the model and the binary exist on
    /// disk; the binary path is resolved from (in order):
    ///   1. `PROMPTLOOM_WHISPER_BIN` env var
    ///   2. Sibling of `model_path`: `<dir>/whisper-cli.exe`
    ///   3. `<dir>/main.exe` (older release naming)
    pub fn load(model_path: &Path) -> Result<Self, WhisperError> {
        if !model_path.exists() {
            return Err(WhisperError::ModelMissing(model_path.into()));
        }
        let binary_path = resolve_binary(model_path)
            .ok_or_else(|| WhisperError::BinaryNotFound(model_path.into()))?;

        let scratch_dir = std::env::temp_dir().join("promptloom-scratch");
        std::fs::create_dir_all(&scratch_dir)?;

        Ok(Self {
            binary_path,
            model_path: model_path.to_path_buf(),
            scratch_dir,
            language: Some("en".into()),
            threads: None,
        })
    }

    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }
    pub fn model_path(&self) -> &Path {
        &self.model_path
    }
    pub fn set_language(&mut self, lang: Option<String>) {
        self.language = lang;
    }
    pub fn set_threads(&mut self, n: Option<u32>) {
        self.threads = n;
    }
}

fn resolve_binary(model_path: &Path) -> Option<PathBuf> {
    if let Ok(env) = std::env::var("PROMPTLOOM_WHISPER_BIN") {
        let p = PathBuf::from(env);
        if p.exists() {
            return Some(p);
        }
    }
    let parent = model_path.parent()?;
    for name in &["whisper-cli.exe", "main.exe", "whisper-cli", "main"] {
        let p = parent.join(name);
        if p.exists() {
            return Some(p);
        }
    }
    // Also try sibling `bin/` directory.
    for name in &["whisper-cli.exe", "main.exe", "whisper-cli", "main"] {
        let p = parent.join("bin").join(name);
        if p.exists() {
            return Some(p);
        }
    }
    None
}

fn write_wav(path: &Path, samples: &[i16]) -> Result<(), WhisperError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: 16_000,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };
    // Build the full WAV (header + data + patched sizes) in memory first so
    // hound's final seek-and-patch cannot race the subprocess reader.
    let mut buf: Vec<u8> = Vec::with_capacity(44 + samples.len() * 2);
    {
        let cursor = std::io::Cursor::new(&mut buf);
        let mut w = WavWriter::new(cursor, spec).map_err(|e| WhisperError::Wav(e.to_string()))?;
        for &s in samples {
            w.write_sample(s)
                .map_err(|e| WhisperError::Wav(e.to_string()))?;
        }
        w.finalize().map_err(|e| WhisperError::Wav(e.to_string()))?;
    }
    // One atomic file write; file is closed before this function returns.
    std::fs::write(path, &buf).map_err(WhisperError::Io)?;
    tracing::debug!(
        path = %path.display(),
        bytes = buf.len(),
        samples = samples.len(),
        "wrote wav fixture"
    );
    Ok(())
}

/// Strip whisper.cpp's `[00:00:00.000 --> 00:00:00.000]` prefixes and
/// `[BLANK_AUDIO]` / `[MUSIC]` markers that leak through even with `-nt`.
fn clean_transcript(raw: &str) -> String {
    let mut out = String::new();
    for line in raw.lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        // Drop timestamp prefix if present.
        let trimmed = if let Some(idx) = line.find(']') {
            if line.starts_with('[') && line[..idx].contains("-->") {
                line[idx + 1..].trim()
            } else {
                line
            }
        } else {
            line
        };
        // Drop bracketed non-speech markers.
        let trimmed = trimmed.trim();
        if trimmed.is_empty()
            || (trimmed.starts_with('[') && trimmed.ends_with(']'))
            || (trimmed.starts_with('(') && trimmed.ends_with(')'))
        {
            continue;
        }
        if !out.is_empty() {
            out.push(' ');
        }
        out.push_str(trimmed);
    }
    out.trim().to_string()
}

impl Transcriber for WhisperTranscriber {
    fn transcribe(&mut self, audio: &AudioBuffer) -> Result<String, PromptLoomError> {
        if audio.sample_rate_hz != 16_000 {
            return Err(WhisperError::WrongSampleRate(audio.sample_rate_hz).into());
        }

        // Make sure the scratch dir exists on every call. Windows temp cleanup
        // or the user's "clear" scripts can delete it between app launch and
        // the next dictation; we recreate silently rather than fail.
        std::fs::create_dir_all(&self.scratch_dir).map_err(WhisperError::Io)?;

        // Reserve a .wav scratch path, then CLOSE the tempfile's handle
        // before writing. Keeping the NamedTempFile's handle open while the
        // whisper subprocess tried to read the same path raced miniaudio's
        // decoder on Windows. `TempPath` keeps the path and the auto-delete
        // on drop, but releases every file handle we own.
        let wav_path: tempfile::TempPath = tempfile::Builder::new()
            .prefix("rec-")
            .suffix(".wav")
            .tempfile_in(&self.scratch_dir)
            .map_err(WhisperError::Io)?
            .into_temp_path();
        write_wav(&wav_path, &audio.samples)?;

        let model_s = self.model_path.to_string_lossy().into_owned();
        let wav_s = wav_path.to_string_lossy().into_owned();

        let mut cmd = Command::new(&self.binary_path);
        cmd.args([
            "-m", &model_s, "-f", &wav_s, "-nt", // no timestamps
            "-np", // no progress
        ]);
        if let Some(lang) = &self.language {
            cmd.args(["-l", lang]);
        }
        if let Some(t) = self.threads {
            cmd.args(["-t", &t.to_string()]);
        }
        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let output = cmd
            .output()
            .map_err(|e| WhisperError::Spawn(e.to_string()))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).into_owned();
            drop(wav_path);
            return Err(WhisperError::NonZero {
                code: output.status.code(),
                stderr,
            }
            .into());
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let cleaned = clean_transcript(&stdout);

        // Privacy: do NOT log the transcript or any audio-derived preview.
        // On an empty result we only log the two byte lengths so operators
        // can tell whether whisper wrote nothing vs. we filtered everything.
        if cleaned.is_empty() {
            tracing::info!(
                stdout_bytes = output.stdout.len(),
                stderr_bytes = output.stderr.len(),
                "whisper produced empty transcript"
            );
        }

        drop(wav_path);
        Ok(cleaned)
    }
}

// `Command` is Send; this is just explicit so the orchestrator's type bounds
// are satisfied without surprises.
unsafe impl Send for WhisperTranscriber {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn clean_strips_timestamps() {
        let raw = "[00:00:00.000 --> 00:00:02.000]  hello world\n\
                   [00:00:02.000 --> 00:00:04.000]  how are you";
        assert_eq!(clean_transcript(raw), "hello world how are you");
    }

    #[test]
    fn clean_drops_non_speech_markers() {
        let raw = "[BLANK_AUDIO]\n hello\n(music)";
        assert_eq!(clean_transcript(raw), "hello");
    }

    #[test]
    fn clean_passes_plain_text() {
        assert_eq!(clean_transcript("just text"), "just text");
    }

    /// Shared mutex so the two tests below don't race on the global
    /// PROMPTLOOM_WHISPER_BIN env var (cargo runs tests in parallel).
    static ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn resolve_binary_env_var_wins() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::env::set_var("PROMPTLOOM_WHISPER_BIN", tmp.path());
        let fake_model = tempfile::NamedTempFile::new().unwrap();
        let resolved = resolve_binary(fake_model.path()).unwrap();
        assert_eq!(resolved, tmp.path());
        std::env::remove_var("PROMPTLOOM_WHISPER_BIN");
    }

    #[test]
    fn resolve_binary_falls_back_to_sibling() {
        let _guard = ENV_LOCK.lock().unwrap_or_else(|e| e.into_inner());
        std::env::remove_var("PROMPTLOOM_WHISPER_BIN");
        let dir = tempfile::tempdir().unwrap();
        let model = dir.path().join("ggml-base.en.bin");
        std::fs::write(&model, b"not a real model").unwrap();
        let bin = dir.path().join("whisper-cli.exe");
        std::fs::write(&bin, b"").unwrap();
        assert_eq!(resolve_binary(&model).unwrap(), bin);
    }
}

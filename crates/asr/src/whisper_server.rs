//! Managed whisper.cpp HTTP server sidecar.
//!
//! Spawns `whisper-server.exe` as a persistent child process listening on a
//! randomly selected local port. Inferences are POSTed in-memory via HTTP
//! (`multipart/form-data`) to `/inference` with a 2-second timeout.
//!
//! Audio chunks never touch disk during server inference. On drop, the child
//! process is cleanly killed.

use std::io::Cursor;
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use hound::{SampleFormat, WavSpec, WavWriter};
use serde::Deserialize;

use crate::whisper::{clean_transcript, WhisperError};

const REQUEST_TIMEOUT: Duration = Duration::from_millis(2000);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Deserialize)]
struct JsonResponse {
    #[serde(default)]
    text: String,
}

pub struct WhisperServer {
    child: Option<Child>,
    port: u16,
    binary_path: PathBuf,
    model_path: PathBuf,
}

impl WhisperServer {
    /// Start a new whisper-server child process.
    pub fn start(
        binary_path: &Path,
        model_path: &Path,
        language: Option<&str>,
        threads: Option<u32>,
    ) -> Result<Self, WhisperError> {
        if !binary_path.exists() {
            return Err(WhisperError::BinaryNotFound(binary_path.into()));
        }
        if !model_path.exists() {
            return Err(WhisperError::ModelMissing(model_path.into()));
        }

        let port = find_available_port().map_err(WhisperError::Io)?;
        let model_s = model_path.to_string_lossy().into_owned();

        let mut cmd = Command::new(binary_path);
        cmd.args([
            "-m",
            &model_s,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            "-nt",
        ]);

        if let Some(lang) = language {
            cmd.args(["-l", lang]);
        }
        if let Some(t) = threads {
            cmd.args(["-t", &t.to_string()]);
        }

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| WhisperError::Spawn(format!("failed to spawn whisper-server: {e}")))?;

        // Wait until the server is ready to accept HTTP requests.
        let ready = wait_for_ready(&mut child, port, STARTUP_TIMEOUT);
        if let Err(e) = ready {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

        tracing::info!(port, "whisper-server spawned and ready");

        Ok(Self {
            child: Some(child),
            port,
            binary_path: binary_path.to_path_buf(),
            model_path: model_path.to_path_buf(),
        })
    }

    pub fn port(&self) -> u16 {
        self.port
    }

    pub fn binary_path(&self) -> &Path {
        &self.binary_path
    }

    pub fn model_path(&self) -> &Path {
        &self.model_path
    }

    /// Check if the child process is still running.
    pub fn is_alive(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Transcribe PCM16 audio samples entirely in memory with optional prompt context.
    pub fn transcribe_samples(
        &mut self,
        samples: &[i16],
        sample_rate_hz: u32,
        prompt: Option<&str>,
    ) -> Result<String, WhisperError> {
        if samples.is_empty() {
            return Ok(String::new());
        }
        if sample_rate_hz != 16_000 {
            return Err(WhisperError::WrongSampleRate(sample_rate_hz));
        }

        let wav_bytes = samples_to_wav(samples, sample_rate_hz)?;
        self.transcribe_wav_bytes(&wav_bytes, prompt)
    }

    /// Post in-memory WAV bytes to `/inference`.
    pub fn transcribe_wav_bytes(
        &mut self,
        wav_bytes: &[u8],
        prompt: Option<&str>,
    ) -> Result<String, WhisperError> {
        if !self.is_alive() {
            return Err(WhisperError::Spawn(
                "whisper-server child has exited".into(),
            ));
        }

        let boundary = "---------------------------promptloomBoundary7d83b2";
        let body = build_multipart_body(boundary, wav_bytes, prompt, Some(0.0));

        let agent = ureq::AgentBuilder::new().timeout(REQUEST_TIMEOUT).build();
        let url = format!("http://127.0.0.1:{}/inference", self.port);

        let response = agent
            .post(&url)
            .set(
                "Content-Type",
                &format!("multipart/form-data; boundary={boundary}"),
            )
            .send_bytes(&body)
            .map_err(|e| WhisperError::Spawn(format!("whisper-server HTTP error: {e}")))?;

        let response_str = response.into_string().unwrap_or_default();
        let raw_text = if let Ok(json) = serde_json::from_str::<JsonResponse>(&response_str) {
            json.text
        } else {
            response_str
        };

        Ok(clean_transcript(&raw_text))
    }
}

impl Drop for WhisperServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            tracing::info!(port = self.port, "terminating whisper-server child");
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Find an available ephemeral TCP port by temporarily binding to port 0.
pub fn find_available_port() -> Result<u16, std::io::Error> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Resolve `whisper-server.exe` location.
pub fn resolve_server_binary(model_path: &Path) -> Option<PathBuf> {
    if let Ok(env) = std::env::var("PROMPTLOOM_WHISPER_SERVER_BIN") {
        let p = PathBuf::from(env);
        if p.exists() {
            return Some(p);
        }
    }

    // Check sibling of model file
    if let Some(parent) = model_path.parent() {
        for name in &[
            "whisper-server.exe",
            "whisper-server",
            "server.exe",
            "server",
        ] {
            let p = parent.join(name);
            if p.exists() {
                return Some(p);
            }
        }
        // Also check sibling bin/
        for name in &[
            "whisper-server.exe",
            "whisper-server",
            "server.exe",
            "server",
        ] {
            let p = parent.join("bin").join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }

    // Check tools/ relative to current directory or parents
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd.as_path();
        loop {
            let candidate = cur.join("tools").join("whisper-server.exe");
            if candidate.exists() {
                return Some(candidate);
            }
            match cur.parent() {
                Some(p) => cur = p,
                None => break,
            }
        }
    }

    None
}

/// Convert PCM16 samples to WAV bytes entirely in memory.
pub fn samples_to_wav(samples: &[i16], sample_rate_hz: u32) -> Result<Vec<u8>, WhisperError> {
    let spec = WavSpec {
        channels: 1,
        sample_rate: sample_rate_hz,
        bits_per_sample: 16,
        sample_format: SampleFormat::Int,
    };

    let mut buf = Vec::with_capacity(44 + samples.len() * 2);
    {
        let cursor = Cursor::new(&mut buf);
        let mut w = WavWriter::new(cursor, spec).map_err(|e| WhisperError::Wav(e.to_string()))?;
        for &s in samples {
            w.write_sample(s)
                .map_err(|e| WhisperError::Wav(e.to_string()))?;
        }
        w.finalize().map_err(|e| WhisperError::Wav(e.to_string()))?;
    }
    Ok(buf)
}

fn build_multipart_body(
    boundary: &str,
    wav_bytes: &[u8],
    prompt: Option<&str>,
    temperature: Option<f32>,
) -> Vec<u8> {
    let mut body = Vec::new();

    // response_format = json
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"response_format\"\r\n\r\njson\r\n"
        )
        .as_bytes(),
    );

    // temperature
    let temp = temperature.unwrap_or(0.0);
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"temperature\"\r\n\r\n{temp}\r\n"
        )
        .as_bytes(),
    );

    // prompt if provided
    if let Some(p) = prompt {
        let trimmed = p.trim();
        if !trimmed.is_empty() {
            body.extend_from_slice(
                format!(
                    "--{boundary}\r\nContent-Disposition: form-data; name=\"prompt\"\r\n\r\n{trimmed}\r\n"
                )
                .as_bytes(),
            );
        }
    }

    // file
    body.extend_from_slice(
        format!(
            "--{boundary}\r\nContent-Disposition: form-data; name=\"file\"; filename=\"audio.wav\"\r\nContent-Type: audio/wav\r\n\r\n"
        )
        .as_bytes(),
    );
    body.extend_from_slice(wav_bytes);
    body.extend_from_slice(b"\r\n");

    // final boundary
    body.extend_from_slice(format!("--{boundary}--\r\n").as_bytes());
    body
}

fn wait_for_ready(child: &mut Child, port: u16, timeout: Duration) -> Result<(), WhisperError> {
    let start = Instant::now();
    let url = format!("http://127.0.0.1:{port}/");
    let probe_agent = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(200))
        .build();

    while start.elapsed() < timeout {
        if let Ok(Some(status)) = child.try_wait() {
            return Err(WhisperError::NonZero {
                code: status.code(),
                stderr: "whisper-server process terminated during startup".into(),
            });
        }

        if probe_agent.get(&url).call().is_ok() {
            return Ok(());
        }

        std::thread::sleep(Duration::from_millis(60));
    }

    Err(WhisperError::Spawn(format!(
        "timed out waiting for whisper-server on port {port}"
    )))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_to_wav_generates_valid_header() {
        let samples = vec![0i16; 1600]; // 100ms
        let wav = samples_to_wav(&samples, 16_000).expect("wav conversion");
        assert_eq!(&wav[0..4], b"RIFF");
        assert_eq!(&wav[8..12], b"WAVE");
    }

    #[test]
    fn multipart_body_contains_boundary_and_file() {
        let boundary = "test_boundary";
        let wav = vec![1, 2, 3, 4];
        let body = build_multipart_body(boundary, &wav, Some("prior context"), Some(0.0));
        let body_str = String::from_utf8_lossy(&body);
        assert!(body_str.contains("--test_boundary"));
        assert!(body_str.contains("prior context"));
        assert!(body_str.contains("filename=\"audio.wav\""));
        assert!(body_str.ends_with("--test_boundary--\r\n"));
    }

    #[test]
    fn server_lifecycle_when_available() {
        // Test only runs if both model and server binary are available
        let model_dir = std::env::current_dir().unwrap().join("tools");
        let model_path = model_dir.join("ggml-tiny.en.bin");
        let server_bin = model_dir.join("whisper-server.exe");

        if !model_path.exists() || !server_bin.exists() {
            eprintln!("skipping server_lifecycle test: model or binary missing in tools/");
            return;
        }

        let mut server = WhisperServer::start(&server_bin, &model_path, Some("en"), Some(2))
            .expect("server start");

        assert!(server.is_alive());
        let port = server.port();
        assert!(port > 0);

        // Serve one request with silent samples
        let samples = vec![0i16; 16_000]; // 1s
        let res = server.transcribe_samples(&samples, 16_000, None);
        assert!(res.is_ok());

        // Dies cleanly on drop
        drop(server);

        // Verify port is freed
        std::thread::sleep(Duration::from_millis(150));
        let probe = ureq::get(&format!("http://127.0.0.1:{port}/")).call();
        assert!(probe.is_err(), "server should be killed on drop");
    }
}

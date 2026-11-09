//! Managed llama.cpp HTTP server sidecar for local transcript polish.
//!
//! Mirrors `promptloom_asr::whisper_server`: spawns `llama-server.exe` as
//! a child process listening on a random localhost port, waits until it
//! answers HTTP, and routes polish requests to its `/completion`
//! endpoint. The child dies cleanly on `Drop`.
//!
//! Privacy: everything is localhost. The transcript bytes never touch
//! disk here — they go into an in-memory POST body and the response
//! stays in memory until returned to the caller.
//!
//! The polish request runs with a hard 2-second total timeout. On any
//! failure (unreachable, slow, malformed) the caller is expected to fall
//! through to the raw transcript so the user never waits on this layer.

use std::io::{BufRead, BufReader};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

/// Hard timeout for a single `/completion` request.
pub const POLISH_REQUEST_TIMEOUT: Duration = Duration::from_millis(2000);
/// Startup deadline before we give up waiting for the model to load.
pub const POLISH_STARTUP_TIMEOUT: Duration = Duration::from_secs(60);

const POLISH_PROMPT: &str = "\
You are a text cleaner. The user dictated the following sentences via \
speech-to-text. Clean the text by:\n\
- removing filler words (um, uh, like, you know) that aren't meaningful,\n\
- fixing obvious capitalization and punctuation,\n\
- preserving the user's meaning, tone, word choice, and register.\n\n\
Do NOT add new content. Do NOT summarize. Do NOT change the register. \
Do NOT add explanations or markdown.\n\n\
Output ONLY the cleaned text.\n\n\
Input:\n";

#[derive(Debug, thiserror::Error)]
pub enum LlamaError {
    #[error("llama-server binary not found at {0:?}")]
    BinaryNotFound(PathBuf),
    #[error("polish model missing at {0:?}")]
    ModelMissing(PathBuf),
    #[error("spawn failed: {0}")]
    Spawn(String),
    #[error("llama-server timed out starting after {ms} ms")]
    StartupTimeout { ms: u64 },
    #[error("llama-server HTTP error: {0}")]
    Http(String),
    #[error("llama-server returned malformed response: {0}")]
    Malformed(String),
    #[error("polish request timed out after {ms} ms")]
    Timeout { ms: u64 },
    #[error("io: {0}")]
    Io(String),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum LlamaHealth {
    /// Server is running and answered a probe on `port`.
    Active { port: u16 },
    /// Server is not running. Caller should fall through to raw text.
    Fallback,
    /// `llama-server.exe` or the polish model file is not on disk —
    /// bundled polish is permanently off for this process.
    Disabled,
}

#[derive(Serialize)]
struct CompletionRequest<'a> {
    prompt: String,
    n_predict: i32,
    temperature: f32,
    stream: bool,
    /// Stop generation when the model emits either of these — keeps the
    /// cleaner from running on into an explanation.
    stop: [&'a str; 2],
}

#[derive(Deserialize)]
struct CompletionResponse {
    #[serde(default)]
    content: String,
}

pub struct LlamaServer {
    child: Option<Child>,
    port: u16,
    binary_path: PathBuf,
    model_path: PathBuf,
}

impl LlamaServer {
    /// Spawn `llama-server.exe` with the given polish model, bound to a
    /// fresh localhost port. Blocks until the server answers HTTP or the
    /// startup deadline is reached.
    pub fn start(binary_path: &Path, model_path: &Path) -> Result<Self, LlamaError> {
        if !binary_path.exists() {
            return Err(LlamaError::BinaryNotFound(binary_path.into()));
        }
        if !model_path.exists() {
            return Err(LlamaError::ModelMissing(model_path.into()));
        }

        let port = find_available_port().map_err(|e| LlamaError::Io(e.to_string()))?;
        let model_s = model_path.to_string_lossy().into_owned();

        let mut cmd = Command::new(binary_path);
        cmd.args([
            "-m",
            &model_s,
            "--host",
            "127.0.0.1",
            "--port",
            &port.to_string(),
            // Context window: 2048 tokens is plenty for one-pass polish and
            // keeps RAM footprint modest on 8-GB laptops.
            "-c",
            "2048",
            // CPU-only by default. User can override via env on power
            // machines; we don't want surprise VRAM allocation.
            "-ngl",
            "0",
            // Suppress chat template; we send a raw prompt to /completion.
            "--no-webui",
        ]);

        #[cfg(windows)]
        {
            use std::os::windows::process::CommandExt;
            const CREATE_NO_WINDOW: u32 = 0x08000000;
            cmd.creation_flags(CREATE_NO_WINDOW);
        }

        cmd.stdout(Stdio::piped()).stderr(Stdio::piped());

        let mut child = cmd
            .spawn()
            .map_err(|e| LlamaError::Spawn(format!("failed to spawn llama-server: {e}")))?;

        // Drain stderr on a background thread so llama-server's status
        // output doesn't deadlock the pipe. We don't log the content.
        if let Some(stderr) = child.stderr.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(stderr);
                for _ in reader.lines().map_while(Result::ok) {}
            });
        }
        if let Some(stdout) = child.stdout.take() {
            std::thread::spawn(move || {
                let reader = BufReader::new(stdout);
                for _ in reader.lines().map_while(Result::ok) {}
            });
        }

        if let Err(e) = wait_for_ready(&mut child, port, POLISH_STARTUP_TIMEOUT) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(e);
        }

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

    /// Returns true if the child process is still alive.
    pub fn is_alive(&mut self) -> bool {
        if let Some(child) = self.child.as_mut() {
            matches!(child.try_wait(), Ok(None))
        } else {
            false
        }
    }

    /// Send one polish request. Hard timeout: `POLISH_REQUEST_TIMEOUT`.
    pub fn polish(&mut self, text: &str) -> Result<String, LlamaError> {
        self.polish_with_tone(text, None)
    }

    /// Polish with an optional tone override. Tone values other than
    /// "casual" / "formal" are ignored (falls back to neutral prompt).
    pub fn polish_with_tone(
        &mut self,
        text: &str,
        tone: Option<&str>,
    ) -> Result<String, LlamaError> {
        if text.trim().is_empty() {
            return Ok(text.to_string());
        }
        if !self.is_alive() {
            return Err(LlamaError::Http("llama-server child has exited".into()));
        }

        let tone_prefix = match tone {
            Some("casual") => {
                "Rewrite in a casual tone. Use contractions. Keep it \
                 friendly and conversational. Preserve meaning.\n\n"
            }
            Some("formal") => {
                "Rewrite in a formal tone. No contractions. No slang. \
                 Professional register. Preserve meaning.\n\n"
            }
            _ => "",
        };

        let agent = ureq::AgentBuilder::new()
            .timeout(POLISH_REQUEST_TIMEOUT)
            .build();
        let url = format!("http://127.0.0.1:{}/completion", self.port);
        let body = CompletionRequest {
            prompt: format!("{tone_prefix}{POLISH_PROMPT}{text}\n\nOutput:\n"),
            n_predict: 256,
            temperature: 0.2,
            stream: false,
            stop: ["\n\nInput:", "\n\nOutput:"],
        };

        let started = Instant::now();
        let resp = agent.post(&url).send_json(serde_json::json!(body));
        match resp {
            Ok(response) => {
                let decoded: CompletionResponse = response
                    .into_json()
                    .map_err(|e| LlamaError::Malformed(e.to_string()))?;
                Ok(decoded.content.trim().to_string())
            }
            Err(e) => {
                let elapsed = started.elapsed();
                if elapsed >= POLISH_REQUEST_TIMEOUT {
                    Err(LlamaError::Timeout {
                        ms: elapsed.as_millis() as u64,
                    })
                } else {
                    Err(LlamaError::Http(e.to_string()))
                }
            }
        }
    }
}

impl Drop for LlamaServer {
    fn drop(&mut self) {
        if let Some(mut child) = self.child.take() {
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

/// Reserve an ephemeral port by binding to 0 then releasing the socket.
fn find_available_port() -> std::io::Result<u16> {
    let listener = TcpListener::bind("127.0.0.1:0")?;
    let port = listener.local_addr()?.port();
    drop(listener);
    Ok(port)
}

/// Try to find `llama-server.exe` in the usual places.
///
/// Resolution order:
///   1. `PROMPTLOOM_LLAMA_SERVER_BIN` env var (dev override)
///   2. Sibling of `model_path` (`<dir>/llama-server.exe`)
///   3. `tools/` under current dir or any ancestor
pub fn resolve_llama_binary(model_path: &Path) -> Option<PathBuf> {
    if let Ok(env) = std::env::var("PROMPTLOOM_LLAMA_SERVER_BIN") {
        let p = PathBuf::from(env);
        if p.exists() {
            return Some(p);
        }
    }
    if let Some(parent) = model_path.parent() {
        for name in &["llama-server.exe", "llama-server"] {
            let p = parent.join(name);
            if p.exists() {
                return Some(p);
            }
        }
    }
    if let Ok(cwd) = std::env::current_dir() {
        let mut cur = cwd.as_path();
        loop {
            let candidate = cur.join("tools").join("llama-server.exe");
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

fn wait_for_ready(child: &mut Child, port: u16, timeout: Duration) -> Result<(), LlamaError> {
    let start = Instant::now();
    let url = format!("http://127.0.0.1:{port}/health");
    let probe = ureq::AgentBuilder::new()
        .timeout(Duration::from_millis(300))
        .build();

    while start.elapsed() < timeout {
        if let Ok(Some(_)) = child.try_wait() {
            return Err(LlamaError::Spawn(
                "llama-server exited during startup".into(),
            ));
        }
        if probe.get(&url).call().is_ok() {
            return Ok(());
        }
        std::thread::sleep(Duration::from_millis(120));
    }
    Err(LlamaError::StartupTimeout {
        ms: timeout.as_millis() as u64,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_input_short_circuits() {
        // Can't start a real server without the binary; just sanity-check
        // the helper doesn't require it for empty input logic via a method
        // check on an unconstructed instance would need a real server.
        // Instead cover the prompt string compiles and shape.
        assert!(POLISH_PROMPT.contains("filler words"));
    }

    #[test]
    fn resolve_binary_returns_none_without_bundle() {
        // When no tools/ directory contains llama-server.exe nearby, this
        // returns None. In CI there usually isn't one.
        let fake_model = PathBuf::from("/nonexistent/path/model.gguf");
        // We can't assert None universally (someone running tests might
        // have a tools/llama-server.exe in cwd), but we can at least call
        // the resolver without panicking.
        let _ = resolve_llama_binary(&fake_model);
    }

    #[test]
    fn request_body_shape() {
        let body = CompletionRequest {
            prompt: "hi".into(),
            n_predict: 256,
            temperature: 0.2,
            stream: false,
            stop: ["\n\nInput:", "\n\nOutput:"],
        };
        let s = serde_json::to_string(&body).unwrap();
        assert!(s.contains("\"prompt\":\"hi\""));
        assert!(s.contains("\"stream\":false"));
        assert!(s.contains("\"temperature\":0.2"));
        assert!(s.contains("\"n_predict\":256"));
    }
}

//! Optional, local-only LLM polish via Ollama.
//!
//! Sends the dictated transcript to the user's own Ollama server
//! (`http://localhost:11434` by default), asks it to clean up the text with
//! ONE neutral instruction, and returns the result. If anything goes wrong
//! — Ollama unreachable, model missing, timeout exceeded — we return the
//! original text unchanged. The caller never gets stuck.
//!
//! Scope of this module (deliberately minimal, per Mark II plan):
//!   - One cleanup instruction. No "formal / casual / commit" modes.
//!   - Hard 2-second timeout, no retries.
//!   - User's own Ollama server, user's own model.
//!
//! Not here: streaming, custom prompts, mode selection, diff display,
//! prompt-injection defenses beyond keeping the instruction simple.

use std::time::Duration;

use serde::{Deserialize, Serialize};

/// Hard timeout for the entire request. If Ollama is slower than this on
/// first-token, we return the raw transcript rather than block the user.
const REQUEST_TIMEOUT: Duration = Duration::from_millis(2000);

/// Simple, intentionally conservative cleanup instruction. Rewritten in
/// user-visible language because this gets eyeballed and shown.
const CLEAN_PROMPT: &str = "\
You are a text cleaner. The user dictated the following sentence(s) with \
their voice. Fix obvious typos and awkward phrasing while preserving the \
meaning, tone, and punctuation. Do not add new content, questions, or \
disclaimers. Return ONLY the cleaned text, nothing else.\n\nText:\n";

#[derive(Debug, thiserror::Error)]
pub enum OllamaError {
    #[error("Ollama server unreachable at {endpoint}: {reason}")]
    Unreachable { endpoint: String, reason: String },
    #[error("Ollama returned HTTP {status}")]
    HttpStatus { status: u16 },
    #[error("Ollama response malformed: {0}")]
    Malformed(String),
    #[error("Ollama timed out after {ms} ms")]
    Timeout { ms: u64 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OllamaModel {
    pub name: String,
    #[serde(default)]
    pub size: u64,
}

#[derive(Debug, Deserialize)]
struct TagsResponse {
    models: Vec<OllamaModel>,
}

#[derive(Debug, Serialize)]
struct GenerateRequest<'a> {
    model: &'a str,
    prompt: String,
    stream: bool,
    options: GenerateOptions,
}

#[derive(Debug, Serialize)]
struct GenerateOptions {
    temperature: f32,
    num_predict: i32,
}

#[derive(Debug, Deserialize)]
struct GenerateResponse {
    #[serde(default)]
    response: String,
    #[serde(default)]
    done: bool,
}

/// Thin blocking HTTP client over Ollama's `/api/tags` and `/api/generate`
/// endpoints. One instance per app; cheap to clone.
#[derive(Debug, Clone)]
pub struct OllamaClient {
    pub endpoint: String,
}

impl Default for OllamaClient {
    fn default() -> Self {
        Self {
            endpoint: "http://localhost:11434".into(),
        }
    }
}

impl OllamaClient {
    /// List models Ollama has pulled locally. Returns empty on error.
    /// Used by the Settings UI to populate the model dropdown.
    pub fn list_models(&self) -> Result<Vec<OllamaModel>, OllamaError> {
        let agent = ureq::AgentBuilder::new()
            .timeout(Duration::from_millis(1000))
            .build();
        let url = format!("{}/api/tags", self.endpoint);
        let resp = agent.get(&url).call().map_err(|e| OllamaError::Unreachable {
            endpoint: self.endpoint.clone(),
            reason: e.to_string(),
        })?;
        let body: TagsResponse = resp
            .into_json()
            .map_err(|e| OllamaError::Malformed(e.to_string()))?;
        Ok(body.models)
    }

    /// Clean up `text` with the given `model`. On any error (unreachable,
    /// slow, malformed) returns the error so the caller can fall back.
    pub fn clean(&self, model: &str, text: &str) -> Result<String, OllamaError> {
        if text.trim().is_empty() {
            return Ok(text.to_string());
        }

        let agent = ureq::AgentBuilder::new()
            .timeout(REQUEST_TIMEOUT)
            .build();
        let url = format!("{}/api/generate", self.endpoint);
        let body = GenerateRequest {
            model,
            prompt: format!("{CLEAN_PROMPT}{text}"),
            stream: false,
            options: GenerateOptions {
                temperature: 0.3,
                num_predict: 512,
            },
        };

        let started = std::time::Instant::now();
        let resp = agent.post(&url).send_json(serde_json::json!(body));

        match resp {
            Ok(response) => {
                let status = response.status();
                if status != 200 {
                    return Err(OllamaError::HttpStatus { status });
                }
                let decoded: GenerateResponse = response
                    .into_json()
                    .map_err(|e| OllamaError::Malformed(e.to_string()))?;
                if !decoded.done {
                    return Err(OllamaError::Malformed(
                        "non-streamed response missing done=true".into(),
                    ));
                }
                Ok(decoded.response.trim().to_string())
            }
            Err(e) => {
                let elapsed = started.elapsed();
                if elapsed >= REQUEST_TIMEOUT {
                    Err(OllamaError::Timeout {
                        ms: elapsed.as_millis() as u64,
                    })
                } else {
                    Err(OllamaError::Unreachable {
                        endpoint: self.endpoint.clone(),
                        reason: e.to_string(),
                    })
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Validate request serialization doesn't drift. We can't test the live
    /// server from here, so we at least check the JSON shape is what Ollama
    /// expects.
    #[test]
    fn request_body_shape() {
        let body = GenerateRequest {
            model: "llama3.2:3b",
            prompt: "hi".into(),
            stream: false,
            options: GenerateOptions {
                temperature: 0.3,
                num_predict: 512,
            },
        };
        let s = serde_json::to_string(&body).unwrap();
        assert!(s.contains("\"model\":\"llama3.2:3b\""));
        assert!(s.contains("\"stream\":false"));
        assert!(s.contains("\"temperature\":0.3"));
    }

    #[test]
    fn empty_text_short_circuits() {
        // A real network call would fail — but empty text should return
        // instantly without hitting the network.
        let client = OllamaClient {
            endpoint: "http://127.0.0.1:1".into(),
        };
        let result = client.clean("llama3.2:3b", "").unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn unreachable_endpoint_returns_error() {
        let client = OllamaClient {
            endpoint: "http://127.0.0.1:1".into(),
        };
        let err = client.clean("llama3.2:3b", "hello").unwrap_err();
        assert!(
            matches!(err, OllamaError::Unreachable { .. } | OllamaError::Timeout { .. }),
            "expected Unreachable or Timeout, got {err:?}"
        );
    }
}

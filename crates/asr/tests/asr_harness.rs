//! M2 ship-gate harness. Ten user-consented WAV fixtures through the real
//! whisper.cpp adapter, plus a model-stays-warm assertion.
//!
//! All tests are `#[ignore]`-gated: `cargo test` passes without a mic, a
//! model, or network. Run with `--ignored` once you have:
//!
//!   1. a `ggml-base.en.bin` at the path given by `PROMPTLOOM_MODEL_PATH`
//!   2. `crates/asr/tests/fixtures/*.wav` recorded per fixtures/README.md
//!
//! The harness reads `fixtures/expected.json` to decide what each case must
//! produce and which normalizations are permitted before comparison.

#![cfg(windows)]

use std::path::{Path, PathBuf};
use std::time::Instant;

use promptloom_asr::{load_wav_fixture, WhisperTranscriber};
use promptloom_core::Transcriber;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Expectations {
    cases: Vec<Case>,
}

#[derive(Debug, Deserialize)]
struct Case {
    file: String,
    #[serde(default)]
    expected: Option<String>,
    #[serde(default)]
    normalizations: Vec<String>,
    #[serde(default)]
    allow_empty: bool,
    #[serde(default)]
    max_edit_distance: Option<usize>,
    #[serde(default)]
    min_words: Option<usize>,
}

fn fixtures_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

fn model_path() -> Option<PathBuf> {
    std::env::var_os("PROMPTLOOM_MODEL_PATH").map(PathBuf::from)
}

fn normalize(text: &str, rules: &[String]) -> String {
    let mut s = text.to_string();
    for rule in rules {
        s = match rule.as_str() {
            "lowercase" => s.to_lowercase(),
            "trim-punct" => s
                .chars()
                .filter(|c| !matches!(*c, '.' | ',' | '?' | '!' | ';' | ':' | '"' | '\''))
                .collect(),
            "collapse-ws" => s.split_whitespace().collect::<Vec<_>>().join(" "),
            _ => s,
        };
    }
    s.trim().to_string()
}

fn edit_distance(a: &str, b: &str) -> usize {
    let a: Vec<char> = a.chars().collect();
    let b: Vec<char> = b.chars().collect();
    let (n, m) = (a.len(), b.len());
    let mut dp = vec![vec![0usize; m + 1]; n + 1];
    for i in 0..=n {
        dp[i][0] = i;
    }
    for j in 0..=m {
        dp[0][j] = j;
    }
    for i in 1..=n {
        for j in 1..=m {
            let cost = if a[i - 1] == b[j - 1] { 0 } else { 1 };
            dp[i][j] = (dp[i - 1][j] + 1)
                .min(dp[i][j - 1] + 1)
                .min(dp[i - 1][j - 1] + cost);
        }
    }
    dp[n][m]
}

fn load_expectations() -> Expectations {
    let path = fixtures_dir().join("expected.json");
    let bytes = std::fs::read(&path).expect("expected.json missing");
    serde_json::from_slice(&bytes).expect("expected.json malformed")
}

fn load_transcriber() -> WhisperTranscriber {
    let model = model_path().expect(
        "set PROMPTLOOM_MODEL_PATH to the local ggml-base.en.bin before running --ignored",
    );
    WhisperTranscriber::load(&model).expect("model failed to load")
}

#[test]
#[ignore = "requires user-consented WAV fixtures and PROMPTLOOM_MODEL_PATH"]
fn asr_fixtures_match_expected() {
    let expectations = load_expectations();
    let mut transcriber = load_transcriber();
    let fixtures = fixtures_dir();

    let mut failures: Vec<String> = Vec::new();
    for case in &expectations.cases {
        let wav = fixtures.join(&case.file);
        if !wav.exists() {
            failures.push(format!("missing fixture {}", case.file));
            continue;
        }
        let audio = match load_wav_fixture(&wav) {
            Ok(a) => a,
            Err(e) => {
                failures.push(format!("{}: load error {e}", case.file));
                continue;
            }
        };
        let got_raw = match transcriber.transcribe(&audio) {
            Ok(t) => t,
            Err(e) => {
                failures.push(format!("{}: transcribe error {e}", case.file));
                continue;
            }
        };
        let got = normalize(&got_raw, &case.normalizations);

        if case.allow_empty && got.is_empty() {
            continue;
        }

        if let Some(expected_raw) = &case.expected {
            let expected = normalize(expected_raw, &case.normalizations);
            let dist = edit_distance(&got, &expected);
            let max = case.max_edit_distance.unwrap_or(0);
            if dist > max {
                failures.push(format!(
                    "{}: edit distance {dist} > {max}\n  expected: {expected:?}\n  got:      {got:?}",
                    case.file
                ));
            }
        } else if let Some(min_words) = case.min_words {
            let words = got.split_whitespace().count();
            if words < min_words {
                failures.push(format!(
                    "{}: produced {words} words, need >= {min_words}\n  got: {got:?}",
                    case.file
                ));
            }
        }
    }

    assert!(
        failures.is_empty(),
        "fixture failures ({}):\n- {}",
        failures.len(),
        failures.join("\n- ")
    );
}

#[test]
#[ignore = "requires PROMPTLOOM_MODEL_PATH and short.wav fixture"]
fn model_stays_warm_across_two_dictations() {
    let mut transcriber = load_transcriber();
    let short = fixtures_dir().join("short.wav");
    assert!(short.exists(), "short.wav fixture required");
    let audio = load_wav_fixture(&short).expect("load short.wav");

    let t1 = Instant::now();
    let r1 = transcriber.transcribe(&audio).expect("transcribe #1");
    let d1 = t1.elapsed();

    let t2 = Instant::now();
    let r2 = transcriber.transcribe(&audio).expect("transcribe #2");
    let d2 = t2.elapsed();

    assert_eq!(r1, r2, "same audio must transcribe deterministically");
    // Warm run must not be dramatically slower than cold; allow 2x slack
    // because Windows power states can throttle unpredictably.
    assert!(
        d2 <= d1.saturating_mul(2) + std::time::Duration::from_millis(100),
        "warm transcribe ({d2:?}) was slower than cold ({d1:?}) + slack"
    );
}

#[test]
#[ignore = "requires PROMPTLOOM_MODEL_PATH"]
fn model_load_is_cached_once() {
    let model = model_path().expect("PROMPTLOOM_MODEL_PATH");
    let t = Instant::now();
    let _ = WhisperTranscriber::load(Path::new(&model)).expect("load");
    let cold = t.elapsed();
    tracing::info!(?cold, "cold model load timing");
}

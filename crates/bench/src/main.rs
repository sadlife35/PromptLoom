//! PromptLoom end-to-end benchmark.
//!
//! Runs each fixture WAV through the real pipeline (whisper sidecar →
//! polish → capitalize → vocabulary) and records cold / warm latency,
//! transcript previews are intentionally NOT written to log files to keep
//! the privacy claim honest; the Markdown report writes transcripts to a
//! separate `--write-transcripts` output path only when the user asks.
//!
//! Residue check: after every single transcribe, scan the scratch dir and
//! fail the whole run if a `.wav` or `.pcm` remains. Nothing in this binary
//! is for CI — it runs on your laptop against your voice.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use clap::Parser;
use promptloom_asr::{load_wav_fixture, WhisperTranscriber};
use promptloom_core::Transcriber;
use promptloom_vocabulary::{Capitalizer, Polisher, Vocabulary};

/// Warm-run sample count for p50/p95.
const WARM_SAMPLES: usize = 5;

#[derive(Parser)]
#[command(name = "promptloom-bench", about = "End-to-end latency + residue harness")]
struct Args {
    /// Path to the ggml model (same file the Tauri app uses).
    #[arg(long)]
    model: PathBuf,

    /// Directory containing fixture WAVs. Default: ./benches/fixtures.
    #[arg(long, default_value = "benches/fixtures")]
    fixtures: PathBuf,

    /// Where to write the Markdown report. Default: ./benches/results/<ts>.md.
    #[arg(long)]
    out: Option<PathBuf>,

    /// If set, also writes transcripts for human inspection. Off by default
    /// so the harness itself can be rerun safely without leaking anything.
    #[arg(long)]
    write_transcripts: bool,

    /// Scratch directory to scan for residue. Default: the OS temp dir
    /// under `promptloom-scratch`.
    #[arg(long)]
    scratch_dir: Option<PathBuf>,
}

struct FixtureResult {
    name: String,
    audio_duration: Duration,
    cold: Duration,
    warm_p50: Duration,
    warm_p95: Duration,
    final_text_len: usize,
    transcript_len: usize,
    // Written to disk only when --write-transcripts is set.
    final_text: Option<String>,
    transcript: Option<String>,
    residue_after: usize,
}

fn percentile(mut values: Vec<Duration>, p: f64) -> Duration {
    values.sort();
    if values.is_empty() {
        return Duration::ZERO;
    }
    let idx = ((values.len() as f64 - 1.0) * p).round() as usize;
    values[idx]
}

fn scan_scratch(dir: &Path) -> usize {
    std::fs::read_dir(dir)
        .map(|iter| {
            iter.flatten()
                .filter(|e| {
                    e.path()
                        .extension()
                        .and_then(|s| s.to_str())
                        .map(|s| matches!(s.to_ascii_lowercase().as_str(), "wav" | "pcm"))
                        .unwrap_or(false)
                })
                .count()
        })
        .unwrap_or(0)
}

fn main() -> anyhow::Result<()> {
    let args = Args::parse();

    if !args.model.exists() {
        anyhow::bail!("model file not found: {:?}", args.model);
    }
    if !args.fixtures.exists() {
        anyhow::bail!(
            "fixtures directory not found: {:?}\n\
             Create it and record WAVs per benches/README.md.",
            args.fixtures
        );
    }

    let scratch_dir = args
        .scratch_dir
        .clone()
        .unwrap_or_else(|| std::env::temp_dir().join("promptloom-scratch"));

    // Build the same text pipeline the app uses.
    let polisher = Polisher::default();
    let capitalizer = Capitalizer::default();
    let vocab = Vocabulary::empty();

    // Collect fixtures (sorted so the report is stable across runs).
    let mut fixtures: Vec<PathBuf> = std::fs::read_dir(&args.fixtures)?
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| {
            p.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.eq_ignore_ascii_case("wav"))
                .unwrap_or(false)
        })
        .collect();
    fixtures.sort();

    if fixtures.is_empty() {
        anyhow::bail!(
            "no .wav fixtures found in {:?}\n\
             See benches/README.md for the suggested fixture set.",
            args.fixtures
        );
    }

    println!("Model : {}", args.model.display());
    println!("Fixtures : {} files in {}", fixtures.len(), args.fixtures.display());
    println!("Scratch dir : {}", scratch_dir.display());
    println!();

    let mut results: Vec<FixtureResult> = Vec::new();
    let mut total_residue_leaks: usize = 0;

    for path in &fixtures {
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or("?")
            .to_string();
        println!("=> {name}");

        let audio = load_wav_fixture(path)?;
        let audio_duration = audio.duration();

        // Cold: fresh WhisperTranscriber load.
        let t0 = Instant::now();
        let mut transcriber = WhisperTranscriber::load(&args.model)?;
        let cold_transcript = transcriber.transcribe(&audio).map_err(|e| anyhow::anyhow!("{e}"))?;
        let cold = t0.elapsed();

        // Residue check after cold.
        let residue_cold = scan_scratch(&scratch_dir);
        if residue_cold > 0 {
            eprintln!("   RESIDUE after cold: {residue_cold} .wav/.pcm file(s) remain in scratch");
            total_residue_leaks += residue_cold;
        }

        // Warm runs: reuse the same transcriber.
        let mut warm: Vec<Duration> = Vec::with_capacity(WARM_SAMPLES);
        let mut last_transcript = cold_transcript.clone();
        for _ in 0..WARM_SAMPLES {
            let t = Instant::now();
            last_transcript = transcriber.transcribe(&audio).map_err(|e| anyhow::anyhow!("{e}"))?;
            warm.push(t.elapsed());
        }
        let residue_warm = scan_scratch(&scratch_dir);
        if residue_warm > 0 {
            eprintln!("   RESIDUE after warm: {residue_warm} .wav/.pcm file(s) remain in scratch");
            total_residue_leaks += residue_warm;
        }

        // Final text through the full pipeline (user-visible result).
        let polished = polisher.polish(&last_transcript);
        let capitalized = capitalizer.capitalize(&polished);
        let final_text = vocab.apply(&capitalized);

        let warm_p50 = percentile(warm.clone(), 0.50);
        let warm_p95 = percentile(warm.clone(), 0.95);

        println!(
            "   audio {:.2}s  cold {:>7}ms  warm p50 {:>5}ms  p95 {:>5}ms  text {}ch",
            audio_duration.as_secs_f64(),
            cold.as_millis(),
            warm_p50.as_millis(),
            warm_p95.as_millis(),
            final_text.chars().count(),
        );

        results.push(FixtureResult {
            name,
            audio_duration,
            cold,
            warm_p50,
            warm_p95,
            final_text_len: final_text.chars().count(),
            transcript_len: last_transcript.chars().count(),
            final_text: args.write_transcripts.then(|| final_text.clone()),
            transcript: args.write_transcripts.then_some(last_transcript),
            residue_after: residue_cold + residue_warm,
        });
    }

    // Suite-level p50/p95 across all fixtures' warm runs.
    let all_warm: Vec<Duration> = results.iter().flat_map(|r| vec![r.warm_p50, r.warm_p95]).collect();
    let suite_p50 = percentile(results.iter().map(|r| r.warm_p50).collect(), 0.50);
    let suite_p95 = percentile(results.iter().map(|r| r.warm_p95).collect(), 0.95);

    println!();
    println!("Suite warm p50 : {} ms", suite_p50.as_millis());
    println!("Suite warm p95 : {} ms", suite_p95.as_millis());
    println!("Residue leaks  : {total_residue_leaks} (must be 0)");

    // Write the Markdown report.
    let out = args
        .out
        .clone()
        .unwrap_or_else(|| {
            let ts = chrono::Local::now().format("%Y%m%d-%H%M%S");
            PathBuf::from(format!("benches/results/{ts}.md"))
        });
    if let Some(parent) = out.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let report = build_report(&args, &results, suite_p50, suite_p95, total_residue_leaks);
    std::fs::write(&out, report)?;
    println!();
    println!("Report : {}", out.display());

    if total_residue_leaks > 0 {
        anyhow::bail!("privacy residue check failed: {total_residue_leaks} leftover files");
    }

    // Keep so the compiler doesn't warn.
    let _ = all_warm;
    Ok(())
}

fn build_report(
    args: &Args,
    results: &[FixtureResult],
    suite_p50: Duration,
    suite_p95: Duration,
    residue_leaks: usize,
) -> String {
    let mut out = String::new();
    use std::fmt::Write;

    let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S");
    writeln!(out, "# PromptLoom benchmark").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "- Date : {ts}").unwrap();
    writeln!(out, "- Model : `{}`", args.model.display()).unwrap();
    writeln!(out, "- Fixtures : {}", results.len()).unwrap();
    writeln!(out, "- Residue leaks : **{residue_leaks}** (0 = pass)").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Suite").unwrap();
    writeln!(out).unwrap();
    writeln!(out, "| Metric | Value |").unwrap();
    writeln!(out, "|---|---|").unwrap();
    writeln!(out, "| Warm p50 | {} ms |", suite_p50.as_millis()).unwrap();
    writeln!(out, "| Warm p95 | {} ms |", suite_p95.as_millis()).unwrap();
    writeln!(out).unwrap();
    writeln!(out, "## Per-fixture").unwrap();
    writeln!(out).unwrap();
    writeln!(
        out,
        "| Fixture | Audio | Cold | Warm p50 | Warm p95 | Final chars | Residue |"
    )
    .unwrap();
    writeln!(out, "|---|---|---|---|---|---|---|").unwrap();
    for r in results {
        writeln!(
            out,
            "| `{}` | {:.2}s | {} ms | {} ms | {} ms | {} | {} |",
            r.name,
            r.audio_duration.as_secs_f64(),
            r.cold.as_millis(),
            r.warm_p50.as_millis(),
            r.warm_p95.as_millis(),
            r.final_text_len,
            r.residue_after,
        )
        .unwrap();
    }

    if args.write_transcripts {
        writeln!(out).unwrap();
        writeln!(out, "## Transcripts (for review)").unwrap();
        writeln!(out).unwrap();
        for r in results {
            writeln!(out, "### `{}`", r.name).unwrap();
            if let Some(t) = &r.transcript {
                writeln!(out, "**raw whisper**: {t}").unwrap();
            }
            if let Some(f) = &r.final_text {
                writeln!(out, "**user sees**: {f}").unwrap();
            }
            writeln!(out).unwrap();
        }
    } else {
        writeln!(out).unwrap();
        writeln!(
            out,
            "_Transcripts are not written to the report (privacy). Re-run with `--write-transcripts` to inspect text._"
        )
        .unwrap();
    }

    // Nothing clever at the bottom. Keep the report plain.
    let _ = results.iter().map(|r| r.transcript_len).sum::<usize>();
    out
}

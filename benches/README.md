# PromptLoom benchmark harness

Measures the real end-to-end latency and the no-residue privacy claim on
your actual machine. Numbers published in the README / landing page MUST
come from these runs.

## Fixtures

`fixtures/` holds short WAV files that cover the common dictation shapes.
These are **user-recorded** (not committed to the repo) because they are
your voice. Record them with any voice recorder app at any sample rate; the
loader downmixes and resamples to 16 kHz mono.

Suggested fixture set:

| File              | What to say                                                      | Why                     |
|-------------------|------------------------------------------------------------------|-------------------------|
| `short.wav`       | "Hello world"                                                    | smallest realistic case |
| `prose.wav`       | "Open the terminal and run the test suite before pushing"        | normal English prose    |
| `dev_terms.wav`   | "Deploy the service to Supabase using the Postgres backend"      | developer vocabulary    |
| `code.wav`        | "Rust TypeScript and Tauri version two"                          | tech acronyms           |
| `long.wav`        | *(any 10-second paragraph)*                                      | long-form dictation     |
| `noisy.wav`       | prose with keyboard typing in the background                     | moderate noise          |
| `fillers.wav`     | "Um, I think, uh, we should ship this"                          | polish stage exercise   |
| `commands.wav`    | "Hello comma world period new paragraph goodbye"                 | dictation command parse |
| `silence.wav`     | *(0.5 s of silence)*                                             | reject-too-short gate   |
| `pause.wav`       | Short sentence with a mid-sentence pause of about 1 second       | pause handling          |

Record them, drop them in this folder, and run:

```powershell
cargo run --release --bin promptloom-bench -- --model "$env:APPDATA\com.promptloom.desktop\models\ggml-base.en.bin"
```

## What it measures

For each fixture:
- **cold inference**: a fresh WhisperTranscriber load + first transcribe
- **warm inference**: 5 back-to-back transcribes, p50 and p95
- **transcript**: what whisper produced (so you can eyeball accuracy)
- **polished + capitalized + vocab-applied final text**: what the user would actually get

For the suite:
- **scratch residue scan**: after every transcribe, assert the scratch dir has no `.wav` or `.pcm` left behind. This is the privacy claim as a test, not a doc promise.

Output lands in `benches/results/` as a timestamped Markdown report.

## Why a binary and not `cargo bench`

`cargo bench` uses the libtest harness which doesn't play nicely with
subprocess work (whisper-cli is a child process; libtest wraps weirdly).
A plain binary with `std::time::Instant` and `Instant::now()` snapshots is
simpler, works with the subprocess sidecar, and makes the output something
you can post in a release.

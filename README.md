# PromptLoom

A fast, local-first open-source voice typing tool for Windows. Hold a key,
speak, release — your words are transcribed on your PC with whisper.cpp
and pasted into whatever window was focused. No account. No cloud
transcription. No subscription.

Apache-2.0 licensed. Written in Rust and TypeScript.

> **Status: alpha.** Windows 10/11 x64 only. Rough edges documented in
> [`RELEASE_NOTES.md`](RELEASE_NOTES.md).

<!-- 30-second demo GIF goes here. Record with ScreenToGif or OBS:
     hold F8 in VS Code, say "the deploy pipeline runs on Tuesday",
     release, text appears. Keep under 10 MB. -->

## Why PromptLoom

- **Local transcription.** whisper.cpp runs on your PC. After the model is
  installed, no network calls happen.
- **Open source.** Apache-2.0, about 6,500 lines of Rust and TypeScript.
  Read it, fork it, run it.
- **No account, no limits.** One-time ~148 MB download. After that you
  dictate forever.

## Install

Download the latest signed installer from
[Releases](../../releases/latest). SHA-256 is published next to each
release artifact so you can verify.

Alpha installers are **not code-signed** today. Windows SmartScreen will
warn the first time you run them. Click **"More info" → "Run anyway"**.

## Quick start

1. Launch PromptLoom from the Start menu
2. Complete the first-run wizard (privacy, microphone, model download,
   shortcut, optional project vocabulary)
3. Click into any text field (Notepad, VS Code, your browser, a terminal)
4. Hold **F8** and speak
5. Release — text appears

Press **Esc** while holding to cancel. Press F8 again in Toggle mode to
stop recording.

## Features

- Hold-to-talk or toggle hotkey (choose from F8/F9/F10/F2)
- Non-focusable HUD overlay with live level meter during recording
- Smart target-identity check — alt-tab mid-dictation → copy-only
  (never injects keystrokes blindly)
- **Transcript modes**: Normal (prose), Code (preserve case + no auto
  punctuation), Terminal (same as Code, no auto-caps)
- **Dictation commands**: *"comma"*, *"period"*, *"new line"*, *"new
  paragraph"*, *"open brace"*, *"close brace"*, *"backslash"*,
  *"forward slash"*, *"all caps … end caps"*, etc.
- **Filler stripping**: *"um"*, *"uh"*, *"er"*, *"ah"* removed by default
- **Repeat-loop collapse**: whisper hallucinations like *"thank you thank
  you thank you"* collapse to one
- **Smart capitalization**: sentence starts, standalone *"i"* → *"I"*,
  honorifics (*"mr."* → *"Mr."*)
- **Project vocabulary** from `.promptloom/terms.yml` — your own
  substitution rules
- **Optional local LLM polish** via your own Ollama server (opt-in in
  Settings; hard 2-second timeout; raw-text fallback)
- Two speech model options (`ggml-base.en` default, `ggml-tiny.en` faster)
- Built-in privacy audit scans the scratch directory for residue

## Privacy

- **No network calls** happen after the model is installed. You can
  disable your network and the app still works.
- **No audio or transcript is retained** across dictations.
- **Each dictation briefly writes a scoped `.wav`** to your system temp
  directory for whisper.cpp to decode, then deletes it when transcription
  returns. Nothing is written on cancel. The in-app privacy audit scans
  this directory on demand.
- **Optional LLM polish is local-only** — it sends the transcript to the
  Ollama server you run on your own machine. Nothing goes to PromptLoom.

See [`docs/adr/`](docs/adr/) for architectural decisions and
[`RELEASE_NOTES.md`](RELEASE_NOTES.md) for known limitations.

## Build from source

Requirements:
- Windows 10/11 x64
- [Rust](https://rustup.rs/) ≥ 1.78 — `winget install Rustlang.Rustup`
- [Node.js](https://nodejs.org/) ≥ 20 — for the Tauri frontend
- [Tauri CLI](https://tauri.app/) — `cargo install tauri-cli --version "^2.0.0"`
- [whisper.cpp Windows binary](https://github.com/ggerganov/whisper.cpp/releases/tag/v1.5.5)
  — extract `whisper-cli.exe` + the DLLs into
  `%APPDATA%\com.promptloom.desktop\models\`

```powershell
# Core crates — hardware-free ship gate
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings

# Run the app
cd apps/desktop
npm install
cargo tauri dev
```

Full build-from-source + release-build instructions in the previous
sections of this file and `BUILDING.md`.

## Benchmark harness

`crates/bench` ships a runnable binary that measures real end-to-end
latency against your voice. See [`benches/README.md`](benches/README.md)
for the WAV fixture protocol.

```powershell
cargo run --release -p promptloom-bench -- \
  --model "$env:APPDATA\com.promptloom.desktop\models\ggml-base.en.bin"
```

Numbers in the release notes come from this harness. The residue scan
built into the harness verifies the privacy claim — if any WAV or PCM
file remains in the scratch directory after transcription, the run fails.

## Contributing

See [`CONTRIBUTING.md`](CONTRIBUTING.md).

**TL;DR:**
- All contributions require a DCO sign-off (`git commit -s`)
- Discussion happens in GitHub Issues; chat on Discord (link TBD)
- Run `cargo test --workspace`, `cargo fmt --all`, `cargo clippy -D warnings`
  before pushing

## Security

See [`SECURITY.md`](SECURITY.md). TL;DR: email `security@promptloom.app`
(temporary — switching to a dedicated address soon). Please do not open
public issues for security problems.

## License

Apache-2.0. See [`LICENSE`](LICENSE).

- whisper.cpp (the engine) is MIT-licensed
- The ggml speech model weights are the responsibility of their respective
  authors — see [`models/manifest.json`](models/manifest.json) for licenses
- Full third-party credits in [`THIRD_PARTY_NOTICES.md`](THIRD_PARTY_NOTICES.md)

## Honest limitations

PromptLoom is an alpha built by one person. It is good enough for
daily dictation of code comments, emails, Slack, and commit messages.
It is not good enough for:

- Medical or legal transcripts of record
- High-stakes real-time captioning
- Non-English transcription (we ship English-only models)
- Mac or Linux (planned, not shipped)
- Users without Windows SmartScreen workarounds who refuse to run
  unsigned installers

If any of those matter to you, use Windows's built-in dictation, Wispr
Flow, Granola, or OpenAI's Advanced Voice Mode. If you want something
local, open, and auditable — you're in the right place.

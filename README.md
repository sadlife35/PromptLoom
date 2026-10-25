# PromptLoom

Windows local dictation prototype. Records only while a global hotkey is held,
transcribes locally with whisper.cpp, applies project vocabulary corrections,
and inserts text into the previously focused app through a clipboard
transaction. No cloud, no accounts, no telemetry.

See [`promptloom-build-spec.docx`](../promptloom-build-spec.docx) for the full
build specification.

## Status

**All M0–M4 milestones complete per build spec.**

- **M4 Vocabulary + dogfood** — project-folder picker, persisted settings,
  `.promptloom/terms.yml` loaded on save and on boot, hot-swapped into the
  live orchestrator, built-in privacy audit, 30-run manual dogfood matrix
  template.
- **M3 Delivery** — Windows clipboard transaction + `Ctrl+V` to captured
  HWND, copy-only fallback on focus change, prior-clipboard restore.
- **M2 Local ASR** — whisper.cpp sidecar + SHA-256 verified model.
- **M1 Capture** — WASAPI capture + F8 hold-to-talk + level HUD + Escape
  cancel.
- **M0 Foundation** — repo, workspace, state machine, CI.

## Requirements

- Windows 10/11 x64
- [Rust](https://rustup.rs/) >= 1.78 — `winget install Rustlang.Rustup` then
  `rustup default stable`
- [Node.js](https://nodejs.org/) >= 20 and npm (desktop shell only)
- [WebView2 Runtime](https://developer.microsoft.com/en-us/microsoft-edge/webview2/)
  (preinstalled on Windows 11)
- **whisper.cpp prebuilt binary** — download `whisper-blas-bin-x64.zip` (or
  any newer Windows build) from
  https://github.com/ggerganov/whisper.cpp/releases/latest and extract
  `whisper-cli.exe` next to the model (`%APPDATA%\PromptLoom\models\`), or
  set `PROMPTLOOM_WHISPER_BIN=<full path to whisper-cli.exe>`. No C++
  toolchain is required to build PromptLoom itself.

For the desktop shell:

- `cargo install tauri-cli --version "^2.0.0"`

## How to run from source (developer)

Core crates (ship gate — no microphone, model, or network):

```powershell
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

Desktop shell (run from `apps/desktop/`):

```powershell
cd apps/desktop
npm install
cargo tauri dev
```

In the Settings window the first-run wizard appears. Walk through Privacy →
Microphone → Model (click **Download**, ~148 MB) → Shortcut → Vocabulary →
Finish. Then hold **F8** in Notepad, speak, release — text appears.

The desktop crate (`apps/desktop/src-tauri`) is intentionally **outside** the
default Cargo workspace so that the workspace ship-gate `cargo test` does not
require Tauri/WebView2/Node to be installed.

## How to build a release installer

Produces a Windows NSIS `.exe` installer in
`apps/desktop/src-tauri/target/release/bundle/nsis/`.

**One-time: generate icons** (Tauri's bundler won't start without them):

```powershell
cd apps/desktop
# Any square PNG >= 1024x1024 works. Transparent background preferred.
cargo tauri icon path\to\your-logo.png
```

**Then build:**

```powershell
cd apps/desktop
npm install
cargo tauri build
```

First build downloads NSIS once (~1–2 min). Subsequent builds skip that.

**Checksum the installer:**

```powershell
Get-FileHash .\src-tauri\target\release\bundle\nsis\PromptLoom_0.1.0-alpha.1_x64-setup.exe -Algorithm SHA256
```

## How to use the alpha (non-technical tester)

1. Download `PromptLoom_0.1.0-alpha.1_x64-setup.exe` and the published SHA-256
   from the release page.
2. (Optional, recommended) Verify the SHA-256 matches:
   ```powershell
   Get-FileHash .\PromptLoom_0.1.0-alpha.1_x64-setup.exe -Algorithm SHA256
   ```
3. Double-click the installer. **Windows SmartScreen will warn** (unsigned
   alpha). Click **"More info" → "Run anyway"** to continue.
4. Launch PromptLoom from the Start menu. The first-run wizard appears.
   Walk through all six steps. The Model step downloads ~148 MB — this is
   the only network traffic the app ever makes.
5. Open **Notepad**. Click into the empty window so it has keyboard focus.
   Hold **F8**, say "the deploy pipeline runs on Tuesday", release. Text
   appears.
6. Press **Esc** while holding F8 to cancel a dictation.

Full user docs: [`RELEASE_NOTES.md`](RELEASE_NOTES.md).

## Developer verification pass

Before cutting a release artifact, run:

```powershell
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cd apps/desktop
npm install
cargo tauri build
```

All five must succeed with no warnings before the installer is shippable.

## Run the ASR fixture harness (optional)

Record the ten WAVs described in
[`crates/asr/tests/fixtures/README.md`](crates/asr/tests/fixtures/README.md),
then:

```powershell
$env:PROMPTLOOM_MODEL_PATH = "$env:APPDATA\PromptLoom\models\ggml-base.en.bin"
cargo test -p promptloom-asr --tests -- --ignored
```

## Privacy and limitations

- Raw microphone audio, transcript text, clipboard content, and
  foreground-window text are **never** written to logs, telemetry, crash
  reports, or disk.
- No network calls after the model is installed. The app functions with the
  network disabled.
- Password managers, UAC/elevated windows, and known protected fields are
  treated as **copy-only** targets; cross-integrity paste may not work.
- No installer signing, auto-updater, cloud feature, extension host, or
  public API is included.

## Project layout

```
promptloom/
  apps/desktop/                 # Tauri v2 shell and React settings UI + HUD
  crates/core/                  # session state machine, orchestrator, interfaces
  crates/audio/                 # WASAPI capture, resampler, level meter
  crates/asr/                   # whisper.cpp adapter, model manifest, WAV loader
  crates/output-windows/        # clipboard transaction (M3)
  crates/vocabulary/            # YAML parsing, correction stage, fixtures
  crates/test-support/          # fake ASR, fake clock, event collectors
  tests/e2e/                    # Windows-only manual smoke harness
  models/manifest.json          # model URL, SHA-256, version, license link
  docs/adr/                     # non-obvious architecture decisions
  .promptloom/terms.yml.example # committed project vocabulary example
```

## License

Apache-2.0 for application code. Third-party components retain their own
terms — see [`THIRD_PARTY_NOTICES.md`](./THIRD_PARTY_NOTICES.md). Model
weights are governed by the licenses linked in `models/manifest.json`.

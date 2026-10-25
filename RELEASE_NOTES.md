# PromptLoom 0.1.0-alpha.1

Local Windows dictation prototype. Hold a key, speak, release — the text is
transcribed on your PC with whisper.cpp and pasted into the window you were
focused on. No account, no cloud, no telemetry, no transcript history.

## Supported Windows versions

- Windows 10 x64 (1909 or newer)
- Windows 11 x64 (any build)

Tested on Windows 11 Home 24H2. Not tested or supported on Windows 10 LTSC,
Windows Server, 32-bit Windows, or ARM.

## What the alpha installer gives you

- The desktop app (`PromptLoom.exe`) — opens Settings and a hidden HUD overlay
- First-run wizard: privacy, microphone, model, hotkey, project vocabulary
- Global hotkey (default **F8**, hold-to-talk) + **Esc** to cancel
- Local SHA-256-verified download of the `ggml-base.en` whisper model
  (~148 MB, one-time, happens only after you click Download)

**Not** included:
- Cloud ASR, LLM post-processing, meeting recording, mobile apps,
  extensions, MCP, any kind of team/sync features
- An auto-updater
- Code signing

## Privacy behavior (verbatim)

- Speech is transcribed **locally** by `whisper-cli.exe` on your PC.
- **No account** required or supported.
- **Transcripts are not retained.** Each dictation is shown briefly in the HUD,
  pasted (or copied) once, then cleared.
- **Audio behavior, stated precisely:** each dictation is written to a scoped
  `.wav` file in your system temp directory (`%LOCALAPPDATA%\Temp\promptloom-scratch\`)
  for whisper to decode, then deleted when transcription returns. Nothing is
  written on cancel. A built-in privacy audit (Settings → Run audit now)
  verifies this at any time.
- **After the model is installed**, no network calls are made. You can
  disable your network and the app still works.

## Known limitations

- **Unsigned installer.** Windows SmartScreen will warn when a tester runs it
  the first time. Click **"More info"** → **"Run anyway"**. If your org
  requires signed binaries, do not use this alpha.
- **Model stays warm via the OS page cache**, not a resident whisper
  process. The second dictation hits the cached model file; a fresh dictation
  after you reboot pays the ~148 MB file read again. See
  [`docs/adr/0003-whisper-rs-adapter.md`](docs/adr/0003-whisper-rs-adapter.md).
- **Temp WAV touches disk briefly** (see Privacy above). This is a documented
  tradeoff of the sidecar approach vs. in-process bindings.
- **Target focus must stay** on the window you captured when you started
  recording. If you alt-tab mid-dictation, the result lands on your clipboard
  instead of being pasted (by design — see
  [`docs/adr/0004-clipboard-transaction.md`](docs/adr/0004-clipboard-transaction.md)).
- **Short utterances (< 250 ms) are rejected** as accidental key taps.
- **UAC-elevated windows and password managers** are copy-only — PromptLoom
  won't inject keystrokes into higher-integrity processes.
- **Hotkey choice is limited** to a small safe set (F8/F9/F10/F2). No
  arbitrary chord recording in this alpha.

## Recovery steps

| Symptom | Fix |
|---|---|
| Wizard reappears every launch | You skipped the "Finish setup" button, or deleted the model file. Finish the wizard. |
| "No microphone" in HUD | Settings → System → Sound → confirm the device is enabled and Microphone privacy is allowed for desktop apps. |
| "Transcription failed" | Make sure both the model `.bin` and `whisper-cli.exe` are in `%APPDATA%\com.promptloom.desktop\models\`. Open Settings → Models → Verify installed model. |
| Dictation produces empty text | Speak closer to the mic, hold for at least 500 ms, make sure the mic level moves during the Recording state. |
| Transcript went to the wrong window | By design — focus changed between recording and pasting. The text is still on your clipboard: paste with Ctrl+V in the right window. |
| App won't start after an update | Delete `%APPDATA%\com.promptloom.desktop\settings.json` and relaunch; wizard will re-run. |

## Checksum verification

Installer file: `PromptLoom_0.1.0_x64-setup.exe` (3.58 MiB)

**SHA-256:**
```
B222A655BC5BC511EDB23CAE653A54B79B79152EB9AE8F636EA1810034039FB5
```

Verify after download:

```powershell
Get-FileHash .\PromptLoom_0.1.0_x64-setup.exe -Algorithm SHA256
```

The output must match the hash above exactly. If it doesn't, do not run the
installer — the file was modified or corrupted in transit.

## Licenses

- PromptLoom itself: Apache-2.0
- whisper.cpp (bundled binary + model weights): see [THIRD_PARTY_NOTICES](THIRD_PARTY_NOTICES.md)

# Microsoft Store listing

Copy-paste these fields into the Partner Center when submitting PromptLoom.
Keep this file updated when the listing changes.

## Product name

```
PromptLoom
```

## Short description (≤ 200 characters)

```
Local-first voice typing for Windows. Hold a key, speak, release. Nothing goes to any cloud server. Open source.
```

## Full description

```
PromptLoom is a free, open-source voice typing tool for Windows that runs completely on your PC.

Hold a key. Speak. Release. The words you spoke appear in whatever app had keyboard focus — Notepad, VS Code, your browser, Slack, Terminal, anywhere.

Why PromptLoom instead of another dictation tool?

• Nothing is sent to any PromptLoom server. All transcription happens locally with whisper.cpp on your PC. After the one-time model download, you can disconnect from the internet and the app still works.

• No account, no subscription, no quota. You own the setup. The app is Apache-2.0 licensed; the source is on GitHub.

• Dictation commands work out of the box: say "comma", "period", "new paragraph", "open brace", "backslash", "all caps … end caps", and many more.

• Three transcript modes for different targets: Normal (prose), Code (preserve case, no auto-punctuation), Terminal (same as Code, no auto-caps).

• Smart cleanup out of the box: strips "um" / "uh", collapses whisper's repeat-loop hallucinations, applies basic capitalization and honorifics.

• Optional local LLM polish: if you run Ollama on your PC, PromptLoom can send transcripts through your own local model for one additional cleanup pass. Nothing goes to a cloud server.

• Project vocabulary: tell PromptLoom your team's unusual terms (e.g. "super base" → "Supabase") in a committed .promptloom/terms.yml file.

• Safety-first clipboard: if you alt-tab to a different window while dictating, PromptLoom copies the text to your clipboard instead of pasting into the wrong window. Never injects keystrokes blindly.

• Built-in privacy audit: a one-click scan that verifies no audio or logs remain on disk.

PromptLoom is alpha software. It works for daily dictation of code comments, emails, Slack messages, and commit messages. It is not for medical or legal transcripts of record. Known limitations are documented in the release notes.

License: Apache-2.0. The whisper.cpp speech engine is MIT. Model weights follow their original licenses.
```

## Features (bullet list)

```
Local voice-to-text transcription using whisper.cpp
Global hotkey (F8 by default, F9/F10/F2 also available)
Hold-to-talk or toggle mode
Live level meter overlay during recording
Dictation commands (comma, period, new line, open brace, backslash, all caps ... end caps, and more)
Three transcript modes: Normal / Code / Terminal
Automatic filler-word stripping (um, uh, er, ah)
Smart sentence capitalization and honorifics
Project vocabulary from .promptloom/terms.yml
Optional local LLM polish via your own Ollama server (nothing sent to the cloud)
Built-in privacy audit
No account, no subscription, no network calls after model install
Open source (Apache-2.0)
```

## Keywords

```
dictation, voice typing, speech to text, whisper, local, offline, privacy, open source, local AI, STT, voice input, hands-free, accessibility
```

## Category

```
Productivity
```

## Age rating

```
3+
```

## Requires

```
Windows 10 version 1909 or newer
Windows 11
~200 MB free disk for the app + speech model
Working microphone
```

## Privacy policy URL

```
https://promptloom.app/privacy
```

(Must be live before the Store listing is submitted. See `docs/privacy-policy-template.md`.)

## Support contact

```
https://github.com/[OWNER]/promptloom/issues
```

## Website

```
https://promptloom.app
```

## Screenshots to upload

1. Hero: HUD overlay with level meter in a desktop context (take at
   1920×1080 for the Store's required aspect)
2. Settings → Models section showing both models + verify button
3. Settings → Transcript mode section (Normal / Code / Terminal buttons)
4. Settings → LLM polish section with Ollama detected
5. Notepad post-dictation showing a clean paragraph
6. VS Code post-dictation in Code mode showing lowercase preserved

Required: 1366×768 min, 1920×1080 recommended, up to 10 screenshots.

## Submission notes

- **Install mode**: perUser (set in `tauri.conf.json`). The Store respects
  this.
- **Code signing**: not signed. Microsoft Store distribution bypasses
  SmartScreen; this is the primary reason we're submitting.
- **Age rating**: International Age Rating Coalition (IARC) questionnaire
  → will answer "No" to all content categories. Rating should come out
  "3+" or "E for Everyone."
- **Review window**: typically 1–3 days for first submission on the
  developer program.
- **One-time dev account fee**: $19 USD for individual accounts.

# Changelog

All notable changes to PromptLoom are documented here. Format based on
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), versioning on
[SemVer](https://semver.org/).

## [Unreleased] — Mark II

Features and polish shipping toward v0.2.0 (date TBD).

### Added
- **Dictation commands**: 40+ voice commands including punctuation (comma,
  period, question mark), structural (new line, new paragraph), brackets
  (open paren / close paren / open brace / close brace), symbols
  (backslash, forward slash, at sign, hashtag, dollar sign), and the
  `all caps … end caps` modifier
- **Transcript modes**: Normal (prose), Code (preserve case, no auto
  punctuation), Terminal (same as Code, no auto-capitalization). Picked
  in Settings; takes effect on the next dictation
- **Filler-word stripping**: "um", "uh", "er", "ah" removed by default
- **Repeat-loop collapse**: whisper's `"thank you thank you thank you"`
  hallucination now collapses to a single occurrence
- **Smart capitalization**: sentence starts, standalone `i` → `I`,
  honorifics (`mr.` → `Mr.`, `mrs.`, `dr.`, `prof.`, `jr.`, `sr.`, `st.`)
- **Opt-in local LLM polish**: if you run Ollama on your PC, PromptLoom
  can send the transcript through your own model for one additional
  cleanup pass. Hard 2-second timeout. Raw-text fallback on error.
  Nothing goes to a PromptLoom server.
- **Two speech models**: `ggml-base.en` (default, ~148 MB, balanced) and
  `ggml-tiny.en` (~78 MB, faster, lower quality). Switch in Settings.
- **Benchmark harness**: `cargo run -p promptloom-bench` reports cold +
  warm latency against your own voice fixtures, and verifies no WAV or
  PCM residue remains in the scratch directory after transcription
- **Infrastructure**: GitHub Actions release workflow (tag → signed
  installer + SHA-256 attached), CONTRIBUTING, SECURITY, Code of
  Conduct, issue templates

### Changed
- Orchestrator moved to a dedicated worker thread. The UI never freezes
  during whisper transcription (fixes known limitation from v0.1.0).
- Settings UI reorganized: new sections for Transcript mode, LLM polish,
  and Models. Visual hierarchy improved.
- `models/manifest.json` schema now carries multiple models

### Removed
- The `last-failed.wav` debug file saved on empty transcripts (privacy
  regression from debugging; removed from the pipeline entirely)
- Transcript-preview logging on empty transcripts

### Security
- Privacy audit now ignores no exception files — any `.wav` or `.pcm` in
  the scratch directory fails the audit

## [0.1.0] — 2026-10-24

First shippable alpha. See [`RELEASE_NOTES.md`](RELEASE_NOTES.md) for the
full behavior and known limitations as of 0.1.0.

### Added
- Core state machine: `Idle → Recording → Finalizing → Transcribing →
  Correcting → Delivering → Idle`
- Global hotkey (F8 hold-to-talk, Esc cancel)
- CPAL/WASAPI capture + resampler + atomic level meter
- whisper.cpp sidecar + SHA-256 verified model download (`ggml-base.en`)
- Windows clipboard transaction + `Ctrl+V` paste with target-identity
  check (focus change → copy-only, never injects keystrokes)
- Project vocabulary from `.promptloom/terms.yml`
- First-run wizard: privacy, mic, model, hotkey, vocab, finish
- Settings persistence
- Built-in privacy audit
- Signed NSIS installer

[Unreleased]: https://github.com/[OWNER]/promptloom/compare/v0.1.0...HEAD
[0.1.0]: https://github.com/[OWNER]/promptloom/releases/tag/v0.1.0

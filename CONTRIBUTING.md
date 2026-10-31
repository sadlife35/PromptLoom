# Contributing to PromptLoom

Thanks for considering a contribution. PromptLoom is a one-person weekend
project that outgrew the author, which means every PR actually matters.

## Ground rules

- **Be kind.** We follow the
  [Contributor Covenant 2.1](CODE_OF_CONDUCT.md).
- **Open an issue first for non-trivial changes.** A short discussion
  before you write code saves everyone a merge conflict.
- **One PR per concern.** Easier to review, easier to revert.
- **DCO sign-off required on every commit** — `git commit -s`. See
  [Developer Certificate of Origin](https://developercertificate.org/).
  This replaces a CLA; nothing fancy. The sign-off goes in the commit
  message as `Signed-off-by: Your Name <you@example.com>`.

## Where to find work

- [`good first issue`](../../labels/good%20first%20issue) — small,
  self-contained, documented
- [`help wanted`](../../labels/help%20wanted) — bigger scope but we've
  thought through the design
- Anything in `docs/adr/` that mentions "future work" or "deferred"

If you want to tackle something not listed, open an issue proposing it
before you start.

## Local setup

See the "Build from source" section of [`README.md`](README.md).

## Before you push

Run the full ship gate locally:

```powershell
cargo test --workspace --all-targets
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

All three must pass. If any fail on your machine but you believe the fix
is unrelated to your change, mention it in the PR description.

## What makes a PR likely to merge

- Fixes a real user-reported bug or implements a feature from an open
  issue that's been triaged
- Includes tests for the behavior change (we have 55+ Rust tests; one
  more never hurts)
- Preserves the privacy model — nothing new that persists audio,
  transcript text, or clipboard content
- Preserves the no-cloud model — no network calls after model install
- Doesn't widen scope beyond the issue it addresses

## What makes a PR likely to get bounced

- "Refactor everything" PRs with no discussion first
- Adding dependencies without a clear need
- Logging transcript text, clipboard contents, or audio to anything
- Introducing a cloud call anywhere, even "optional"
- Breaking tests without explaining why

## Project structure

```
apps/desktop/         Tauri v2 app (Rust backend + React/TS UI)
crates/
  core/               State machine + interface traits + orchestrator
  audio/              CPAL/WASAPI capture + resampling
  asr/                whisper.cpp sidecar + model manifest + verify
  output-windows/     Clipboard transaction + Ctrl+V paste
  vocabulary/         Polish + capitalize + user terms
  commands/           Dictation commands ("comma", "new paragraph", ...)
  llm-polish/         Optional Ollama cleanup
  test-support/       Fakes for every trait
  bench/              Latency + residue benchmark
docs/adr/             Architecture decision records
benches/              Benchmark fixtures and reports
```

## Design decisions

If you're changing something non-obvious (new crate, new trait, new state
machine transition, new security claim), add an ADR under `docs/adr/`
with a short write-up: context, decision, consequences. Look at the
existing numbered ADRs for the format.

## Discussing

- GitHub Issues for bugs and feature proposals
- GitHub Discussions for broader "should we do X" conversations
- Discord link coming when the server is set up

## Thanks

Seriously. This project got started as a weekend hack. Every reasonable
PR, every bug report with a good repro, every "actually your docs are
wrong here" message makes it better for the next person.

# Security policy

## Reporting a vulnerability

**Please do not open a public GitHub issue for security problems.**

Email `security@promptloom.app` with:
- A description of the vulnerability
- Steps to reproduce (even a minimal sketch helps)
- The version of PromptLoom you're running (Settings → About)
- Your Windows version and hardware if the issue appears hardware-dependent

If you prefer encrypted email, our PGP key is published at
`promptloom.app/.well-known/pgp-key.asc` once the site goes live.
Until then, email plaintext — the issue is between us.

We aim to acknowledge your report within **72 hours**.

## Scope

PromptLoom's attack surface is deliberately small, but the following are
in scope for a security report:

- **Audio or transcript leaks** — anything that causes raw audio, raw
  transcript text, clipboard content, or foreground-window text to be
  written to logs, telemetry, crash dumps, or any disk location beyond
  the documented scoped scratch WAV
- **Network calls** that happen outside the model-download flow
- **Clipboard transaction mishaps** — pasting into a window other than
  the one captured at hotkey-press time, failing to restore prior
  clipboard contents when expected, injecting keystrokes into protected
  windows
- **Unsigned-code-execution path** — PromptLoom should never execute code
  it didn't ship (model files are verified by SHA-256 before load; the
  whisper-cli sidecar is user-supplied)
- **Privilege escalation** or any ability to perform an action the user
  did not authorize

Out of scope (for now):
- Attacks requiring prior full control of the user's PC
- Social-engineering a user to swap their model file
- Denial of service against your own installation
- Ollama server misuse — Ollama is user-provided; its security is
  governed by its own project

## Disclosure window

We follow a **90-day disclosure window** by default:
- Day 0: You report
- Day ≤ 3: We acknowledge
- Day ≤ 14: We confirm or ask for more info
- Day ≤ 90: We ship a fix, release a security advisory, and credit you
  (or stay anonymous — your call)

If a vulnerability is being actively exploited, we may release a fix
immediately with coordinated disclosure.

## Credit

With your permission, we credit reporters by name in release notes and
in a `SECURITY-ACKNOWLEDGEMENTS.md` file. If you prefer to stay anonymous,
we respect that.

## What's NOT a vulnerability

A few things that look like problems but aren't:
- **Windows SmartScreen warning on the unsigned installer** — intended
  until we have an EV cert or Microsoft Store approval; use SHA-256 to
  verify the file is unmodified
- **Scratch WAV touching disk briefly during transcription** — documented
  behavior; the scratch file is deleted on return and the privacy audit
  verifies it
- **"The model file is big"** — yes, whisper models are ~148 MB; this is
  by design
- **"Why does the app read the foreground window title"** — to determine
  the paste target, documented in `docs/adr/0004-clipboard-transaction.md`

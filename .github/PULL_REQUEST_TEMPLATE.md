<!-- Thanks for the PR! A few things make merging easier: -->

## What this changes

<!-- One paragraph. What did you do and why? -->

## How to try it

<!-- Steps for the reviewer. "Run cargo tauri dev, hold F8, say X, release" -->

## Tests

<!-- Did you add tests? If no, why not? -->
- [ ] Added or updated unit tests
- [ ] `cargo test --workspace --all-targets` passes locally
- [ ] `cargo fmt --all -- --check` passes locally
- [ ] `cargo clippy --workspace --all-targets -- -D warnings` passes locally

## Privacy and security

<!-- Required checks even if "obviously fine" -->
- [ ] No audio, transcript text, clipboard content, or window text is
      logged anywhere new
- [ ] No new network calls introduced
- [ ] Changes to the clipboard transaction or paste logic have been
      sanity-tested against focus-change (alt-tab mid-dictation)

## Related issues

<!-- "Closes #123" or "Related to #456" -->

## Checklist

- [ ] I signed off every commit with `git commit -s` (DCO)
- [ ] I read [`CONTRIBUTING.md`](../CONTRIBUTING.md)
- [ ] This PR is scoped to one concern

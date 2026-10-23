# Third-Party Notices

PromptLoom includes or depends on third-party software with its own licenses.
A build-time dependency inventory will be generated with `cargo about` in a
later milestone. The notable upstream components are:

- **whisper.cpp** — MIT License. Will be linked or shelled in M2.
  https://github.com/ggerganov/whisper.cpp/blob/master/LICENSE
- **Tauri v2** — Apache-2.0 OR MIT dual-licensed.
- **CPAL** — Apache-2.0.
- **serde / tokio / tracing / thiserror** — MIT/Apache-2.0.
- **React 18** — MIT.

Model weights (e.g. `ggml-base.en`) are distributed under their own terms,
listed in [`models/manifest.json`](./models/manifest.json). PromptLoom does
not redistribute weights; the user downloads them explicitly at setup.

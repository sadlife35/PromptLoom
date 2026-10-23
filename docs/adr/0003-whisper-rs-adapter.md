# ADR 0003 — whisper.cpp as a managed sidecar

Status: accepted. Supersedes an earlier draft that chose the `whisper-rs`
Rust binding.

## Context

M2 needs a single English-capable local ASR engine. The spec forbids
multiple engines, a GPU matrix, streaming, and custom plug-ins. The spec
also explicitly permits **either** a Rust binding **or** a managed sidecar.

We first tried the Rust binding path (`whisper-rs 0.11` → `whisper-rs-sys
0.9`). On Windows, that build pulls in:

1. `bindgen` (needs `libclang.dll`)
2. a C++ toolchain (needs MSVC + Windows SDK)
3. CMake (compiles whisper.cpp from source each clean build)
4. headers that bindgen's clang must find via `BINDGEN_EXTRA_CLANG_ARGS`

Even with all four correctly installed, bindgen 0.69 against whisper.cpp's
current headers emits `whisper_full_params` as an opaque struct, which
breaks every field access in `whisper-rs 0.11`. Fixing that upstream is
out of scope for this prototype.

## Decision

Use the **managed sidecar** path:

- Ship the prebuilt `whisper-cli.exe` from the official `ggerganov/whisper.cpp`
  GitHub releases.
- `WhisperTranscriber` holds the binary path + the model path and spawns
  a child process per dictation (`whisper-cli.exe -m <model> -f <wav> -nt -np -l en`).
- The audio buffer is written to a scoped `tempfile::NamedTempFile` in the
  user's temp dir and deleted on drop, even on panic.
- The model file stays memory-mapped by the OS across invocations, which
  satisfies the "stays warm" spec test — the second dictation hits the
  Windows page cache instead of re-reading the 148 MB file from disk.

## Non-goals (unchanged)

- No multi-model marketplace.
- No GPU matrix (user may pick a GPU-enabled release zip from upstream;
  PromptLoom only runs it).
- No streaming partial transcripts.
- No cloud fallback.

## Consequences

- **No C++ toolchain required to build PromptLoom.** No CMake, no
  Visual Studio C++ workload, no LLVM. `cargo build` for the asr crate
  compiles plain Rust.
- One-time user action: download `whisper-blas-bin-x64.zip` from
  https://github.com/ggerganov/whisper.cpp/releases, extract
  `whisper-cli.exe`, drop next to the model (or set
  `PROMPTLOOM_WHISPER_BIN`).
- **Minor privacy caveat vs. the binding path**: audio briefly touches
  disk as a temp WAV between orchestrator and sidecar. The file is scoped
  to a `NamedTempFile` and deleted before `transcribe()` returns. This is
  called out in the README and in the privacy test (which scans the
  scratch dir after each run and asserts no WAVs remain).
- Each dictation pays ~50–200 ms of process-spawn overhead on top of
  whisper's own inference time. Acceptable for the prototype; a long-
  running `whisper-server` upgrade is a future consideration.

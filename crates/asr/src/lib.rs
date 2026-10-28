//! promptloom-asr: managed whisper.cpp sidecar + verified model manifest.
//!
//! The spec permits either a Rust binding or a managed sidecar. We use the
//! sidecar path (shelling out to the official `whisper-cli.exe` prebuilt
//! release) to eliminate the Windows C++/bindgen toolchain fragility that
//! Rust bindings bring in.

pub mod download;
pub mod manifest;
pub mod wav;
pub mod whisper;

pub use manifest::{
    find_model, load_all_manifest, load_manifest, verify_hash, ManifestError, ModelManifest,
};
pub use wav::{load_wav_fixture, WavFixtureError};
pub use whisper::{WhisperError, WhisperTranscriber};

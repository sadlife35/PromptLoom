//! Model manifest: URL, SHA-256, version, license.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelManifest {
    pub name: String,
    pub url: String,
    pub sha256: String,
    pub version: String,
    pub license_url: String,
    pub size_bytes: u64,
    #[serde(default)]
    pub note: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ManifestFile {
    pub models: Vec<ModelManifest>,
}

#[derive(Debug, thiserror::Error)]
pub enum ManifestError {
    #[error("manifest file not found")]
    NotFound,
    #[error("manifest invalid: {0}")]
    Invalid(String),
    #[error("sha256 mismatch: expected {expected}, got {actual}")]
    HashMismatch { expected: String, actual: String },
    #[error("no model named {0} in manifest")]
    NoSuchModel(String),
}

pub fn load_manifest(path: &Path) -> Result<ModelManifest, ManifestError> {
    let bytes = std::fs::read(path).map_err(|_| ManifestError::NotFound)?;
    // Accept either {models: [...]} or a bare ModelManifest.
    if let Ok(file) = serde_json::from_slice::<ManifestFile>(&bytes) {
        return file
            .models
            .into_iter()
            .next()
            .ok_or_else(|| ManifestError::Invalid("manifest has no models".into()));
    }
    serde_json::from_slice::<ModelManifest>(&bytes)
        .map_err(|e| ManifestError::Invalid(e.to_string()))
}

/// Verify `actual` SHA-256 hex string matches the manifest's.
pub fn verify_hash(manifest: &ModelManifest, actual: &str) -> Result<(), ManifestError> {
    if manifest.sha256.eq_ignore_ascii_case(actual) {
        Ok(())
    } else {
        Err(ManifestError::HashMismatch {
            expected: manifest.sha256.clone(),
            actual: actual.to_string(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn verify_hash_accepts_matching() {
        let m = ModelManifest {
            name: "ggml-base.en".into(),
            url: "https://example/".into(),
            sha256: "ABCDEF".into(),
            version: "1".into(),
            license_url: "https://example/LICENSE".into(),
            size_bytes: 0,
            note: None,
        };
        assert!(verify_hash(&m, "abcdef").is_ok());
    }

    #[test]
    fn verify_hash_rejects_mismatch() {
        let m = ModelManifest {
            name: "x".into(),
            url: "x".into(),
            sha256: "aaaa".into(),
            version: "1".into(),
            license_url: "x".into(),
            size_bytes: 0,
            note: None,
        };
        assert!(matches!(
            verify_hash(&m, "bbbb"),
            Err(ManifestError::HashMismatch { .. })
        ));
    }

    #[test]
    fn parses_models_array_file() {
        let json = r#"{"models":[{"name":"x","url":"u","sha256":"ab","version":"1","license_url":"l","size_bytes":1}]}"#;
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), json).unwrap();
        let m = load_manifest(tmp.path()).unwrap();
        assert_eq!(m.name, "x");
    }
}

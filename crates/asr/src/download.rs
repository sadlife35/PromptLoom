//! Verified model download. Streams the body, computes SHA-256 as it writes,
//! and atomic-renames into place only if the hash matches the manifest.

use std::fs::File;
use std::io::{BufWriter, Read, Write};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::manifest::{ManifestError, ModelManifest};

#[derive(Debug, thiserror::Error)]
pub enum DownloadError {
    #[error("http: {0}")]
    Http(String),
    #[error("io: {0}")]
    Io(#[from] std::io::Error),
    #[error("manifest: {0}")]
    Manifest(#[from] ManifestError),
}

pub struct DownloadReport {
    pub bytes: u64,
    pub sha256: String,
    pub path: PathBuf,
}

/// Download `manifest.url` to `dest`, writing to a `.part` sibling and
/// atomically renaming on success. The partial file is removed on hash
/// mismatch or I/O failure.
///
/// `progress` is called with `(bytes_so_far, total)` where `total` is the
/// manifest's `size_bytes` (0 if unknown). The callback must not block.
pub fn download_model(
    manifest: &ModelManifest,
    dest: &Path,
    mut progress: impl FnMut(u64, u64),
) -> Result<DownloadReport, DownloadError> {
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let part_path = dest.with_extension("part");

    let resp = ureq::get(&manifest.url)
        .call()
        .map_err(|e| DownloadError::Http(e.to_string()))?;
    let mut reader = resp.into_reader();
    let mut hasher = Sha256::new();
    let total = manifest.size_bytes;

    let outcome = (|| -> Result<DownloadReport, DownloadError> {
        let f = File::create(&part_path)?;
        let mut writer = BufWriter::new(f);
        let mut buf = vec![0u8; 1024 * 64];
        let mut bytes: u64 = 0;
        loop {
            let n = reader.read(&mut buf)?;
            if n == 0 {
                break;
            }
            hasher.update(&buf[..n]);
            writer.write_all(&buf[..n])?;
            bytes += n as u64;
            progress(bytes, total);
        }
        writer.flush()?;
        let actual = format!("{:x}", hasher.finalize());
        if !manifest.sha256.eq_ignore_ascii_case(&actual) {
            return Err(DownloadError::Manifest(ManifestError::HashMismatch {
                expected: manifest.sha256.clone(),
                actual,
            }));
        }
        drop(writer);
        std::fs::rename(&part_path, dest)?;
        Ok(DownloadReport {
            bytes,
            sha256: actual,
            path: dest.to_path_buf(),
        })
    })();

    if outcome.is_err() {
        let _ = std::fs::remove_file(&part_path);
    }
    outcome
}

/// Verify an existing file on disk against the manifest.
pub fn verify_file(manifest: &ModelManifest, path: &Path) -> Result<(), DownloadError> {
    let mut f = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buf = vec![0u8; 1024 * 64];
    loop {
        let n = f.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    let actual = format!("{:x}", hasher.finalize());
    if !manifest.sha256.eq_ignore_ascii_case(&actual) {
        return Err(DownloadError::Manifest(ManifestError::HashMismatch {
            expected: manifest.sha256.clone(),
            actual,
        }));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::ModelManifest;

    #[test]
    fn verify_file_matches_sha256() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"hello world").unwrap();
        // sha256("hello world") = b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9
        let m = ModelManifest {
            name: "t".into(),
            url: "x".into(),
            sha256: "b94d27b9934d3e08a52e52d7da7dabfac484efe37a5380ee9088f7ace2efcde9".into(),
            version: "1".into(),
            license_url: "x".into(),
            size_bytes: 11,
            note: None,
        };
        assert!(verify_file(&m, tmp.path()).is_ok());
    }

    #[test]
    fn verify_file_rejects_wrong_hash() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(tmp.path(), b"nope").unwrap();
        let m = ModelManifest {
            name: "t".into(),
            url: "x".into(),
            sha256: "deadbeef".into(),
            version: "1".into(),
            license_url: "x".into(),
            size_bytes: 4,
            note: None,
        };
        assert!(matches!(
            verify_file(&m, tmp.path()),
            Err(DownloadError::Manifest(ManifestError::HashMismatch { .. }))
        ));
    }
}

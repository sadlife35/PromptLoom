//! promptloom-vocabulary: parse `.promptloom/terms.yml` and apply
//! deterministic case-insensitive longest-match replacements.

use std::path::Path;

use promptloom_core::TextTransformer;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct TermsFile {
    terms: std::collections::BTreeMap<String, String>,
}

#[derive(Debug, Default, Clone)]
pub struct Vocabulary {
    /// Source phrase (lowercased) -> destination spelling (preserved exactly).
    /// Sorted longest-first so longer phrases win over shorter prefixes.
    rules: Vec<(String, String)>,
}

#[derive(Debug, thiserror::Error)]
pub enum VocabularyError {
    #[error("terms file not found")]
    NotFound,
    #[error("terms file invalid: {0}")]
    Invalid(String),
}

impl Vocabulary {
    pub fn empty() -> Self {
        Self::default()
    }

    pub fn load(path: &Path) -> Result<Self, VocabularyError> {
        let bytes = std::fs::read(path).map_err(|_| VocabularyError::NotFound)?;
        let parsed: TermsFile =
            serde_yaml::from_slice(&bytes).map_err(|e| VocabularyError::Invalid(e.to_string()))?;
        Ok(Self::from_map(parsed.terms))
    }

    pub fn from_map(map: std::collections::BTreeMap<String, String>) -> Self {
        let mut rules: Vec<(String, String)> = map
            .into_iter()
            .filter(|(src, _)| !src.trim().is_empty())
            .map(|(src, dst)| (src.to_lowercase(), dst))
            .collect();
        // Longest source phrase first for deterministic longest-match.
        rules.sort_by(|a, b| b.0.len().cmp(&a.0.len()).then(a.0.cmp(&b.0)));
        Self { rules }
    }

    pub fn len(&self) -> usize {
        self.rules.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rules.is_empty()
    }

    /// Apply ASCII-case-insensitive whole-phrase replacement,
    /// non-overlapping, left to right. Source phrases must be ASCII
    /// (the spec's examples are all ASCII); non-ASCII bytes in the input
    /// are preserved and never participate in a match.
    pub fn apply(&self, text: &str) -> String {
        if self.rules.is_empty() || text.is_empty() {
            return text.to_string();
        }
        let bytes = text.as_bytes();
        let mut out = String::with_capacity(text.len());
        let mut i = 0usize;
        while i < bytes.len() {
            let mut matched = false;
            for (src, dst) in &self.rules {
                let s = src.as_bytes();
                let slen = s.len();
                if i + slen > bytes.len() {
                    continue;
                }
                let haystack = &bytes[i..i + slen];
                if !ascii_eq_ignore_case(haystack, s) {
                    continue;
                }
                let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
                let right_ok = i + slen == bytes.len() || !is_word_byte(bytes[i + slen]);
                if !(left_ok && right_ok) {
                    continue;
                }
                out.push_str(dst);
                i += slen;
                matched = true;
                break;
            }
            if !matched {
                let ch_len = utf8_char_len(bytes[i]);
                let end = (i + ch_len).min(bytes.len());
                if let Ok(s) = std::str::from_utf8(&bytes[i..end]) {
                    out.push_str(s);
                }
                i = end;
            }
        }
        out
    }
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn ascii_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.iter()
        .zip(b.iter())
        .all(|(x, y)| x.eq_ignore_ascii_case(y))
}

fn utf8_char_len(b: u8) -> usize {
    if b < 0xC0 {
        1
    } else if b < 0xE0 {
        2
    } else if b < 0xF0 {
        3
    } else {
        4
    }
}

impl TextTransformer for Vocabulary {
    fn transform(&self, text: &str) -> String {
        self.apply(text)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vocab(pairs: &[(&str, &str)]) -> Vocabulary {
        let mut map = std::collections::BTreeMap::new();
        for (k, v) in pairs {
            map.insert(k.to_string(), v.to_string());
        }
        Vocabulary::from_map(map)
    }

    #[test]
    fn case_insensitive_replacement_preserves_destination() {
        let v = vocab(&[("super base", "Supabase")]);
        assert_eq!(v.apply("Open Super Base now"), "Open Supabase now");
        assert_eq!(v.apply("open super base now"), "open Supabase now");
    }

    #[test]
    fn longest_match_wins() {
        let v = vocab(&[("prompt", "Prompt"), ("prompt loom", "PromptLoom")]);
        assert_eq!(v.apply("prompt loom is here"), "PromptLoom is here");
    }

    #[test]
    fn whole_phrase_boundary() {
        let v = vocab(&[("loom", "Loom")]);
        // Inside another word: do not replace.
        assert_eq!(v.apply("heirloom"), "heirloom");
        // Standalone: replace.
        assert_eq!(v.apply("the loom"), "the Loom");
    }

    #[test]
    fn empty_vocab_is_identity() {
        let v = Vocabulary::empty();
        assert_eq!(v.apply("whatever"), "whatever");
    }

    #[test]
    fn transformer_trait_delegates() {
        let v: Box<dyn TextTransformer> = Box::new(vocab(&[("post gress", "Postgres")]));
        assert_eq!(v.transform("Use post gress"), "Use Postgres");
    }

    #[test]
    fn deterministic_across_runs() {
        let v = vocab(&[
            ("prompt loom", "PromptLoom"),
            ("super base", "Supabase"),
            ("post gress", "Postgres"),
        ]);
        let input = "Deploy prompt loom to super base backed by post gress today.";
        let expected = "Deploy PromptLoom to Supabase backed by Postgres today.";
        for _ in 0..5 {
            assert_eq!(v.apply(input), expected);
        }
    }
}

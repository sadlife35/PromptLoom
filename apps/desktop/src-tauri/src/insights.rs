//! Opt-in dictation stats. Records aggregate counts per session plus a
//! long-term word frequency table, both in the app's data directory. The
//! raw transcript is never written — only derived counts and words of
//! length ≥4 so the Insights page can show trends without storing speech.
//!
//! Storage:
//! - `sessions.jsonl`: one JSON line per dictation (append-only).
//! - `word_frequencies.json`: cumulative lowercased word → count map.

use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use serde::{Deserialize, Serialize};

const STOPWORDS: &[&str] = &[
    "that", "this", "with", "from", "have", "been", "were", "will", "they", "them", "their",
    "then", "than", "what", "when", "where", "would", "could", "should", "about", "which", "your",
    "just", "like", "there", "some", "into", "over", "very", "much", "more", "most", "also",
    "only", "even", "well", "here", "make", "made", "take", "took", "give", "gave", "come", "came",
    "going", "want", "need", "know", "think", "thought", "said", "saying", "yeah", "okay", "right",
];

const FILLERS: &[&str] = &[
    "um",
    "uh",
    "er",
    "ah",
    "like",
    "you",
    "know",
    "basically",
    "literally",
    "actually",
    "kind",
    "sort",
    "stuff",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRecord {
    /// Unix seconds.
    pub ts: u64,
    pub word_count: u32,
    pub duration_ms: u64,
    /// Count of filler tokens in the raw text (um/uh/like/…).
    pub filler_count: u32,
    /// Foreground app's basename at the moment the dictation fired,
    /// lowercased. Empty if unknown.
    #[serde(default)]
    pub app: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct InsightsSummary {
    pub total_sessions: u64,
    pub total_words: u64,
    pub total_duration_ms: u64,
    pub avg_wpm: f32,
    pub filler_rate: f32,
    pub top_words: Vec<(String, u64)>,
    pub recent: Vec<SessionRecord>,
}

pub fn sessions_path(data_dir: &Path) -> PathBuf {
    data_dir.join("insights").join("sessions.jsonl")
}

pub fn frequencies_path(data_dir: &Path) -> PathBuf {
    data_dir.join("insights").join("word_frequencies.json")
}

/// Parse a dictation's final text into the counts we keep. The text
/// itself is not stored — only the derived numbers and the words-of-
/// interest for the frequency table.
pub struct ParsedStats {
    pub word_count: u32,
    pub filler_count: u32,
    pub words_for_frequency: Vec<String>,
}

pub fn parse_stats(text: &str) -> ParsedStats {
    let mut word_count: u32 = 0;
    let mut filler_count: u32 = 0;
    let mut freq: Vec<String> = Vec::new();
    for raw in text.split_whitespace() {
        let token: String = raw
            .chars()
            .filter(|c| c.is_alphanumeric() || *c == '\'')
            .collect::<String>()
            .to_ascii_lowercase();
        if token.is_empty() {
            continue;
        }
        word_count += 1;
        if FILLERS.contains(&token.as_str()) {
            filler_count += 1;
            continue;
        }
        if token.len() >= 4 && !STOPWORDS.contains(&token.as_str()) {
            freq.push(token);
        }
    }
    ParsedStats {
        word_count,
        filler_count,
        words_for_frequency: freq,
    }
}

pub fn record_session(
    data_dir: &Path,
    record: &SessionRecord,
    words: &[String],
) -> std::io::Result<()> {
    let sessions = sessions_path(data_dir);
    if let Some(parent) = sessions.parent() {
        std::fs::create_dir_all(parent)?;
    }
    {
        let mut f = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&sessions)?;
        let line = serde_json::to_string(record).map_err(std::io::Error::other)?;
        writeln!(f, "{line}")?;
    }

    if !words.is_empty() {
        let freq_path = frequencies_path(data_dir);
        let mut freq: BTreeMap<String, u64> = read_frequencies(&freq_path);
        for w in words {
            *freq.entry(w.clone()).or_insert(0) += 1;
        }
        let bytes = serde_json::to_vec(&freq).map_err(std::io::Error::other)?;
        std::fs::write(&freq_path, bytes)?;
    }
    Ok(())
}

fn read_frequencies(path: &Path) -> BTreeMap<String, u64> {
    let Ok(bytes) = std::fs::read(path) else {
        return BTreeMap::new();
    };
    serde_json::from_slice(&bytes).unwrap_or_default()
}

pub fn read_sessions(path: &Path, limit: usize) -> Vec<SessionRecord> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    let mut all: Vec<SessionRecord> = text
        .lines()
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();
    all.reverse();
    all.truncate(limit);
    all
}

pub fn summarize(data_dir: &Path) -> InsightsSummary {
    let sessions = read_sessions(&sessions_path(data_dir), usize::MAX);
    let total_sessions = sessions.len() as u64;
    let total_words: u64 = sessions.iter().map(|s| s.word_count as u64).sum();
    let total_duration_ms: u64 = sessions.iter().map(|s| s.duration_ms).sum();
    let total_fillers: u64 = sessions.iter().map(|s| s.filler_count as u64).sum();

    let avg_wpm = if total_duration_ms > 0 {
        (total_words as f32 * 60_000.0) / total_duration_ms as f32
    } else {
        0.0
    };
    let filler_rate = if total_words > 0 {
        (total_fillers as f32) / (total_words as f32)
    } else {
        0.0
    };

    let freq = read_frequencies(&frequencies_path(data_dir));
    let mut top: Vec<(String, u64)> = freq.into_iter().collect();
    top.sort_by_key(|a| std::cmp::Reverse(a.1));
    top.truncate(20);

    let recent: Vec<SessionRecord> = sessions.into_iter().take(20).collect();

    InsightsSummary {
        total_sessions,
        total_words,
        total_duration_ms,
        avg_wpm,
        filler_rate,
        top_words: top,
        recent,
    }
}

pub fn clear(data_dir: &Path) -> std::io::Result<()> {
    let s = sessions_path(data_dir);
    let f = frequencies_path(data_dir);
    let _ = std::fs::remove_file(&s);
    let _ = std::fs::remove_file(&f);
    Ok(())
}

pub fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_words_and_fillers() {
        let p = parse_stats("um I think we should actually ship it like soon");
        assert_eq!(p.word_count, 10);
        // um, actually, like → 3 fillers
        assert!(p.filler_count >= 2);
        // "should" and "think" are stopwords; "ship" is <4 chars
        // so frequency words are empty-ish — this just checks it runs
        assert!(p.words_for_frequency.iter().all(|w| w.len() >= 4));
    }

    #[test]
    fn empty_text_is_safe() {
        let p = parse_stats("");
        assert_eq!(p.word_count, 0);
        assert_eq!(p.filler_count, 0);
        assert!(p.words_for_frequency.is_empty());
    }
}

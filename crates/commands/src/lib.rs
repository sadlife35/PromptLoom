//! Deterministic voice-dictation commands.
//!
//! A rules-based parser that scans the raw whisper transcript for spoken
//! commands and rewrites them to their literal counterparts. Runs BEFORE
//! polish/capitalize so downstream stages see the finished text.
//!
//! Example:
//!   `"hello comma world new paragraph goodbye"` → `"hello, world\n\ngoodbye"`
//!
//! Three design rules keep this honest:
//!   1. Case-insensitive match on whole word tokens only — never fragments.
//!   2. Each command's replacement is emitted verbatim; no spaces around
//!      punctuation unless the user said one.
//!   3. Unknown "command-shaped" tokens pass through unchanged.
//!      When in doubt, the parser does nothing.
//!
//! The transcript mode gates which commands fire. Code/Terminal mode skip
//! sentence punctuation ("period", "comma") because they would damage code
//! more often than they help.

use serde::{Deserialize, Serialize};

/// Which family of substitutions applies at parse time.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TranscriptMode {
    /// Default prose dictation: all commands active.
    Normal,
    /// Code editor target: skip sentence punctuation, keep structural
    /// commands (new line, brackets, symbols). Preserves verbatim case.
    Code,
    /// Terminal target: like Code, no auto-caps, no auto-period.
    Terminal,
}

impl Default for TranscriptMode {
    fn default() -> Self {
        TranscriptMode::Normal
    }
}

impl TranscriptMode {
    /// Should sentence-level punctuation commands fire in this mode?
    /// (comma / period / question mark / exclamation point / colon /
    /// semicolon / quote.)
    pub fn sentence_punct_active(self) -> bool {
        matches!(self, TranscriptMode::Normal)
    }

    /// Should the downstream pipeline run sentence capitalization?
    pub fn capitalize_sentences(self) -> bool {
        matches!(self, TranscriptMode::Normal)
    }
}

/// A single command: match the spoken phrase (lowercased, as whole tokens
/// separated by spaces), emit the literal in its place.
struct Command {
    phrase: &'static [&'static str],
    replacement: &'static str,
    /// If false, this command is skipped when `mode.sentence_punct_active()`
    /// returns false. Used for prose punctuation that would damage code.
    sentence_punct: bool,
}

const COMMANDS: &[Command] = &[
    // Punctuation (prose only).
    Command { phrase: &["comma"], replacement: ",", sentence_punct: true },
    Command { phrase: &["period"], replacement: ".", sentence_punct: true },
    Command { phrase: &["full", "stop"], replacement: ".", sentence_punct: true },
    Command { phrase: &["question", "mark"], replacement: "?", sentence_punct: true },
    Command { phrase: &["exclamation", "point"], replacement: "!", sentence_punct: true },
    Command { phrase: &["exclamation", "mark"], replacement: "!", sentence_punct: true },
    Command { phrase: &["colon"], replacement: ":", sentence_punct: true },
    Command { phrase: &["semicolon"], replacement: ";", sentence_punct: true },
    Command { phrase: &["open", "quote"], replacement: "\"", sentence_punct: true },
    Command { phrase: &["close", "quote"], replacement: "\"", sentence_punct: true },
    Command { phrase: &["apostrophe"], replacement: "'", sentence_punct: true },

    // Structural (all modes).
    Command { phrase: &["new", "line"], replacement: "\n", sentence_punct: false },
    Command { phrase: &["new", "paragraph"], replacement: "\n\n", sentence_punct: false },
    Command { phrase: &["tab"], replacement: "\t", sentence_punct: false },
    Command { phrase: &["space"], replacement: " ", sentence_punct: false },

    // Brackets (all modes).
    Command { phrase: &["open", "paren"], replacement: "(", sentence_punct: false },
    Command { phrase: &["close", "paren"], replacement: ")", sentence_punct: false },
    Command { phrase: &["open", "parenthesis"], replacement: "(", sentence_punct: false },
    Command { phrase: &["close", "parenthesis"], replacement: ")", sentence_punct: false },
    Command { phrase: &["open", "bracket"], replacement: "[", sentence_punct: false },
    Command { phrase: &["close", "bracket"], replacement: "]", sentence_punct: false },
    Command { phrase: &["open", "brace"], replacement: "{", sentence_punct: false },
    Command { phrase: &["close", "brace"], replacement: "}", sentence_punct: false },
    Command { phrase: &["open", "angle", "bracket"], replacement: "<", sentence_punct: false },
    Command { phrase: &["close", "angle", "bracket"], replacement: ">", sentence_punct: false },

    // Symbols (all modes).
    Command { phrase: &["backslash"], replacement: "\\", sentence_punct: false },
    Command { phrase: &["forward", "slash"], replacement: "/", sentence_punct: false },
    Command { phrase: &["slash"], replacement: "/", sentence_punct: false },
    Command { phrase: &["at", "sign"], replacement: "@", sentence_punct: false },
    Command { phrase: &["hashtag"], replacement: "#", sentence_punct: false },
    Command { phrase: &["pound", "sign"], replacement: "#", sentence_punct: false },
    Command { phrase: &["dollar", "sign"], replacement: "$", sentence_punct: false },
    Command { phrase: &["percent", "sign"], replacement: "%", sentence_punct: false },
    Command { phrase: &["ampersand"], replacement: "&", sentence_punct: false },
    Command { phrase: &["asterisk"], replacement: "*", sentence_punct: false },
    Command { phrase: &["dash"], replacement: "-", sentence_punct: false },
    Command { phrase: &["hyphen"], replacement: "-", sentence_punct: false },
    Command { phrase: &["underscore"], replacement: "_", sentence_punct: false },
    Command { phrase: &["equals", "sign"], replacement: "=", sentence_punct: false },
    Command { phrase: &["plus", "sign"], replacement: "+", sentence_punct: false },
    Command { phrase: &["pipe"], replacement: "|", sentence_punct: false },
    Command { phrase: &["tilde"], replacement: "~", sentence_punct: false },
    Command { phrase: &["caret"], replacement: "^", sentence_punct: false },
];

/// Parse the transcript, replacing recognized command phrases with their
/// literal replacements. Also handles the `all caps … end caps` modifier.
pub fn apply(text: &str, mode: TranscriptMode) -> String {
    if text.is_empty() {
        return String::new();
    }

    // Tokenize on whitespace; keep original casing for words that pass through.
    let tokens: Vec<&str> = text.split_whitespace().collect();
    let n = tokens.len();
    let lower: Vec<String> = tokens.iter().map(|t| strip_punct_lower(t)).collect();

    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    let mut in_all_caps = false;

    while i < n {
        // "all caps" starts an uppercase block.
        if i + 1 < n && lower[i] == "all" && lower[i + 1] == "caps" {
            in_all_caps = true;
            i += 2;
            continue;
        }
        // "end caps" closes it.
        if i + 1 < n && lower[i] == "end" && lower[i + 1] == "caps" {
            in_all_caps = false;
            i += 2;
            continue;
        }

        // Longest match wins.
        let matched = find_command(&lower[i..n], mode);
        if let Some((cmd_len, replacement)) = matched {
            // Punctuation attaches to the preceding word (no leading space).
            if is_attaching_punct(replacement) {
                trim_trailing_space(&mut out);
                out.push_str(replacement);
            } else {
                maybe_space(&mut out);
                out.push_str(replacement);
            }
            i += cmd_len;
            continue;
        }

        // Not a command — emit the original token (uppercased if we're
        // inside an all-caps block).
        maybe_space(&mut out);
        if in_all_caps {
            out.push_str(&tokens[i].to_uppercase());
        } else {
            out.push_str(tokens[i]);
        }
        i += 1;
    }

    out
}

fn find_command(window: &[String], mode: TranscriptMode) -> Option<(usize, &'static str)> {
    // Longest match first — scan commands in decreasing phrase length.
    let mut best: Option<(usize, &'static str)> = None;
    for cmd in COMMANDS {
        if cmd.sentence_punct && !mode.sentence_punct_active() {
            continue;
        }
        let len = cmd.phrase.len();
        if len > window.len() {
            continue;
        }
        let ok = cmd
            .phrase
            .iter()
            .enumerate()
            .all(|(k, w)| window[k].eq_ignore_ascii_case(w));
        if ok && best.map(|(bl, _)| len > bl).unwrap_or(true) {
            best = Some((len, cmd.replacement));
        }
    }
    best
}

fn strip_punct_lower(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

/// Punctuation that should attach to the preceding word with no space before it.
fn is_attaching_punct(repl: &str) -> bool {
    matches!(repl, "," | "." | "?" | "!" | ":" | ";")
}

fn maybe_space(out: &mut String) {
    if !out.is_empty() {
        let last = out.chars().last().unwrap();
        if !matches!(last, ' ' | '\n' | '\t') {
            out.push(' ');
        }
    }
}

fn trim_trailing_space(out: &mut String) {
    while out.ends_with(' ') {
        out.pop();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn normal(text: &str) -> String {
        apply(text, TranscriptMode::Normal)
    }
    fn code(text: &str) -> String {
        apply(text, TranscriptMode::Code)
    }

    #[test]
    fn basic_comma_period() {
        assert_eq!(normal("hello comma world period"), "hello, world.");
    }

    #[test]
    fn question_and_exclamation() {
        assert_eq!(
            normal("really question mark yes exclamation point"),
            "really? yes!"
        );
    }

    #[test]
    fn new_line_and_paragraph() {
        assert_eq!(
            normal("alpha new line beta new paragraph gamma"),
            "alpha\nbeta\n\ngamma"
        );
    }

    #[test]
    fn brackets() {
        assert_eq!(
            normal("open paren x comma y close paren"),
            "(x, y)"
        );
        assert_eq!(
            normal("open brace close brace open bracket close bracket"),
            "{ } [ ]"
        );
    }

    #[test]
    fn symbols() {
        assert_eq!(normal("backslash n"), "\\ n");
        assert_eq!(normal("at sign example dot com"), "@ example dot com");
        assert_eq!(normal("dollar sign five"), "$ five");
    }

    #[test]
    fn all_caps_block() {
        assert_eq!(
            normal("whisper all caps shout here end caps stop"),
            "whisper SHOUT HERE stop"
        );
    }

    #[test]
    fn code_mode_skips_sentence_punct() {
        // In Code mode "comma" and "period" are NOT substituted — they stay
        // as words so you don't get code full of accidental punctuation.
        assert_eq!(
            code("let x comma y equals sign five"),
            "let x comma y = five"
        );
    }

    #[test]
    fn code_mode_keeps_structural_commands() {
        assert_eq!(
            code("fn new paragraph open brace close brace"),
            "fn\n\n{ }"
        );
    }

    #[test]
    fn unknown_tokens_pass_through() {
        assert_eq!(
            normal("the quick brown fox"),
            "the quick brown fox"
        );
    }

    #[test]
    fn empty_input() {
        assert_eq!(normal(""), "");
    }

    #[test]
    fn attaching_punct_has_no_leading_space() {
        assert_eq!(normal("hello comma"), "hello,");
        assert_eq!(normal("yes period"), "yes.");
    }

    #[test]
    fn longest_match_wins() {
        // "exclamation point" should fire, not just "exclamation".
        assert_eq!(normal("yes exclamation point"), "yes!");
        assert_eq!(normal("full stop here"), ". here");
    }

    #[test]
    fn case_insensitive_match() {
        assert_eq!(normal("Hello Comma World Period"), "Hello, World.");
    }

    #[test]
    fn mixed_case_preserved_outside_all_caps() {
        assert_eq!(
            normal("hello MixedCase world"),
            "hello MixedCase world"
        );
    }

    #[test]
    fn newline_does_not_get_leading_space() {
        assert_eq!(normal("one new line two"), "one\ntwo");
    }

    #[test]
    fn no_double_space() {
        assert_eq!(normal("one new paragraph two"), "one\n\ntwo");
    }

    #[test]
    fn full_realistic_sentence() {
        assert_eq!(
            normal("deploy comma then test new line finally ship period"),
            "deploy, then test\nfinally ship."
        );
    }
}

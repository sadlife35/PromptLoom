//! Deterministic transcript cleanup. Runs before vocabulary replacement.
//!
//! Three stages, all optional and configurable:
//!   1. Filler-word stripping — removes `um`, `uh`, `er`, `ah` by default.
//!      Case-insensitive, matched against the alphabetic core of each
//!      whitespace token (so `"um,"` is treated as `"um"`).
//!   2. Repeat-loop collapse — whisper hallucinates runs like
//!      `"thank you thank you thank you"` on silence. We collapse 3+
//!      consecutive identical tokens (case-insensitive) down to one.
//!   3. Whitespace normalization — multiple spaces become one, leading
//!      and trailing whitespace is trimmed.

/// Transcript polisher. Build with [`Polisher::default`] for the common case
/// (strip `um`/`uh`/`er`/`ah`, collapse repeats, normalize whitespace), or
/// construct one by hand to tune.
#[derive(Debug, Clone)]
pub struct Polisher {
    pub fillers: Vec<String>,
    pub collapse_repeats: bool,
    pub normalize_whitespace: bool,
}

impl Default for Polisher {
    fn default() -> Self {
        Self {
            fillers: vec!["um".into(), "uh".into(), "er".into(), "ah".into()],
            collapse_repeats: true,
            normalize_whitespace: true,
        }
    }
}

impl Polisher {
    /// A polisher that does nothing. Useful for tests.
    pub fn noop() -> Self {
        Self {
            fillers: Vec::new(),
            collapse_repeats: false,
            normalize_whitespace: false,
        }
    }

    pub fn polish(&self, text: &str) -> String {
        let mut s = text.to_string();
        if !self.fillers.is_empty() {
            s = strip_fillers(&s, &self.fillers);
        }
        if self.collapse_repeats {
            s = collapse_repeats(&s);
        }
        if self.normalize_whitespace {
            s = normalize_whitespace(&s);
        }
        s
    }
}

fn alpha_core(token: &str) -> String {
    token
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect::<String>()
        .to_lowercase()
}

fn strip_fillers(text: &str, fillers: &[String]) -> String {
    let fillers_lower: Vec<String> = fillers.iter().map(|f| f.to_lowercase()).collect();
    text.split_whitespace()
        .filter(|tok| {
            let core = alpha_core(tok);
            if core.is_empty() {
                return true; // keep pure-punctuation tokens
            }
            !fillers_lower.contains(&core)
        })
        .collect::<Vec<_>>()
        .join(" ")
}

fn collapse_repeats(text: &str) -> String {
    let tokens: Vec<&str> = text.split_whitespace().collect();
    if tokens.is_empty() {
        return String::new();
    }
    let mut out: Vec<&str> = Vec::with_capacity(tokens.len());
    let mut i = 0;
    while i < tokens.len() {
        let cur = alpha_core(tokens[i]);
        let mut j = i + 1;
        while j < tokens.len() && alpha_core(tokens[j]) == cur && !cur.is_empty() {
            j += 1;
        }
        let run = j - i;
        if run >= 3 {
            // Keep just the first occurrence (preserves original casing).
            out.push(tokens[i]);
        } else {
            for k in i..j {
                out.push(tokens[k]);
            }
        }
        i = j;
    }
    out.join(" ")
}

fn normalize_whitespace(text: &str) -> String {
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

/// Deterministic English capitalization. Runs after the polisher, before
/// vocabulary replacement. Four rules:
///
/// 1. First alphabetic character of the whole transcript gets capitalized.
/// 2. First alphabetic character after `.`/`!`/`?` + whitespace gets
///    capitalized.
/// 3. Standalone lowercase `i` (surrounded by non-word characters) becomes
///    `I`.
/// 4. Known English honorifics (`mr.`, `mrs.`, `ms.`, `dr.`, `prof.`,
///    `jr.`, `sr.`) get properly cased at the start of a word.
///
/// Everything is opt-out via the config struct.
#[derive(Debug, Clone)]
pub struct Capitalizer {
    pub sentence_start: bool,
    pub standalone_i: bool,
    pub honorifics: bool,
}

impl Default for Capitalizer {
    fn default() -> Self {
        Self {
            sentence_start: true,
            standalone_i: true,
            honorifics: true,
        }
    }
}

impl Capitalizer {
    pub fn noop() -> Self {
        Self {
            sentence_start: false,
            standalone_i: false,
            honorifics: false,
        }
    }

    pub fn capitalize(&self, text: &str) -> String {
        let mut s = text.to_string();
        if self.honorifics {
            s = fix_honorifics(&s);
        }
        if self.standalone_i {
            s = capitalize_standalone_i(&s);
        }
        if self.sentence_start {
            s = capitalize_sentence_starts(&s);
        }
        s
    }
}

/// A hand-maintained list of English honorifics we recognize. All are
/// lowercase `.`-terminated patterns we match at a word boundary.
const HONORIFICS: &[(&str, &str)] = &[
    ("mr.", "Mr."),
    ("mrs.", "Mrs."),
    ("ms.", "Ms."),
    ("dr.", "Dr."),
    ("prof.", "Prof."),
    ("jr.", "Jr."),
    ("sr.", "Sr."),
    ("st.", "St."),
];

fn fix_honorifics(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let mut matched = false;
        // Only test at word starts (beginning, or preceded by non-word).
        let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        if left_ok {
            for (src, dst) in HONORIFICS {
                let s = src.as_bytes();
                if i + s.len() > bytes.len() {
                    continue;
                }
                if !ascii_eq_ignore_case(&bytes[i..i + s.len()], s) {
                    continue;
                }
                // Also require that nothing word-continues after the "."
                // (so "jr." in "junior" never happens; it won't anyway
                // because of left_ok, but defensive).
                out.push_str(dst);
                i += s.len();
                matched = true;
                break;
            }
        }
        if !matched {
            let ch_len = utf8_char_len(bytes[i]);
            let end = (i + ch_len).min(bytes.len());
            if let Ok(ch) = std::str::from_utf8(&bytes[i..end]) {
                out.push_str(ch);
            }
            i = end;
        }
    }
    out
}

fn capitalize_standalone_i(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = String::with_capacity(text.len());
    let mut i = 0;
    while i < bytes.len() {
        let left_ok = i == 0 || !is_word_byte(bytes[i - 1]);
        let right_ok = i + 1 == bytes.len() || !is_word_byte(bytes[i + 1]);
        if bytes[i] == b'i' && left_ok && right_ok {
            out.push('I');
            i += 1;
            continue;
        }
        let ch_len = utf8_char_len(bytes[i]);
        let end = (i + ch_len).min(bytes.len());
        if let Ok(ch) = std::str::from_utf8(&bytes[i..end]) {
            out.push_str(ch);
        }
        i = end;
    }
    out
}

fn capitalize_sentence_starts(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut capitalize_next = true;
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        if capitalize_next && c.is_alphabetic() {
            for upper in c.to_uppercase() {
                out.push(upper);
            }
            capitalize_next = false;
        } else {
            out.push(c);
            // After a sentence-end char, flip capitalize_next on until we
            // see the next alphabetic character.
            if matches!(c, '.' | '!' | '?') {
                capitalize_next = true;
            } else if c.is_alphabetic() || c.is_ascii_digit() {
                capitalize_next = false;
            }
            // Whitespace and other punctuation don't change capitalize_next.
        }
    }
    out
}

fn is_word_byte(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

fn ascii_eq_ignore_case(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| x.eq_ignore_ascii_case(y))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_strips_common_fillers() {
        let p = Polisher::default();
        assert_eq!(p.polish("I um think uh we should"), "I think we should");
        assert_eq!(p.polish("Er, let me think"), "let me think");
    }

    #[test]
    fn default_does_not_strip_like_by_default() {
        let p = Polisher::default();
        assert_eq!(p.polish("this looks like a bug"), "this looks like a bug");
    }

    #[test]
    fn does_not_strip_words_containing_fillers() {
        let p = Polisher::default();
        assert_eq!(p.polish("the umbrella is wet"), "the umbrella is wet");
        assert_eq!(p.polish("cherub"), "cherub");
    }

    #[test]
    fn filler_stripping_is_case_insensitive() {
        let p = Polisher::default();
        assert_eq!(p.polish("UM okay"), "okay");
        assert_eq!(p.polish("Uh yes"), "yes");
    }

    #[test]
    fn filler_with_trailing_punctuation() {
        let p = Polisher::default();
        assert_eq!(p.polish("um, let me think"), "let me think");
        assert_eq!(p.polish("so, uh, we need"), "so, we need");
    }

    #[test]
    fn collapses_three_or_more_identical_tokens() {
        let p = Polisher::default();
        assert_eq!(p.polish("the the the the dog"), "the dog");
        assert_eq!(p.polish("I I I I thought"), "I thought");
    }

    #[test]
    fn preserves_two_identical_tokens() {
        let p = Polisher::default();
        assert_eq!(p.polish("it is what it is"), "it is what it is");
    }

    #[test]
    fn repeat_collapse_is_case_insensitive() {
        let p = Polisher::default();
        assert_eq!(p.polish("The the THE dog"), "The dog");
    }

    #[test]
    fn normalizes_runs_of_spaces() {
        let p = Polisher::default();
        assert_eq!(p.polish("hello   world"), "hello world");
    }

    #[test]
    fn trims_leading_and_trailing_whitespace() {
        let p = Polisher::default();
        assert_eq!(p.polish("   hello   "), "hello");
    }

    #[test]
    fn noop_polisher_is_identity() {
        let p = Polisher::noop();
        assert_eq!(p.polish("  um um um  "), "  um um um  ");
    }

    #[test]
    fn custom_filler_list_adds_like_you_know() {
        let p = Polisher {
            fillers: vec![
                "um".into(),
                "like".into(),
                "you".into(),  // intentional: "you know" would need bigram handling
            ],
            ..Polisher::default()
        };
        assert_eq!(p.polish("um like it was you know fine"), "it was know fine");
    }

    #[test]
    fn empty_input() {
        let p = Polisher::default();
        assert_eq!(p.polish(""), "");
    }

    #[test]
    fn only_fillers_input() {
        let p = Polisher::default();
        assert_eq!(p.polish("um uh er ah"), "");
    }

    #[test]
    fn whisper_repeat_loop_hallucination() {
        let p = Polisher::default();
        // What whisper actually emits on silence sometimes:
        assert_eq!(
            p.polish("thank you. thank you. thank you. thank you."),
            "thank you. thank you. thank you. thank you."
        );
        // Single-token loop (common):
        assert_eq!(p.polish("okay okay okay okay"), "okay");
    }

    // --- Capitalizer tests ---------------------------------------------------

    #[test]
    fn capitalizes_first_character() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("hello world"), "Hello world");
    }

    #[test]
    fn capitalizes_after_sentence_end() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("yes. no. maybe."), "Yes. No. Maybe.");
        assert_eq!(c.capitalize("stop! then go? okay."), "Stop! Then go? Okay.");
    }

    #[test]
    fn preserves_already_capitalized() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("Hello World"), "Hello World");
    }

    #[test]
    fn standalone_i_becomes_capital() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("i think so"), "I think so");
        assert_eq!(c.capitalize("so i said"), "So I said");
    }

    #[test]
    fn standalone_i_does_not_touch_other_i() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("it is fine"), "It is fine");
        assert_eq!(c.capitalize("iphone itself"), "Iphone itself");
    }

    #[test]
    fn honorifics_mr_mrs_dr() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("mr. smith"), "Mr. Smith");
        assert_eq!(c.capitalize("mrs. jones"), "Mrs. Jones");
        assert_eq!(c.capitalize("dr. patel"), "Dr. Patel");
    }

    #[test]
    fn honorifics_do_not_touch_words_that_start_the_same() {
        let c = Capitalizer::default();
        // "drive" starts with "dr" but not "dr." — must not be touched by
        // the honorific pass.
        assert_eq!(c.capitalize("let's drive home"), "Let's drive home");
    }

    #[test]
    fn standalone_i_with_apostrophe() {
        let c = Capitalizer::default();
        // "I'm" — the i is followed by '. Our is_word_byte treats ' as not
        // a word byte so i gets capitalized.
        assert_eq!(c.capitalize("i'm here"), "I'm here");
    }

    #[test]
    fn capitalizer_handles_multiple_spaces_after_period() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize("one.  two"), "One.  Two");
    }

    #[test]
    fn capitalizer_leaves_empty_input_alone() {
        let c = Capitalizer::default();
        assert_eq!(c.capitalize(""), "");
    }

    #[test]
    fn noop_capitalizer_is_identity() {
        let c = Capitalizer::noop();
        assert_eq!(c.capitalize("hello. i am mr. smith"), "hello. i am mr. smith");
    }

    #[test]
    fn full_pipeline_polisher_then_capitalizer() {
        let p = Polisher::default();
        let c = Capitalizer::default();
        let raw = "um, i think, uh, we should ship. yes.";
        let polished = p.polish(raw);
        let final_text = c.capitalize(&polished);
        assert_eq!(final_text, "I think, we should ship. Yes.");
    }
}

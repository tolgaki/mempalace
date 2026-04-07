use regex::Regex;
use std::collections::HashSet;
use std::sync::OnceLock;

#[allow(dead_code)]
static HAS_DIGIT: OnceLock<Regex> = OnceLock::new();
#[allow(dead_code)]
static IS_CAMEL: OnceLock<Regex> = OnceLock::new();
#[allow(dead_code)]
static IS_ALLCAPS: OnceLock<Regex> = OnceLock::new();
#[allow(dead_code)]
static IS_TECHNICAL: OnceLock<Regex> = OnceLock::new();
#[allow(dead_code)]
static IS_URL: OnceLock<Regex> = OnceLock::new();
#[allow(dead_code)]
static TOKEN_RE: OnceLock<Regex> = OnceLock::new();

#[allow(dead_code)]
fn has_digit() -> &'static Regex {
    HAS_DIGIT.get_or_init(|| Regex::new(r"\d").unwrap())
}
#[allow(dead_code)]
fn is_camel() -> &'static Regex {
    IS_CAMEL.get_or_init(|| Regex::new(r"[A-Z][a-z]+[A-Z]").unwrap())
}
#[allow(dead_code)]
fn is_allcaps() -> &'static Regex {
    IS_ALLCAPS.get_or_init(|| Regex::new(r"^[A-Z_@#$%^&*()+=\[\]{}|<>?.:/\\]+$").unwrap())
}
#[allow(dead_code)]
fn is_technical() -> &'static Regex {
    IS_TECHNICAL.get_or_init(|| Regex::new(r"[-_]").unwrap())
}
#[allow(dead_code)]
fn is_url() -> &'static Regex {
    IS_URL.get_or_init(|| Regex::new(r"(?i)https?://|www\.|/Users/|~/|\.[a-z]{2,4}$").unwrap())
}
#[allow(dead_code)]
fn token_re() -> &'static Regex {
    TOKEN_RE.get_or_init(|| Regex::new(r"\S+").unwrap())
}

#[allow(dead_code)]
const MIN_LENGTH: usize = 4;

#[allow(dead_code)]
fn should_skip(token: &str, known_names: &HashSet<String>) -> bool {
    if token.len() < MIN_LENGTH {
        return true;
    }
    if has_digit().is_match(token) {
        return true;
    }
    if is_camel().is_match(token) {
        return true;
    }
    if is_allcaps().is_match(token) {
        return true;
    }
    if is_technical().is_match(token) {
        return true;
    }
    if is_url().is_match(token) {
        return true;
    }
    if known_names.contains(&token.to_lowercase()) {
        return true;
    }
    false
}

/// Levenshtein edit distance between two strings.
pub fn edit_distance(a: &str, b: &str) -> usize {
    if a == b {
        return 0;
    }
    if a.is_empty() {
        return b.len();
    }
    if b.is_empty() {
        return a.len();
    }
    let b_chars: Vec<char> = b.chars().collect();
    let mut prev: Vec<usize> = (0..=b_chars.len()).collect();
    for (i, ca) in a.chars().enumerate() {
        let mut curr = vec![i + 1];
        for (j, &cb) in b_chars.iter().enumerate() {
            let cost = if ca == cb { 0 } else { 1 };
            curr.push((prev[j + 1] + 1).min(curr[j] + 1).min(prev[j] + cost));
        }
        prev = curr;
    }
    prev[b_chars.len()]
}

/// Spell-correct user text. Since we don't have the Python `autocorrect` library,
/// this is a pass-through that preserves known names and technical terms.
/// In a full implementation, a dictionary-based corrector would be plugged in here.
pub fn spellcheck_user_text(text: &str, known_names: Option<&HashSet<String>>) -> String {
    let empty = HashSet::new();
    let _names = known_names.unwrap_or(&empty);
    // Without an autocorrect dictionary, pass through unchanged.
    // The structure is here for future enhancement.
    text.to_string()
}

/// Spell-correct a single transcript line.
/// Only touches lines starting with '>' (user turns).
pub fn spellcheck_transcript_line(line: &str, known_names: Option<&HashSet<String>>) -> String {
    let stripped = line.trim_start();
    if !stripped.starts_with('>') {
        return line.to_string();
    }
    let prefix_len = line.len() - stripped.len() + 2; // "> "
    if prefix_len >= line.len() {
        return line.to_string();
    }
    let message = &line[prefix_len..];
    if message.trim().is_empty() {
        return line.to_string();
    }
    let corrected = spellcheck_user_text(message, known_names);
    format!("{}{}", &line[..prefix_len], corrected)
}

/// Spell-correct all user turns in a full transcript.
pub fn spellcheck_transcript(content: &str, known_names: Option<&HashSet<String>>) -> String {
    content
        .lines()
        .map(|line| spellcheck_transcript_line(line, known_names))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_edit_distance_identical() {
        assert_eq!(edit_distance("hello", "hello"), 0);
    }

    #[test]
    fn test_edit_distance_one_off() {
        assert_eq!(edit_distance("hello", "hallo"), 1);
    }

    #[test]
    fn test_edit_distance_empty() {
        assert_eq!(edit_distance("", "abc"), 3);
        assert_eq!(edit_distance("abc", ""), 3);
    }

    #[test]
    fn test_should_skip_short() {
        let names = HashSet::new();
        assert!(should_skip("hi", &names));
        assert!(should_skip("ok", &names));
    }

    #[test]
    fn test_should_skip_digits() {
        let names = HashSet::new();
        assert!(should_skip("test123", &names));
        assert!(should_skip("3am", &names));
    }

    #[test]
    fn test_should_skip_camelcase() {
        let names = HashSet::new();
        assert!(should_skip("ChromaDB", &names));
        assert!(should_skip("MemPalace", &names));
    }

    #[test]
    fn test_should_skip_technical() {
        let names = HashSet::new();
        assert!(should_skip("bge-large", &names));
        assert!(should_skip("train_test", &names));
    }

    #[test]
    fn test_should_skip_known_name() {
        let mut names = HashSet::new();
        names.insert("riley".to_string());
        assert!(should_skip("Riley", &names));
    }

    #[test]
    fn test_spellcheck_passthrough() {
        let result = spellcheck_user_text("hello world", None);
        assert_eq!(result, "hello world");
    }

    #[test]
    fn test_transcript_line_user_turn() {
        let line = "> hello world";
        let result = spellcheck_transcript_line(line, None);
        assert_eq!(result, "> hello world");
    }

    #[test]
    fn test_transcript_line_assistant_turn() {
        let line = "This is the assistant response.";
        let result = spellcheck_transcript_line(line, None);
        assert_eq!(result, line);
    }

    #[test]
    fn test_spellcheck_transcript() {
        let content = "> user message\nassistant response\n> another user message";
        let result = spellcheck_transcript(content, None);
        assert_eq!(result, content);
    }

    #[test]
    fn test_should_skip_allcaps() {
        let names = HashSet::new();
        assert!(should_skip("NDCG", &names));
    }
}

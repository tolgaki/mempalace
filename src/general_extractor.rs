//! general_extractor.rs — Extract 5 types of memories from text.
//!
//! Types:
//!   1. DECISIONS    — "we went with X because Y", choices made
//!   2. PREFERENCES  — "always use X", "never do Y", "I prefer Z"
//!   3. MILESTONES   — breakthroughs, things that finally worked
//!   4. PROBLEMS     — what broke, what fixed it, root causes
//!   5. EMOTIONAL    — feelings, vulnerability, relationships
//!
//! No LLM required. Pure keyword/pattern heuristics.

use regex::Regex;
use std::collections::{HashMap, HashSet};

// ── Data types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Memory {
    pub content: String,
    pub memory_type: String,
    pub chunk_index: usize,
}

// ── Marker sets ────────────────────────────────────────────────────────────

pub const DECISION_MARKERS: &[&str] = &[
    r"\blet'?s (use|go with|try|pick|choose|switch to)\b",
    r"\bwe (should|decided|chose|went with|picked|settled on)\b",
    r"\bi'?m going (to|with)\b",
    r"\bbetter (to|than|approach|option|choice)\b",
    r"\binstead of\b",
    r"\brather than\b",
    r"\bthe reason (is|was|being)\b",
    r"\bbecause\b",
    r"\btrade-?off\b",
    r"\bpros and cons\b",
    r"\bover\b.*\bbecause\b",
    r"\barchitecture\b",
    r"\bapproach\b",
    r"\bstrategy\b",
    r"\bpattern\b",
    r"\bstack\b",
    r"\bframework\b",
    r"\binfrastructure\b",
    r"\bset (it |this )?to\b",
    r"\bconfigure\b",
    r"\bdefault\b",
];

pub const PREFERENCE_MARKERS: &[&str] = &[
    r"\bi prefer\b",
    r"\balways use\b",
    r"\bnever use\b",
    r"\bdon'?t (ever |like to )?(use|do|mock|stub|import)\b",
    r"\bi like (to|when|how)\b",
    r"\bi hate (when|how|it when)\b",
    r"\bplease (always|never|don'?t)\b",
    r"\bmy (rule|preference|style|convention) is\b",
    r"\bwe (always|never)\b",
    r"\bfunctional\b.*\bstyle\b",
    r"\bimperative\b",
    r"\bsnake_?case\b",
    r"\bcamel_?case\b",
    r"\btabs\b.*\bspaces\b",
    r"\bspaces\b.*\btabs\b",
    r"\buse\b.*\binstead of\b",
];

pub const MILESTONE_MARKERS: &[&str] = &[
    r"\bit works\b",
    r"\bit worked\b",
    r"\bgot it working\b",
    r"\bfixed\b",
    r"\bsolved\b",
    r"\bbreakthrough\b",
    r"\bfigured (it )?out\b",
    r"\bnailed it\b",
    r"\bcracked (it|the)\b",
    r"\bfinally\b",
    r"\bfirst time\b",
    r"\bfirst ever\b",
    r"\bnever (done|been|had) before\b",
    r"\bdiscovered\b",
    r"\brealized\b",
    r"\bfound (out|that)\b",
    r"\bturns out\b",
    r"\bthe key (is|was|insight)\b",
    r"\bthe trick (is|was)\b",
    r"\bnow i (understand|see|get it)\b",
    r"\bbuilt\b",
    r"\bcreated\b",
    r"\bimplemented\b",
    r"\bshipped\b",
    r"\blaunched\b",
    r"\bdeployed\b",
    r"\breleased\b",
    r"\bprototype\b",
    r"\bproof of concept\b",
    r"\bdemo\b",
    r"\bversion \d",
    r"\bv\d+\.\d+",
    r"\d+x (compression|faster|slower|better|improvement|reduction)",
    r"\d+% (reduction|improvement|faster|better|smaller)",
];

pub const PROBLEM_MARKERS: &[&str] = &[
    r"\b(bug|error|crash|fail|broke|broken|issue|problem)\b",
    r"\bdoesn'?t work\b",
    r"\bnot working\b",
    r"\bwon'?t\b.*\bwork\b",
    r"\bkeeps? (failing|crashing|breaking|erroring)\b",
    r"\broot cause\b",
    r"\bthe (problem|issue|bug) (is|was)\b",
    r"\bturns out\b.*\b(was|because|due to)\b",
    r"\bthe fix (is|was)\b",
    r"\bworkaround\b",
    r"\bthat'?s why\b",
    r"\bthe reason it\b",
    r"\bfixed (it |the |by )\b",
    r"\bsolution (is|was)\b",
    r"\bresolved\b",
    r"\bpatched\b",
    r"\bthe answer (is|was)\b",
];

pub const EMOTION_MARKERS: &[&str] = &[
    r"\blove\b",
    r"\bscared\b",
    r"\bafraid\b",
    r"\bproud\b",
    r"\bhurt\b",
    r"\bhappy\b",
    r"\bsad\b",
    r"\bcry\b",
    r"\bcrying\b",
    r"\bmiss\b",
    r"\bsorry\b",
    r"\bgrateful\b",
    r"\bangry\b",
    r"\bworried\b",
    r"\blonely\b",
    r"\bbeautiful\b",
    r"\bamazing\b",
    r"\bwonderful\b",
    r"i feel",
    r"i'm sorry",
];

// ── Sentiment word sets ────────────────────────────────────────────────────

fn positive_words() -> HashSet<&'static str> {
    [
        "pride",
        "proud",
        "joy",
        "happy",
        "love",
        "loving",
        "beautiful",
        "amazing",
        "wonderful",
        "incredible",
        "fantastic",
        "brilliant",
        "perfect",
        "excited",
        "thrilled",
        "grateful",
        "warm",
        "breakthrough",
        "success",
        "works",
        "working",
        "solved",
        "fixed",
        "nailed",
        "heart",
        "hug",
        "precious",
        "adore",
    ]
    .into_iter()
    .collect()
}

fn negative_words() -> HashSet<&'static str> {
    [
        "bug",
        "error",
        "crash",
        "crashing",
        "crashed",
        "fail",
        "failed",
        "failing",
        "failure",
        "broken",
        "broke",
        "breaking",
        "breaks",
        "issue",
        "problem",
        "wrong",
        "stuck",
        "blocked",
        "unable",
        "impossible",
        "missing",
        "terrible",
        "horrible",
        "awful",
        "worse",
        "worst",
        "panic",
        "disaster",
        "mess",
    ]
    .into_iter()
    .collect()
}

// ── Scoring ────────────────────────────────────────────────────────────────

/// Score text against a set of regex marker patterns.
/// Returns `(score, matched_keywords)` where score is the total number of
/// individual matches across all patterns.
pub fn score_markers(text: &str, markers: &[&str]) -> (f64, Vec<String>) {
    let text_lower = text.to_lowercase();
    let mut score = 0.0;
    let mut keywords: HashSet<String> = HashSet::new();

    for pattern_str in markers {
        if let Ok(re) = Regex::new(pattern_str) {
            let matches: Vec<_> = re.find_iter(&text_lower).collect();
            if !matches.is_empty() {
                score += matches.len() as f64;
                for m in matches {
                    keywords.insert(m.as_str().to_string());
                }
            }
        }
    }

    let kw_vec: Vec<String> = keywords.into_iter().collect();
    (score, kw_vec)
}

// ── Sentiment ──────────────────────────────────────────────────────────────

/// Quick sentiment classification: "positive", "negative", or "neutral".
pub fn get_sentiment(text: &str) -> &'static str {
    let word_re = Regex::new(r"\b\w+\b").unwrap();
    let words: HashSet<String> = word_re
        .find_iter(&text.to_lowercase())
        .map(|m| m.as_str().to_string())
        .collect();

    let pos_set = positive_words();
    let neg_set = negative_words();

    let pos_count = words
        .iter()
        .filter(|w| pos_set.contains(w.as_str()))
        .count();
    let neg_count = words
        .iter()
        .filter(|w| neg_set.contains(w.as_str()))
        .count();

    if pos_count > neg_count {
        "positive"
    } else if neg_count > pos_count {
        "negative"
    } else {
        "neutral"
    }
}

// ── Resolution detection ───────────────────────────────────────────────────

/// Check if text describes a resolved problem.
pub fn has_resolution(text: &str) -> bool {
    let text_lower = text.to_lowercase();
    let patterns = [
        r"\bfixed\b",
        r"\bsolved\b",
        r"\bresolved\b",
        r"\bpatched\b",
        r"\bgot it working\b",
        r"\bit works\b",
        r"\bnailed it\b",
        r"\bfigured (it )?out\b",
        r"\bthe (fix|answer|solution)\b",
    ];
    for p in &patterns {
        if let Ok(re) = Regex::new(p) {
            if re.is_match(&text_lower) {
                return true;
            }
        }
    }
    false
}

// ── Disambiguation ─────────────────────────────────────────────────────────

/// Fix misclassifications using sentiment and resolution signals.
pub fn disambiguate(memory_type: &str, text: &str, scores: &HashMap<String, f64>) -> String {
    let sentiment = get_sentiment(text);

    // Resolved problems are milestones
    if memory_type == "problem" && has_resolution(text) {
        if scores.get("emotional").copied().unwrap_or(0.0) > 0.0 && sentiment == "positive" {
            return "emotional".to_string();
        }
        return "milestone".to_string();
    }

    // Problem + positive sentiment => milestone or emotional
    if memory_type == "problem" && sentiment == "positive" {
        if scores.get("milestone").copied().unwrap_or(0.0) > 0.0 {
            return "milestone".to_string();
        }
        if scores.get("emotional").copied().unwrap_or(0.0) > 0.0 {
            return "emotional".to_string();
        }
    }

    memory_type.to_string()
}

// ── Code-line detection ────────────────────────────────────────────────────

/// Returns true if a line looks like code rather than prose.
pub fn is_code_line(line: &str) -> bool {
    let stripped = line.trim();
    if stripped.is_empty() {
        return false;
    }

    let code_patterns = [
        r"^\s*[\$#]\s",
        r"^\s*(cd|source|echo|export|pip|npm|git|python|bash|curl|wget|mkdir|rm|cp|mv|ls|cat|grep|find|chmod|sudo|brew|docker)\s",
        r"^\s*```",
        r"^\s*(import|from|def|class|function|const|let|var|return)\s",
        r"^\s*[A-Z_]{2,}=",
        r"^\s*\|",
        r"^\s*[-]{2,}",
        r"^\s*[{}\[\]]\s*$",
        r"^\s*(if|for|while|try|except|elif|else:)\b",
        r"^\s*\w+\.\w+\(",
        r"^\s*\w+ = \w+\.\w+",
    ];

    for pat_str in &code_patterns {
        if let Ok(re) = Regex::new(pat_str) {
            if re.is_match(stripped) {
                return true;
            }
        }
    }

    // Low alphabetic ratio with sufficient length => likely code
    let alpha_count = stripped.chars().filter(|c| c.is_alphabetic()).count();
    let ratio = alpha_count as f64 / stripped.len().max(1) as f64;
    if ratio < 0.4 && stripped.len() > 10 {
        return true;
    }

    false
}

// ── Prose extraction ───────────────────────────────────────────────────────

/// Extract only prose lines from text, stripping code blocks and code lines.
pub fn extract_prose(text: &str) -> String {
    let lines: Vec<&str> = text.split('\n').collect();
    let mut prose = Vec::new();
    let mut in_code = false;

    for line in &lines {
        if line.trim().starts_with("```") {
            in_code = !in_code;
            continue;
        }
        if in_code {
            continue;
        }
        if !is_code_line(line) {
            prose.push(*line);
        }
    }

    let result = prose.join("\n").trim().to_string();
    if result.is_empty() {
        text.to_string()
    } else {
        result
    }
}

// ── Segment splitting ──────────────────────────────────────────────────────

/// Split text into segments suitable for memory extraction.
/// Tries speaker-turn splitting first, then falls back to paragraph splitting.
pub fn split_into_segments(text: &str) -> Vec<String> {
    let lines: Vec<&str> = text.split('\n').collect();

    let turn_patterns = [
        Regex::new(r"^>\s").unwrap(),
        Regex::new(r"(?i)^(Human|User|Q)\s*:").unwrap(),
        Regex::new(r"(?i)^(Assistant|AI|A|Claude|ChatGPT)\s*:").unwrap(),
    ];

    let mut turn_count = 0;
    for line in &lines {
        let stripped = line.trim();
        for pat in &turn_patterns {
            if pat.is_match(stripped) {
                turn_count += 1;
                break;
            }
        }
    }

    // If enough turn markers, split by turns
    if turn_count >= 3 {
        return split_by_turns(&lines, &turn_patterns);
    }

    // Fallback: paragraph splitting
    let paragraphs: Vec<String> = text
        .split("\n\n")
        .map(|p| p.trim().to_string())
        .filter(|p| !p.is_empty())
        .collect();

    // If single giant block, chunk by line groups
    if paragraphs.len() <= 1 && lines.len() > 20 {
        let mut segments = Vec::new();
        for chunk in lines.chunks(25) {
            let group = chunk.join("\n").trim().to_string();
            if !group.is_empty() {
                segments.push(group);
            }
        }
        return segments;
    }

    paragraphs
}

fn split_by_turns(lines: &[&str], turn_patterns: &[Regex]) -> Vec<String> {
    let mut segments = Vec::new();
    let mut current: Vec<&str> = Vec::new();

    for line in lines {
        let stripped = line.trim();
        let is_turn = turn_patterns.iter().any(|pat| pat.is_match(stripped));

        if is_turn && !current.is_empty() {
            segments.push(current.join("\n"));
            current = vec![line];
        } else {
            current.push(line);
        }
    }

    if !current.is_empty() {
        segments.push(current.join("\n"));
    }

    segments
}

// ── Main extraction ────────────────────────────────────────────────────────

/// Extract memories from a text string.
///
/// Scores each segment against all five memory types, picks the highest,
/// applies disambiguation, and filters by confidence.
pub fn extract_memories(text: &str, min_confidence: f64) -> Vec<Memory> {
    let all_markers: Vec<(&str, &[&str])> = vec![
        ("decision", DECISION_MARKERS),
        ("preference", PREFERENCE_MARKERS),
        ("milestone", MILESTONE_MARKERS),
        ("problem", PROBLEM_MARKERS),
        ("emotional", EMOTION_MARKERS),
    ];

    let paragraphs = split_into_segments(text);
    let mut memories = Vec::new();

    for para in &paragraphs {
        if para.trim().len() < 20 {
            continue;
        }

        let prose = extract_prose(para);

        // Score against all types
        let mut scores: HashMap<String, f64> = HashMap::new();
        for (mem_type, markers) in &all_markers {
            let (score, _) = score_markers(&prose, markers);
            if score > 0.0 {
                scores.insert(mem_type.to_string(), score);
            }
        }

        if scores.is_empty() {
            continue;
        }

        // Length bonus
        let length_bonus: f64 = if para.len() > 500 {
            2.0
        } else if para.len() > 200 {
            1.0
        } else {
            0.0
        };

        let max_type = scores
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap())
            .map(|(k, _)| k.clone())
            .unwrap();
        let max_score = scores[&max_type] + length_bonus;

        // Disambiguate
        let final_type = disambiguate(&max_type, &prose, &scores);

        // Confidence
        let confidence = (max_score / 5.0).min(1.0);
        if confidence < min_confidence {
            continue;
        }

        memories.push(Memory {
            content: para.trim().to_string(),
            memory_type: final_type,
            chunk_index: memories.len(),
        });
    }

    memories
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decision_extraction() {
        let text = "We decided to use Rust because it is fast and safe. \
                     The trade-off is longer compile times but better runtime.";
        let memories = extract_memories(text, 0.1);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, "decision");
    }

    #[test]
    fn test_preference_extraction() {
        let text = "I prefer snake_case over camelCase. We always use functional style \
                     and never use mutable globals. Please always format with tabs.";
        let memories = extract_memories(text, 0.1);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, "preference");
    }

    #[test]
    fn test_milestone_extraction() {
        let text = "After weeks of struggle, it finally works! We shipped v2.0 \
                     and deployed the prototype. This is a real breakthrough.";
        let memories = extract_memories(text, 0.1);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, "milestone");
    }

    #[test]
    fn test_problem_extraction() {
        let text = "There is a critical bug in the parser. The error keeps crashing \
                     the server and it won't work no matter what we try.";
        let memories = extract_memories(text, 0.1);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, "problem");
    }

    #[test]
    fn test_emotional_extraction() {
        let text = "I feel so grateful for this team. I love working with everyone \
                     and I'm proud of what we have built. It is truly wonderful.";
        let memories = extract_memories(text, 0.1);
        assert!(!memories.is_empty());
        assert_eq!(memories[0].memory_type, "emotional");
    }

    #[test]
    fn test_disambiguation_resolved_problem_becomes_milestone() {
        let text = "There was a terrible bug but we fixed it and now it works perfectly.";
        let mut scores = HashMap::new();
        scores.insert("problem".to_string(), 3.0);
        scores.insert("milestone".to_string(), 2.0);
        let result = disambiguate("problem", text, &scores);
        assert_eq!(result, "milestone");
    }

    #[test]
    fn test_disambiguation_resolved_positive_emotional() {
        let text = "The bug was fixed and I am so proud and happy and grateful it works.";
        let mut scores = HashMap::new();
        scores.insert("problem".to_string(), 2.0);
        scores.insert("emotional".to_string(), 3.0);
        let result = disambiguate("problem", text, &scores);
        // has_resolution is true, emotional > 0, sentiment positive => emotional
        assert_eq!(result, "emotional");
    }

    #[test]
    fn test_no_match_below_threshold() {
        let text = "The weather is nice today and I went for a walk in the park.";
        let memories = extract_memories(text, 0.5);
        assert!(memories.is_empty());
    }

    #[test]
    fn test_code_filtering() {
        assert!(is_code_line("import os"));
        assert!(is_code_line("$ pip install foo"));
        assert!(is_code_line("export PATH=/usr/bin"));
        assert!(!is_code_line("We decided to use Rust"));
    }

    #[test]
    fn test_extract_prose_strips_code_block() {
        let text = "Here is prose.\n```\nlet x = 5;\nlet y = 10;\n```\nMore prose here.";
        let prose = extract_prose(text);
        assert!(prose.contains("Here is prose"));
        assert!(prose.contains("More prose here"));
        assert!(!prose.contains("let x = 5"));
    }

    #[test]
    fn test_sentiment() {
        assert_eq!(
            get_sentiment("I am happy and proud and excited"),
            "positive"
        );
        assert_eq!(
            get_sentiment("The bug caused a crash and failure"),
            "negative"
        );
        assert_eq!(get_sentiment("The table is made of wood"), "neutral");
    }

    #[test]
    fn test_has_resolution() {
        assert!(has_resolution("we finally fixed it"));
        assert!(has_resolution("got it working after hours"));
        assert!(!has_resolution("the bug keeps crashing"));
    }

    #[test]
    fn test_split_segments_paragraphs() {
        let text = "First paragraph here.\n\nSecond paragraph here.\n\nThird paragraph here.";
        let segments = split_into_segments(text);
        assert_eq!(segments.len(), 3);
    }

    #[test]
    fn test_split_segments_turns() {
        let text = "Human: Hello there\nAssistant: Hi!\nHuman: How are you?\nAssistant: Good!";
        let segments = split_into_segments(text);
        // Should detect turn markers and split by them
        assert!(segments.len() >= 3);
    }

    #[test]
    fn test_chunk_index_increments() {
        let text = "We decided to use Rust because safety.\n\n\
                     I prefer tabs over spaces always.\n\n\
                     It finally works after the breakthrough.";
        let memories = extract_memories(text, 0.1);
        for (i, m) in memories.iter().enumerate() {
            assert_eq!(m.chunk_index, i);
        }
    }

    #[test]
    fn test_score_markers_returns_keywords() {
        let text = "we decided to use rust because it is fast";
        let (score, keywords) = score_markers(text, DECISION_MARKERS);
        assert!(score >= 2.0);
        assert!(!keywords.is_empty());
    }
}

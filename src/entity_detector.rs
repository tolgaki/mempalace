//! entity_detector.rs — Auto-detect people and projects from file content.
//!
//! Scans prose and readable files in a project directory, extracts capitalized
//! proper nouns, scores them for person vs project signals, and classifies each
//! candidate entity with confidence levels.

use regex::Regex;
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Verb patterns that suggest a name refers to a person (use `{name}` placeholder).
pub const PERSON_VERB_PATTERNS: &[&str] = &[
    r"\b{name}\s+said\b",
    r"\b{name}\s+says\b",
    r"\b{name}\s+told\b",
    r"\b{name}\s+asked\b",
    r"\b{name}\s+wrote\b",
    r"\b{name}\s+mentioned\b",
    r"\b{name}\s+explained\b",
    r"\b{name}\s+suggested\b",
    r"\b{name}\s+thinks\b",
    r"\b{name}\s+thought\b",
    r"\b{name}\s+believes\b",
    r"\b{name}\s+wants\b",
    r"\b{name}\s+wanted\b",
    r"\b{name}\s+agreed\b",
    r"\b{name}\s+disagreed\b",
    r"\b{name}\s+replied\b",
    r"\b{name}\s+responded\b",
    r"\b{name}\s+noted\b",
    r"\b{name}\s+argued\b",
    r"\b{name}\s+felt\b",
];

/// Pronoun patterns that appear near a person's name.
pub const PRONOUN_PATTERNS: &[&str] = &[
    r"\bhe\b",
    r"\bshe\b",
    r"\bhis\b",
    r"\bher\b",
    r"\bhim\b",
    r"\bhimself\b",
    r"\bherself\b",
    r"\bthey\b",
    r"\bthem\b",
];

/// Dialogue patterns (use `{name}` placeholder).
pub const DIALOGUE_PATTERNS: &[&str] = &[
    r#""{name}\s*:"#,
    r"\b{name}\s*:",
    r"@{name}\b",
    r"\b{name}\s+>\s+",
];

/// Verb patterns that suggest a name refers to a project.
pub const PROJECT_VERB_PATTERNS: &[&str] = &[
    r"\b{name}\s+v\d",
    r"\b{name}\s+version\b",
    r"\b{name}\s+release\b",
    r"\b{name}\s+update\b",
    r"\b{name}\s+install\b",
    r"\b{name}\s+deploy\b",
    r"\b{name}\s+build\b",
    r"\b{name}\s+config\b",
    r"\b{name}\s+module\b",
    r"\b{name}\s+library\b",
    r"\b{name}\s+framework\b",
    r"\b{name}\s+plugin\b",
    r"\b{name}\s+package\b",
    r"\b{name}\s+API\b",
    r"\b{name}\s+SDK\b",
];

/// File extensions considered prose.
pub fn prose_extensions() -> HashSet<&'static str> {
    ["md", "txt", "rst", "org", "adoc", "tex", "rtf", "log"]
        .into_iter()
        .collect()
}

/// File extensions that are readable (prose + code).
pub fn readable_extensions() -> HashSet<&'static str> {
    [
        "md", "txt", "rst", "org", "adoc", "tex", "rtf", "log", "py", "rs", "js", "ts", "jsx",
        "tsx", "java", "kt", "go", "rb", "php", "c", "cpp", "h", "hpp", "cs", "swift", "sh",
        "bash", "zsh", "yaml", "yml", "toml", "json", "xml", "html", "css", "scss", "sql", "r",
        "jl", "lua", "pl", "ex", "exs", "hs", "ml", "scala", "clj", "erl", "elm", "vue", "svelte",
    ]
    .into_iter()
    .collect()
}

/// Directories to skip when scanning.
pub fn skip_dirs() -> HashSet<&'static str> {
    [
        ".git",
        "node_modules",
        "__pycache__",
        ".venv",
        "venv",
        "env",
        "dist",
        "build",
        ".next",
        "coverage",
        ".mempalace",
        "target",
        ".idea",
        ".vscode",
        ".gradle",
        ".mvn",
        "vendor",
        "bower_components",
        ".tox",
        ".mypy_cache",
        ".pytest_cache",
        ".eggs",
        "htmlcov",
        ".cargo",
        "pkg",
        "bin",
        "obj",
    ]
    .into_iter()
    .collect()
}

/// Common English stopwords, code keywords, UI words, and abstract concepts
/// that should not be treated as proper nouns.
pub fn stopwords() -> HashSet<&'static str> {
    [
        // Common English words
        "the",
        "a",
        "an",
        "and",
        "or",
        "but",
        "in",
        "on",
        "at",
        "to",
        "for",
        "of",
        "with",
        "by",
        "from",
        "as",
        "is",
        "was",
        "are",
        "were",
        "be",
        "been",
        "being",
        "have",
        "has",
        "had",
        "do",
        "does",
        "did",
        "will",
        "would",
        "could",
        "should",
        "may",
        "might",
        "shall",
        "can",
        "need",
        "must",
        "it",
        "its",
        "this",
        "that",
        "these",
        "those",
        "i",
        "me",
        "my",
        "we",
        "us",
        "our",
        "you",
        "your",
        "he",
        "him",
        "his",
        "she",
        "her",
        "they",
        "them",
        "their",
        "what",
        "which",
        "who",
        "whom",
        "when",
        "where",
        "why",
        "how",
        "all",
        "each",
        "every",
        "both",
        "few",
        "more",
        "most",
        "other",
        "some",
        "such",
        "no",
        "not",
        "only",
        "same",
        "so",
        "than",
        "too",
        "very",
        "just",
        "because",
        "if",
        "then",
        "else",
        "while",
        "about",
        "up",
        "out",
        "into",
        "through",
        "during",
        "before",
        "after",
        "above",
        "below",
        "between",
        "under",
        "again",
        "further",
        "once",
        "here",
        "there",
        "also",
        "new",
        "old",
        "first",
        "last",
        "long",
        "great",
        "little",
        "own",
        "still",
        "back",
        "even",
        "well",
        "way",
        "many",
        "much",
        "now",
        "ever",
        "never",
        "always",
        "sometimes",
        "often",
        "already",
        "soon",
        // Sentence starters / common capitalized words
        "The",
        "This",
        "That",
        "These",
        "Those",
        "Here",
        "There",
        "When",
        "Where",
        "What",
        "Which",
        "Who",
        "How",
        "Why",
        "It",
        "He",
        "She",
        "They",
        "We",
        "You",
        "My",
        "Our",
        "Your",
        "If",
        "But",
        "And",
        "Or",
        "So",
        "Yet",
        "Not",
        "Also",
        "Just",
        "Then",
        "Now",
        "Some",
        "All",
        "Each",
        "Every",
        "Both",
        "Many",
        "Much",
        "Most",
        "Any",
        "No",
        "Each",
        "Other",
        "New",
        "Old",
        "First",
        "Last",
        // Code keywords
        "fn",
        "let",
        "mut",
        "pub",
        "use",
        "mod",
        "struct",
        "enum",
        "impl",
        "trait",
        "type",
        "const",
        "static",
        "async",
        "await",
        "self",
        "super",
        "crate",
        "return",
        "match",
        "loop",
        "break",
        "continue",
        "move",
        "ref",
        "where",
        "unsafe",
        "extern",
        "dyn",
        "macro",
        "def",
        "class",
        "import",
        "from",
        "print",
        "pass",
        "raise",
        "try",
        "except",
        "finally",
        "lambda",
        "yield",
        "global",
        "nonlocal",
        "function",
        "var",
        "const",
        "export",
        "default",
        "switch",
        "case",
        "void",
        "null",
        "undefined",
        "true",
        "false",
        "nil",
        "none",
        "True",
        "False",
        "None",
        "NULL",
        "TRUE",
        "FALSE",
        // UI / web words
        "button",
        "input",
        "form",
        "table",
        "header",
        "footer",
        "sidebar",
        "modal",
        "dialog",
        "menu",
        "toolbar",
        "panel",
        "card",
        "list",
        "grid",
        "layout",
        "container",
        "wrapper",
        "component",
        "widget",
        "page",
        "view",
        "screen",
        "window",
        "tab",
        "icon",
        "image",
        "text",
        "label",
        "title",
        "link",
        "style",
        "color",
        "font",
        // Abstract / technical concepts
        "error",
        "warning",
        "info",
        "debug",
        "trace",
        "log",
        "status",
        "result",
        "value",
        "data",
        "state",
        "config",
        "option",
        "setting",
        "feature",
        "module",
        "system",
        "service",
        "server",
        "client",
        "request",
        "response",
        "message",
        "event",
        "action",
        "command",
        "query",
        "update",
        "create",
        "delete",
        "read",
        "write",
        "open",
        "close",
        "start",
        "stop",
        "run",
        "test",
        "check",
        "build",
        "deploy",
        "release",
        "version",
        "file",
        "path",
        "name",
        "key",
        "index",
        "count",
        "size",
        "length",
        "width",
        "height",
        "depth",
        "TODO",
        "FIXME",
        "NOTE",
        "HACK",
        "XXX",
        "IMPORTANT",
        "Monday",
        "Tuesday",
        "Wednesday",
        "Thursday",
        "Friday",
        "Saturday",
        "Sunday",
        "January",
        "February",
        "March",
        "April",
        "May",
        "June",
        "July",
        "August",
        "September",
        "October",
        "November",
        "December",
    ]
    .into_iter()
    .collect()
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// Compiled regex patterns for a specific entity name.
struct EntityPatterns {
    dialogue: Vec<Regex>,
    person_verbs: Vec<Regex>,
    project_verbs: Vec<Regex>,
    direct: Regex,
    versioned: Regex,
    code_ref: Regex,
}

/// Scores for person vs project classification.
#[derive(Debug, Clone, Default)]
pub struct EntityScores {
    pub person_score: f64,
    pub project_score: f64,
    pub person_signals: Vec<String>,
    pub project_signals: Vec<String>,
}

/// Classification result for a single entity.
#[derive(Debug, Clone)]
pub struct EntityClassification {
    pub name: String,
    pub entity_type: String,
    pub confidence: f64,
    pub frequency: usize,
    pub signals: Vec<String>,
}

/// Aggregated detection results.
#[derive(Debug, Clone, Default)]
pub struct DetectedEntities {
    pub people: Vec<EntityClassification>,
    pub projects: Vec<EntityClassification>,
    pub uncertain: Vec<EntityClassification>,
}

// ---------------------------------------------------------------------------
// Pattern building
// ---------------------------------------------------------------------------

fn build_patterns(name: &str) -> EntityPatterns {
    let escaped = regex::escape(name);

    let dialogue: Vec<Regex> = DIALOGUE_PATTERNS
        .iter()
        .filter_map(|p| {
            let pat = p.replace("{name}", &escaped);
            Regex::new(&pat).ok()
        })
        .collect();

    let person_verbs: Vec<Regex> = PERSON_VERB_PATTERNS
        .iter()
        .filter_map(|p| {
            let pat = p.replace("{name}", &escaped);
            Regex::new(&format!("(?i){}", pat)).ok()
        })
        .collect();

    let project_verbs: Vec<Regex> = PROJECT_VERB_PATTERNS
        .iter()
        .filter_map(|p| {
            let pat = p.replace("{name}", &escaped);
            Regex::new(&format!("(?i){}", pat)).ok()
        })
        .collect();

    let direct = Regex::new(&format!(
        r"(?i)\b(hi|hey|hello|thanks|thank you),?\s+{}",
        escaped
    ))
    .unwrap_or_else(|_| Regex::new(r"[^\s\S]").unwrap());

    let versioned = Regex::new(&format!(r"(?i){}\s*v?\d+\.\d+", escaped))
        .unwrap_or_else(|_| Regex::new(r"[^\s\S]").unwrap());

    let code_ref = Regex::new(&format!(
        r"(?i)(import\s+{name}|require\(.?{name}.?\)|{name}\.\w+\(|{name}::\w+)",
        name = escaped
    ))
    .unwrap_or_else(|_| Regex::new(r"[^\s\S]").unwrap());

    EntityPatterns {
        dialogue,
        person_verbs,
        project_verbs,
        direct,
        versioned,
        code_ref,
    }
}

// ---------------------------------------------------------------------------
// Candidate extraction
// ---------------------------------------------------------------------------

/// Extract capitalized proper nouns appearing 3+ times.
/// Also finds multi-word proper nouns (e.g. "John Smith").
pub fn extract_candidates(text: &str) -> HashMap<String, usize> {
    let stops = stopwords();
    let mut counts: HashMap<String, usize> = HashMap::new();

    // Single-word capitalized proper nouns
    let single_re = Regex::new(r"\b([A-Z][a-z]{2,})\b").unwrap();
    for cap in single_re.captures_iter(text) {
        let word = cap[1].to_string();
        if !stops.contains(word.as_str()) {
            *counts.entry(word).or_insert(0) += 1;
        }
    }

    // Multi-word proper nouns (e.g. "John Smith", "Mary Jane Watson")
    let multi_re = Regex::new(r"\b([A-Z][a-z]+(?:\s+[A-Z][a-z]+)+)\b").unwrap();
    for cap in multi_re.captures_iter(text) {
        let phrase = cap[1].to_string();
        // Check none of the words are stopwords
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let all_ok = words.iter().all(|w| !stops.contains(*w));
        if all_ok && words.len() <= 4 {
            *counts.entry(phrase).or_insert(0) += 1;
        }
    }

    // Filter to 3+ occurrences
    counts.into_iter().filter(|(_, c)| *c >= 3).collect()
}

// ---------------------------------------------------------------------------
// Scoring
// ---------------------------------------------------------------------------

/// Score an entity name for person vs project signals.
pub fn score_entity(name: &str, text: &str, lines: &[&str]) -> EntityScores {
    let patterns = build_patterns(name);
    let mut scores = EntityScores::default();

    // Dialogue markers (x3 weight)
    for pat in &patterns.dialogue {
        let count = pat.find_iter(text).count();
        if count > 0 {
            scores.person_score += count as f64 * 3.0;
            scores.person_signals.push(format!("dialogue({})", count));
        }
    }

    // Person verb patterns (x2 weight)
    for pat in &patterns.person_verbs {
        let count = pat.find_iter(text).count();
        if count > 0 {
            scores.person_score += count as f64 * 2.0;
            scores
                .person_signals
                .push(format!("person_verb({})", count));
        }
    }

    // Pronoun proximity (x2 weight) — check lines containing the name
    let name_lower = name.to_lowercase();
    let pronoun_regexes: Vec<Regex> = PRONOUN_PATTERNS
        .iter()
        .filter_map(|p| Regex::new(&format!("(?i){}", p)).ok())
        .collect();

    let mut pronoun_hits = 0usize;
    for line in lines {
        if line.to_lowercase().contains(&name_lower) {
            for pr in &pronoun_regexes {
                pronoun_hits += pr.find_iter(line).count();
            }
        }
    }
    if pronoun_hits > 0 {
        scores.person_score += pronoun_hits as f64 * 2.0;
        scores
            .person_signals
            .push(format!("pronoun_proximity({})", pronoun_hits));
    }

    // Direct address (x4 weight)
    let direct_count = patterns.direct.find_iter(text).count();
    if direct_count > 0 {
        scores.person_score += direct_count as f64 * 4.0;
        scores
            .person_signals
            .push(format!("direct_address({})", direct_count));
    }

    // Project verb patterns (x2 weight)
    for pat in &patterns.project_verbs {
        let count = pat.find_iter(text).count();
        if count > 0 {
            scores.project_score += count as f64 * 2.0;
            scores
                .project_signals
                .push(format!("project_verb({})", count));
        }
    }

    // Versioned references (x3 weight)
    let ver_count = patterns.versioned.find_iter(text).count();
    if ver_count > 0 {
        scores.project_score += ver_count as f64 * 3.0;
        scores
            .project_signals
            .push(format!("versioned({})", ver_count));
    }

    // Code references (x3 weight)
    let code_count = patterns.code_ref.find_iter(text).count();
    if code_count > 0 {
        scores.project_score += code_count as f64 * 3.0;
        scores
            .project_signals
            .push(format!("code_ref({})", code_count));
    }

    scores
}

// ---------------------------------------------------------------------------
// Classification
// ---------------------------------------------------------------------------

/// Classify an entity. Requires TWO signal categories for confident person
/// classification.
pub fn classify_entity(
    name: &str,
    frequency: usize,
    scores: &EntityScores,
) -> EntityClassification {
    let person_categories = scores.person_signals.len();
    let project_categories = scores.project_signals.len();

    let (entity_type, confidence) =
        if person_categories >= 2 && scores.person_score > scores.project_score {
            // Confident person — two or more distinct signal types
            let conf = if person_categories >= 3 { 0.95 } else { 0.80 };
            ("person".to_string(), conf)
        } else if person_categories == 1 && scores.person_score > scores.project_score {
            // Only one signal category — lower confidence
            ("person".to_string(), 0.55)
        } else if project_categories >= 2 && scores.project_score > scores.person_score {
            let conf = if project_categories >= 3 { 0.95 } else { 0.80 };
            ("project".to_string(), conf)
        } else if project_categories == 1 && scores.project_score > scores.person_score {
            ("project".to_string(), 0.55)
        } else if scores.person_score > 0.0 && scores.project_score == 0.0 {
            ("person".to_string(), 0.50)
        } else if scores.project_score > 0.0 && scores.person_score == 0.0 {
            ("project".to_string(), 0.50)
        } else {
            ("uncertain".to_string(), 0.30)
        };

    let mut signals = scores.person_signals.clone();
    signals.extend(scores.project_signals.clone());

    EntityClassification {
        name: name.to_string(),
        entity_type,
        confidence,
        frequency,
        signals,
    }
}

// ---------------------------------------------------------------------------
// File scanning
// ---------------------------------------------------------------------------

/// Collect prose files from a project directory; fall back to all readable files.
pub fn scan_for_detection(project_dir: &str, max_files: usize) -> Vec<PathBuf> {
    let skips = skip_dirs();
    let prose_exts = prose_extensions();
    let read_exts = readable_extensions();

    let mut prose_files = Vec::new();
    let mut readable_files = Vec::new();

    let walker = walkdir::WalkDir::new(project_dir)
        .max_depth(6)
        .into_iter()
        .filter_entry(|e| {
            if e.file_type().is_dir() {
                let name = e.file_name().to_string_lossy();
                !skips.contains(name.as_ref())
            } else {
                true
            }
        });

    for entry in walker.filter_map(|e| e.ok()) {
        if !entry.file_type().is_file() {
            continue;
        }
        let path = entry.path().to_path_buf();
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            let ext_lower = ext.to_lowercase();
            if prose_exts.contains(ext_lower.as_str()) {
                prose_files.push(path.clone());
            }
            if read_exts.contains(ext_lower.as_str()) {
                readable_files.push(path);
            }
        }
    }

    let files = if prose_files.len() >= 3 {
        prose_files
    } else {
        readable_files
    };

    files.into_iter().take(max_files).collect()
}

// ---------------------------------------------------------------------------
// Main detection entry point
// ---------------------------------------------------------------------------

/// Read files (first 5KB each), combine text, extract/score/classify candidates.
pub fn detect_entities(file_paths: &[PathBuf], max_files: usize) -> DetectedEntities {
    let mut combined = String::new();

    for path in file_paths.iter().take(max_files) {
        // Read only the first 6KB to avoid unbounded memory allocation on large files
        use std::io::Read;
        let mut buf = vec![0u8; 6144];
        if let Ok(mut file) = std::fs::File::open(path) {
            if let Ok(n) = file.read(&mut buf) {
                let text = String::from_utf8_lossy(&buf[..n]);
                let chunk: String = text.chars().take(5120).collect();
                combined.push_str(&chunk);
                combined.push('\n');
            }
        }
    }

    let candidates = extract_candidates(&combined);
    let lines: Vec<&str> = combined.lines().collect();

    let mut result = DetectedEntities::default();

    for (name, freq) in &candidates {
        let scores = score_entity(name, &combined, &lines);
        let classification = classify_entity(name, *freq, &scores);

        match classification.entity_type.as_str() {
            "person" => result.people.push(classification),
            "project" => result.projects.push(classification),
            _ => result.uncertain.push(classification),
        }
    }

    // Sort by confidence descending
    result
        .people
        .sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    result
        .projects
        .sort_by(|a, b| b.confidence.partial_cmp(&a.confidence).unwrap());
    result
        .uncertain
        .sort_by(|a, b| b.frequency.cmp(&a.frequency));

    result
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_candidates_basic() {
        let text = "Alice went home. Alice said hello. Alice likes cats. Bob is here.";
        let candidates = extract_candidates(text);
        assert!(candidates.contains_key("Alice"));
        assert_eq!(candidates["Alice"], 3);
        // Bob only appears once, should not be included
        assert!(!candidates.contains_key("Bob"));
    }

    #[test]
    fn test_extract_candidates_multiword() {
        let text =
            "John Smith arrived. John Smith spoke. John Smith left. Then John Smith returned.";
        let candidates = extract_candidates(text);
        assert!(candidates.contains_key("John Smith"));
    }

    #[test]
    fn test_extract_candidates_filters_stopwords() {
        // "The" appears many times but should be filtered
        let text = "The The The The The cat sat.";
        let candidates = extract_candidates(text);
        assert!(!candidates.contains_key("The"));
    }

    #[test]
    fn test_score_person_signals() {
        let text = "Alice said something. Alice told us. Alice: hello everyone.";
        let lines: Vec<&str> = text.lines().collect();
        let scores = score_entity("Alice", text, &lines);
        assert!(scores.person_score > 0.0);
        assert!(!scores.person_signals.is_empty());
    }

    #[test]
    fn test_score_project_signals() {
        let text = "Webpack v4.0 is great. Webpack build works. import Webpack from somewhere.";
        let lines: Vec<&str> = text.lines().collect();
        let scores = score_entity("Webpack", text, &lines);
        assert!(scores.project_score > 0.0);
        assert!(!scores.project_signals.is_empty());
    }

    #[test]
    fn test_classify_person_confident() {
        let scores = EntityScores {
            person_score: 10.0,
            project_score: 0.0,
            person_signals: vec!["dialogue(2)".into(), "person_verb(3)".into()],
            project_signals: vec![],
        };
        let c = classify_entity("Alice", 5, &scores);
        assert_eq!(c.entity_type, "person");
        assert!(c.confidence >= 0.80);
    }

    #[test]
    fn test_classify_project_confident() {
        let scores = EntityScores {
            person_score: 0.0,
            project_score: 12.0,
            person_signals: vec![],
            project_signals: vec!["versioned(2)".into(), "code_ref(3)".into()],
        };
        let c = classify_entity("React", 8, &scores);
        assert_eq!(c.entity_type, "project");
        assert!(c.confidence >= 0.80);
    }

    #[test]
    fn test_classify_uncertain() {
        let scores = EntityScores::default();
        let c = classify_entity("Mystery", 4, &scores);
        assert_eq!(c.entity_type, "uncertain");
        assert!(c.confidence < 0.50);
    }

    #[test]
    fn test_classify_single_signal_lower_confidence() {
        let scores = EntityScores {
            person_score: 4.0,
            project_score: 0.0,
            person_signals: vec!["person_verb(2)".into()],
            project_signals: vec![],
        };
        let c = classify_entity("Dana", 3, &scores);
        assert_eq!(c.entity_type, "person");
        assert!(c.confidence < 0.80);
    }

    #[test]
    fn test_detect_entities_end_to_end() {
        // Write a temporary file and detect
        let dir = tempfile::tempdir().unwrap();
        let file_path = dir.path().join("notes.md");
        std::fs::write(
            &file_path,
            "Alice said hello. Alice told Bob something.\n\
             Alice: I think we should proceed.\n\
             Alice mentioned the plan. He agreed with her.\n\
             Alice wants to go home. Alice replied quickly.\n\
             Alice felt happy about the result.\n\
             React v18.0 is released. React build passed.\n\
             import React from 'react'; React.render();\n\
             React version 18. React module loaded. React package updated.\n",
        )
        .unwrap();

        let paths = vec![file_path];
        let result = detect_entities(&paths, 10);

        // Alice should be detected as person
        let alice = result.people.iter().find(|e| e.name == "Alice");
        assert!(alice.is_some(), "Alice should be detected as a person");

        // React should be detected as project
        let react = result.projects.iter().find(|e| e.name == "React");
        assert!(react.is_some(), "React should be detected as a project");
    }

    #[test]
    fn test_prose_extensions() {
        let exts = prose_extensions();
        assert!(exts.contains("md"));
        assert!(exts.contains("txt"));
        assert!(!exts.contains("py"));
    }

    #[test]
    fn test_readable_extensions_superset_of_prose() {
        let prose = prose_extensions();
        let readable = readable_extensions();
        for ext in &prose {
            assert!(readable.contains(ext), "{} should be in readable", ext);
        }
    }
}

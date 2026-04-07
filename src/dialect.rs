use crate::error::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::OnceLock;

// ---------------------------------------------------------------------------
// Static lookup tables
// ---------------------------------------------------------------------------

fn emotion_codes() -> &'static HashMap<&'static str, &'static str> {
    static INSTANCE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("vulnerability", "vul");
        m.insert("joy", "joy");
        m.insert("fear", "fear");
        m.insert("anger", "ang");
        m.insert("sadness", "sad");
        m.insert("love", "lov");
        m.insert("trust", "tru");
        m.insert("surprise", "sur");
        m.insert("disgust", "dis");
        m.insert("anticipation", "ant");
        m.insert("guilt", "gui");
        m.insert("shame", "sha");
        m.insert("pride", "pri");
        m.insert("hope", "hop");
        m.insert("anxiety", "anx");
        m.insert("curiosity", "cur");
        m.insert("confusion", "con");
        m.insert("gratitude", "gra");
        m.insert("loneliness", "lon");
        m.insert("nostalgia", "nos");
        m.insert("determination", "determ");
        m.insert("frustration", "frus");
        m.insert("excitement", "exc");
        m.insert("contentment", "cont");
        m.insert("empathy", "emp");
        m.insert("awe", "awe");
        m.insert("relief", "rel");
        m.insert("envy", "env");
        m.insert("compassion", "comp");
        m.insert("conviction", "convict");
        m.insert("wonder", "won");
        m.insert("melancholy", "mel");
        m.insert("tenderness", "ten");
        m.insert("defiance", "def");
        m.insert("serenity", "ser");
        m
    })
}

fn emotion_signals() -> &'static HashMap<&'static str, &'static str> {
    static INSTANCE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("decided", "determ");
        m.insert("prefer", "convict");
        m.insert("worried", "anx");
        m.insert("afraid", "fear");
        m.insert("scared", "fear");
        m.insert("happy", "joy");
        m.insert("glad", "joy");
        m.insert("excited", "exc");
        m.insert("angry", "ang");
        m.insert("furious", "ang");
        m.insert("sad", "sad");
        m.insert("depressed", "sad");
        m.insert("love", "lov");
        m.insert("adore", "lov");
        m.insert("trust", "tru");
        m.insert("believe", "tru");
        m.insert("surprised", "sur");
        m.insert("shocked", "sur");
        m.insert("disgusted", "dis");
        m.insert("repulsed", "dis");
        m.insert("hopeful", "hop");
        m.insert("hope", "hop");
        m.insert("guilty", "gui");
        m.insert("ashamed", "sha");
        m.insert("proud", "pri");
        m.insert("curious", "cur");
        m.insert("confused", "con");
        m.insert("grateful", "gra");
        m.insert("thankful", "gra");
        m.insert("lonely", "lon");
        m.insert("nostalgic", "nos");
        m.insert("frustrated", "frus");
        m.insert("content", "cont");
        m.insert("relieved", "rel");
        m.insert("envious", "env");
        m.insert("jealous", "env");
        m.insert("compassionate", "comp");
        m.insert("tender", "ten");
        m.insert("defiant", "def");
        m.insert("serene", "ser");
        m.insert("calm", "ser");
        m.insert("anxious", "anx");
        m.insert("nervous", "anx");
        m.insert("wonderful", "won");
        m.insert("amazed", "awe");
        m.insert("vulnerable", "vul");
        m
    })
}

fn flag_signals() -> &'static HashMap<&'static str, &'static str> {
    static INSTANCE: OnceLock<HashMap<&str, &str>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        let mut m = HashMap::new();
        m.insert("decided", "DECISION");
        m.insert("decide", "DECISION");
        m.insert("choice", "DECISION");
        m.insert("choose", "DECISION");
        m.insert("founded", "ORIGIN");
        m.insert("origin", "ORIGIN");
        m.insert("began", "ORIGIN");
        m.insert("started", "ORIGIN");
        m.insert("core", "CORE");
        m.insert("essential", "CORE");
        m.insert("fundamental", "CORE");
        m.insert("important", "CORE");
        m.insert("breakthrough", "BREAKTHROUGH");
        m.insert("discovery", "BREAKTHROUGH");
        m.insert("realized", "BREAKTHROUGH");
        m.insert("turning", "TURNING_POINT");
        m.insert("changed", "TURNING_POINT");
        m.insert("transformed", "TURNING_POINT");
        m.insert("promise", "PROMISE");
        m.insert("commit", "PROMISE");
        m.insert("vow", "PROMISE");
        m.insert("boundary", "BOUNDARY");
        m.insert("limit", "BOUNDARY");
        m.insert("refuse", "BOUNDARY");
        m.insert("dream", "DREAM");
        m.insert("aspire", "DREAM");
        m.insert("goal", "DREAM");
        m.insert("fear", "FEAR");
        m.insert("afraid", "FEAR");
        m.insert("terrified", "FEAR");
        m.insert("loss", "LOSS");
        m.insert("lost", "LOSS");
        m.insert("grief", "LOSS");
        m.insert("growth", "GROWTH");
        m.insert("learned", "GROWTH");
        m.insert("grew", "GROWTH");
        m
    })
}

fn stop_words() -> &'static HashSet<&'static str> {
    static INSTANCE: OnceLock<HashSet<&str>> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        [
            "a",
            "an",
            "the",
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
            "dare",
            "ought",
            "used",
            "it",
            "its",
            "he",
            "she",
            "they",
            "them",
            "their",
            "his",
            "her",
            "my",
            "your",
            "our",
            "we",
            "you",
            "i",
            "me",
            "him",
            "us",
            "this",
            "that",
            "these",
            "those",
            "what",
            "which",
            "who",
            "whom",
            "whose",
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
            "nor",
            "not",
            "only",
            "own",
            "same",
            "so",
            "than",
            "too",
            "very",
            "just",
            "about",
            "above",
            "after",
            "again",
            "against",
            "am",
            "any",
            "because",
            "before",
            "below",
            "between",
            "both",
            "during",
            "further",
            "get",
            "got",
            "here",
            "if",
            "into",
            "itself",
            "let",
            "like",
            "make",
            "many",
            "much",
            "must",
            "never",
            "now",
            "off",
            "once",
            "out",
            "over",
            "really",
            "right",
            "said",
            "say",
            "says",
            "since",
            "still",
            "such",
            "take",
            "tell",
            "then",
            "there",
            "thing",
            "think",
            "through",
            "time",
            "under",
            "until",
            "up",
            "upon",
            "want",
            "way",
            "well",
            "went",
            "what",
            "while",
            "also",
            "back",
            "been",
            "come",
            "even",
            "going",
            "gone",
            "good",
            "great",
            "know",
            "long",
            "look",
            "made",
            "new",
            "old",
            "one",
            "people",
            "put",
            "see",
            "seem",
            "something",
            "t",
            "though",
            "two",
            "work",
            "yes",
            "yet",
            "don",
            "doesn",
            "didn",
            "won",
            "wouldn",
            "couldn",
            "shouldn",
            "isn",
            "aren",
            "wasn",
            "weren",
            "hasn",
            "haven",
            "hadn",
            "ll",
            "ve",
            "re",
            "d",
            "s",
            "m",
        ]
        .iter()
        .copied()
        .collect()
    })
}

fn word_tokenize_regex() -> &'static Regex {
    static INSTANCE: OnceLock<Regex> = OnceLock::new();
    INSTANCE.get_or_init(|| {
        Regex::new(r"[A-Za-z][a-z]*(?:[A-Z][a-z]*)*|[A-Za-z]+(?:-[A-Za-z]+)+|[A-Za-z]+").unwrap()
    })
}

fn sentence_split_regex() -> &'static Regex {
    static INSTANCE: OnceLock<Regex> = OnceLock::new();
    INSTANCE.get_or_init(|| Regex::new(r"[.!?\n]+").unwrap())
}

fn proper_noun_regex() -> &'static Regex {
    static INSTANCE: OnceLock<Regex> = OnceLock::new();
    INSTANCE.get_or_init(|| Regex::new(r"^[A-Z][a-z]").unwrap())
}

fn camel_case_regex() -> &'static Regex {
    static INSTANCE: OnceLock<Regex> = OnceLock::new();
    INSTANCE.get_or_init(|| Regex::new(r"[a-z][A-Z]").unwrap())
}

fn hyphenated_regex() -> &'static Regex {
    static INSTANCE: OnceLock<Regex> = OnceLock::new();
    INSTANCE.get_or_init(|| Regex::new(r"^[A-Za-z]+-[A-Za-z]+").unwrap())
}

// ---------------------------------------------------------------------------
// CompressionStats
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompressionStats {
    pub original_tokens: usize,
    pub compressed_tokens: usize,
    pub ratio: f64,
    pub original_chars: usize,
    pub compressed_chars: usize,
}

// ---------------------------------------------------------------------------
// Dialect config (for JSON serialization)
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DialectConfig {
    entity_codes: HashMap<String, String>,
    #[serde(default)]
    skip_names: Vec<String>,
}

// ---------------------------------------------------------------------------
// Dialect
// ---------------------------------------------------------------------------

#[derive(Debug, Clone)]
pub struct Dialect {
    pub entity_codes: HashMap<String, String>,
    skip_names: Vec<String>,
}

impl Dialect {
    /// Create a new Dialect with optional entity codes and skip names.
    pub fn new(entities: Option<HashMap<String, String>>, skip_names: Option<Vec<String>>) -> Self {
        let skip_names: Vec<String> = skip_names
            .unwrap_or_default()
            .into_iter()
            .map(|s| s.to_lowercase())
            .collect();

        let mut entity_codes = HashMap::new();
        if let Some(ents) = entities {
            for (name, code) in &ents {
                entity_codes.insert(name.clone(), code.clone());
                let lower = name.to_lowercase();
                if lower != *name {
                    entity_codes.insert(lower, code.clone());
                }
            }
        }

        Self {
            entity_codes,
            skip_names,
        }
    }

    /// Load a Dialect from a JSON config file.
    pub fn from_config(config_path: &str) -> Result<Self> {
        let content = std::fs::read_to_string(config_path)?;
        let cfg: DialectConfig = serde_json::from_str(&content)?;
        Ok(Self::new(Some(cfg.entity_codes), Some(cfg.skip_names)))
    }

    /// Save the current Dialect configuration to a JSON file.
    pub fn save_config(&self, config_path: &str) -> Result<()> {
        let cfg = DialectConfig {
            entity_codes: self.entity_codes.clone(),
            skip_names: self.skip_names.clone(),
        };
        let json = serde_json::to_string_pretty(&cfg)?;
        std::fs::write(config_path, json)?;
        Ok(())
    }

    /// Encode a human name to a short entity code.
    ///
    /// Priority: skip_names -> exact match -> lowercase match -> partial match
    /// -> fallback to first 3 chars uppercased.
    pub fn encode_entity(&self, name: &str) -> Option<String> {
        let lower = name.to_lowercase();

        // Skip names produce None
        if self.skip_names.iter().any(|s| s == &lower) {
            return None;
        }

        // Exact match
        if let Some(code) = self.entity_codes.get(name) {
            return Some(code.clone());
        }

        // Lowercase match
        if let Some(code) = self.entity_codes.get(&lower) {
            return Some(code.clone());
        }

        // Partial match: check if any known entity name is contained in the
        // input or vice-versa
        for (key, code) in &self.entity_codes {
            let key_lower = key.to_lowercase();
            if lower.contains(&key_lower) || key_lower.contains(&lower) {
                return Some(code.clone());
            }
        }

        // Fallback: first 3 characters uppercased
        let fallback: String = lower
            .chars()
            .filter(|c| c.is_alphabetic())
            .take(3)
            .collect::<String>()
            .to_uppercase();
        if fallback.is_empty() {
            None
        } else {
            Some(fallback)
        }
    }

    /// Encode a list of emotion names to a short string like "joy+fear+vul".
    /// Deduplicates and limits to 3.
    pub fn encode_emotions(emotions: &[String]) -> String {
        let codes = emotion_codes();
        let mut seen = HashSet::new();
        let mut result = Vec::new();

        for e in emotions {
            let lower = e.to_lowercase();
            let code = codes.get(lower.as_str()).copied().unwrap_or_else(|| {
                // fallback: first 3 chars
                // We leak a short string here to get a &'static str — but
                // since this is bounded it is acceptable.
                let s: &str = lower.as_str();
                if s.len() >= 3 {
                    &s[..3]
                } else {
                    s
                }
            });
            if seen.insert(code.to_string()) {
                result.push(code.to_string());
            }
            if result.len() >= 3 {
                break;
            }
        }

        result.join("+")
    }

    /// Detect emotions present in text by scanning for keyword signals.
    pub fn detect_emotions(text: &str) -> Vec<String> {
        let signals = emotion_signals();
        let lower = text.to_lowercase();
        let mut found = Vec::new();
        let mut seen = HashSet::new();

        for (keyword, code) in signals.iter() {
            if lower.contains(keyword) && seen.insert(*code) {
                found.push(code.to_string());
            }
        }
        found
    }

    /// Detect flags present in text by scanning for keyword signals.
    /// Deduplicates and limits to 3.
    pub fn detect_flags(text: &str) -> Vec<String> {
        let signals = flag_signals();
        let lower = text.to_lowercase();
        let mut found = Vec::new();
        let mut seen = HashSet::new();

        for (keyword, flag) in signals.iter() {
            if lower.contains(keyword) && seen.insert(*flag) {
                found.push(flag.to_string());
                if found.len() >= 3 {
                    break;
                }
            }
        }
        found
    }

    /// Extract the most relevant topics from text.
    ///
    /// Tokenises, removes stop words, boosts proper nouns / CamelCase /
    /// hyphenated words, and returns the top N by frequency.
    pub fn extract_topics(&self, text: &str, max_topics: usize) -> Vec<String> {
        let re = word_tokenize_regex();
        let stops = stop_words();
        let proper_re = proper_noun_regex();
        let camel_re = camel_case_regex();
        let hyphen_re = hyphenated_regex();

        let mut freq: HashMap<String, usize> = HashMap::new();

        for mat in re.find_iter(text) {
            let word = mat.as_str();
            let lower = word.to_lowercase();

            if lower.len() < 2 || stops.contains(lower.as_str()) {
                continue;
            }

            // Skip known entity names (they are captured separately)
            if self.entity_codes.contains_key(word) || self.entity_codes.contains_key(&lower) {
                continue;
            }

            let boost = if camel_re.is_match(word) || hyphen_re.is_match(word) {
                3
            } else if proper_re.is_match(word) {
                2
            } else {
                1
            };

            *freq.entry(lower).or_insert(0) += boost;
        }

        let mut items: Vec<(String, usize)> = freq.into_iter().collect();
        items.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
        items.into_iter().take(max_topics).map(|(w, _)| w).collect()
    }

    /// Extract the top N most relevant sentences from text.
    ///
    /// Splits on sentence boundaries, scores each sentence by keyword density
    /// (presence of decision-related words) and sentence length, then returns
    /// the top `n` sentences ranked by relevance. Each result is truncated to
    /// 55 chars.
    pub fn extract_key_sentences(text: &str, n: usize) -> Vec<String> {
        let split_re = sentence_split_regex();

        let decision_words = [
            "decided",
            "choose",
            "chose",
            "must",
            "will",
            "need",
            "want",
            "believe",
            "realize",
            "realized",
            "important",
            "core",
            "always",
            "never",
            "promise",
            "because",
            "should",
            "essential",
        ];

        let sentences: Vec<&str> = split_re
            .split(text)
            .map(|s| s.trim())
            .filter(|s| s.len() > 3)
            .collect();

        if sentences.is_empty() {
            let trimmed = text.trim();
            let single = if trimmed.len() > 55 {
                format!("{}...", &trimmed[..52])
            } else {
                trimmed.to_string()
            };
            return if single.is_empty() {
                Vec::new()
            } else {
                vec![single]
            };
        }

        // Score each sentence by keyword density + length preference
        let mut scored: Vec<(i64, &str)> = sentences
            .iter()
            .map(|&s| {
                let lower = s.to_lowercase();
                let mut score: i64 = 0;

                // Keyword density: count matching decision words
                for dw in &decision_words {
                    if lower.contains(dw) {
                        score += 2;
                    }
                }

                // Sentence length bonus: prefer medium-length sentences
                if s.len() < 80 {
                    score += 1;
                }
                if s.len() < 40 {
                    score += 1;
                }

                // Longer sentences carry more information (length component)
                score += (s.len() / 20) as i64;

                (score, s)
            })
            .collect();

        // Sort descending by score, then by original order for ties
        scored.sort_by(|a, b| b.0.cmp(&a.0));

        scored
            .into_iter()
            .take(n)
            .map(|(_, s)| {
                if s.len() > 55 {
                    format!("{}...", &s[..52])
                } else {
                    s.to_string()
                }
            })
            .collect()
    }

    /// Detect entities mentioned in text.
    ///
    /// Checks known entity names first, then falls back to capitalised words
    /// that are not at the start of a sentence.
    pub fn detect_entities_in_text(&self, text: &str) -> Vec<String> {
        let mut found = Vec::new();
        let mut seen = HashSet::new();

        // Check known entities
        for key in self.entity_codes.keys() {
            if text.contains(key.as_str()) && seen.insert(key.to_lowercase()) {
                found.push(key.clone());
            }
        }

        // Fallback: capitalised words (simple heuristic)
        let cap_re = Regex::new(r"\b([A-Z][a-z]{2,})").unwrap();
        let stops = stop_words();
        for cap in cap_re.find_iter(text) {
            let word = cap.as_str().to_string();
            let lower = word.to_lowercase();
            if !stops.contains(lower.as_str())
                && !seen.contains(&lower)
                && !self.entity_codes.contains_key(&word)
                && !self.entity_codes.contains_key(&lower)
            {
                seen.insert(lower);
                found.push(word);
            }
        }

        found
    }

    /// Main compression function. Produces AAAK-format compressed text.
    pub fn compress(&self, text: &str, metadata: Option<&HashMap<String, String>>) -> String {
        if text.trim().is_empty() {
            return String::new();
        }

        let entities = self.detect_entities_in_text(text);
        let entity_codes: Vec<String> = entities
            .iter()
            .filter_map(|e| self.encode_entity(e))
            .collect();

        let topics = self.extract_topics(text, 5);
        let key_sentences = Self::extract_key_sentences(text, 3);
        let emotions = Self::detect_emotions(text);
        let emotion_str = Self::encode_emotions(&emotions);
        let flags = Self::detect_flags(text);

        let mut parts: Vec<String> = Vec::new();

        // Entities
        if !entity_codes.is_empty() {
            let deduped: Vec<String> = {
                let mut seen = HashSet::new();
                entity_codes
                    .into_iter()
                    .filter(|c| seen.insert(c.clone()))
                    .collect()
            };
            parts.push(format!("@{}", deduped.join(",")));
        }

        // Topics
        if !topics.is_empty() {
            parts.push(format!("[{}]", topics.join(",")));
        }

        // Key sentences
        for sentence in &key_sentences {
            if !sentence.is_empty() {
                parts.push(format!("\"{}\"", sentence));
            }
        }

        // Emotions
        if !emotion_str.is_empty() {
            parts.push(format!("~{}", emotion_str));
        }

        // Flags
        if !flags.is_empty() {
            parts.push(format!("!{}", flags.join(",")));
        }

        // Metadata pass-through
        if let Some(meta) = metadata {
            for (k, v) in meta {
                parts.push(format!("{}={}", k, v));
            }
        }

        parts.join(" ")
    }

    /// Estimate token count (rough: len / 3).
    pub fn count_tokens(text: &str) -> usize {
        if text.is_empty() {
            0
        } else {
            text.len().div_ceil(3) // ceiling division
        }
    }

    /// Compute compression statistics.
    pub fn compression_stats(original: &str, compressed: &str) -> CompressionStats {
        let original_tokens = Self::count_tokens(original);
        let compressed_tokens = Self::count_tokens(compressed);
        let ratio = if original_tokens > 0 {
            compressed_tokens as f64 / original_tokens as f64
        } else {
            0.0
        };

        CompressionStats {
            original_tokens,
            compressed_tokens,
            ratio,
            original_chars: original.len(),
            compressed_chars: compressed.len(),
        }
    }
}

// ===========================================================================
// Tests
// ===========================================================================

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn sample_dialect() -> Dialect {
        let mut entities = HashMap::new();
        entities.insert("Alice".to_string(), "ALC".to_string());
        entities.insert("Bob".to_string(), "BOB".to_string());
        entities.insert("Charlie".to_string(), "CHA".to_string());
        Dialect::new(Some(entities), Some(vec!["narrator".to_string()]))
    }

    // -- Emotion code lookup -----------------------------------------------

    #[test]
    fn test_emotion_code_lookup_known() {
        let codes = emotion_codes();
        assert_eq!(codes.get("joy"), Some(&"joy"));
        assert_eq!(codes.get("vulnerability"), Some(&"vul"));
        assert_eq!(codes.get("fear"), Some(&"fear"));
        assert_eq!(codes.get("determination"), Some(&"determ"));
    }

    #[test]
    fn test_emotion_code_lookup_unknown() {
        let codes = emotion_codes();
        assert_eq!(codes.get("nonexistent"), None);
    }

    #[test]
    fn test_encode_emotions_basic() {
        let emotions = vec![
            "joy".to_string(),
            "fear".to_string(),
            "vulnerability".to_string(),
        ];
        let encoded = Dialect::encode_emotions(&emotions);
        assert_eq!(encoded, "joy+fear+vul");
    }

    #[test]
    fn test_encode_emotions_dedup() {
        let emotions = vec!["joy".to_string(), "joy".to_string(), "fear".to_string()];
        let encoded = Dialect::encode_emotions(&emotions);
        assert_eq!(encoded, "joy+fear");
    }

    #[test]
    fn test_encode_emotions_max_three() {
        let emotions = vec![
            "joy".to_string(),
            "fear".to_string(),
            "anger".to_string(),
            "sadness".to_string(),
        ];
        let encoded = Dialect::encode_emotions(&emotions);
        // Only first 3
        let parts: Vec<&str> = encoded.split('+').collect();
        assert_eq!(parts.len(), 3);
    }

    // -- detect_emotions ---------------------------------------------------

    #[test]
    fn test_detect_emotions() {
        let emotions = Dialect::detect_emotions("I am happy and a little worried about the future");
        assert!(emotions.contains(&"joy".to_string()));
        assert!(emotions.contains(&"anx".to_string()));
    }

    #[test]
    fn test_detect_emotions_empty() {
        let emotions = Dialect::detect_emotions("the quick brown fox");
        assert!(emotions.is_empty());
    }

    // -- Flag detection ----------------------------------------------------

    #[test]
    fn test_detect_flags_basic() {
        let flags =
            Dialect::detect_flags("I decided this is a core principle that changed everything");
        assert!(flags.contains(&"DECISION".to_string()));
        assert!(flags.contains(&"CORE".to_string()));
    }

    #[test]
    fn test_detect_flags_max_three() {
        let flags = Dialect::detect_flags("I decided on a core origin dream goal boundary");
        assert!(flags.len() <= 3);
    }

    #[test]
    fn test_detect_flags_empty() {
        let flags = Dialect::detect_flags("nothing special here");
        assert!(flags.is_empty());
    }

    // -- Entity encoding ---------------------------------------------------

    #[test]
    fn test_encode_entity_known() {
        let d = sample_dialect();
        assert_eq!(d.encode_entity("Alice"), Some("ALC".to_string()));
        assert_eq!(d.encode_entity("alice"), Some("ALC".to_string()));
    }

    #[test]
    fn test_encode_entity_unknown() {
        let d = sample_dialect();
        // Unknown entity falls back to first 3 chars uppercased
        assert_eq!(d.encode_entity("Zara"), Some("ZAR".to_string()));
    }

    #[test]
    fn test_encode_entity_skip() {
        let d = sample_dialect();
        assert_eq!(d.encode_entity("Narrator"), None);
        assert_eq!(d.encode_entity("narrator"), None);
    }

    #[test]
    fn test_encode_entity_partial_match() {
        let d = sample_dialect();
        // "Alice Smith" contains "alice" which matches "alice" key
        assert_eq!(d.encode_entity("Alice Smith"), Some("ALC".to_string()));
    }

    // -- Topic extraction --------------------------------------------------

    #[test]
    fn test_extract_topics() {
        let d = sample_dialect();
        let topics = d.extract_topics(
            "The neural network architecture uses transformer layers for machine learning",
            3,
        );
        assert!(!topics.is_empty());
        assert!(topics.len() <= 3);
        // "neural", "network", "architecture", "transformer", "layers", "machine", "learning"
        // should appear (stop words removed)
    }

    #[test]
    fn test_extract_topics_empty() {
        let d = sample_dialect();
        let topics = d.extract_topics("", 5);
        assert!(topics.is_empty());
    }

    // -- Key sentence extraction -------------------------------------------

    #[test]
    fn test_extract_key_sentences() {
        let text = "Hello world. I decided to change everything. It was fine.";
        let keys = Dialect::extract_key_sentences(text, 3);
        assert!(!keys.is_empty());
        assert!(keys.len() <= 3);
        // The top-ranked sentence should contain "decided"
        assert!(keys[0].contains("decided"));
    }

    #[test]
    fn test_extract_key_sentences_truncation() {
        let long = "A".repeat(200);
        let keys = Dialect::extract_key_sentences(&long, 3);
        assert!(!keys.is_empty());
        assert!(keys[0].len() <= 55);
        assert!(keys[0].ends_with("..."));
    }

    #[test]
    fn test_extract_key_sentences_returns_n() {
        let text = "First sentence here. Second sentence here. Third one. Fourth one. Fifth one.";
        let keys = Dialect::extract_key_sentences(text, 2);
        assert_eq!(keys.len(), 2);
    }

    #[test]
    fn test_extract_key_sentences_single() {
        let text = "Hello world. I decided to change everything. It was fine.";
        let keys = Dialect::extract_key_sentences(text, 1);
        assert_eq!(keys.len(), 1);
        assert!(keys[0].contains("decided"));
    }

    // -- Full compress round-trip ------------------------------------------

    #[test]
    fn test_compress_basic() {
        let d = sample_dialect();
        let text = "Alice decided she was happy about the new project architecture.";
        let compressed = d.compress(text, None);
        assert!(!compressed.is_empty());
        // Should contain entity code
        assert!(compressed.contains("ALC") || compressed.contains('@'));
    }

    #[test]
    fn test_compress_empty() {
        let d = sample_dialect();
        let compressed = d.compress("", None);
        assert!(compressed.is_empty());
    }

    #[test]
    fn test_compress_with_metadata() {
        let d = sample_dialect();
        let mut meta = HashMap::new();
        meta.insert("ts".to_string(), "2026-01-01".to_string());
        let compressed = d.compress("Alice likes dogs.", Some(&meta));
        assert!(compressed.contains("ts=2026-01-01"));
    }

    // -- Compression stats -------------------------------------------------

    #[test]
    fn test_compression_stats() {
        let original = "This is a long text that should compress significantly when processed.";
        let compressed = "@ALC [text] ~joy";
        let stats = Dialect::compression_stats(original, compressed);
        assert!(stats.ratio < 1.0);
        assert_eq!(stats.original_chars, original.len());
        assert_eq!(stats.compressed_chars, compressed.len());
        assert!(stats.original_tokens > stats.compressed_tokens);
    }

    #[test]
    fn test_count_tokens() {
        assert_eq!(Dialect::count_tokens(""), 0);
        assert_eq!(Dialect::count_tokens("abc"), 1);
        assert!(Dialect::count_tokens("hello world") > 0);
    }

    // -- Config save/load --------------------------------------------------

    #[test]
    fn test_config_save_load() {
        let tmp = tempfile::NamedTempFile::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();

        let d = sample_dialect();
        d.save_config(&path).unwrap();

        let d2 = Dialect::from_config(&path).unwrap();
        assert_eq!(d2.encode_entity("Alice"), Some("ALC".to_string()));
    }

    #[test]
    fn test_config_load_missing() {
        let result = Dialect::from_config("/nonexistent/path.json");
        assert!(result.is_err());
    }

    // -- detect_entities_in_text -------------------------------------------

    #[test]
    fn test_detect_entities_in_text_known() {
        let d = sample_dialect();
        let entities = d.detect_entities_in_text("Alice and Bob went out");
        assert!(entities.iter().any(|e| e == "Alice" || e == "alice"));
        assert!(entities.iter().any(|e| e == "Bob" || e == "bob"));
    }

    #[test]
    fn test_detect_entities_in_text_capitalized_fallback() {
        let d = Dialect::new(None, None);
        let entities = d.detect_entities_in_text("Then Zara said hello to Marcus");
        let lowers: Vec<String> = entities.iter().map(|e| e.to_lowercase()).collect();
        assert!(lowers.contains(&"zara".to_string()));
        assert!(lowers.contains(&"marcus".to_string()));
    }
}

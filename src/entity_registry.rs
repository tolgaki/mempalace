//! entity_registry.rs — Persistent entity registry for tracking people, projects,
//! and ambiguous words across sessions.
//!
//! Stores data at ~/.mempalace/entity_registry.json and provides lookup,
//! disambiguation, and learning capabilities.

use crate::error::Result;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use crate::entity_detector::{self, EntityClassification};

// ---------------------------------------------------------------------------
// Constants
// ---------------------------------------------------------------------------

/// Common English words that could also be names (require disambiguation).
pub fn common_english_words() -> HashSet<&'static str> {
    [
        "ever", "grace", "will", "bill", "mark", "april", "may", "june", "august", "frank",
        "penny", "joy", "hope", "faith", "charity", "patience", "iris", "ivy", "rose", "lily",
        "daisy", "holly", "violet", "ruby", "pearl", "amber", "jade", "crystal", "dawn", "eve",
        "summer", "autumn", "winter", "storm", "rain", "sky", "brook", "cliff", "dale", "glen",
        "heath", "lance", "ray", "rob", "bob", "pat", "sue", "art", "gene", "dean", "miles",
        "chase", "chance", "sterling", "hunter", "mason", "cooper", "carter", "taylor", "parker",
        "ford", "grant", "wade", "cash", "duke", "earl", "king", "prince",
    ]
    .into_iter()
    .collect()
}

/// Regex patterns indicating the word is used as a person's name.
pub fn person_context_patterns() -> Vec<Regex> {
    let patterns = [
        r"\b{word}\s+said\b",
        r"\b{word}\s+told\b",
        r"\b{word}\s+asked\b",
        r"\b{word}\s+thinks\b",
        r"\b{word}\s+wants\b",
        r"\b{word}\s+wrote\b",
        r"\b{word}\s+mentioned\b",
        r"\b{word}\s+replied\b",
        r"\b{word}'s\s+(idea|opinion|suggestion|project|work|code)\b",
        r"(ask|tell|email|call|message|ping|thank)\s+{word}\b",
        r"(with|from|to|for)\s+{word}\b",
        r"@{word}\b",
        r"\b{word}\s*:",
    ];
    // Return with {word} as literal placeholder; actual matching done at lookup time
    patterns
        .iter()
        .filter_map(|p| {
            let test = p.replace("{word}", "PLACEHOLDER");
            Regex::new(&format!("(?i){}", test)).ok()?;
            Regex::new(&format!("(?i){}", p.replace("{word}", r"\w+"))).ok()
        })
        .collect()
}

/// Regex patterns indicating the word is used as a concept, not a name.
pub fn concept_context_patterns() -> Vec<Regex> {
    let patterns = [
        r"\bthe\s+{word}\b",
        r"\ba\s+{word}\b",
        r"\b{word}\s+is\s+(a|an|the)\b",
        r"\b{word}\s+(of|for|in|on|at|by)\b",
        r"\bno\s+{word}\b",
        r"\bmore\s+{word}\b",
        r"\bless\s+{word}\b",
        r"\b{word}\s+(level|score|rating|amount|value)\b",
        r"\bfree\s+{word}\b",
        r"\b{word}\s+and\s+(joy|hope|peace|love)\b",
    ];
    patterns
        .iter()
        .filter_map(|p| {
            let test = p.replace("{word}", "PLACEHOLDER");
            Regex::new(&format!("(?i){}", test)).ok()?;
            Regex::new(&format!("(?i){}", p.replace("{word}", r"\w+"))).ok()
        })
        .collect()
}

// ---------------------------------------------------------------------------
// Structs
// ---------------------------------------------------------------------------

/// A registered person entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonEntry {
    pub name: String,
    #[serde(default)]
    pub relationship: String,
    #[serde(default)]
    pub context: String,
}

/// Internal person info stored in the registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PersonInfo {
    pub name: String,
    #[serde(default)]
    pub relationship: String,
    #[serde(default)]
    pub context: String,
    #[serde(default)]
    pub aliases: Vec<String>,
    #[serde(default)]
    pub ambiguous: bool,
}

/// Result of looking up a word in the registry.
#[derive(Debug, Clone)]
pub struct LookupResult {
    pub entity_type: String,
    pub confidence: f64,
    pub source: String,
    pub name: String,
    pub needs_disambiguation: bool,
}

/// Serializable registry data on disk.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
struct RegistryData {
    #[serde(default)]
    mode: String,
    #[serde(default)]
    people: HashMap<String, PersonInfo>,
    #[serde(default)]
    projects: HashSet<String>,
    #[serde(default)]
    aliases: HashMap<String, String>,
    #[serde(default)]
    ambiguous_flags: HashMap<String, bool>,
}

/// The persistent entity registry.
#[derive(Debug)]
pub struct EntityRegistry {
    data: RegistryData,
    path: PathBuf,
}

impl EntityRegistry {
    // -----------------------------------------------------------------------
    // Persistence
    // -----------------------------------------------------------------------

    /// Load the registry from disk, or create a new empty one.
    pub fn load(config_dir: Option<&Path>) -> Self {
        let dir = config_dir.map(PathBuf::from).unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mempalace")
        });

        let path = dir.join("entity_registry.json");

        let data = if path.exists() {
            std::fs::read_to_string(&path)
                .ok()
                .and_then(|s| serde_json::from_str::<RegistryData>(&s).ok())
                .unwrap_or_default()
        } else {
            RegistryData::default()
        };

        EntityRegistry { data, path }
    }

    /// Persist the registry to disk.
    pub fn save(&self) -> Result<()> {
        if let Some(parent) = self.path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string_pretty(&self.data)?;
        std::fs::write(&self.path, json)?;
        Ok(())
    }

    // -----------------------------------------------------------------------
    // Seeding
    // -----------------------------------------------------------------------

    /// Seed the registry with initial data from setup / config.
    pub fn seed(
        &mut self,
        mode: &str,
        people: &[PersonEntry],
        projects: &[String],
        aliases: &HashMap<String, String>,
    ) {
        self.data.mode = mode.to_string();

        for person in people {
            let key = person.name.to_lowercase();
            let ambiguous = common_english_words().contains(key.as_str());
            self.data.people.insert(
                key.clone(),
                PersonInfo {
                    name: person.name.clone(),
                    relationship: person.relationship.clone(),
                    context: person.context.clone(),
                    aliases: vec![],
                    ambiguous,
                },
            );
            if ambiguous {
                self.data.ambiguous_flags.insert(key, true);
            }
        }

        for project in projects {
            self.data.projects.insert(project.clone());
        }

        for (alias, target) in aliases {
            self.data
                .aliases
                .insert(alias.to_lowercase(), target.clone());
        }
    }

    /// Register a single person entity with name, relationship, and context.
    pub fn register_person(&mut self, name: &str, relationship: &str, context: &str) {
        let lower = name.to_lowercase();
        self.data.people.insert(
            lower,
            PersonInfo {
                name: name.to_string(),
                relationship: relationship.to_string(),
                context: context.to_string(),
                aliases: vec![],
                ambiguous: false,
            },
        );
    }

    /// Register a single project by name.
    pub fn register_project(&mut self, name: &str) {
        self.data.projects.insert(name.to_string());
    }

    // -----------------------------------------------------------------------
    // Lookup
    // -----------------------------------------------------------------------

    /// Look up a word and determine if it's a known person, project, or unknown.
    pub fn lookup(&self, word: &str, context: &str) -> LookupResult {
        let lower = word.to_lowercase();

        // Check aliases first
        if let Some(target) = self.data.aliases.get(&lower) {
            let target_lower = target.to_lowercase();
            if let Some(info) = self.data.people.get(&target_lower) {
                return LookupResult {
                    entity_type: "person".to_string(),
                    confidence: 0.90,
                    source: "alias".to_string(),
                    name: info.name.clone(),
                    needs_disambiguation: false,
                };
            }
        }

        // Check people
        if let Some(info) = self.data.people.get(&lower) {
            if info.ambiguous && !context.is_empty() {
                if let Some(result) = self.disambiguate(word, context, info) {
                    return result;
                }
            }
            return LookupResult {
                entity_type: "person".to_string(),
                confidence: if info.ambiguous { 0.60 } else { 0.95 },
                source: "registry".to_string(),
                name: info.name.clone(),
                needs_disambiguation: info.ambiguous,
            };
        }

        // Check projects (case-insensitive)
        let is_project = self.data.projects.iter().any(|p| p.to_lowercase() == lower);
        if is_project {
            return LookupResult {
                entity_type: "project".to_string(),
                confidence: 0.95,
                source: "registry".to_string(),
                name: word.to_string(),
                needs_disambiguation: false,
            };
        }

        // Unknown
        LookupResult {
            entity_type: "unknown".to_string(),
            confidence: 0.0,
            source: "none".to_string(),
            name: word.to_string(),
            needs_disambiguation: false,
        }
    }

    /// Disambiguate an ambiguous word using context patterns.
    fn disambiguate(
        &self,
        word: &str,
        context: &str,
        person_info: &PersonInfo,
    ) -> Option<LookupResult> {
        let escaped = regex::escape(word);

        // Check person context patterns
        let person_patterns_raw = [
            format!(r"(?i)\b{}\s+said\b", escaped),
            format!(r"(?i)\b{}\s+told\b", escaped),
            format!(r"(?i)\b{}\s+asked\b", escaped),
            format!(r"(?i)\b{}\s+thinks\b", escaped),
            format!(r"(?i)\b{}\s+wants\b", escaped),
            format!(r"(?i)\b{}\s+wrote\b", escaped),
            format!(r"(?i)\b{}\s+mentioned\b", escaped),
            format!(
                r"(?i)(ask|tell|email|call|message|ping|thank)\s+{}\b",
                escaped
            ),
            format!(r"(?i)(with|from|to|for)\s+{}\b", escaped),
            format!(r"(?i)@{}\b", escaped),
            format!(r"(?i)\b{}\s*:", escaped),
        ];

        let concept_patterns_raw = [
            format!(r"(?i)\bthe\s+{}\b", escaped),
            format!(r"(?i)\ba\s+{}\b", escaped),
            format!(r"(?i)\b{}\s+is\s+(a|an|the)\b", escaped),
            format!(r"(?i)\b{}\s+(of|for|in|on|at|by)\b", escaped),
            format!(r"(?i)\bno\s+{}\b", escaped),
            format!(r"(?i)\bmore\s+{}\b", escaped),
            format!(r"(?i)\bfree\s+{}\b", escaped),
        ];

        let mut person_hits = 0;
        let mut concept_hits = 0;

        for pat_str in &person_patterns_raw {
            if let Ok(re) = Regex::new(pat_str) {
                person_hits += re.find_iter(context).count();
            }
        }

        for pat_str in &concept_patterns_raw {
            if let Ok(re) = Regex::new(pat_str) {
                concept_hits += re.find_iter(context).count();
            }
        }

        if person_hits > concept_hits {
            Some(LookupResult {
                entity_type: "person".to_string(),
                confidence: 0.85,
                source: "disambiguation".to_string(),
                name: person_info.name.clone(),
                needs_disambiguation: false,
            })
        } else if concept_hits > person_hits {
            Some(LookupResult {
                entity_type: "concept".to_string(),
                confidence: 0.70,
                source: "disambiguation".to_string(),
                name: word.to_string(),
                needs_disambiguation: false,
            })
        } else {
            None // Truly ambiguous — fall through to default
        }
    }

    // -----------------------------------------------------------------------
    // Learning
    // -----------------------------------------------------------------------

    /// Learn new entities from text using the entity detector.
    pub fn learn_from_text(
        &mut self,
        text: &str,
        min_confidence: f64,
    ) -> Vec<EntityClassification> {
        let candidates = entity_detector::extract_candidates(text);
        let lines: Vec<&str> = text.lines().collect();
        let mut learned = Vec::new();

        for (name, freq) in &candidates {
            // Skip already known entities
            let lower = name.to_lowercase();
            if self.data.people.contains_key(&lower)
                || self.data.projects.iter().any(|p| p.to_lowercase() == lower)
            {
                continue;
            }

            let scores = entity_detector::score_entity(name, text, &lines);
            let classification = entity_detector::classify_entity(name, *freq, &scores);

            if classification.confidence >= min_confidence {
                match classification.entity_type.as_str() {
                    "person" => {
                        let ambiguous = common_english_words().contains(lower.as_str());
                        self.data.people.insert(
                            lower.clone(),
                            PersonInfo {
                                name: name.clone(),
                                relationship: String::new(),
                                context: format!(
                                    "auto-detected (confidence: {:.2})",
                                    classification.confidence
                                ),
                                aliases: vec![],
                                ambiguous,
                            },
                        );
                        if ambiguous {
                            self.data.ambiguous_flags.insert(lower, true);
                        }
                    }
                    "project" => {
                        self.data.projects.insert(name.clone());
                    }
                    _ => {}
                }
                learned.push(classification);
            }
        }

        learned
    }

    // -----------------------------------------------------------------------
    // Query helpers
    // -----------------------------------------------------------------------

    /// Extract known people mentioned in a query string.
    pub fn extract_people_from_query(&self, query: &str) -> Vec<String> {
        let mut found = Vec::new();
        let lower_query = query.to_lowercase();

        for (key, info) in &self.data.people {
            if lower_query.contains(key) {
                found.push(info.name.clone());
                continue;
            }
            // Check aliases
            for (alias, target) in &self.data.aliases {
                if target.to_lowercase() == *key && lower_query.contains(alias) {
                    found.push(info.name.clone());
                    break;
                }
            }
        }

        found.sort();
        found.dedup();
        found
    }

    /// Extract unknown capitalized candidates from a query that are not already registered.
    pub fn extract_unknown_candidates(&self, query: &str) -> Vec<String> {
        let stops = entity_detector::stopwords();
        let word_re = Regex::new(r"\b([A-Z][a-z]{2,})\b").unwrap();
        let mut candidates = Vec::new();

        for cap in word_re.captures_iter(query) {
            let word = cap[1].to_string();
            let lower = word.to_lowercase();
            if stops.contains(word.as_str()) {
                continue;
            }
            if self.data.people.contains_key(&lower) {
                continue;
            }
            if self.data.projects.iter().any(|p| p.to_lowercase() == lower) {
                continue;
            }
            if !candidates.contains(&word) {
                candidates.push(word);
            }
        }

        candidates
    }

    // -----------------------------------------------------------------------
    // Summary / properties
    // -----------------------------------------------------------------------

    /// Human-readable summary of the registry contents.
    pub fn summary(&self) -> String {
        let people_count = self.data.people.len();
        let project_count = self.data.projects.len();
        let alias_count = self.data.aliases.len();
        let ambiguous_count = self.data.ambiguous_flags.values().filter(|v| **v).count();

        let mut parts = vec![
            format!(
                "Mode: {}",
                if self.data.mode.is_empty() {
                    "unset"
                } else {
                    &self.data.mode
                }
            ),
            format!("People: {}", people_count),
            format!("Projects: {}", project_count),
        ];

        if alias_count > 0 {
            parts.push(format!("Aliases: {}", alias_count));
        }
        if ambiguous_count > 0 {
            parts.push(format!("Ambiguous: {}", ambiguous_count));
        }

        if people_count > 0 {
            let names: Vec<&str> = self
                .data
                .people
                .values()
                .map(|p| p.name.as_str())
                .take(10)
                .collect();
            parts.push(format!("Known people: {}", names.join(", ")));
        }

        if project_count > 0 {
            let projs: Vec<&String> = self.data.projects.iter().take(10).collect();
            let proj_strs: Vec<&str> = projs.iter().map(|s| s.as_str()).collect();
            parts.push(format!("Known projects: {}", proj_strs.join(", ")));
        }

        parts.join("\n")
    }

    /// Current mode.
    pub fn mode(&self) -> &str {
        &self.data.mode
    }

    /// Access to registered people.
    pub fn people(&self) -> &HashMap<String, PersonInfo> {
        &self.data.people
    }

    /// Access to registered projects.
    pub fn projects(&self) -> &HashSet<String> {
        &self.data.projects
    }

    /// Access to ambiguous flags.
    pub fn ambiguous_flags(&self) -> &HashMap<String, bool> {
        &self.data.ambiguous_flags
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn make_registry(dir: &TempDir) -> EntityRegistry {
        EntityRegistry::load(Some(dir.path()))
    }

    #[test]
    fn test_load_and_save() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Alice".into(),
                relationship: "colleague".into(),
                context: "works on frontend".into(),
            }],
            &["React".into()],
            &HashMap::new(),
        );
        reg.save().unwrap();

        // Reload and verify
        let reg2 = EntityRegistry::load(Some(dir.path()));
        assert_eq!(reg2.mode(), "project");
        assert!(reg2.people().contains_key("alice"));
        assert!(reg2.projects().contains("React"));
    }

    #[test]
    fn test_lookup_known_person() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Alice".into(),
                relationship: "friend".into(),
                context: String::new(),
            }],
            &[],
            &HashMap::new(),
        );

        let result = reg.lookup("alice", "");
        assert_eq!(result.entity_type, "person");
        assert!(result.confidence > 0.9);
        assert_eq!(result.name, "Alice");
    }

    #[test]
    fn test_lookup_known_project() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed("project", &[], &["Webpack".into()], &HashMap::new());

        let result = reg.lookup("webpack", "");
        assert_eq!(result.entity_type, "project");
        assert!(result.confidence > 0.9);
    }

    #[test]
    fn test_lookup_alias() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        let mut aliases = HashMap::new();
        aliases.insert("al".to_string(), "Alice".to_string());
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Alice".into(),
                relationship: String::new(),
                context: String::new(),
            }],
            &[],
            &aliases,
        );

        let result = reg.lookup("al", "");
        assert_eq!(result.entity_type, "person");
        assert_eq!(result.name, "Alice");
        assert_eq!(result.source, "alias");
    }

    #[test]
    fn test_lookup_unknown() {
        let dir = TempDir::new().unwrap();
        let reg = make_registry(&dir);
        let result = reg.lookup("xyzzy", "");
        assert_eq!(result.entity_type, "unknown");
        assert_eq!(result.confidence, 0.0);
    }

    #[test]
    fn test_disambiguate_person_context() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Grace".into(),
                relationship: "mentor".into(),
                context: String::new(),
            }],
            &[],
            &HashMap::new(),
        );

        // The word "grace" is ambiguous; with person context it should resolve
        let result = reg.lookup("grace", "Grace said we should refactor the code");
        assert_eq!(result.entity_type, "person");
        assert!(result.confidence >= 0.80);
    }

    #[test]
    fn test_disambiguate_concept_context() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Grace".into(),
                relationship: String::new(),
                context: String::new(),
            }],
            &[],
            &HashMap::new(),
        );

        let result = reg.lookup("grace", "the grace of the dancer was remarkable");
        assert_eq!(result.entity_type, "concept");
    }

    #[test]
    fn test_extract_people_from_query() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[
                PersonEntry {
                    name: "Alice".into(),
                    relationship: String::new(),
                    context: String::new(),
                },
                PersonEntry {
                    name: "Bob".into(),
                    relationship: String::new(),
                    context: String::new(),
                },
            ],
            &[],
            &HashMap::new(),
        );

        let found = reg.extract_people_from_query("what did alice say about the bug?");
        assert!(found.contains(&"Alice".to_string()));
        assert!(!found.contains(&"Bob".to_string()));
    }

    #[test]
    fn test_extract_unknown_candidates() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Alice".into(),
                relationship: String::new(),
                context: String::new(),
            }],
            &[],
            &HashMap::new(),
        );

        let unknowns = reg.extract_unknown_candidates("Alice met Charlie at the Nexus conference");
        assert!(!unknowns.contains(&"Alice".to_string()));
        assert!(unknowns.contains(&"Charlie".to_string()));
        assert!(unknowns.contains(&"Nexus".to_string()));
    }

    #[test]
    fn test_learn_from_text() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);

        let text = "Diana said something. Diana told us a secret.\n\
                     Diana: I have an idea. Diana mentioned the deadline.\n\
                     Diana replied with enthusiasm. She agreed.\n\
                     Diana wants to refactor. Diana asked about the tests.";

        let learned = reg.learn_from_text(text, 0.50);
        // Diana should have been learned as a person
        let diana = learned.iter().find(|e| e.name == "Diana");
        assert!(diana.is_some(), "Diana should be learned");
        assert_eq!(diana.unwrap().entity_type, "person");
        // And now stored
        assert!(reg.people().contains_key("diana"));
    }

    #[test]
    fn test_summary() {
        let dir = TempDir::new().unwrap();
        let mut reg = make_registry(&dir);
        reg.seed(
            "project",
            &[PersonEntry {
                name: "Alice".into(),
                relationship: String::new(),
                context: String::new(),
            }],
            &["React".into()],
            &HashMap::new(),
        );

        let s = reg.summary();
        assert!(s.contains("Mode: project"));
        assert!(s.contains("People: 1"));
        assert!(s.contains("Projects: 1"));
        assert!(s.contains("Alice"));
        assert!(s.contains("React"));
    }

    #[test]
    fn test_common_english_words_set() {
        let words = common_english_words();
        assert!(words.contains("grace"));
        assert!(words.contains("will"));
        assert!(words.contains("mark"));
        assert!(words.contains("may"));
        assert!(!words.contains("alice"));
    }
}

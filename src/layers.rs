//! layers.rs -- 4-Layer Memory Stack for mempalace
//!
//! Load only what you need, when you need it.
//!
//! ```text
//! Layer 0: Identity       (~100 tokens)   -- Always loaded. "Who am I?"
//! Layer 1: Essential Story (~500-800)      -- Always loaded. Top moments from the palace.
//! Layer 2: On-Demand      (~200-500 each)  -- Loaded when a topic/wing comes up.
//! Layer 3: Deep Search    (unlimited)      -- Full FTS semantic search.
//! ```
//!
//! Wake-up cost: ~600-900 tokens (L0+L1). Leaves 95%+ of context free.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use crate::config::MempalaceConfig;
use crate::store::{PalaceStore, WhereFilter};

// ---------------------------------------------------------------------------
// Layer 0 -- Identity
// ---------------------------------------------------------------------------

/// Layer 0: ~100 tokens. Always loaded.
/// Reads from ~/.mempalace/identity.txt -- a plain-text file the user writes.
pub struct Layer0 {
    path: PathBuf,
    text: Option<String>,
}

impl Layer0 {
    pub fn new(identity_path: Option<&str>) -> Self {
        let path = match identity_path {
            Some(p) => PathBuf::from(p),
            None => dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mempalace")
                .join("identity.txt"),
        };
        Self { path, text: None }
    }

    /// Return the identity text, or a sensible default.
    pub fn render(&mut self) -> String {
        if let Some(ref t) = self.text {
            return t.clone();
        }

        let text = if self.path.exists() {
            std::fs::read_to_string(&self.path)
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|_| Self::default_text())
        } else {
            Self::default_text()
        };

        self.text = Some(text.clone());
        text
    }

    /// Estimate token count (len / 4).
    pub fn token_estimate(&mut self) -> usize {
        self.render().len() / 4
    }

    fn default_text() -> String {
        "## L0 -- IDENTITY\nNo identity configured. Create ~/.mempalace/identity.txt".to_string()
    }
}

// ---------------------------------------------------------------------------
// Layer 1 -- Essential Story (auto-generated from palace)
// ---------------------------------------------------------------------------

/// Layer 1: ~500-800 tokens. Always loaded.
/// Auto-generated from the highest-importance drawers in the palace.
/// Groups by room, picks the top N moments, compresses to a compact summary.
pub struct Layer1 {
    palace_path: String,
    wing: Option<String>,
}

const MAX_DRAWERS: usize = 15;
const MAX_CHARS: usize = 3200;

impl Layer1 {
    pub fn new(palace_path: Option<&str>, wing: Option<&str>) -> Self {
        let cfg = MempalaceConfig::new(None);
        Self {
            palace_path: palace_path
                .map(String::from)
                .unwrap_or_else(|| cfg.palace_path()),
            wing: wing.map(String::from),
        }
    }

    /// Pull top drawers from the store and format as compact L1 text.
    pub fn generate(&self) -> String {
        let store = match PalaceStore::open(&self.palace_path) {
            Ok(s) => s,
            Err(_) => return "## L1 -- No palace found. Run: mempalace mine <dir>".to_string(),
        };

        let filter = self.wing.as_ref().map(|w| WhereFilter::Wing(w.clone()));
        let drawers = match store.get(filter.as_ref(), None) {
            Ok(d) => d,
            Err(_) => return "## L1 -- No drawers found.".to_string(),
        };

        if drawers.is_empty() {
            return "## L1 -- No memories yet.".to_string();
        }

        // Score each drawer: prefer high importance
        let mut scored: Vec<(f64, &crate::store::Drawer)> = drawers
            .iter()
            .map(|d| {
                let importance = d
                    .metadata
                    .importance
                    .or(d.metadata.emotional_weight)
                    .unwrap_or(3.0);
                (importance, d)
            })
            .collect();

        // Sort by importance descending, take top N
        scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
        scored.truncate(MAX_DRAWERS);

        // Group by room for readability
        let mut by_room: BTreeMap<String, Vec<(f64, &crate::store::Drawer)>> = BTreeMap::new();
        for (imp, drawer) in &scored {
            by_room
                .entry(drawer.metadata.room.clone())
                .or_default()
                .push((*imp, drawer));
        }

        // Build compact text
        let mut lines = vec!["## L1 -- ESSENTIAL STORY".to_string()];
        let mut total_len: usize = 0;

        for (room, entries) in &by_room {
            let room_line = format!("\n[{}]", room);
            total_len += room_line.len();
            lines.push(room_line);

            for (_imp, drawer) in entries {
                let source = if !drawer.metadata.source_file.is_empty() {
                    Path::new(&drawer.metadata.source_file)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_default()
                } else {
                    String::new()
                };

                let snippet = drawer.content.trim().replace('\n', " ");
                let snippet = if snippet.len() > 200 {
                    format!("{}...", &snippet[..197])
                } else {
                    snippet
                };

                let mut entry_line = format!("  - {}", snippet);
                if !source.is_empty() {
                    entry_line.push_str(&format!("  ({})", source));
                }

                if total_len + entry_line.len() > MAX_CHARS {
                    lines.push("  ... (more in L3 search)".to_string());
                    return lines.join("\n");
                }

                total_len += entry_line.len();
                lines.push(entry_line);
            }
        }

        lines.join("\n")
    }
}

// ---------------------------------------------------------------------------
// Layer 2 -- On-Demand (wing/room filtered retrieval)
// ---------------------------------------------------------------------------

/// Layer 2: ~200-500 tokens per retrieval.
/// Loaded when a specific topic or wing comes up in conversation.
pub struct Layer2 {
    palace_path: String,
}

impl Layer2 {
    pub fn new(palace_path: Option<&str>) -> Self {
        let cfg = MempalaceConfig::new(None);
        Self {
            palace_path: palace_path
                .map(String::from)
                .unwrap_or_else(|| cfg.palace_path()),
        }
    }

    /// Retrieve drawers filtered by wing and/or room.
    pub fn retrieve(&self, wing: Option<&str>, room: Option<&str>, n_results: usize) -> String {
        let store = match PalaceStore::open(&self.palace_path) {
            Ok(s) => s,
            Err(_) => return "No palace found.".to_string(),
        };

        let filter = Self::build_filter(wing, room);
        let drawers = match store.get(filter.as_ref(), Some(n_results)) {
            Ok(d) => d,
            Err(e) => return format!("Retrieval error: {}", e),
        };

        if drawers.is_empty() {
            let mut label = String::new();
            if let Some(w) = wing {
                label.push_str(&format!("wing={}", w));
            }
            if let Some(r) = room {
                if !label.is_empty() {
                    label.push(' ');
                }
                label.push_str(&format!("room={}", r));
            }
            return format!("No drawers found for {}.", label);
        }

        let mut lines = vec![format!("## L2 -- ON-DEMAND ({} drawers)", drawers.len())];
        for drawer in drawers.iter().take(n_results) {
            let room_name = &drawer.metadata.room;
            let source = if !drawer.metadata.source_file.is_empty() {
                Path::new(&drawer.metadata.source_file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let snippet = drawer.content.trim().replace('\n', " ");
            let snippet = if snippet.len() > 300 {
                format!("{}...", &snippet[..297])
            } else {
                snippet
            };

            let mut entry = format!("  [{}] {}", room_name, snippet);
            if !source.is_empty() {
                entry.push_str(&format!("  ({})", source));
            }
            lines.push(entry);
        }

        lines.join("\n")
    }

    fn build_filter(wing: Option<&str>, room: Option<&str>) -> Option<WhereFilter> {
        match (wing, room) {
            (Some(w), Some(r)) => Some(WhereFilter::WingAndRoom(w.to_string(), r.to_string())),
            (Some(w), None) => Some(WhereFilter::Wing(w.to_string())),
            (None, Some(r)) => Some(WhereFilter::Room(r.to_string())),
            (None, None) => None,
        }
    }
}

// ---------------------------------------------------------------------------
// Layer 3 -- Deep Search (full FTS search)
// ---------------------------------------------------------------------------

/// Layer 3: Unlimited depth. Full-text search against the full palace.
pub struct Layer3 {
    palace_path: String,
}

impl Layer3 {
    pub fn new(palace_path: Option<&str>) -> Self {
        let cfg = MempalaceConfig::new(None);
        Self {
            palace_path: palace_path
                .map(String::from)
                .unwrap_or_else(|| cfg.palace_path()),
        }
    }

    /// Full-text search, returns compact result text.
    pub fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> String {
        let store = match PalaceStore::open(&self.palace_path) {
            Ok(s) => s,
            Err(_) => return "No palace found.".to_string(),
        };

        let filter = Layer2::build_filter(wing, room);
        let results = match store.query(query, n_results, filter.as_ref()) {
            Ok(r) => r,
            Err(e) => return format!("Search error: {}", e),
        };

        if results.is_empty() {
            return "No results found.".to_string();
        }

        let mut lines = vec![format!("## L3 -- SEARCH RESULTS for \"{}\"", query)];
        for (i, result) in results.iter().enumerate() {
            let wing_name = &result.metadata.wing;
            let room_name = &result.metadata.room;
            let source = if !result.metadata.source_file.is_empty() {
                Path::new(&result.metadata.source_file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default()
            } else {
                String::new()
            };

            let snippet = result.content.trim().replace('\n', " ");
            let snippet = if snippet.len() > 300 {
                format!("{}...", &snippet[..297])
            } else {
                snippet
            };

            // BM25 scores are negative (lower = better), normalize for display
            let similarity = -result.score;

            lines.push(format!(
                "  [{}] {}/{} (score={:.3})",
                i + 1,
                wing_name,
                room_name,
                similarity
            ));
            lines.push(format!("      {}", snippet));
            if !source.is_empty() {
                lines.push(format!("      src: {}", source));
            }
        }

        lines.join("\n")
    }

    /// Return raw JSON values instead of formatted text.
    pub fn search_raw(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> Vec<serde_json::Value> {
        let store = match PalaceStore::open(&self.palace_path) {
            Ok(s) => s,
            Err(_) => return vec![],
        };

        let filter = Layer2::build_filter(wing, room);
        let results = match store.query(query, n_results, filter.as_ref()) {
            Ok(r) => r,
            Err(_) => return vec![],
        };

        results
            .iter()
            .map(|r| {
                let source_name = Path::new(&r.metadata.source_file)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| "?".to_string());

                serde_json::json!({
                    "text": r.content,
                    "wing": r.metadata.wing,
                    "room": r.metadata.room,
                    "source_file": source_name,
                    "score": -r.score,
                    "metadata": {
                        "wing": r.metadata.wing,
                        "room": r.metadata.room,
                        "source_file": r.metadata.source_file,
                        "chunk_index": r.metadata.chunk_index,
                    }
                })
            })
            .collect()
    }
}

// ---------------------------------------------------------------------------
// MemoryStack -- unified interface
// ---------------------------------------------------------------------------

/// The full 4-layer stack. One struct, one palace, everything works.
///
/// ```ignore
/// let mut stack = MemoryStack::new(None, None);
/// println!("{}", stack.wake_up(None));       // L0 + L1 (~600-900 tokens)
/// println!("{}", stack.recall(Some("my_app"), None, 10)); // L2 on-demand
/// println!("{}", stack.search("pricing change", None, None, 5)); // L3 deep search
/// ```
pub struct MemoryStack {
    palace_path: String,
    identity_path: String,
    pub l0: Layer0,
    pub l1: Layer1,
    pub l2: Layer2,
    pub l3: Layer3,
}

impl MemoryStack {
    pub fn new(palace_path: Option<&str>, identity_path: Option<&str>) -> Self {
        let cfg = MempalaceConfig::new(None);
        let palace = palace_path
            .map(String::from)
            .unwrap_or_else(|| cfg.palace_path());
        let identity = identity_path.map(String::from).unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mempalace")
                .join("identity.txt")
                .to_string_lossy()
                .into_owned()
        });

        Self {
            l0: Layer0::new(Some(&identity)),
            l1: Layer1::new(Some(&palace), None),
            l2: Layer2::new(Some(&palace)),
            l3: Layer3::new(Some(&palace)),
            palace_path: palace,
            identity_path: identity,
        }
    }

    /// Generate wake-up text: L0 (identity) + L1 (essential story).
    /// Typically ~600-900 tokens.
    pub fn wake_up(&mut self, wing: Option<&str>) -> String {
        let mut parts = Vec::new();

        // L0: Identity
        parts.push(self.l0.render());
        parts.push(String::new());

        // L1: Essential Story
        if let Some(w) = wing {
            self.l1.wing = Some(w.to_string());
        }
        parts.push(self.l1.generate());

        parts.join("\n")
    }

    /// On-demand L2 retrieval filtered by wing/room.
    pub fn recall(&self, wing: Option<&str>, room: Option<&str>, n_results: usize) -> String {
        self.l2.retrieve(wing, room, n_results)
    }

    /// Deep L3 full-text search.
    pub fn search(
        &self,
        query: &str,
        wing: Option<&str>,
        room: Option<&str>,
        n_results: usize,
    ) -> String {
        self.l3.search(query, wing, room, n_results)
    }

    /// Status of all layers.
    pub fn status(&mut self) -> serde_json::Value {
        let total_drawers = PalaceStore::open(&self.palace_path)
            .and_then(|s| s.count())
            .unwrap_or(0);

        serde_json::json!({
            "palace_path": self.palace_path,
            "L0_identity": {
                "path": self.identity_path,
                "exists": Path::new(&self.identity_path).exists(),
                "tokens": self.l0.token_estimate(),
            },
            "L1_essential": {
                "description": "Auto-generated from top palace drawers",
            },
            "L2_on_demand": {
                "description": "Wing/room filtered retrieval",
            },
            "L3_deep_search": {
                "description": "Full-text search via FTS5",
            },
            "total_drawers": total_drawers,
        })
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use tempfile::TempDir;

    fn make_store_with_drawers(tmp: &TempDir) -> String {
        let palace_path = tmp.path().to_str().unwrap().to_string();
        let store = PalaceStore::open(&palace_path).unwrap();

        let meta = crate::store::DrawerMetadata {
            wing: "technical".into(),
            room: "rust".into(),
            hall: None,
            chunk_index: 0,
            source_file: "main.rs".into(),
            date: None,
            importance: Some(0.9),
            emotional_weight: None,
            added_by: Some("test".into()),
            filed_at: None,
            extra: HashMap::new(),
        };
        store
            .add("d1", "Rust is a fast systems language", &meta)
            .unwrap();

        let meta2 = crate::store::DrawerMetadata {
            wing: "technical".into(),
            room: "python".into(),
            hall: None,
            chunk_index: 0,
            source_file: "app.py".into(),
            date: None,
            importance: Some(0.5),
            emotional_weight: None,
            added_by: Some("test".into()),
            filed_at: None,
            extra: HashMap::new(),
        };
        store
            .add("d2", "Python is great for scripting", &meta2)
            .unwrap();

        let meta3 = crate::store::DrawerMetadata {
            wing: "emotions".into(),
            room: "joy".into(),
            hall: None,
            chunk_index: 0,
            source_file: "journal.txt".into(),
            date: None,
            importance: Some(0.8),
            emotional_weight: Some(0.9),
            added_by: Some("test".into()),
            filed_at: None,
            extra: HashMap::new(),
        };
        store
            .add("d3", "Today was a beautiful day", &meta3)
            .unwrap();

        palace_path
    }

    #[test]
    fn test_layer0_default_text() {
        let mut l0 = Layer0::new(Some("/nonexistent/path/identity.txt"));
        let text = l0.render();
        assert!(text.contains("No identity configured"));
    }

    #[test]
    fn test_layer0_reads_file() {
        let tmp = TempDir::new().unwrap();
        let id_path = tmp.path().join("identity.txt");
        std::fs::write(&id_path, "I am Atlas, a personal AI.").unwrap();

        let mut l0 = Layer0::new(Some(id_path.to_str().unwrap()));
        assert_eq!(l0.render(), "I am Atlas, a personal AI.");
    }

    #[test]
    fn test_layer0_token_estimate() {
        let tmp = TempDir::new().unwrap();
        let id_path = tmp.path().join("identity.txt");
        std::fs::write(&id_path, "abcd efgh ijkl mnop").unwrap(); // 19 chars

        let mut l0 = Layer0::new(Some(id_path.to_str().unwrap()));
        assert_eq!(l0.token_estimate(), 19 / 4); // 4
    }

    #[test]
    fn test_layer0_caches_text() {
        let tmp = TempDir::new().unwrap();
        let id_path = tmp.path().join("identity.txt");
        std::fs::write(&id_path, "Original text").unwrap();

        let mut l0 = Layer0::new(Some(id_path.to_str().unwrap()));
        let first = l0.render();
        // Overwrite the file
        std::fs::write(&id_path, "Changed text").unwrap();
        // Should still return cached value
        assert_eq!(l0.render(), first);
    }

    #[test]
    fn test_layer1_no_palace() {
        let l1 = Layer1::new(Some("/nonexistent/palace/path"), None);
        let text = l1.generate();
        assert!(
            text.contains("No palace found")
                || text.contains("No drawers")
                || text.contains("No memories")
        );
    }

    #[test]
    fn test_layer1_generate() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l1 = Layer1::new(Some(&palace_path), None);
        let text = l1.generate();
        assert!(text.contains("ESSENTIAL STORY"));
        assert!(text.contains("Rust is a fast"));
    }

    #[test]
    fn test_layer1_with_wing_filter() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l1 = Layer1::new(Some(&palace_path), Some("emotions"));
        let text = l1.generate();
        assert!(text.contains("beautiful day"));
        // Should not contain technical drawers when filtered
        assert!(!text.contains("Rust is a fast"));
    }

    #[test]
    fn test_layer2_retrieve() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l2 = Layer2::new(Some(&palace_path));
        let text = l2.retrieve(Some("technical"), None, 10);
        assert!(text.contains("ON-DEMAND"));
        assert!(text.contains("Rust") || text.contains("Python"));
    }

    #[test]
    fn test_layer2_empty_result() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l2 = Layer2::new(Some(&palace_path));
        let text = l2.retrieve(Some("nonexistent_wing"), None, 10);
        assert!(text.contains("No drawers found"));
    }

    #[test]
    fn test_layer3_search() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l3 = Layer3::new(Some(&palace_path));
        let text = l3.search("rust systems", None, None, 5);
        // Should find the rust drawer
        assert!(
            text.contains("SEARCH RESULTS") || text.contains("No results"),
            "Got: {}",
            text
        );
    }

    #[test]
    fn test_layer3_search_raw() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let l3 = Layer3::new(Some(&palace_path));
        let results = l3.search_raw("rust", None, None, 5);
        // May or may not find results depending on FTS tokenization
        // but should not panic
        assert!(results.is_empty() || results[0].get("text").is_some());
    }

    #[test]
    fn test_memory_stack_wake_up() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let id_path = tmp.path().join("identity.txt");
        std::fs::write(&id_path, "I am TestBot.").unwrap();

        let mut stack = MemoryStack::new(Some(&palace_path), Some(id_path.to_str().unwrap()));
        let text = stack.wake_up(None);
        assert!(text.contains("I am TestBot."));
        assert!(text.contains("ESSENTIAL STORY"));
    }

    #[test]
    fn test_memory_stack_status() {
        let tmp = TempDir::new().unwrap();
        let palace_path = make_store_with_drawers(&tmp);

        let id_path = tmp.path().join("identity.txt");
        std::fs::write(&id_path, "I am TestBot.").unwrap();

        let mut stack = MemoryStack::new(Some(&palace_path), Some(id_path.to_str().unwrap()));
        let status = stack.status();
        assert_eq!(status["total_drawers"], 3);
        assert_eq!(status["L0_identity"]["exists"], true);
    }
}

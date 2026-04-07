use md5::{Digest, Md5};
use rusqlite::{params, Connection};
use serde_json::json;

use crate::error::Result;

pub struct KnowledgeGraph {
    conn: Connection,
}

impl KnowledgeGraph {
    /// Create or open a knowledge graph database.
    /// If `db_path` is None, uses the default path `~/.mempalace/knowledge_graph.sqlite3`.
    pub fn new(db_path: Option<&str>) -> Result<Self> {
        let resolved = match db_path {
            Some(p) => p.to_string(),
            None => {
                let home = dirs::home_dir().unwrap_or_else(|| std::path::PathBuf::from("."));
                home.join(".mempalace")
                    .join("knowledge_graph.sqlite3")
                    .to_string_lossy()
                    .into_owned()
            }
        };

        // Ensure parent directory exists
        if let Some(parent) = std::path::Path::new(&resolved).parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&resolved)?;
        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS entities (
                id TEXT PRIMARY KEY,
                name TEXT NOT NULL,
                type TEXT DEFAULT 'unknown',
                properties TEXT DEFAULT '{}',
                created_at TEXT DEFAULT CURRENT_TIMESTAMP
            );

            CREATE TABLE IF NOT EXISTS triples (
                id TEXT PRIMARY KEY,
                subject TEXT NOT NULL,
                predicate TEXT NOT NULL,
                object TEXT NOT NULL,
                valid_from TEXT,
                valid_to TEXT,
                confidence REAL DEFAULT 1.0,
                source_closet TEXT,
                source_file TEXT,
                extracted_at TEXT DEFAULT CURRENT_TIMESTAMP,
                FOREIGN KEY (subject) REFERENCES entities(id),
                FOREIGN KEY (object) REFERENCES entities(id)
            );

            CREATE INDEX IF NOT EXISTS idx_triples_subject ON triples(subject);
            CREATE INDEX IF NOT EXISTS idx_triples_object ON triples(object);
            CREATE INDEX IF NOT EXISTS idx_triples_predicate ON triples(predicate);
            CREATE INDEX IF NOT EXISTS idx_triples_valid ON triples(valid_from, valid_to);",
        )?;

        Ok(Self { conn })
    }

    /// Normalize an entity name to a stable ID: lowercase, spaces to underscores, remove apostrophes.
    fn entity_id(name: &str) -> String {
        name.to_lowercase().replace(' ', "_").replace('\'', "")
    }

    // ── Write operations ────────────────────────────────────────────────

    /// Add or update an entity node. Returns the entity id.
    pub fn add_entity(
        &self,
        name: &str,
        entity_type: &str,
        properties: Option<&serde_json::Value>,
    ) -> Result<String> {
        let eid = Self::entity_id(name);
        let props = properties
            .map(|p| serde_json::to_string(p).unwrap_or_else(|_| "{}".to_string()))
            .unwrap_or_else(|| "{}".to_string());

        self.conn.execute(
            "INSERT OR REPLACE INTO entities (id, name, type, properties) VALUES (?, ?, ?, ?)",
            params![eid, name, entity_type, props],
        )?;

        Ok(eid)
    }

    /// Add a relationship triple: subject -> predicate -> object.
    /// Auto-creates entities if they don't exist.
    /// Returns the existing triple id if an identical active triple already exists.
    #[allow(clippy::too_many_arguments)]
    pub fn add_triple(
        &self,
        subject: &str,
        predicate: &str,
        obj: &str,
        valid_from: Option<&str>,
        valid_to: Option<&str>,
        confidence: Option<f64>,
        source_closet: Option<&str>,
        source_file: Option<&str>,
    ) -> Result<String> {
        let sub_id = Self::entity_id(subject);
        let obj_id = Self::entity_id(obj);
        let pred = predicate.to_lowercase().replace(' ', "_");
        let conf = confidence.unwrap_or(1.0);

        // Auto-create entities if they don't exist
        self.conn.execute(
            "INSERT OR IGNORE INTO entities (id, name) VALUES (?, ?)",
            params![sub_id, subject],
        )?;
        self.conn.execute(
            "INSERT OR IGNORE INTO entities (id, name) VALUES (?, ?)",
            params![obj_id, obj],
        )?;

        // Check for existing identical active triple
        let existing: Option<String> = self
            .conn
            .query_row(
                "SELECT id FROM triples WHERE subject=? AND predicate=? AND object=? AND valid_to IS NULL",
                params![sub_id, pred, obj_id],
                |row| row.get(0),
            )
            .ok();

        if let Some(id) = existing {
            return Ok(id);
        }

        // Generate triple id with md5 hash
        let now = chrono::Utc::now().to_rfc3339();
        let hash_input = format!("{}{}", valid_from.unwrap_or(""), now);
        let mut hasher = Md5::new();
        hasher.update(hash_input.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        let triple_id = format!("t_{}_{}_{}_{}", sub_id, pred, obj_id, &hash[..8]);

        self.conn.execute(
            "INSERT INTO triples (id, subject, predicate, object, valid_from, valid_to, confidence, source_closet, source_file) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?)",
            params![triple_id, sub_id, pred, obj_id, valid_from, valid_to, conf, source_closet, source_file],
        )?;

        Ok(triple_id)
    }

    /// Mark a relationship as no longer valid (set valid_to date).
    /// If `ended` is None, uses today's date.
    pub fn invalidate(
        &self,
        subject: &str,
        predicate: &str,
        obj: &str,
        ended: Option<&str>,
    ) -> Result<()> {
        let sub_id = Self::entity_id(subject);
        let obj_id = Self::entity_id(obj);
        let pred = predicate.to_lowercase().replace(' ', "_");
        let ended_date = ended
            .map(|s| s.to_string())
            .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%d").to_string());

        self.conn.execute(
            "UPDATE triples SET valid_to=? WHERE subject=? AND predicate=? AND object=? AND valid_to IS NULL",
            params![ended_date, sub_id, pred, obj_id],
        )?;

        Ok(())
    }

    // ── Query operations ────────────────────────────────────────────────

    /// Get all relationships for an entity.
    /// direction: "outgoing" (entity -> ?), "incoming" (? -> entity), "both"
    pub fn query_entity(
        &self,
        name: &str,
        as_of: Option<&str>,
        direction: &str,
    ) -> Result<Vec<serde_json::Value>> {
        let eid = Self::entity_id(name);
        let mut results = Vec::new();

        if direction == "outgoing" || direction == "both" {
            let mut sql = String::from(
                "SELECT t.id, t.subject, t.predicate, t.object, t.valid_from, t.valid_to, \
                 t.confidence, t.source_closet, e.name as obj_name \
                 FROM triples t JOIN entities e ON t.object = e.id WHERE t.subject = ?",
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            param_values.push(Box::new(eid.clone()));

            if let Some(date) = as_of {
                sql.push_str(
                    " AND (t.valid_from IS NULL OR t.valid_from <= ?) AND (t.valid_to IS NULL OR t.valid_to >= ?)",
                );
                param_values.push(Box::new(date.to_string()));
                param_values.push(Box::new(date.to_string()));
            }

            let mut stmt = self.conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = param_values
                .iter()
                .map(|b| b.as_ref() as &dyn rusqlite::types::ToSql)
                .collect();
            let mut rows = stmt.query(params.as_slice())?;

            while let Some(row) = rows.next()? {
                let valid_to: Option<String> = row.get(5)?;
                results.push(json!({
                    "direction": "outgoing",
                    "subject": name,
                    "predicate": row.get::<_, String>(2)?,
                    "object": row.get::<_, String>(8)?,
                    "valid_from": row.get::<_, Option<String>>(4)?,
                    "valid_to": &valid_to,
                    "confidence": row.get::<_, f64>(6)?,
                    "source_closet": row.get::<_, Option<String>>(7)?,
                    "current": valid_to.is_none(),
                }));
            }
        }

        if direction == "incoming" || direction == "both" {
            let mut sql = String::from(
                "SELECT t.id, t.subject, t.predicate, t.object, t.valid_from, t.valid_to, \
                 t.confidence, t.source_closet, e.name as sub_name \
                 FROM triples t JOIN entities e ON t.subject = e.id WHERE t.object = ?",
            );
            let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
            param_values.push(Box::new(eid.clone()));

            if let Some(date) = as_of {
                sql.push_str(
                    " AND (t.valid_from IS NULL OR t.valid_from <= ?) AND (t.valid_to IS NULL OR t.valid_to >= ?)",
                );
                param_values.push(Box::new(date.to_string()));
                param_values.push(Box::new(date.to_string()));
            }

            let mut stmt = self.conn.prepare(&sql)?;
            let params: Vec<&dyn rusqlite::types::ToSql> = param_values
                .iter()
                .map(|b| b.as_ref() as &dyn rusqlite::types::ToSql)
                .collect();
            let mut rows = stmt.query(params.as_slice())?;

            while let Some(row) = rows.next()? {
                let valid_to: Option<String> = row.get(5)?;
                results.push(json!({
                    "direction": "incoming",
                    "subject": row.get::<_, String>(8)?,
                    "predicate": row.get::<_, String>(2)?,
                    "object": name,
                    "valid_from": row.get::<_, Option<String>>(4)?,
                    "valid_to": &valid_to,
                    "confidence": row.get::<_, f64>(6)?,
                    "source_closet": row.get::<_, Option<String>>(7)?,
                    "current": valid_to.is_none(),
                }));
            }
        }

        Ok(results)
    }

    /// Get all triples with a given relationship type.
    pub fn query_relationship(
        &self,
        predicate: &str,
        as_of: Option<&str>,
    ) -> Result<Vec<serde_json::Value>> {
        let pred = predicate.to_lowercase().replace(' ', "_");
        let mut sql = String::from(
            "SELECT t.*, s.name as sub_name, o.name as obj_name \
             FROM triples t \
             JOIN entities s ON t.subject = s.id \
             JOIN entities o ON t.object = o.id \
             WHERE t.predicate = ?",
        );
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        param_values.push(Box::new(pred.clone()));

        if let Some(date) = as_of {
            sql.push_str(
                " AND (t.valid_from IS NULL OR t.valid_from <= ?) AND (t.valid_to IS NULL OR t.valid_to >= ?)",
            );
            param_values.push(Box::new(date.to_string()));
            param_values.push(Box::new(date.to_string()));
        }

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|b| b.as_ref() as &dyn rusqlite::types::ToSql)
            .collect();
        let mut rows = stmt.query(params.as_slice())?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let valid_to: Option<String> = row.get(5)?;
            results.push(json!({
                "subject": row.get::<_, String>(10)?,
                "predicate": &pred,
                "object": row.get::<_, String>(11)?,
                "valid_from": row.get::<_, Option<String>>(4)?,
                "valid_to": &valid_to,
                "current": valid_to.is_none(),
            }));
        }

        Ok(results)
    }

    /// Get all facts in chronological order, optionally filtered by entity.
    pub fn timeline(&self, entity_name: Option<&str>) -> Result<Vec<serde_json::Value>> {
        let mut results = Vec::new();

        if let Some(name) = entity_name {
            let eid = Self::entity_id(name);
            let mut stmt = self.conn.prepare(
                "SELECT t.*, s.name as sub_name, o.name as obj_name \
                 FROM triples t \
                 JOIN entities s ON t.subject = s.id \
                 JOIN entities o ON t.object = o.id \
                 WHERE (t.subject = ? OR t.object = ?) \
                 ORDER BY t.valid_from ASC NULLS LAST",
            )?;
            let mut rows = stmt.query(params![eid, eid])?;
            while let Some(row) = rows.next()? {
                let valid_to: Option<String> = row.get(5)?;
                results.push(json!({
                    "subject": row.get::<_, String>(10)?,
                    "predicate": row.get::<_, String>(2)?,
                    "object": row.get::<_, String>(11)?,
                    "valid_from": row.get::<_, Option<String>>(4)?,
                    "valid_to": &valid_to,
                    "current": valid_to.is_none(),
                }));
            }
        } else {
            let mut stmt = self.conn.prepare(
                "SELECT t.*, s.name as sub_name, o.name as obj_name \
                 FROM triples t \
                 JOIN entities s ON t.subject = s.id \
                 JOIN entities o ON t.object = o.id \
                 ORDER BY t.valid_from ASC NULLS LAST \
                 LIMIT 100",
            )?;
            let mut rows = stmt.query([])?;
            while let Some(row) = rows.next()? {
                let valid_to: Option<String> = row.get(5)?;
                results.push(json!({
                    "subject": row.get::<_, String>(10)?,
                    "predicate": row.get::<_, String>(2)?,
                    "object": row.get::<_, String>(11)?,
                    "valid_from": row.get::<_, Option<String>>(4)?,
                    "valid_to": &valid_to,
                    "current": valid_to.is_none(),
                }));
            }
        }

        Ok(results)
    }

    // ── Stats ───────────────────────────────────────────────────────────

    /// Get summary statistics for the knowledge graph.
    pub fn stats(&self) -> Result<serde_json::Value> {
        let entities: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM entities", [], |row| row.get(0))?;
        let triples: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM triples", [], |row| row.get(0))?;
        let current: i64 = self.conn.query_row(
            "SELECT COUNT(*) FROM triples WHERE valid_to IS NULL",
            [],
            |row| row.get(0),
        )?;
        let expired = triples - current;

        let mut stmt = self
            .conn
            .prepare("SELECT DISTINCT predicate FROM triples ORDER BY predicate")?;
        let predicates: Vec<String> = stmt
            .query_map([], |row| row.get(0))?
            .filter_map(|r| r.ok())
            .collect();

        Ok(json!({
            "entities": entities,
            "triples": triples,
            "current_facts": current,
            "expired_facts": expired,
            "relationship_types": predicates,
        }))
    }

    // ── Seed from known facts ───────────────────────────────────────────

    /// Seed the knowledge graph from a JSON structure matching the Python ENTITY_FACTS format.
    pub fn seed_from_entity_facts(&self, entity_facts: &serde_json::Value) -> Result<()> {
        let obj = entity_facts.as_object().ok_or_else(|| {
            crate::error::MempalaceError::Parse("entity_facts must be a JSON object".into())
        })?;

        for (key, facts) in obj {
            let capitalized_key = capitalize(key);
            let name = facts
                .get("full_name")
                .and_then(|v| v.as_str())
                .unwrap_or(&capitalized_key);

            let etype = facts
                .get("type")
                .and_then(|v| v.as_str())
                .unwrap_or("person");

            let props = json!({
                "gender": facts.get("gender").and_then(|v| v.as_str()).unwrap_or(""),
                "birthday": facts.get("birthday").and_then(|v| v.as_str()).unwrap_or(""),
            });
            self.add_entity(name, etype, Some(&props))?;

            // Parent relationship
            if let Some(parent) = facts.get("parent").and_then(|v| v.as_str()) {
                let birthday = facts.get("birthday").and_then(|v| v.as_str());
                let cap = capitalize(parent);
                self.add_triple(name, "child_of", &cap, birthday, None, None, None, None)?;
            }

            // Partner relationship
            if let Some(partner) = facts.get("partner").and_then(|v| v.as_str()) {
                let cap = capitalize(partner);
                self.add_triple(name, "married_to", &cap, None, None, None, None, None)?;
            }

            // Typed relationships
            if let Some(rel) = facts.get("relationship").and_then(|v| v.as_str()) {
                match rel {
                    "daughter" => {
                        let p = facts
                            .get("parent")
                            .and_then(|v| v.as_str())
                            .map(capitalize)
                            .unwrap_or_else(|| name.to_string());
                        let birthday = facts.get("birthday").and_then(|v| v.as_str());
                        self.add_triple(name, "is_child_of", &p, birthday, None, None, None, None)?;
                    }
                    "husband" => {
                        let p = facts
                            .get("partner")
                            .and_then(|v| v.as_str())
                            .map(capitalize)
                            .unwrap_or_else(|| name.to_string());
                        self.add_triple(name, "is_partner_of", &p, None, None, None, None, None)?;
                    }
                    "brother" => {
                        let s = facts
                            .get("sibling")
                            .and_then(|v| v.as_str())
                            .map(capitalize)
                            .unwrap_or_else(|| name.to_string());
                        self.add_triple(name, "is_sibling_of", &s, None, None, None, None, None)?;
                    }
                    "dog" => {
                        let o = facts
                            .get("owner")
                            .and_then(|v| v.as_str())
                            .map(capitalize)
                            .unwrap_or_else(|| name.to_string());
                        self.add_triple(name, "is_pet_of", &o, None, None, None, None, None)?;
                        self.add_entity(name, "animal", None)?;
                    }
                    _ => {}
                }
            }

            // Interests
            if let Some(interests) = facts.get("interests").and_then(|v| v.as_array()) {
                for interest in interests {
                    if let Some(i) = interest.as_str() {
                        let cap = capitalize(i);
                        self.add_triple(
                            name,
                            "loves",
                            &cap,
                            Some("2025-01-01"),
                            None,
                            None,
                            None,
                            None,
                        )?;
                    }
                }
            }
        }

        Ok(())
    }
}

fn capitalize(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        None => String::new(),
        Some(f) => f.to_uppercase().to_string() + c.as_str(),
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn open_tmp_kg() -> (KnowledgeGraph, TempDir) {
        let tmp = TempDir::new().unwrap();
        let db_path = tmp.path().join("test_kg.sqlite3");
        let kg = KnowledgeGraph::new(Some(db_path.to_str().unwrap())).unwrap();
        (kg, tmp)
    }

    #[test]
    fn test_entity_id_normalization() {
        assert_eq!(KnowledgeGraph::entity_id("Max"), "max");
        assert_eq!(KnowledgeGraph::entity_id("Mary Jane"), "mary_jane");
        assert_eq!(KnowledgeGraph::entity_id("O'Brien"), "obrien");
        assert_eq!(KnowledgeGraph::entity_id("Alice Bob"), "alice_bob");
    }

    #[test]
    fn test_add_entity() {
        let (kg, _tmp) = open_tmp_kg();
        let eid = kg.add_entity("Max", "person", None).unwrap();
        assert_eq!(eid, "max");

        let stats = kg.stats().unwrap();
        assert_eq!(stats["entities"], 1);
    }

    #[test]
    fn test_add_entity_with_properties() {
        let (kg, _tmp) = open_tmp_kg();
        let props = json!({"gender": "male", "birthday": "2015-04-01"});
        let eid = kg.add_entity("Max", "person", Some(&props)).unwrap();
        assert_eq!(eid, "max");
    }

    #[test]
    fn test_add_entity_replace() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_entity("Max", "unknown", None).unwrap();
        kg.add_entity("Max", "person", Some(&json!({"age": 10})))
            .unwrap();

        // Should still be 1 entity (replaced)
        let stats = kg.stats().unwrap();
        assert_eq!(stats["entities"], 1);
    }

    #[test]
    fn test_add_triple() {
        let (kg, _tmp) = open_tmp_kg();
        let tid = kg
            .add_triple(
                "Max",
                "child_of",
                "Alice",
                Some("2015-04-01"),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(tid.starts_with("t_max_child_of_alice_"));

        let stats = kg.stats().unwrap();
        assert_eq!(stats["triples"], 1);
        assert_eq!(stats["entities"], 2); // auto-created
    }

    #[test]
    fn test_add_triple_auto_creates_entities() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Bob", "knows", "Carol", None, None, None, None, None)
            .unwrap();

        let stats = kg.stats().unwrap();
        assert_eq!(stats["entities"], 2);
    }

    #[test]
    fn test_duplicate_triple_detection() {
        let (kg, _tmp) = open_tmp_kg();
        let t1 = kg
            .add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        let t2 = kg
            .add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();

        // Should return the same triple id (dedup)
        assert_eq!(t1, t2);
        let stats = kg.stats().unwrap();
        assert_eq!(stats["triples"], 1);
    }

    #[test]
    fn test_invalidate() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "does",
            "swimming",
            Some("2025-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.invalidate("Max", "does", "swimming", Some("2026-02-15"))
            .unwrap();

        let stats = kg.stats().unwrap();
        assert_eq!(stats["current_facts"], 0);
        assert_eq!(stats["expired_facts"], 1);
    }

    #[test]
    fn test_query_entity_outgoing() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "child_of",
            "Alice",
            Some("2015-04-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple("Max", "loves", "Chess", None, None, None, None, None)
            .unwrap();

        let results = kg.query_entity("Max", None, "outgoing").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r["direction"] == "outgoing"));
        assert!(results.iter().all(|r| r["subject"] == "Max"));
    }

    #[test]
    fn test_query_entity_incoming() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        kg.add_triple("Bob", "knows", "Alice", None, None, None, None, None)
            .unwrap();

        let results = kg.query_entity("Alice", None, "incoming").unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r["direction"] == "incoming"));
        assert!(results.iter().all(|r| r["object"] == "Alice"));
    }

    #[test]
    fn test_query_entity_both() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        kg.add_triple("Alice", "married_to", "Bob", None, None, None, None, None)
            .unwrap();

        let results = kg.query_entity("Alice", None, "both").unwrap();
        assert_eq!(results.len(), 2);
        let directions: Vec<&str> = results
            .iter()
            .map(|r| r["direction"].as_str().unwrap())
            .collect();
        assert!(directions.contains(&"incoming"));
        assert!(directions.contains(&"outgoing"));
    }

    #[test]
    fn test_query_entity_with_time_filter() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "does",
            "swimming",
            Some("2025-01-01"),
            Some("2025-12-31"),
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Max",
            "does",
            "chess",
            Some("2026-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        // Query at 2025-06-01 — only swimming should be active
        let results = kg
            .query_entity("Max", Some("2025-06-01"), "outgoing")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["object"], "swimming");

        // Query at 2026-03-01 — only chess should be active
        let results = kg
            .query_entity("Max", Some("2026-03-01"), "outgoing")
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0]["object"], "chess");
    }

    #[test]
    fn test_query_relationship() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        kg.add_triple("Bob", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        kg.add_triple("Max", "loves", "Chess", None, None, None, None, None)
            .unwrap();

        let results = kg.query_relationship("child_of", None).unwrap();
        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|r| r["predicate"] == "child_of"));
    }

    #[test]
    fn test_timeline_all() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "child_of",
            "Alice",
            Some("2015-04-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Max",
            "does",
            "swimming",
            Some("2025-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let tl = kg.timeline(None).unwrap();
        assert_eq!(tl.len(), 2);
        // Should be sorted by valid_from ascending
        assert_eq!(tl[0]["valid_from"], "2015-04-01");
        assert_eq!(tl[1]["valid_from"], "2025-01-01");
    }

    #[test]
    fn test_timeline_entity() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "child_of",
            "Alice",
            Some("2015-04-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Bob",
            "knows",
            "Carol",
            Some("2020-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.add_triple(
            "Max",
            "does",
            "swimming",
            Some("2025-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();

        let tl = kg.timeline(Some("Max")).unwrap();
        assert_eq!(tl.len(), 2); // Only Max's triples
    }

    #[test]
    fn test_stats() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Max", "child_of", "Alice", None, None, None, None, None)
            .unwrap();
        kg.add_triple("Max", "loves", "Chess", None, None, None, None, None)
            .unwrap();
        kg.invalidate("Max", "loves", "Chess", Some("2026-01-01"))
            .unwrap();

        let stats = kg.stats().unwrap();
        assert_eq!(stats["entities"], 3); // Max, Alice, Chess
        assert_eq!(stats["triples"], 2);
        assert_eq!(stats["current_facts"], 1);
        assert_eq!(stats["expired_facts"], 1);
        let types = stats["relationship_types"].as_array().unwrap();
        assert_eq!(types.len(), 2);
    }

    #[test]
    fn test_seed_from_entity_facts() {
        let (kg, _tmp) = open_tmp_kg();
        let facts = json!({
            "max": {
                "full_name": "Max",
                "type": "person",
                "gender": "male",
                "birthday": "2015-04-01",
                "parent": "alice",
                "relationship": "daughter",
                "interests": ["chess", "swimming"]
            }
        });
        kg.seed_from_entity_facts(&facts).unwrap();

        let stats = kg.stats().unwrap();
        // Max + Alice (auto) + Chess + Swimming = at least 4 entities
        assert!(stats["entities"].as_i64().unwrap() >= 4);
        assert!(stats["triples"].as_i64().unwrap() >= 3);
    }

    #[test]
    fn test_predicate_normalization() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple("Max", "Child Of", "Alice", None, None, None, None, None)
            .unwrap();

        let results = kg.query_relationship("child_of", None).unwrap();
        assert_eq!(results.len(), 1);
    }

    #[test]
    fn test_invalidate_then_readd() {
        let (kg, _tmp) = open_tmp_kg();
        kg.add_triple(
            "Max",
            "does",
            "swimming",
            Some("2025-01-01"),
            None,
            None,
            None,
            None,
        )
        .unwrap();
        kg.invalidate("Max", "does", "swimming", Some("2025-12-31"))
            .unwrap();

        // Now re-add (should create a new triple since the old one has valid_to set)
        let t2 = kg
            .add_triple(
                "Max",
                "does",
                "swimming",
                Some("2026-06-01"),
                None,
                None,
                None,
                None,
            )
            .unwrap();
        assert!(t2.starts_with("t_max_does_swimming_"));

        let stats = kg.stats().unwrap();
        assert_eq!(stats["triples"], 2);
        assert_eq!(stats["current_facts"], 1);
    }
}

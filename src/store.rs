use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

use crate::error::Result;

// ── Data types ──────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DrawerMetadata {
    pub wing: String,
    pub room: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hall: Option<String>,
    #[serde(default)]
    pub chunk_index: u32,
    pub source_file: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub date: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub importance: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub emotional_weight: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub added_by: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub filed_at: Option<String>,
    #[serde(flatten)]
    pub extra: HashMap<String, String>,
}

#[derive(Debug, Clone)]
pub struct Drawer {
    pub id: String,
    pub content: String,
    pub metadata: DrawerMetadata,
}

#[derive(Debug, Clone)]
pub struct SearchResult {
    pub id: String,
    pub content: String,
    pub metadata: DrawerMetadata,
    pub score: f64,
}

#[derive(Debug, Clone)]
pub enum WhereFilter {
    Wing(String),
    Room(String),
    WingAndRoom(String, String),
    SourceFile(String),
    Custom(String, String),
}

// ── PalaceStore ─────────────────────────────────────────────────────────────

pub struct PalaceStore {
    conn: Connection,
}

impl PalaceStore {
    /// Open (or create) a palace store at the given directory path.
    /// The SQLite database is stored at `palace_path/palace.sqlite3`.
    pub fn open(palace_path: &str) -> Result<Self> {
        let dir = Path::new(palace_path);
        std::fs::create_dir_all(dir)?;
        let db_path = dir.join("palace.sqlite3");
        let conn = Connection::open(&db_path)?;

        conn.execute_batch("PRAGMA journal_mode=WAL;")?;

        conn.execute_batch(
            "CREATE TABLE IF NOT EXISTS drawers (
                id TEXT PRIMARY KEY,
                content TEXT NOT NULL,
                metadata TEXT NOT NULL
            );",
        )?;

        conn.execute_batch(
            "CREATE VIRTUAL TABLE IF NOT EXISTS drawers_fts USING fts5(content, content_rowid='rowid');",
        )?;

        Ok(Self { conn })
    }

    /// Insert a drawer. Returns `false` if a drawer with the same id already exists.
    pub fn add(&self, id: &str, content: &str, metadata: &DrawerMetadata) -> Result<bool> {
        let meta_json = serde_json::to_string(metadata)?;

        // Check for duplicate
        let exists: bool = self.conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM drawers WHERE id = ?)",
            params![id],
            |row| row.get(0),
        )?;
        if exists {
            return Ok(false);
        }

        self.conn.execute(
            "INSERT INTO drawers (id, content, metadata) VALUES (?, ?, ?)",
            params![id, content, meta_json],
        )?;

        // Get the rowid we just inserted
        let rowid: i64 = self.conn.query_row(
            "SELECT rowid FROM drawers WHERE id = ?",
            params![id],
            |row| row.get(0),
        )?;

        self.conn.execute(
            "INSERT INTO drawers_fts (rowid, content) VALUES (?, ?)",
            params![rowid, content],
        )?;

        Ok(true)
    }

    /// Get a single drawer by id.
    pub fn get_by_id(&self, id: &str) -> Result<Option<Drawer>> {
        let mut stmt = self
            .conn
            .prepare("SELECT id, content, metadata FROM drawers WHERE id = ?")?;
        let mut rows = stmt.query(params![id])?;
        match rows.next()? {
            Some(row) => {
                let meta_str: String = row.get(2)?;
                Ok(Some(Drawer {
                    id: row.get(0)?,
                    content: row.get(1)?,
                    metadata: serde_json::from_str(&meta_str)?,
                }))
            }
            None => Ok(None),
        }
    }

    /// Get drawers with optional metadata filtering.
    pub fn get(
        &self,
        where_clause: Option<&WhereFilter>,
        limit: Option<usize>,
    ) -> Result<Vec<Drawer>> {
        let (where_sql, bind_values) = Self::build_where(where_clause);
        let limit_sql = limit.map(|n| format!(" LIMIT {}", n)).unwrap_or_default();
        let sql = format!(
            "SELECT id, content, metadata FROM drawers{}{}",
            where_sql, limit_sql
        );

        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = bind_values
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        let mut rows = stmt.query(params.as_slice())?;

        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let meta_str: String = row.get(2)?;
            results.push(Drawer {
                id: row.get(0)?,
                content: row.get(1)?,
                metadata: serde_json::from_str(&meta_str)?,
            });
        }
        Ok(results)
    }

    /// Full-text search using FTS5 with BM25 scoring.
    pub fn query(
        &self,
        query_text: &str,
        n_results: usize,
        where_clause: Option<&WhereFilter>,
    ) -> Result<Vec<SearchResult>> {
        let (extra_where, bind_values) = Self::build_where_for_join(where_clause);
        let sql = format!(
            "SELECT d.id, d.content, d.metadata, bm25(drawers_fts) AS score \
             FROM drawers_fts f \
             JOIN drawers d ON d.rowid = f.rowid \
             WHERE drawers_fts MATCH ?{} \
             ORDER BY score \
             LIMIT ?",
            extra_where
        );

        let mut stmt = self.conn.prepare(&sql)?;

        // Build parameter list: query_text, then any filter binds, then limit
        let limit_val = n_results as i64;
        let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
        param_values.push(Box::new(query_text.to_string()));
        for v in &bind_values {
            param_values.push(Box::new(v.clone()));
        }
        param_values.push(Box::new(limit_val));

        let params: Vec<&dyn rusqlite::types::ToSql> = param_values
            .iter()
            .map(|b| b.as_ref() as &dyn rusqlite::types::ToSql)
            .collect();

        let mut rows = stmt.query(params.as_slice())?;
        let mut results = Vec::new();
        while let Some(row) = rows.next()? {
            let meta_str: String = row.get(2)?;
            results.push(SearchResult {
                id: row.get(0)?,
                content: row.get(1)?,
                metadata: serde_json::from_str(&meta_str)?,
                score: row.get(3)?,
            });
        }
        Ok(results)
    }

    /// Delete a drawer by id. Returns `true` if a row was deleted.
    pub fn delete(&self, id: &str) -> Result<bool> {
        // Get rowid before deleting so we can remove FTS entry
        let rowid: Option<i64> = self
            .conn
            .query_row(
                "SELECT rowid FROM drawers WHERE id = ?",
                params![id],
                |row| row.get(0),
            )
            .ok();

        if let Some(rid) = rowid {
            self.conn
                .execute("DELETE FROM drawers_fts WHERE rowid = ?", params![rid])?;
        }

        let deleted = self
            .conn
            .execute("DELETE FROM drawers WHERE id = ?", params![id])?;
        Ok(deleted > 0)
    }

    /// Count all drawers.
    pub fn count(&self) -> Result<usize> {
        let n: i64 = self
            .conn
            .query_row("SELECT COUNT(*) FROM drawers", [], |row| row.get(0))?;
        Ok(n as usize)
    }

    /// Count drawers matching a filter.
    pub fn count_where(&self, filter: &WhereFilter) -> Result<usize> {
        let (where_sql, bind_values) = Self::build_where(Some(filter));
        let sql = format!("SELECT COUNT(*) FROM drawers{}", where_sql);
        let mut stmt = self.conn.prepare(&sql)?;
        let params: Vec<&dyn rusqlite::types::ToSql> = bind_values
            .iter()
            .map(|v| v as &dyn rusqlite::types::ToSql)
            .collect();
        let n: i64 = stmt.query_row(params.as_slice(), |row| row.get(0))?;
        Ok(n as usize)
    }

    // ── Private helpers ─────────────────────────────────────────────────

    fn build_where(filter: Option<&WhereFilter>) -> (String, Vec<String>) {
        match filter {
            None => (String::new(), vec![]),
            Some(f) => {
                let (clause, vals) = Self::filter_to_sql(f);
                (format!(" WHERE {}", clause), vals)
            }
        }
    }

    fn build_where_for_join(filter: Option<&WhereFilter>) -> (String, Vec<String>) {
        match filter {
            None => (String::new(), vec![]),
            Some(f) => {
                let (clause, vals) = Self::filter_to_sql_prefixed(f, "d");
                (format!(" AND {}", clause), vals)
            }
        }
    }

    fn filter_to_sql(filter: &WhereFilter) -> (String, Vec<String>) {
        match filter {
            WhereFilter::Wing(w) => (
                "json_extract(metadata, '$.wing') = ?".to_string(),
                vec![w.clone()],
            ),
            WhereFilter::Room(r) => (
                "json_extract(metadata, '$.room') = ?".to_string(),
                vec![r.clone()],
            ),
            WhereFilter::WingAndRoom(w, r) => (
                "json_extract(metadata, '$.wing') = ? AND json_extract(metadata, '$.room') = ?"
                    .to_string(),
                vec![w.clone(), r.clone()],
            ),
            WhereFilter::SourceFile(s) => (
                "json_extract(metadata, '$.source_file') = ?".to_string(),
                vec![s.clone()],
            ),
            WhereFilter::Custom(key, val) => {
                // Sanitize key to prevent SQL injection — only allow alphanumeric + underscores
                let safe_key: String = key
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                (
                    format!("json_extract(metadata, '$.{}') = ?", safe_key),
                    vec![val.clone()],
                )
            }
        }
    }

    fn filter_to_sql_prefixed(filter: &WhereFilter, prefix: &str) -> (String, Vec<String>) {
        match filter {
            WhereFilter::Wing(w) => (
                format!("json_extract({}.metadata, '$.wing') = ?", prefix),
                vec![w.clone()],
            ),
            WhereFilter::Room(r) => (
                format!("json_extract({}.metadata, '$.room') = ?", prefix),
                vec![r.clone()],
            ),
            WhereFilter::WingAndRoom(w, r) => (
                format!(
                    "json_extract({p}.metadata, '$.wing') = ? AND json_extract({p}.metadata, '$.room') = ?",
                    p = prefix
                ),
                vec![w.clone(), r.clone()],
            ),
            WhereFilter::SourceFile(s) => (
                format!("json_extract({}.metadata, '$.source_file') = ?", prefix),
                vec![s.clone()],
            ),
            WhereFilter::Custom(key, val) => {
                let safe_key: String = key
                    .chars()
                    .filter(|c| c.is_alphanumeric() || *c == '_')
                    .collect();
                (
                    format!("json_extract({}.metadata, '$.{}') = ?", prefix, safe_key),
                    vec![val.clone()],
                )
            }
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn sample_metadata(wing: &str, room: &str, source: &str) -> DrawerMetadata {
        DrawerMetadata {
            wing: wing.into(),
            room: room.into(),
            hall: None,
            chunk_index: 0,
            source_file: source.into(),
            date: None,
            importance: None,
            emotional_weight: None,
            added_by: None,
            filed_at: None,
            extra: HashMap::new(),
        }
    }

    fn open_tmp_store() -> (PalaceStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = PalaceStore::open(tmp.path().to_str().unwrap()).unwrap();
        (store, tmp)
    }

    #[test]
    fn test_open_creates_db() {
        let tmp = TempDir::new().unwrap();
        let palace_path = tmp.path().join("mypalace");
        PalaceStore::open(palace_path.to_str().unwrap()).unwrap();
        assert!(palace_path.join("palace.sqlite3").exists());
    }

    #[test]
    fn test_add_and_get_by_id() {
        let (store, _tmp) = open_tmp_store();
        let meta = sample_metadata("technical", "rust", "test.rs");
        assert!(store.add("d1", "hello world", &meta).unwrap());

        let drawer = store.get_by_id("d1").unwrap().unwrap();
        assert_eq!(drawer.id, "d1");
        assert_eq!(drawer.content, "hello world");
        assert_eq!(drawer.metadata.wing, "technical");
    }

    #[test]
    fn test_add_duplicate_returns_false() {
        let (store, _tmp) = open_tmp_store();
        let meta = sample_metadata("technical", "rust", "test.rs");
        assert!(store.add("d1", "hello", &meta).unwrap());
        assert!(!store.add("d1", "hello again", &meta).unwrap());
    }

    #[test]
    fn test_get_by_id_not_found() {
        let (store, _tmp) = open_tmp_store();
        assert!(store.get_by_id("nonexistent").unwrap().is_none());
    }

    #[test]
    fn test_delete() {
        let (store, _tmp) = open_tmp_store();
        let meta = sample_metadata("technical", "rust", "test.rs");
        store.add("d1", "hello world", &meta).unwrap();
        assert!(store.delete("d1").unwrap());
        assert!(store.get_by_id("d1").unwrap().is_none());
    }

    #[test]
    fn test_delete_nonexistent() {
        let (store, _tmp) = open_tmp_store();
        assert!(!store.delete("nope").unwrap());
    }

    #[test]
    fn test_count() {
        let (store, _tmp) = open_tmp_store();
        assert_eq!(store.count().unwrap(), 0);

        let meta = sample_metadata("technical", "rust", "test.rs");
        store.add("d1", "one", &meta).unwrap();
        store.add("d2", "two", &meta).unwrap();
        assert_eq!(store.count().unwrap(), 2);

        store.delete("d1").unwrap();
        assert_eq!(store.count().unwrap(), 1);
    }

    #[test]
    fn test_count_where() {
        let (store, _tmp) = open_tmp_store();
        store
            .add("d1", "alpha", &sample_metadata("emotions", "joy", "a.txt"))
            .unwrap();
        store
            .add("d2", "beta", &sample_metadata("technical", "rust", "b.txt"))
            .unwrap();
        store
            .add("d3", "gamma", &sample_metadata("emotions", "fear", "c.txt"))
            .unwrap();

        assert_eq!(
            store
                .count_where(&WhereFilter::Wing("emotions".into()))
                .unwrap(),
            2
        );
        assert_eq!(
            store
                .count_where(&WhereFilter::Wing("technical".into()))
                .unwrap(),
            1
        );
        assert_eq!(
            store
                .count_where(&WhereFilter::Room("rust".into()))
                .unwrap(),
            1
        );
    }

    #[test]
    fn test_get_all_no_filter() {
        let (store, _tmp) = open_tmp_store();
        store
            .add("d1", "alpha", &sample_metadata("emotions", "joy", "a.txt"))
            .unwrap();
        store
            .add("d2", "beta", &sample_metadata("technical", "rust", "b.txt"))
            .unwrap();

        let all = store.get(None, None).unwrap();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn test_get_with_wing_filter() {
        let (store, _tmp) = open_tmp_store();
        store
            .add("d1", "alpha", &sample_metadata("emotions", "joy", "a.txt"))
            .unwrap();
        store
            .add("d2", "beta", &sample_metadata("technical", "rust", "b.txt"))
            .unwrap();
        store
            .add("d3", "gamma", &sample_metadata("emotions", "fear", "c.txt"))
            .unwrap();

        let filtered = store
            .get(Some(&WhereFilter::Wing("emotions".into())), None)
            .unwrap();
        assert_eq!(filtered.len(), 2);
    }

    #[test]
    fn test_get_with_limit() {
        let (store, _tmp) = open_tmp_store();
        for i in 0..10 {
            store
                .add(
                    &format!("d{}", i),
                    &format!("content {}", i),
                    &sample_metadata("tech", "r", "f.txt"),
                )
                .unwrap();
        }
        let limited = store.get(None, Some(3)).unwrap();
        assert_eq!(limited.len(), 3);
    }

    #[test]
    fn test_get_with_wing_and_room() {
        let (store, _tmp) = open_tmp_store();
        store
            .add("d1", "alpha", &sample_metadata("emotions", "joy", "a.txt"))
            .unwrap();
        store
            .add("d2", "beta", &sample_metadata("emotions", "fear", "b.txt"))
            .unwrap();
        store
            .add("d3", "gamma", &sample_metadata("technical", "joy", "c.txt"))
            .unwrap();

        let filtered = store
            .get(
                Some(&WhereFilter::WingAndRoom("emotions".into(), "joy".into())),
                None,
            )
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "d1");
    }

    #[test]
    fn test_fts_query() {
        let (store, _tmp) = open_tmp_store();
        store
            .add(
                "d1",
                "rust programming language is fast",
                &sample_metadata("technical", "rust", "a.txt"),
            )
            .unwrap();
        store
            .add(
                "d2",
                "python is easy to learn",
                &sample_metadata("technical", "python", "b.txt"),
            )
            .unwrap();
        store
            .add(
                "d3",
                "memory management in rust",
                &sample_metadata("technical", "rust", "c.txt"),
            )
            .unwrap();

        let results = store.query("rust", 10, None).unwrap();
        assert!(results.len() >= 2);
        assert!(results.iter().all(|r| r.content.contains("rust")));
    }

    #[test]
    fn test_fts_query_with_filter() {
        let (store, _tmp) = open_tmp_store();
        store
            .add(
                "d1",
                "the dog ran fast",
                &sample_metadata("emotions", "joy", "a.txt"),
            )
            .unwrap();
        store
            .add(
                "d2",
                "the fast car was red",
                &sample_metadata("technical", "cars", "b.txt"),
            )
            .unwrap();

        let results = store
            .query("fast", 10, Some(&WhereFilter::Wing("emotions".into())))
            .unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].id, "d1");
    }

    #[test]
    fn test_metadata_serialization_roundtrip() {
        let mut extra = HashMap::new();
        extra.insert("custom_tag".into(), "value1".into());
        let meta = DrawerMetadata {
            wing: "emotions".into(),
            room: "joy".into(),
            hall: Some("hall_a".into()),
            chunk_index: 3,
            source_file: "test.md".into(),
            date: Some("2026-01-15".into()),
            importance: Some(0.9),
            emotional_weight: Some(0.7),
            added_by: Some("user".into()),
            filed_at: Some("2026-01-15T10:00:00Z".into()),
            extra,
        };

        let json = serde_json::to_string(&meta).unwrap();
        let restored: DrawerMetadata = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.wing, "emotions");
        assert_eq!(restored.hall.as_deref(), Some("hall_a"));
        assert_eq!(restored.chunk_index, 3);
        assert_eq!(restored.extra.get("custom_tag").unwrap(), "value1");
    }

    #[test]
    fn test_source_file_filter() {
        let (store, _tmp) = open_tmp_store();
        store
            .add("d1", "alpha", &sample_metadata("tech", "r", "file_a.md"))
            .unwrap();
        store
            .add("d2", "beta", &sample_metadata("tech", "r", "file_b.md"))
            .unwrap();

        let filtered = store
            .get(Some(&WhereFilter::SourceFile("file_a.md".into())), None)
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "d1");
    }

    #[test]
    fn test_custom_filter() {
        let (store, _tmp) = open_tmp_store();
        let mut meta = sample_metadata("tech", "r", "f.txt");
        meta.added_by = Some("bot".into());
        store.add("d1", "alpha", &meta).unwrap();

        let mut meta2 = sample_metadata("tech", "r", "f.txt");
        meta2.added_by = Some("human".into());
        store.add("d2", "beta", &meta2).unwrap();

        let filtered = store
            .get(
                Some(&WhereFilter::Custom("added_by".into(), "bot".into())),
                None,
            )
            .unwrap();
        assert_eq!(filtered.len(), 1);
        assert_eq!(filtered[0].id, "d1");
    }
}

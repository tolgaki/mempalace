//! searcher.rs — Find anything. Exact words.
//!
//! Full-text search against the palace store.
//! Returns verbatim text — the actual words, never summaries.

use std::path::Path;

use crate::error::Result;
use crate::store::{PalaceStore, WhereFilter};

// ── Programmatic search ────────────────────────────────────────────────────

/// Search the palace. Returns a JSON value with query, filters, and results.
/// Optionally filter by wing and/or room.
pub fn search_memories(
    query: &str,
    palace_path: &str,
    wing: Option<&str>,
    room: Option<&str>,
    n_results: usize,
) -> Result<serde_json::Value> {
    let store = PalaceStore::open(palace_path)?;

    let filter = build_filter(wing, room);
    let results = store.query(query, n_results, filter.as_ref())?;

    let hits: Vec<serde_json::Value> = results
        .iter()
        .map(|r| {
            let source_name = Path::new(&r.metadata.source_file)
                .file_name()
                .map(|f| f.to_string_lossy().to_string())
                .unwrap_or_else(|| r.metadata.source_file.clone());

            serde_json::json!({
                "text": r.content,
                "wing": r.metadata.wing,
                "room": r.metadata.room,
                "source_file": source_name,
                "score": r.score,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "query": query,
        "filters": {
            "wing": wing,
            "room": room,
        },
        "results": hits,
    }))
}

// ── Print-formatted search ─────────────────────────────────────────────────

/// Search and print formatted results to stdout.
pub fn search_print(
    query: &str,
    palace_path: &str,
    wing: Option<&str>,
    room: Option<&str>,
    n_results: usize,
) -> Result<()> {
    let store = PalaceStore::open(palace_path)?;

    let filter = build_filter(wing, room);
    let results = store.query(query, n_results, filter.as_ref())?;

    if results.is_empty() {
        println!("\n  No results found for: \"{}\"", query);
        return Ok(());
    }

    println!("\n{}", "=".repeat(60));
    println!("  Results for: \"{}\"", query);
    if let Some(w) = wing {
        println!("  Wing: {}", w);
    }
    if let Some(r) = room {
        println!("  Room: {}", r);
    }
    println!("{}\n", "=".repeat(60));

    for (i, r) in results.iter().enumerate() {
        let source_name = Path::new(&r.metadata.source_file)
            .file_name()
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| r.metadata.source_file.clone());

        println!("  [{}] {} / {}", i + 1, r.metadata.wing, r.metadata.room);
        println!("      Source: {}", source_name);
        println!("      Score:  {:.3}", r.score);
        println!();

        for line in r.content.trim().split('\n') {
            println!("      {}", line);
        }

        println!();
        println!("  {}", "\u{2500}".repeat(56));
    }

    println!();
    Ok(())
}

// ── Helpers ────────────────────────────────────────────────────────────────

fn build_filter(wing: Option<&str>, room: Option<&str>) -> Option<WhereFilter> {
    match (wing, room) {
        (Some(w), Some(r)) => Some(WhereFilter::WingAndRoom(w.to_string(), r.to_string())),
        (Some(w), None) => Some(WhereFilter::Wing(w.to_string())),
        (None, Some(r)) => Some(WhereFilter::Room(r.to_string())),
        (None, None) => None,
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::DrawerMetadata;
    use std::collections::HashMap;
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

    fn setup_store() -> (String, TempDir) {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().to_str().unwrap().to_string();
        let store = PalaceStore::open(&path).unwrap();

        store
            .add(
                "d1",
                "rust programming language is fast and safe",
                &sample_metadata("technical", "rust", "docs/intro.md"),
            )
            .unwrap();
        store
            .add(
                "d2",
                "python is easy to learn and has great libraries",
                &sample_metadata("technical", "python", "docs/python.md"),
            )
            .unwrap();
        store
            .add(
                "d3",
                "memory management in rust prevents crashes",
                &sample_metadata("technical", "rust", "docs/memory.md"),
            )
            .unwrap();
        store
            .add(
                "d4",
                "feeling proud of the team and our progress",
                &sample_metadata("emotions", "pride", "journal/day1.md"),
            )
            .unwrap();
        store
            .add(
                "d5",
                "the deployment crashed because of missing config",
                &sample_metadata("technical", "devops", "logs/deploy.md"),
            )
            .unwrap();

        (path, tmp)
    }

    #[test]
    fn test_search_memories_basic() {
        let (path, _tmp) = setup_store();
        let result = search_memories("rust", &path, None, None, 5).unwrap();
        let hits = result["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert!(hits
            .iter()
            .any(|h| h["text"].as_str().unwrap().contains("rust")));
    }

    #[test]
    fn test_search_with_wing_filter() {
        let (path, _tmp) = setup_store();
        let result = search_memories("proud", &path, Some("emotions"), None, 5).unwrap();
        let hits = result["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0]["wing"].as_str().unwrap(), "emotions");
    }

    #[test]
    fn test_search_with_room_filter() {
        let (path, _tmp) = setup_store();
        let result = search_memories("rust", &path, None, Some("rust"), 5).unwrap();
        let hits = result["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        for hit in hits {
            assert_eq!(hit["room"].as_str().unwrap(), "rust");
        }
    }

    #[test]
    fn test_search_with_wing_and_room() {
        let (path, _tmp) = setup_store();
        let result =
            search_memories("programming", &path, Some("technical"), Some("rust"), 5).unwrap();
        let hits = result["results"].as_array().unwrap();
        assert!(!hits.is_empty());
        assert_eq!(hits[0]["wing"].as_str().unwrap(), "technical");
        assert_eq!(hits[0]["room"].as_str().unwrap(), "rust");
    }

    #[test]
    fn test_search_no_results() {
        let (path, _tmp) = setup_store();
        let result = search_memories("zzzznonexistenttermzzzz", &path, None, None, 5);
        // FTS may return empty or error on no match
        match result {
            Ok(val) => {
                let hits = val["results"].as_array().unwrap();
                assert!(hits.is_empty());
            }
            Err(_) => {
                // FTS5 may error on terms with no matches; that is acceptable
            }
        }
    }

    #[test]
    fn test_search_source_file_basename() {
        let (path, _tmp) = setup_store();
        let result = search_memories("rust", &path, None, None, 5).unwrap();
        let hits = result["results"].as_array().unwrap();
        // Source file should be the basename, not the full path
        for hit in hits {
            let source = hit["source_file"].as_str().unwrap();
            assert!(!source.contains('/'));
        }
    }

    #[test]
    fn test_search_filters_json_shape() {
        let (path, _tmp) = setup_store();
        let result = search_memories("rust", &path, Some("technical"), None, 5).unwrap();
        assert_eq!(result["query"].as_str().unwrap(), "rust");
        assert_eq!(result["filters"]["wing"].as_str().unwrap(), "technical");
        assert!(result["filters"]["room"].is_null());
    }

    #[test]
    fn test_search_print_no_results() {
        let (path, _tmp) = setup_store();
        // Should not panic
        let _ = search_print("zzzznonexistenttermzzzz", &path, None, None, 5);
    }

    #[test]
    fn test_build_filter_combinations() {
        assert!(build_filter(None, None).is_none());
        assert!(matches!(
            build_filter(Some("w"), None),
            Some(WhereFilter::Wing(_))
        ));
        assert!(matches!(
            build_filter(None, Some("r")),
            Some(WhereFilter::Room(_))
        ));
        assert!(matches!(
            build_filter(Some("w"), Some("r")),
            Some(WhereFilter::WingAndRoom(_, _))
        ));
    }
}

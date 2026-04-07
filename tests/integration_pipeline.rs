//! Integration test: Mine → Search → Retrieve pipeline
//!
//! Verifies the end-to-end flow of mining files into the palace,
//! then searching and retrieving them.

use std::collections::HashMap;
use tempfile::TempDir;

use mempalace::store::{DrawerMetadata, PalaceStore, WhereFilter};

fn make_project_dir(tmp: &TempDir) -> std::path::PathBuf {
    let project = tmp.path().join("my_project");
    std::fs::create_dir_all(project.join("src")).unwrap();
    std::fs::create_dir_all(project.join("docs")).unwrap();

    std::fs::write(
        project.join("src").join("main.rs"),
        "fn main() {\n    println!(\"Hello, world!\");\n}\n",
    )
    .unwrap();

    std::fs::write(
        project.join("docs").join("readme.md"),
        "# My Project\n\nThis is a Rust project for building memory systems.\n\
         It uses SQLite for storage and supports full-text search.\n",
    )
    .unwrap();

    std::fs::write(
        project.join("src").join("lib.rs"),
        "/// Core library for the project.\n\
         pub fn add(a: i32, b: i32) -> i32 { a + b }\n\
         pub fn multiply(a: i32, b: i32) -> i32 { a * b }\n",
    )
    .unwrap();

    // Create a mempalace.yaml config
    std::fs::write(
        project.join("mempalace.yaml"),
        "wing: my_project\nrooms:\n  - name: src\n    description: Source code\n  - name: documentation\n    description: Docs\n",
    )
    .unwrap();

    project
}

#[test]
fn test_mine_and_search_pipeline() {
    let tmp = TempDir::new().unwrap();
    let project = make_project_dir(&tmp);
    let palace_path = tmp.path().join("palace");

    // Mine the project directory
    mempalace::miner::mine(
        project.to_str().unwrap(),
        palace_path.to_str().unwrap(),
        None,   // auto-detect wing
        "test", // agent
        0,      // no limit
        false,  // not dry run
    )
    .unwrap();

    // Verify palace was created
    let store = PalaceStore::open(palace_path.to_str().unwrap()).unwrap();
    let count = store.count().unwrap();
    assert!(count > 0, "Expected drawers after mining, got {}", count);

    // Search for something we know is in the files
    let results = store.query("SQLite storage", 5, None).unwrap();
    assert!(
        !results.is_empty(),
        "Expected search results for 'SQLite storage'"
    );

    // Search with wing filter
    let filter = WhereFilter::Wing("my_project".to_string());
    let results_wing = store.query("project", 5, Some(&filter)).unwrap();
    assert!(
        !results_wing.is_empty(),
        "Expected results with wing filter"
    );
}

#[test]
fn test_mine_dry_run_creates_nothing() {
    let tmp = TempDir::new().unwrap();
    let project = make_project_dir(&tmp);
    let palace_path = tmp.path().join("palace_dry");

    mempalace::miner::mine(
        project.to_str().unwrap(),
        palace_path.to_str().unwrap(),
        None,
        "test",
        0,
        true, // dry run
    )
    .unwrap();

    // Palace should not exist or be empty
    if palace_path.exists() {
        let store = PalaceStore::open(palace_path.to_str().unwrap()).unwrap();
        assert_eq!(store.count().unwrap(), 0, "Dry run should not add drawers");
    }
}

#[test]
fn test_store_add_get_delete_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let palace_path = tmp.path().join("palace.sqlite3");
    let store = PalaceStore::open(palace_path.to_str().unwrap()).unwrap();

    let meta = DrawerMetadata {
        wing: "test_wing".into(),
        room: "test_room".into(),
        hall: Some("test_hall".into()),
        chunk_index: 0,
        source_file: "test.txt".into(),
        date: Some("2025-06-01".into()),
        importance: Some(0.8),
        emotional_weight: Some(0.5),
        added_by: Some("integration_test".into()),
        filed_at: None,
        extra: HashMap::new(),
    };

    // Add
    store
        .add(
            "int_test_1",
            "This is integration test content about Rust programming",
            &meta,
        )
        .unwrap();
    assert_eq!(store.count().unwrap(), 1);

    // Get by ID
    let drawer = store.get_by_id("int_test_1").unwrap();
    assert!(drawer.is_some());
    let d = drawer.unwrap();
    assert_eq!(d.metadata.wing, "test_wing");
    assert_eq!(d.metadata.room, "test_room");
    assert!(d.content.contains("Rust programming"));

    // FTS query
    let results = store.query("Rust programming", 5, None).unwrap();
    assert!(!results.is_empty());

    // Delete
    store.delete("int_test_1").unwrap();
    assert_eq!(store.count().unwrap(), 0);
    assert!(store.get_by_id("int_test_1").unwrap().is_none());
}

#[test]
fn test_knowledge_graph_roundtrip() {
    let tmp = TempDir::new().unwrap();
    let kg_path = tmp.path().join("kg.sqlite3");
    let kg = mempalace::KnowledgeGraph::new(Some(kg_path.to_str().unwrap())).unwrap();

    // Add entities
    kg.add_entity("Alice", "person", None).unwrap();
    kg.add_entity("Rust", "project", None).unwrap();

    // Add triples
    kg.add_triple(
        "Alice",
        "works_on",
        "Rust",
        Some("2025-01-01"),
        None,
        Some(0.9),
        None,
        None,
    )
    .unwrap();

    kg.add_triple(
        "Alice",
        "likes",
        "Rust",
        Some("2025-01-01"),
        None,
        Some(0.8),
        None,
        None,
    )
    .unwrap();

    // Query
    let facts = kg.query_entity("Alice", None, "outgoing").unwrap();
    assert_eq!(facts.len(), 2);

    // Timeline
    let timeline = kg.timeline(Some("Alice")).unwrap();
    assert_eq!(timeline.len(), 2);

    // Invalidate
    kg.invalidate("Alice", "likes", "Rust", Some("2025-06-01"))
        .unwrap();
    let current = kg
        .query_entity("Alice", Some("2025-07-01"), "outgoing")
        .unwrap();
    assert_eq!(current.len(), 1); // only works_on remains current

    // Stats
    let stats = kg.stats().unwrap();
    assert_eq!(stats["entities"], 2);
    assert_eq!(stats["triples"], 2);
    assert_eq!(stats["current_facts"], 1);
}

#[test]
fn test_memory_stack_end_to_end() {
    let tmp = TempDir::new().unwrap();
    let palace_path = tmp.path().join("palace.sqlite3");
    let store = PalaceStore::open(palace_path.to_str().unwrap()).unwrap();

    // Add some drawers
    let meta = DrawerMetadata {
        wing: "technical".into(),
        room: "rust".into(),
        hall: None,
        chunk_index: 0,
        source_file: "notes.md".into(),
        date: None,
        importance: Some(0.9),
        emotional_weight: None,
        added_by: Some("test".into()),
        filed_at: None,
        extra: HashMap::new(),
    };
    store
        .add("ms1", "Rust is a fast, safe systems language", &meta)
        .unwrap();

    // Write identity file
    let id_path = tmp.path().join("identity.txt");
    std::fs::write(&id_path, "I am MemBot, a helpful AI assistant.").unwrap();

    // Test wake_up
    let mut stack = mempalace::layers::MemoryStack::new(
        Some(palace_path.to_str().unwrap()),
        Some(id_path.to_str().unwrap()),
    );
    let wakeup = stack.wake_up(None);
    assert!(wakeup.contains("MemBot"));
    assert!(wakeup.contains("ESSENTIAL STORY"));

    // Test status
    let status = stack.status();
    assert!(status["total_drawers"].as_u64().unwrap() >= 1);
}

#[test]
fn test_room_detection_and_mining() {
    let tmp = TempDir::new().unwrap();
    let project = tmp.path().join("full_project");
    std::fs::create_dir_all(project.join("frontend")).unwrap();
    std::fs::create_dir_all(project.join("backend")).unwrap();
    std::fs::create_dir_all(project.join("docs")).unwrap();

    std::fs::write(
        project.join("frontend").join("app.js"),
        "import React from 'react';\nfunction App() { return <div>Hello</div>; }\n",
    )
    .unwrap();
    std::fs::write(
        project.join("backend").join("server.py"),
        "from flask import Flask\napp = Flask(__name__)\n",
    )
    .unwrap();
    std::fs::write(
        project.join("docs").join("api.md"),
        "# API Documentation\n\nEndpoints for the REST API.\n",
    )
    .unwrap();

    // Detect rooms
    let rooms = mempalace::room_detector::detect_rooms_from_folders(project.to_str().unwrap());
    let names: Vec<&str> = rooms.iter().map(|r| r.name.as_str()).collect();
    assert!(names.contains(&"frontend"));
    assert!(names.contains(&"backend"));
    assert!(names.contains(&"documentation"));
}

#[test]
fn test_entity_detection_pipeline() {
    let tmp = TempDir::new().unwrap();

    // Create files with entity mentions
    std::fs::write(
        tmp.path().join("notes.txt"),
        "Alice said she would handle the deployment.\n\
         Bob asked about the timeline.\n\
         Alice mentioned that MemPalace v2.0 is ready.\n\
         Bob told the team about the new release.\n\
         Alice said she loves the architecture.\n",
    )
    .unwrap();

    let files = mempalace::entity_detector::scan_for_detection(tmp.path().to_str().unwrap(), 10);
    assert!(!files.is_empty());

    let detected = mempalace::entity_detector::detect_entities(&files, 10);
    let all_names: Vec<&str> = detected
        .people
        .iter()
        .chain(detected.projects.iter())
        .chain(detected.uncertain.iter())
        .map(|e| e.name.as_str())
        .collect();

    // Alice and Bob should be detected as entities
    assert!(
        all_names.iter().any(|n| n.contains("Alice")),
        "Alice should be detected, got: {:?}",
        all_names
    );
}

#[test]
fn test_dialect_compression_roundtrip() {
    use mempalace::dialect::Dialect;

    let dialect = Dialect::new(None, None);
    let text = "I decided to switch from Python to Rust for better performance. \
                This was a breakthrough moment! I felt really excited about it. \
                The migration took three weeks but was totally worth it.";

    let mut meta = HashMap::new();
    meta.insert("wing".to_string(), "technical".to_string());
    meta.insert("room".to_string(), "migration".to_string());

    let compressed = dialect.compress(text, Some(&meta));
    let stats = Dialect::compression_stats(text, &compressed);

    assert!(stats.ratio > 1.0, "Compression ratio should be > 1.0");
    assert!(
        !compressed.is_empty(),
        "Compressed output should not be empty"
    );
    // AAAK format markers
    assert!(
        compressed.contains('[') || compressed.contains("⚡") || compressed.contains("🔒"),
        "Compressed output should contain AAAK markers"
    );
}

#[test]
fn test_normalize_file() {
    let tmp = TempDir::new().unwrap();

    // Plain text file
    let plain_path = tmp.path().join("plain.txt");
    std::fs::write(&plain_path, "User says hello\nAssistant says hi").unwrap();
    let result = mempalace::normalize::normalize(plain_path.to_str().unwrap()).unwrap();
    assert!(result.contains("hello"));

    // Claude Code JSONL file
    let jsonl_path = tmp.path().join("convo.jsonl");
    std::fs::write(
        &jsonl_path,
        "{\"type\":\"human\",\"message\":\"What is Rust?\"}\n\
         {\"type\":\"assistant\",\"message\":\"Rust is a systems programming language.\"}",
    )
    .unwrap();
    let result = mempalace::normalize::normalize(jsonl_path.to_str().unwrap()).unwrap();
    assert!(result.contains("Rust"));
}

#[test]
fn test_general_extractor() {
    let memories = mempalace::general_extractor::extract_memories(
        "I decided to use Rust for the project. This was a breakthrough achievement! \
         I prefer strongly-typed languages. I struggled with the borrow checker at first. \
         We chose to migrate the entire codebase. I really like the type system. \
         The team concluded that performance improved significantly.",
        0.1, // lower threshold
    );

    // Should detect at least one of the memory types
    assert!(
        !memories.is_empty(),
        "Should extract at least one memory from text with clear markers, got empty"
    );
}

#[test]
fn test_spellcheck_edit_distance() {
    assert_eq!(mempalace::spellcheck::edit_distance("kitten", "sitting"), 3);
    assert_eq!(mempalace::spellcheck::edit_distance("", "abc"), 3);
    assert_eq!(mempalace::spellcheck::edit_distance("same", "same"), 0);
}

#[test]
fn test_split_mega_files() {
    let tmp = TempDir::new().unwrap();

    // Create a non-mega file (should return empty)
    let single = tmp.path().join("single.txt");
    std::fs::write(&single, "Just a single session.\nNo boundaries.\n").unwrap();
    let result = mempalace::split_mega_files::split_file(&single, None, true).unwrap();
    assert!(result.is_empty());
}

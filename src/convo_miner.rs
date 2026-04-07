//! convo_miner.rs -- Mine conversations into the palace.
//!
//! Ingests chat exports (Claude Code, ChatGPT, Slack, plain text transcripts).
//! Normalizes format, chunks by exchange pair (Q+A = one unit), files to palace.
//!
//! Same palace as project mining. Different ingest strategy.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Local;
use md5::{Digest, Md5};
use walkdir::WalkDir;

use crate::error::Result;
use crate::normalize;
use crate::store::{DrawerMetadata, PalaceStore, WhereFilter};

// ── Constants ──────────────────────────────────────────────────────────────

pub const CONVO_EXTENSIONS: &[&str] = &["txt", "md", "json", "jsonl"];

pub const SKIP_DIRS: &[&str] = &[
    ".git",
    "node_modules",
    "__pycache__",
    ".venv",
    "venv",
    "env",
    "dist",
    "build",
    ".next",
    ".mempalace",
];

pub const MIN_CHUNK_SIZE: usize = 30;

// ── Chunk type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub chunk_index: usize,
    /// Optional memory type for general extraction mode.
    pub memory_type: Option<String>,
}

// ── Topic keywords for room detection ──────────────────────────────────────

/// Returns the keyword map used for conversation room detection.
pub fn topic_keywords() -> HashMap<&'static str, Vec<&'static str>> {
    let mut m = HashMap::new();
    m.insert(
        "technical",
        vec![
            "code", "python", "function", "bug", "error", "api", "database", "server", "deploy",
            "git", "test", "debug", "refactor",
        ],
    );
    m.insert(
        "architecture",
        vec![
            "architecture",
            "design",
            "pattern",
            "structure",
            "schema",
            "interface",
            "module",
            "component",
            "service",
            "layer",
        ],
    );
    m.insert(
        "planning",
        vec![
            "plan",
            "roadmap",
            "milestone",
            "deadline",
            "priority",
            "sprint",
            "backlog",
            "scope",
            "requirement",
            "spec",
        ],
    );
    m.insert(
        "decisions",
        vec![
            "decided",
            "chose",
            "picked",
            "switched",
            "migrated",
            "replaced",
            "trade-off",
            "alternative",
            "option",
            "approach",
        ],
    );
    m.insert(
        "problems",
        vec![
            "problem",
            "issue",
            "broken",
            "failed",
            "crash",
            "stuck",
            "workaround",
            "fix",
            "solved",
            "resolved",
        ],
    );
    m
}

// ── Chunking ───────────────────────────────────────────────────────────────

/// Chunk by exchange pair: one > turn + AI response = one unit.
/// Falls back to paragraph chunking if no > markers.
pub fn chunk_exchanges(content: &str) -> Vec<Chunk> {
    let lines: Vec<&str> = content.split('\n').collect();
    let quote_lines = lines
        .iter()
        .filter(|l| l.trim_start().starts_with('>'))
        .count();

    if quote_lines >= 3 {
        chunk_by_exchange(&lines)
    } else {
        chunk_by_paragraph(content)
    }
}

fn chunk_by_exchange(lines: &[&str]) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i].trim();
        if line.starts_with('>') {
            let user_turn = line.to_string();
            i += 1;

            let mut ai_lines = Vec::new();
            while i < lines.len() {
                let next_line = lines[i].trim();
                if next_line.starts_with('>') || next_line.starts_with("---") {
                    break;
                }
                if !next_line.is_empty() {
                    ai_lines.push(next_line.to_string());
                }
                i += 1;
            }

            let ai_response = ai_lines
                .iter()
                .take(8)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ");

            let content = if !ai_response.is_empty() {
                format!("{}\n{}", user_turn, ai_response)
            } else {
                user_turn
            };

            if content.trim().len() > MIN_CHUNK_SIZE {
                chunks.push(Chunk {
                    content,
                    chunk_index: chunks.len(),
                    memory_type: None,
                });
            }
        } else {
            i += 1;
        }
    }

    chunks
}

fn chunk_by_paragraph(content: &str) -> Vec<Chunk> {
    let mut chunks = Vec::new();
    let paragraphs: Vec<&str> = content
        .split("\n\n")
        .map(|p| p.trim())
        .filter(|p| !p.is_empty())
        .collect();

    // If no paragraph breaks and long content, chunk by line groups
    if paragraphs.len() <= 1 && content.matches('\n').count() > 20 {
        let lines: Vec<&str> = content.split('\n').collect();
        for group_start in (0..lines.len()).step_by(25) {
            let group_end = (group_start + 25).min(lines.len());
            let group = lines[group_start..group_end].join("\n");
            let group = group.trim().to_string();
            if group.len() > MIN_CHUNK_SIZE {
                chunks.push(Chunk {
                    content: group,
                    chunk_index: chunks.len(),
                    memory_type: None,
                });
            }
        }
        return chunks;
    }

    for para in paragraphs {
        if para.len() > MIN_CHUNK_SIZE {
            chunks.push(Chunk {
                content: para.to_string(),
                chunk_index: chunks.len(),
                memory_type: None,
            });
        }
    }

    chunks
}

// ── Room detection ─────────────────────────────────────────────────────────

/// Score conversation content against topic keywords.
pub fn detect_convo_room(content: &str) -> String {
    let content_lower: String = content
        .chars()
        .take(3000)
        .collect::<String>()
        .to_lowercase();
    let kw_map = topic_keywords();

    let mut scores: HashMap<&str, usize> = HashMap::new();
    for (room, keywords) in &kw_map {
        let score: usize = keywords
            .iter()
            .filter(|kw| content_lower.contains(*kw))
            .count();
        if score > 0 {
            scores.insert(room, score);
        }
    }

    if let Some((best, _)) = scores.iter().max_by_key(|(_, &v)| v) {
        return best.to_string();
    }

    "general".to_string()
}

// ── Scanning ───────────────────────────────────────────────────────────────

/// Find all potential conversation files.
pub fn scan_convos(convo_dir: &str) -> Vec<PathBuf> {
    let convo_path = Path::new(convo_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(convo_dir));

    let skip_set: std::collections::HashSet<&str> = SKIP_DIRS.iter().copied().collect();
    let ext_set: std::collections::HashSet<&str> = CONVO_EXTENSIONS.iter().copied().collect();

    let mut files = Vec::new();

    for entry in WalkDir::new(&convo_path).into_iter().filter_entry(|e| {
        if e.file_type().is_dir() {
            let name = e.file_name().to_string_lossy();
            !skip_set.contains(name.as_ref())
        } else {
            true
        }
    }) {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => continue,
        };

        if !entry.file_type().is_file() {
            continue;
        }

        let ext = entry
            .path()
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if ext_set.contains(ext.as_str()) {
            files.push(entry.path().to_path_buf());
        }
    }

    files
}

// ── Helper ─────────────────────────────────────────────────────────────────

fn make_drawer_id(wing: &str, room: &str, source_file: &str, chunk_index: usize) -> String {
    let mut hasher = Md5::new();
    hasher.update(source_file.as_bytes());
    hasher.update(chunk_index.to_string().as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    format!("drawer_{}_{}_{}", wing, room, &hash[..16])
}

// ── Mine conversations ─────────────────────────────────────────────────────

/// Mine a directory of conversation files into the palace.
///
/// `extract_mode`:
///   - `"exchange"` -- default exchange-pair chunking (Q+A = one unit)
///   - `"general"`  -- general extractor: decisions, preferences, milestones, problems, emotions
pub fn mine_convos(
    convo_dir: &str,
    palace_path: &str,
    wing: Option<&str>,
    agent: &str,
    limit: usize,
    dry_run: bool,
    extract_mode: &str,
) -> Result<()> {
    let convo_path = Path::new(convo_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(convo_dir));

    let wing = wing.map(String::from).unwrap_or_else(|| {
        convo_path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| "convos".to_string())
            .to_lowercase()
            .replace([' ', '-'], "_")
    });

    let mut files = scan_convos(convo_dir);
    if limit > 0 && files.len() > limit {
        files.truncate(limit);
    }

    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Mine -- Conversations");
    println!("{}", "=".repeat(55));
    println!("  Wing:    {}", wing);
    println!("  Source:  {}", convo_path.display());
    println!("  Files:   {}", files.len());
    println!("  Palace:  {}", palace_path);
    if dry_run {
        println!("  DRY RUN -- nothing will be filed");
    }
    println!("{}\n", "-".repeat(55));

    let store = if !dry_run {
        Some(PalaceStore::open(palace_path)?)
    } else {
        None
    };

    let mut total_drawers = 0usize;
    let mut files_skipped = 0usize;
    let mut room_counts: HashMap<String, usize> = HashMap::new();

    for (i, filepath) in files.iter().enumerate() {
        let source_file = filepath.to_string_lossy().to_string();

        // Skip if already filed
        if let Some(ref s) = store {
            let existing = s.get(Some(&WhereFilter::SourceFile(source_file.clone())), Some(1))?;
            if !existing.is_empty() {
                files_skipped += 1;
                continue;
            }
        }

        // Normalize format
        let content = match normalize::normalize(&source_file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        if content.trim().len() < MIN_CHUNK_SIZE {
            continue;
        }

        // Chunk
        let chunks = if extract_mode == "general" {
            // In the Rust port, general extraction falls back to exchange chunking
            // since the general_extractor module is not yet ported.
            // Tag chunks with detected room as memory_type.
            chunk_exchanges(&content)
        } else {
            chunk_exchanges(&content)
        };

        if chunks.is_empty() {
            continue;
        }

        // Detect room from content
        let room = detect_convo_room(&content);

        if dry_run {
            println!(
                "    [DRY RUN] {} -> room:{} ({} drawers)",
                filepath.file_name().unwrap_or_default().to_string_lossy(),
                room,
                chunks.len()
            );
            total_drawers += chunks.len();
            *room_counts.entry(room).or_insert(0) += 1;
            continue;
        }

        let s = store.as_ref().unwrap();
        *room_counts.entry(room.clone()).or_insert(0) += 1;

        let mut drawers_added = 0usize;
        for chunk in &chunks {
            let chunk_room = chunk.memory_type.as_deref().unwrap_or(&room);
            let drawer_id = make_drawer_id(&wing, chunk_room, &source_file, chunk.chunk_index);

            let mut extra = HashMap::new();
            extra.insert("ingest_mode".to_string(), "convos".to_string());
            extra.insert("extract_mode".to_string(), extract_mode.to_string());

            let meta = DrawerMetadata {
                wing: wing.clone(),
                room: chunk_room.to_string(),
                hall: None,
                chunk_index: chunk.chunk_index as u32,
                source_file: source_file.clone(),
                date: None,
                importance: None,
                emotional_weight: None,
                added_by: Some(agent.to_string()),
                filed_at: Some(Local::now().to_rfc3339()),
                extra,
            };

            match s.add(&drawer_id, &chunk.content, &meta) {
                Ok(true) => drawers_added += 1,
                Ok(false) => {} // duplicate
                Err(e) => eprintln!("  Warning: failed to add drawer: {}", e),
            }
        }

        total_drawers += drawers_added;
        println!(
            "  [{:4}/{}] {:50} +{}",
            i + 1,
            files.len(),
            filepath
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .chars()
                .take(50)
                .collect::<String>(),
            drawers_added
        );
    }

    println!("\n{}", "=".repeat(55));
    println!("  Done.");
    println!("  Files processed: {}", files.len() - files_skipped);
    println!("  Files skipped (already filed): {}", files_skipped);
    println!("  Drawers filed: {}", total_drawers);
    if !room_counts.is_empty() {
        println!("\n  By room:");
        let mut sorted: Vec<_> = room_counts.iter().collect();
        sorted.sort_by(|a, b| b.1.cmp(a.1));
        for (room, count) in sorted {
            println!("    {:20} {} files", room, count);
        }
    }
    println!("\n  Next: mempalace search \"what you're looking for\"");
    println!("{}\n", "=".repeat(55));

    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_chunk_exchanges_with_quotes() {
        let content = "> What is Rust?\nRust is a systems programming language.\n\n> Why use it?\nFor memory safety.\n";
        let chunks = chunk_exchanges(content);
        assert_eq!(chunks.len(), 2);
        assert!(chunks[0].content.contains("What is Rust?"));
        assert!(chunks[0].content.contains("Rust is a systems"));
        assert!(chunks[1].content.contains("Why use it?"));
    }

    #[test]
    fn test_chunk_exchanges_paragraph_fallback() {
        let content = "First paragraph about something interesting.\n\nSecond paragraph with more details about the topic.\n\nThird paragraph concluding the discussion.";
        let chunks = chunk_exchanges(content);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_chunk_exchanges_line_group_fallback() {
        // More than 20 lines with no paragraph breaks, single paragraph
        let mut lines = Vec::new();
        for i in 0..30 {
            lines.push(format!("Line {} of the conversation about something.", i));
        }
        let content = lines.join("\n");
        let chunks = chunk_exchanges(&content);
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_chunk_exchanges_skips_tiny() {
        let content = "> Hi\nHi\n";
        let chunks = chunk_exchanges(content);
        assert!(
            chunks.is_empty(),
            "Should skip chunks smaller than MIN_CHUNK_SIZE"
        );
    }

    #[test]
    fn test_chunk_exchanges_separator() {
        let content = "> First question about the project design\nFirst answer with details about the approach.\n---\n> Second question about implementation strategy\nSecond answer with more detail.\n> Third question about testing approach\nThird answer.\n";
        let chunks = chunk_exchanges(content);
        assert_eq!(chunks.len(), 3);
    }

    #[test]
    fn test_detect_convo_room_technical() {
        let content =
            "We need to fix the bug in the api server. The function is throwing an error.";
        assert_eq!(detect_convo_room(content), "technical");
    }

    #[test]
    fn test_detect_convo_room_planning() {
        let content = "Let's set the roadmap for the next sprint. The deadline is approaching and we need to prioritize.";
        assert_eq!(detect_convo_room(content), "planning");
    }

    #[test]
    fn test_detect_convo_room_decisions() {
        let content = "We decided to switch the approach. We chose the alternative option instead.";
        assert_eq!(detect_convo_room(content), "decisions");
    }

    #[test]
    fn test_detect_convo_room_problems() {
        let content = "The problem is that it keeps crashing. We need a workaround until we can fix the issue.";
        assert_eq!(detect_convo_room(content), "problems");
    }

    #[test]
    fn test_detect_convo_room_general_fallback() {
        let content = "The weather is nice today and the birds are singing.";
        assert_eq!(detect_convo_room(content), "general");
    }

    #[test]
    fn test_detect_convo_room_architecture() {
        let content = "The architecture uses a layered design pattern with separate modules and components for each service.";
        assert_eq!(detect_convo_room(content), "architecture");
    }

    #[test]
    fn test_scan_convos_empty() {
        let tmp = TempDir::new().unwrap();
        let files = scan_convos(tmp.path().to_str().unwrap());
        assert!(files.is_empty());
    }

    #[test]
    fn test_scan_convos_finds_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("chat.txt"), "conversation content").unwrap();
        std::fs::write(tmp.path().join("export.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("image.png"), "binary").unwrap(); // not a convo

        let files = scan_convos(tmp.path().to_str().unwrap());
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_scan_convos_skips_dirs() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join(".git")).unwrap();
        std::fs::write(tmp.path().join(".git").join("log.txt"), "git log").unwrap();
        std::fs::write(tmp.path().join("chat.txt"), "real content").unwrap();

        let files = scan_convos(tmp.path().to_str().unwrap());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_topic_keywords_has_all_rooms() {
        let kw = topic_keywords();
        assert!(kw.contains_key("technical"));
        assert!(kw.contains_key("architecture"));
        assert!(kw.contains_key("planning"));
        assert!(kw.contains_key("decisions"));
        assert!(kw.contains_key("problems"));
        assert_eq!(kw.len(), 5);
    }

    #[test]
    fn test_convo_extensions() {
        assert!(CONVO_EXTENSIONS.contains(&"txt"));
        assert!(CONVO_EXTENSIONS.contains(&"md"));
        assert!(CONVO_EXTENSIONS.contains(&"json"));
        assert!(CONVO_EXTENSIONS.contains(&"jsonl"));
        assert!(!CONVO_EXTENSIONS.contains(&"py"));
    }
}

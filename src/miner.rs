//! miner.rs -- Mine project files into the palace.
//!
//! Reads mempalace.yaml from the project directory to know the wing + rooms.
//! Routes each file to the right room based on content.
//! Stores verbatim chunks as drawers. No summaries. Ever.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use chrono::Local;
use md5::{Digest, Md5};
use walkdir::WalkDir;

use crate::error::{MempalaceError, Result};
use crate::room_detector::{self, RoomDef};
use crate::store::{DrawerMetadata, PalaceStore, WhereFilter};

// ── Constants ──────────────────────────────────────────────────────────────

pub const READABLE_EXTENSIONS: &[&str] = &[
    // Text / documentation
    "txt",
    "md",
    "rst",
    "adoc",
    "tex",
    "rtf",
    "org",
    "log",
    // Web
    "html",
    "css",
    "scss",
    "js",
    "jsx",
    "ts",
    "tsx",
    "vue",
    "svelte",
    // Data / config
    "json",
    "yaml",
    "yml",
    "toml",
    "xml",
    "csv",
    "ini",
    "cfg",
    "env",
    // Python
    "py",
    "pyi",
    // Rust
    "rs",
    // C / C++
    "c",
    "cpp",
    "h",
    "hpp",
    // C# / .NET
    "cs",
    // Java / JVM
    "java",
    "scala",
    "kt",
    "clj",
    // Go
    "go",
    // Ruby
    "rb",
    // Swift / Apple
    "swift",
    // PHP
    "php",
    // Shell / scripting
    "sh",
    "bash",
    "zsh",
    "bat",
    "ps1",
    // Functional languages
    "hs",
    "ml",
    "ex",
    "exs",
    "erl",
    "elm",
    // Other languages
    "r",
    "jl",
    "lua",
    "pl",
    "dart",
    // SQL / data
    "sql",
    // Infra / DevOps
    "tf",
    "proto",
    "graphql",
    "dockerfile",
    // Build
    "makefile",
];

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
    "coverage",
    ".mempalace",
];

pub const CHUNK_SIZE: usize = 800;
pub const CHUNK_OVERLAP: usize = 100;
pub const MIN_CHUNK_SIZE: usize = 50;

const CONFIG_FILES: &[&str] = &[
    "mempalace.yaml",
    "mempalace.yml",
    "mempal.yaml",
    "mempal.yml",
    ".gitignore",
    "package-lock.json",
];

// ── Chunk type ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub chunk_index: usize,
}

// ── Config loading ─────────────────────────────────────────────────────────

/// Load mempalace.yaml from project directory (falls back to mempal.yaml).
pub fn load_config(project_dir: &str) -> Result<serde_yaml::Value> {
    let project_path = Path::new(project_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_dir));
    let config_path = project_path.join("mempalace.yaml");

    if config_path.exists() {
        let content = std::fs::read_to_string(&config_path)?;
        let val: serde_yaml::Value = serde_yaml::from_str(&content)?;
        return Ok(val);
    }

    // Fallback to legacy name
    let legacy_path = project_path.join("mempal.yaml");
    if legacy_path.exists() {
        let content = std::fs::read_to_string(&legacy_path)?;
        let val: serde_yaml::Value = serde_yaml::from_str(&content)?;
        return Ok(val);
    }

    Err(MempalaceError::NotFound(format!(
        "No mempalace.yaml found in {}. Run: mempalace init {}",
        project_dir, project_dir
    )))
}

// ── Room detection (delegate to room_detector) ─────────────────────────────

/// Detect the room for a file based on its path and content.
pub fn detect_room(
    filepath: &Path,
    content: &str,
    rooms: &[RoomDef],
    project_path: &Path,
) -> String {
    room_detector::detect_room(filepath, content, rooms, project_path)
}

// ── Chunking ───────────────────────────────────────────────────────────────

/// Split content into drawer-sized chunks.
/// Tries to split on paragraph/line boundaries.
pub fn chunk_text(content: &str) -> Vec<Chunk> {
    let content = content.trim();
    if content.is_empty() {
        return Vec::new();
    }

    let mut chunks = Vec::new();
    let mut start = 0;
    while start < content.len() {
        let end = (start + CHUNK_SIZE).min(content.len());

        // Find a safe char boundary for the tentative end
        let end = find_char_boundary(content, end);

        let actual_end = if end < content.len() {
            // Try to break at paragraph boundary (\n\n)
            let search_region = &content[start..end];
            let half = CHUNK_SIZE / 2;
            let half_pos = find_char_boundary(content, start + half);

            if let Some(para_pos) = search_region.rfind("\n\n") {
                let absolute_pos = start + para_pos;
                if absolute_pos > half_pos {
                    absolute_pos
                } else if let Some(nl_pos) = search_region.rfind('\n') {
                    let absolute_nl = start + nl_pos;
                    if absolute_nl > half_pos {
                        absolute_nl
                    } else {
                        end
                    }
                } else {
                    end
                }
            } else if let Some(nl_pos) = search_region.rfind('\n') {
                let absolute_nl = start + nl_pos;
                if absolute_nl > half_pos {
                    absolute_nl
                } else {
                    end
                }
            } else {
                end
            }
        } else {
            end
        };

        let chunk_str = content[start..actual_end].trim();
        if chunk_str.len() >= MIN_CHUNK_SIZE {
            chunks.push(Chunk {
                content: chunk_str.to_string(),
                chunk_index: chunks.len(),
            });
        }

        if actual_end >= content.len() {
            break;
        }
        // Overlap
        let overlap_start = if actual_end > CHUNK_OVERLAP {
            actual_end - CHUNK_OVERLAP
        } else {
            actual_end
        };
        start = find_char_boundary(content, overlap_start);
    }

    chunks
}

/// Find the nearest char boundary at or before `pos`.
fn find_char_boundary(s: &str, pos: usize) -> usize {
    if pos >= s.len() {
        return s.len();
    }
    let mut p = pos;
    while p > 0 && !s.is_char_boundary(p) {
        p -= 1;
    }
    p
}

// ── File scanning ──────────────────────────────────────────────────────────

/// Return list of all readable file paths in the project, skipping config files.
pub fn scan_project(project_dir: &str) -> Vec<PathBuf> {
    let project_path = Path::new(project_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_dir));

    let skip_set: std::collections::HashSet<&str> = SKIP_DIRS.iter().copied().collect();
    let ext_set: std::collections::HashSet<&str> = READABLE_EXTENSIONS.iter().copied().collect();
    let config_set: std::collections::HashSet<&str> = CONFIG_FILES.iter().copied().collect();

    let mut files = Vec::new();

    for entry in WalkDir::new(&project_path).into_iter().filter_entry(|e| {
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

        let path = entry.path();
        let filename = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();

        // Skip config files
        if config_set.contains(filename.as_str()) {
            continue;
        }

        // Check extension
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.to_lowercase())
            .unwrap_or_default();

        if ext_set.contains(ext.as_str()) {
            files.push(path.to_path_buf());
        }
    }

    files
}

// ── Helper: drawer ID ──────────────────────────────────────────────────────

fn make_drawer_id(wing: &str, room: &str, source_file: &str, chunk_index: usize) -> String {
    let mut hasher = Md5::new();
    hasher.update(source_file.as_bytes());
    hasher.update(chunk_index.to_string().as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    format!("drawer_{}_{}_{}", wing, room, &hash[..16])
}

// ── Mine ───────────────────────────────────────────────────────────────────

/// Mine a project directory into the palace.
pub fn mine(
    project_dir: &str,
    palace_path: &str,
    wing_override: Option<&str>,
    agent: &str,
    limit: usize,
    dry_run: bool,
) -> Result<()> {
    let project_path = Path::new(project_dir)
        .canonicalize()
        .unwrap_or_else(|_| PathBuf::from(project_dir));
    let config = load_config(project_dir)?;

    let wing = wing_override
        .map(String::from)
        .or_else(|| {
            config
                .get("wing")
                .and_then(|v| v.as_str())
                .map(String::from)
        })
        .unwrap_or_else(|| "general".to_string());

    let rooms: Vec<RoomDef> = config
        .get("rooms")
        .and_then(|v| v.as_sequence())
        .map(|seq| {
            seq.iter()
                .filter_map(|item| {
                    let name = item.get("name")?.as_str()?.to_string();
                    let description = item
                        .get("description")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let keywords = item
                        .get("keywords")
                        .and_then(|v| v.as_sequence())
                        .map(|kws| {
                            kws.iter()
                                .filter_map(|k| k.as_str().map(String::from))
                                .collect()
                        })
                        .unwrap_or_default();
                    Some(RoomDef {
                        name,
                        description,
                        keywords,
                    })
                })
                .collect()
        })
        .unwrap_or_else(|| {
            vec![RoomDef {
                name: "general".into(),
                description: "All project files".into(),
                keywords: vec![],
            }]
        });

    let mut files = scan_project(project_dir);
    if limit > 0 && files.len() > limit {
        files.truncate(limit);
    }

    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Mine");
    println!("{}", "=".repeat(55));
    println!("  Wing:    {}", wing);
    println!(
        "  Rooms:   {}",
        rooms
            .iter()
            .map(|r| r.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    );
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

        // Skip files larger than 10MB to avoid unbounded memory allocation
        if let Ok(meta) = std::fs::metadata(filepath) {
            if meta.len() > 10 * 1024 * 1024 {
                continue;
            }
        }

        let content = match std::fs::read_to_string(filepath) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let content = content.trim().to_string();
        if content.len() < MIN_CHUNK_SIZE {
            continue;
        }

        let room = detect_room(filepath, &content, &rooms, &project_path);
        let chunks = chunk_text(&content);

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
        let mut drawers_added = 0usize;

        for chunk in &chunks {
            let drawer_id = make_drawer_id(&wing, &room, &source_file, chunk.chunk_index);
            let meta = DrawerMetadata {
                wing: wing.clone(),
                room: room.clone(),
                hall: None,
                chunk_index: chunk.chunk_index as u32,
                source_file: source_file.clone(),
                date: None,
                importance: None,
                emotional_weight: None,
                added_by: Some(agent.to_string()),
                filed_at: Some(Local::now().to_rfc3339()),
                extra: HashMap::new(),
            };

            match s.add(&drawer_id, &chunk.content, &meta) {
                Ok(true) => drawers_added += 1,
                Ok(false) => {} // duplicate
                Err(e) => eprintln!("  Warning: failed to add drawer: {}", e),
            }
        }

        total_drawers += drawers_added;
        *room_counts.entry(room).or_insert(0) += 1;
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
    println!("\n  By room:");
    let mut sorted_rooms: Vec<_> = room_counts.iter().collect();
    sorted_rooms.sort_by(|a, b| b.1.cmp(a.1));
    for (room, count) in sorted_rooms {
        println!("    {:20} {} files", room, count);
    }
    println!("\n  Next: mempalace search \"what you're looking for\"");
    println!("{}\n", "=".repeat(55));

    Ok(())
}

// ── Status ─────────────────────────────────────────────────────────────────

/// Show what has been filed in the palace.
pub fn status(palace_path: &str) -> Result<()> {
    let store = match PalaceStore::open(palace_path) {
        Ok(s) => s,
        Err(_) => {
            println!("\n  No palace found at {}", palace_path);
            println!("  Run: mempalace init <dir> then mempalace mine <dir>");
            return Ok(());
        }
    };

    let drawers = store.get(None, None)?;

    let mut wing_rooms: HashMap<String, HashMap<String, usize>> = HashMap::new();
    for d in &drawers {
        *wing_rooms
            .entry(d.metadata.wing.clone())
            .or_default()
            .entry(d.metadata.room.clone())
            .or_insert(0) += 1;
    }

    println!("\n{}", "=".repeat(55));
    println!("  MemPalace Status -- {} drawers", drawers.len());
    println!("{}\n", "=".repeat(55));

    let mut sorted_wings: Vec<_> = wing_rooms.iter().collect();
    sorted_wings.sort_by_key(|(w, _)| *w);
    for (wing, rooms) in sorted_wings {
        println!("  WING: {}", wing);
        let mut sorted_rooms: Vec<_> = rooms.iter().collect();
        sorted_rooms.sort_by(|a, b| b.1.cmp(a.1));
        for (room, count) in sorted_rooms {
            println!("    ROOM: {:20} {:5} drawers", room, count);
        }
        println!();
    }

    println!("{}\n", "=".repeat(55));
    Ok(())
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_chunk_text_empty() {
        let chunks = chunk_text("");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_small() {
        let chunks = chunk_text("Hello world, this is a small test content for chunking.");
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].chunk_index, 0);
    }

    #[test]
    fn test_chunk_text_large() {
        // Generate content larger than CHUNK_SIZE
        let content = "word ".repeat(300); // 1500 chars
        let chunks = chunk_text(&content);
        assert!(
            chunks.len() >= 2,
            "Expected at least 2 chunks, got {}",
            chunks.len()
        );
    }

    #[test]
    fn test_chunk_text_preserves_content() {
        let content =
            "This is paragraph one.\n\nThis is paragraph two.\n\nThis is paragraph three.";
        let chunks = chunk_text(content);
        assert!(!chunks.is_empty());
        // All original content should be present in at least one chunk
        assert!(chunks.iter().any(|c| c.content.contains("paragraph one")));
        assert!(chunks.iter().any(|c| c.content.contains("paragraph three")));
    }

    #[test]
    fn test_chunk_text_skips_tiny() {
        // Content smaller than MIN_CHUNK_SIZE after trimming
        let chunks = chunk_text("hi");
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_chunk_text_paragraph_boundaries() {
        // Create content with clear paragraph boundaries
        let para1 = "a".repeat(400);
        let para2 = "b".repeat(400);
        let content = format!("{}\n\n{}", para1, para2);
        let chunks = chunk_text(&content);
        // Should prefer breaking at the paragraph boundary
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_scan_project_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let files = scan_project(tmp.path().to_str().unwrap());
        assert!(files.is_empty());
    }

    #[test]
    fn test_scan_project_finds_readable_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("main.rs"), "fn main() {}").unwrap();
        std::fs::write(tmp.path().join("readme.md"), "# Hello").unwrap();
        std::fs::write(tmp.path().join("photo.png"), "binary").unwrap(); // not readable

        let files = scan_project(tmp.path().to_str().unwrap());
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_scan_project_skips_config_files() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("mempalace.yaml"), "wing: test").unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "target/").unwrap();
        std::fs::write(tmp.path().join("real_file.txt"), "content").unwrap();

        let files = scan_project(tmp.path().to_str().unwrap());
        assert_eq!(files.len(), 1);
        assert!(files[0]
            .file_name()
            .unwrap()
            .to_string_lossy()
            .contains("real_file"));
    }

    #[test]
    fn test_scan_project_skips_dirs() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("node_modules")).unwrap();
        std::fs::write(
            tmp.path().join("node_modules").join("pkg.js"),
            "module.exports={}",
        )
        .unwrap();
        std::fs::write(tmp.path().join("app.js"), "console.log('hi')").unwrap();

        let files = scan_project(tmp.path().to_str().unwrap());
        assert_eq!(files.len(), 1);
    }

    #[test]
    fn test_readable_extensions_coverage() {
        assert!(READABLE_EXTENSIONS.contains(&"rs"));
        assert!(READABLE_EXTENSIONS.contains(&"py"));
        assert!(READABLE_EXTENSIONS.contains(&"js"));
        assert!(READABLE_EXTENSIONS.contains(&"md"));
        assert!(READABLE_EXTENSIONS.contains(&"toml"));
        assert!(!READABLE_EXTENSIONS.contains(&"png"));
    }

    #[test]
    fn test_load_config_missing() {
        let tmp = TempDir::new().unwrap();
        let result = load_config(tmp.path().to_str().unwrap());
        assert!(result.is_err());
    }

    #[test]
    fn test_load_config_success() {
        let tmp = TempDir::new().unwrap();
        let config = "wing: test_project\nrooms:\n  - name: general\n    description: All files\n";
        std::fs::write(tmp.path().join("mempalace.yaml"), config).unwrap();

        let result = load_config(tmp.path().to_str().unwrap());
        assert!(result.is_ok());
        let val = result.unwrap();
        assert_eq!(val["wing"].as_str().unwrap(), "test_project");
    }

    #[test]
    fn test_load_config_legacy_fallback() {
        let tmp = TempDir::new().unwrap();
        let config = "wing: legacy_project\n";
        std::fs::write(tmp.path().join("mempal.yaml"), config).unwrap();

        let result = load_config(tmp.path().to_str().unwrap());
        assert!(result.is_ok());
    }

    #[test]
    fn test_make_drawer_id() {
        let id = make_drawer_id("tech", "rust", "main.rs", 0);
        assert!(id.starts_with("drawer_tech_rust_"));
        assert_eq!(id.len(), "drawer_tech_rust_".len() + 16);
    }

    #[test]
    fn test_make_drawer_id_deterministic() {
        let id1 = make_drawer_id("tech", "rust", "main.rs", 0);
        let id2 = make_drawer_id("tech", "rust", "main.rs", 0);
        assert_eq!(id1, id2);

        let id3 = make_drawer_id("tech", "rust", "main.rs", 1);
        assert_ne!(id1, id3);
    }
}

//! split_mega_files.rs -- Split concatenated transcript files into per-session files.
//!
//! Scans for .txt files that contain multiple Claude Code sessions
//! (identified by "Claude Code v" headers). Splits each into individual files
//! named with: date, time, people detected, and subject from first prompt.
//!
//! Distinguishes true session starts from mid-session context restores
//! (which show "Ctrl+E to show X previous messages").

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use regex::Regex;

use crate::error::Result;

// ── Known people loading ───────────────────────────────────────────────────

fn load_known_people() -> Vec<String> {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mempalace")
        .join("known_names.json");

    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(arr) = val.as_array() {
                    return arr
                        .iter()
                        .filter_map(|v| v.as_str().map(String::from))
                        .collect();
                }
                if let Some(obj) = val.as_object() {
                    if let Some(names) = obj.get("names").and_then(|v| v.as_array()) {
                        return names
                            .iter()
                            .filter_map(|v| v.as_str().map(String::from))
                            .collect();
                    }
                }
            }
        }
    }

    // Generic fallback
    vec!["Alice", "Ben", "Riley", "Max", "Sam", "Devon", "Jordan"]
        .into_iter()
        .map(String::from)
        .collect()
}

fn load_username_map() -> HashMap<String, String> {
    let config_path = dirs::home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".mempalace")
        .join("known_names.json");

    if config_path.exists() {
        if let Ok(content) = std::fs::read_to_string(&config_path) {
            if let Ok(val) = serde_json::from_str::<serde_json::Value>(&content) {
                if let Some(obj) = val.as_object() {
                    if let Some(umap) = obj.get("username_map").and_then(|v| v.as_object()) {
                        return umap
                            .iter()
                            .filter_map(|(k, v)| v.as_str().map(|s| (k.clone(), s.to_string())))
                            .collect();
                    }
                }
            }
        }
    }

    HashMap::new()
}

// ── Session boundary detection ─────────────────────────────────────────────

/// True session start: "Claude Code v" header NOT followed by "Ctrl+E" or
/// "previous messages" within the next 6 lines (those are context restores).
pub fn is_true_session_start(lines: &[&str], idx: usize) -> bool {
    let end = (idx + 6).min(lines.len());
    let nearby: String = lines[idx..end].join("");
    !nearby.contains("Ctrl+E") && !nearby.contains("previous messages")
}

/// Return list of line indices where true new sessions begin.
pub fn find_session_boundaries(lines: &[&str]) -> Vec<usize> {
    let mut boundaries = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if line.contains("Claude Code v") && is_true_session_start(lines, i) {
            boundaries.push(i);
        }
    }
    boundaries
}

// ── Timestamp extraction ───────────────────────────────────────────────────

static MONTHS: &[(&str, &str)] = &[
    ("January", "01"),
    ("February", "02"),
    ("March", "03"),
    ("April", "04"),
    ("May", "05"),
    ("June", "06"),
    ("July", "07"),
    ("August", "08"),
    ("September", "09"),
    ("October", "10"),
    ("November", "11"),
    ("December", "12"),
];

/// Find the first timestamp line.
/// Returns (human_readable, iso_date) or (None, None).
pub fn extract_timestamp(lines: &[&str]) -> (Option<String>, Option<String>) {
    let re =
        Regex::new(r"⏺\s+(\d{1,2}:\d{2}\s+[AP]M)\s+\w+,\s+(\w+)\s+(\d{1,2}),\s+(\d{4})").unwrap();

    let month_map: HashMap<&str, &str> = MONTHS.iter().copied().collect();

    for line in lines.iter().take(50) {
        if let Some(caps) = re.captures(line) {
            let time_str = &caps[1];
            let month = &caps[2];
            let day = &caps[3];
            let year = &caps[4];

            let mon = month_map.get(month).copied().unwrap_or("00");
            let day_z = format!("{:02}", day.parse::<u32>().unwrap_or(0));
            let time_safe = time_str.replace([':', ' '], "");
            let iso = format!("{}-{}-{}", year, mon, day_z);
            let human = format!("{}-{}-{}_{}", year, mon, day_z, time_safe);
            return (Some(human), Some(iso));
        }
    }

    (None, None)
}

// ── People extraction ──────────────────────────────────────────────────────

/// Detect people mentioned as speakers or by name in first 100 lines.
/// Returns sorted list of detected names.
pub fn extract_people(lines: &[&str], known_people: &[String]) -> Vec<String> {
    let mut found = std::collections::HashSet::new();
    let text: String = lines
        .iter()
        .take(100)
        .copied()
        .collect::<Vec<_>>()
        .join(" ");

    // Check known people
    for person in known_people {
        let pattern = format!(r"\b(?i){}\b", regex::escape(person));
        if let Ok(re) = Regex::new(&pattern) {
            if re.is_match(&text) {
                found.insert(person.clone());
            }
        }
    }

    // Working directory username hint
    let dir_re = Regex::new(r"/Users/(\w+)/").unwrap();
    if let Some(caps) = dir_re.captures(&text) {
        let username = &caps[1];
        let username_map = load_username_map();
        if let Some(name) = username_map.get(username) {
            found.insert(name.clone());
        }
    }

    let mut result: Vec<String> = found.into_iter().collect();
    result.sort();
    result
}

// ── Subject extraction ─────────────────────────────────────────────────────

/// Find the first meaningful user prompt (> line that is not a shell command).
/// Returns cleaned, filename-safe subject string.
pub fn extract_subject(lines: &[&str]) -> String {
    let skip_re =
        Regex::new(r"^(\.\/|cd |ls |python|bash|git |cat |source |export |claude|./activate)")
            .unwrap();
    let non_word_re = Regex::new(r"[^\w\s-]").unwrap();
    let whitespace_re = Regex::new(r"\s+").unwrap();

    for line in lines {
        if line.starts_with("> ") {
            let prompt = line.strip_prefix("> ").unwrap_or(line).trim();
            if !prompt.is_empty() && !skip_re.is_match(prompt) && prompt.len() > 5 {
                let subject = non_word_re.replace_all(prompt, "");
                let subject = whitespace_re.replace_all(subject.trim(), "-");
                let truncated: String = subject.chars().take(60).collect();
                return truncated;
            }
        }
    }

    "session".to_string()
}

// ── Split file ─────────────────────────────────────────────────────────────

/// Split a single mega-file into per-session files.
/// Returns list of output paths written (or would-be-written if dry_run).
pub fn split_file(
    filepath: &Path,
    output_dir: Option<&Path>,
    dry_run: bool,
) -> Result<Vec<PathBuf>> {
    let content = std::fs::read_to_string(filepath)?;
    let lines_owned: Vec<String> = content.lines().map(String::from).collect();
    let lines: Vec<&str> = lines_owned.iter().map(|s| s.as_str()).collect();

    let mut boundaries = find_session_boundaries(&lines);
    if boundaries.len() < 2 {
        return Ok(vec![]); // Not a mega-file
    }

    // Add sentinel at end
    boundaries.push(lines.len());

    let out_dir = output_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| filepath.parent().unwrap_or(Path::new(".")).to_path_buf());

    let known_people = load_known_people();
    let sanitize_re = Regex::new(r"[^\w\.\-]").unwrap();
    let dedup_under = Regex::new(r"_+").unwrap();

    let mut written = Vec::new();

    for i in 0..boundaries.len() - 1 {
        let start = boundaries[i];
        let end = boundaries[i + 1];

        let chunk: Vec<&str> = lines[start..end].to_vec();
        if chunk.len() < 10 {
            continue; // Skip tiny fragments
        }

        let (ts_human, _ts_iso) = extract_timestamp(&chunk);
        let people = extract_people(&chunk, &known_people);
        let subject = extract_subject(&chunk);

        let ts_part = ts_human.unwrap_or_else(|| format!("part{:02}", i + 1));
        let people_part = if !people.is_empty() {
            people.iter().take(3).cloned().collect::<Vec<_>>().join("-")
        } else {
            "unknown".to_string()
        };

        let src_stem: String = filepath
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| "file".to_string());
        let src_stem: String = sanitize_re
            .replace_all(&src_stem, "_")
            .chars()
            .take(40)
            .collect();

        let name = format!("{}__{}_{}_{}.txt", src_stem, ts_part, people_part, subject);
        let name = sanitize_re.replace_all(&name, "_");
        let name = dedup_under.replace_all(&name, "_");

        let out_path = out_dir.join(name.as_ref());

        if dry_run {
            println!(
                "  [{}/{}] {}  ({} lines)",
                i + 1,
                boundaries.len() - 1,
                name,
                chunk.len()
            );
        } else {
            let session_content: String = chunk.iter().map(|l| format!("{}\n", l)).collect();
            std::fs::write(&out_path, session_content)?;
            println!("  {} ({} lines)", name, chunk.len());
        }

        written.push(out_path);
    }

    Ok(written)
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_is_true_session_start() {
        let lines = vec![
            "Claude Code v1.0.0",
            "Some content",
            "More content",
            "Even more",
            "Still going",
            "Last line",
        ];
        assert!(is_true_session_start(&lines, 0));
    }

    #[test]
    fn test_is_not_true_session_start_ctrl_e() {
        let lines = vec![
            "Claude Code v1.0.0",
            "Ctrl+E to show 5 previous messages",
            "Some content",
            "More content",
            "Even more",
            "Last line",
        ];
        assert!(!is_true_session_start(&lines, 0));
    }

    #[test]
    fn test_is_not_true_session_start_previous_messages() {
        let lines = vec![
            "Claude Code v1.0.0",
            "Some text",
            "previous messages available",
            "More content",
            "Even more",
            "Last line",
        ];
        assert!(!is_true_session_start(&lines, 0));
    }

    #[test]
    fn test_find_session_boundaries() {
        let lines = vec![
            "Claude Code v1.0.0",
            "Session 1 content",
            "More content",
            "Even more content",
            "Still more content",
            "Another line",
            "And another",
            "",
            "Claude Code v1.0.0",
            "Session 2 content",
            "More session 2",
            "Even more session 2",
            "Still more session 2",
            "Another session 2 line",
            "And yet another",
            "",
            "Claude Code v1.0.0",
            "Ctrl+E to show previous messages",
            "This is a context restore",
        ];
        let boundaries = find_session_boundaries(&lines);
        // Third one has Ctrl+E, so only 2 boundaries
        assert_eq!(boundaries.len(), 2);
        assert_eq!(boundaries[0], 0);
        assert_eq!(boundaries[1], 8);
    }

    #[test]
    fn test_find_session_boundaries_none() {
        let lines = vec!["Just some text", "No sessions here", "Nothing special"];
        let boundaries = find_session_boundaries(&lines);
        assert!(boundaries.is_empty());
    }

    #[test]
    fn test_extract_timestamp_found() {
        let lines = vec![
            "Some header",
            "⏺ 2:30 PM Wednesday, March 26, 2026",
            "Content here",
        ];
        let (human, iso) = extract_timestamp(&lines);
        assert!(human.is_some());
        assert!(iso.is_some());
        let human = human.unwrap();
        assert!(human.contains("2026"));
        assert!(human.contains("03"));
        assert!(human.contains("26"));
        let iso = iso.unwrap();
        assert_eq!(iso, "2026-03-26");
    }

    #[test]
    fn test_extract_timestamp_not_found() {
        let lines = vec!["No timestamp here", "Just text"];
        let (human, iso) = extract_timestamp(&lines);
        assert!(human.is_none());
        assert!(iso.is_none());
    }

    #[test]
    fn test_extract_people_known() {
        let lines = vec![
            "Alice said hello",
            "Ben replied with thanks",
            "Working in /Users/jdoe/projects",
        ];
        let known = vec![
            "Alice".to_string(),
            "Ben".to_string(),
            "Charlie".to_string(),
        ];
        let people = extract_people(&lines, &known);
        assert!(people.contains(&"Alice".to_string()));
        assert!(people.contains(&"Ben".to_string()));
        assert!(!people.contains(&"Charlie".to_string()));
    }

    #[test]
    fn test_extract_people_empty() {
        let lines = vec!["No names here", "Just text"];
        let known = vec!["Alice".to_string()];
        let people = extract_people(&lines, &known);
        assert!(people.is_empty());
    }

    #[test]
    fn test_extract_subject() {
        let lines = vec![
            "Some header",
            "> How do we implement the login page?",
            "Response here",
        ];
        let subject = extract_subject(&lines);
        assert!(subject.contains("implement"));
        assert!(subject.contains("login"));
    }

    #[test]
    fn test_extract_subject_skips_commands() {
        let lines = vec![
            "> cd /some/directory",
            "> git status",
            "> python run_tests.py",
            "> What is the best approach for caching?",
        ];
        let subject = extract_subject(&lines);
        assert!(
            subject.contains("best") || subject.contains("approach") || subject.contains("caching")
        );
    }

    #[test]
    fn test_extract_subject_fallback() {
        let lines = vec!["No prompts here", "Just text"];
        let subject = extract_subject(&lines);
        assert_eq!(subject, "session");
    }

    #[test]
    fn test_split_file_not_mega() {
        let tmp = TempDir::new().unwrap();
        let filepath = tmp.path().join("single.txt");
        std::fs::write(
            &filepath,
            "Just a single session.\nNo Claude Code headers.\n",
        )
        .unwrap();

        let result = split_file(&filepath, None, true).unwrap();
        assert!(result.is_empty(), "Should return empty for non-mega files");
    }

    #[test]
    fn test_split_file_dry_run() {
        let tmp = TempDir::new().unwrap();
        let filepath = tmp.path().join("mega.txt");

        let mut content = String::new();
        // Session 1
        content.push_str("Claude Code v1.0.0\n");
        for i in 0..20 {
            content.push_str(&format!("> Question {} about the project\n", i));
            content.push_str(&format!("Answer {} with details\n", i));
        }
        // Session 2
        content.push_str("Claude Code v1.0.0\n");
        for i in 0..20 {
            content.push_str(&format!("> Another question {} about testing\n", i));
            content.push_str(&format!("Another answer {} with details\n", i));
        }

        std::fs::write(&filepath, &content).unwrap();

        let result = split_file(&filepath, Some(tmp.path()), true).unwrap();
        assert_eq!(result.len(), 2);
    }

    #[test]
    fn test_split_file_actual_write() {
        let tmp = TempDir::new().unwrap();
        let filepath = tmp.path().join("mega.txt");
        let out_dir = tmp.path().join("output");
        std::fs::create_dir(&out_dir).unwrap();

        let mut content = String::new();
        // Session 1
        content.push_str("Claude Code v1.0.0\n");
        for i in 0..15 {
            content.push_str(&format!("Line {} of session 1\n", i));
        }
        // Session 2
        content.push_str("Claude Code v1.0.0\n");
        for i in 0..15 {
            content.push_str(&format!("Line {} of session 2\n", i));
        }

        std::fs::write(&filepath, &content).unwrap();

        let result = split_file(&filepath, Some(&out_dir), false).unwrap();
        assert_eq!(result.len(), 2);
        // Verify files were actually written
        for path in &result {
            assert!(path.exists(), "Output file should exist: {:?}", path);
        }
    }
}

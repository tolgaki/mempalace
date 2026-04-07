use std::collections::HashMap;
use std::path::Path;

/// Mapping of folder name keywords to room names.
pub fn folder_room_map() -> HashMap<&'static str, &'static str> {
    let pairs = [
        ("frontend", "frontend"),
        ("front-end", "frontend"),
        ("front_end", "frontend"),
        ("client", "frontend"),
        ("ui", "frontend"),
        ("views", "frontend"),
        ("components", "frontend"),
        ("pages", "frontend"),
        ("backend", "backend"),
        ("back-end", "backend"),
        ("back_end", "backend"),
        ("server", "backend"),
        ("api", "backend"),
        ("routes", "backend"),
        ("services", "backend"),
        ("controllers", "backend"),
        ("models", "backend"),
        ("database", "backend"),
        ("db", "backend"),
        ("docs", "documentation"),
        ("doc", "documentation"),
        ("documentation", "documentation"),
        ("wiki", "documentation"),
        ("readme", "documentation"),
        ("notes", "documentation"),
        ("design", "design"),
        ("designs", "design"),
        ("mockups", "design"),
        ("wireframes", "design"),
        ("assets", "design"),
        ("storyboard", "design"),
        ("costs", "costs"),
        ("cost", "costs"),
        ("budget", "costs"),
        ("finance", "costs"),
        ("financial", "costs"),
        ("pricing", "costs"),
        ("invoices", "costs"),
        ("accounting", "costs"),
        ("meetings", "meetings"),
        ("meeting", "meetings"),
        ("calls", "meetings"),
        ("meeting_notes", "meetings"),
        ("standup", "meetings"),
        ("minutes", "meetings"),
        ("team", "team"),
        ("staff", "team"),
        ("hr", "team"),
        ("hiring", "team"),
        ("employees", "team"),
        ("people", "team"),
        ("research", "research"),
        ("references", "research"),
        ("reading", "research"),
        ("papers", "research"),
        ("planning", "planning"),
        ("roadmap", "planning"),
        ("strategy", "planning"),
        ("specs", "planning"),
        ("requirements", "planning"),
        ("tests", "testing"),
        ("test", "testing"),
        ("testing", "testing"),
        ("qa", "testing"),
        ("scripts", "scripts"),
        ("tools", "scripts"),
        ("utils", "scripts"),
        ("config", "configuration"),
        ("configs", "configuration"),
        ("settings", "configuration"),
        ("infrastructure", "configuration"),
        ("infra", "configuration"),
        ("deploy", "configuration"),
    ];
    pairs.into_iter().collect()
}

/// Directories to skip when scanning.
pub fn skip_dirs() -> std::collections::HashSet<&'static str> {
    [
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
    ]
    .into_iter()
    .collect()
}

/// Room definition from config.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct RoomDef {
    pub name: String,
    pub description: String,
    #[serde(default)]
    pub keywords: Vec<String>,
}

/// Detect rooms from a project's folder structure.
pub fn detect_rooms_from_folders(project_dir: &str) -> Vec<RoomDef> {
    let project_path = Path::new(project_dir);
    let fmap = folder_room_map();
    let skips = skip_dirs();
    let mut found_rooms: HashMap<String, String> = HashMap::new();

    // Check top-level directories
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let name = entry.file_name().to_string_lossy().to_string();
            if skips.contains(name.as_str()) {
                continue;
            }
            let name_lower = name.to_lowercase().replace('-', "_");
            if let Some(&room_name) = fmap.get(name_lower.as_str()) {
                found_rooms
                    .entry(room_name.to_string())
                    .or_insert(name.clone());
            } else if name.len() > 2 && name.chars().next().is_some_and(|c| c.is_alphabetic()) {
                let clean = name.to_lowercase().replace(['-', ' '], "_");
                found_rooms.entry(clean).or_insert(name.clone());
            }
        }
    }

    // Walk one level deeper
    if let Ok(entries) = std::fs::read_dir(project_path) {
        for entry in entries.flatten() {
            let path = entry.path();
            if !path.is_dir() {
                continue;
            }
            let dir_name = entry.file_name().to_string_lossy().to_string();
            if skips.contains(dir_name.as_str()) {
                continue;
            }
            if let Ok(sub_entries) = std::fs::read_dir(&path) {
                for sub_entry in sub_entries.flatten() {
                    if !sub_entry.path().is_dir() {
                        continue;
                    }
                    let sub_name = sub_entry.file_name().to_string_lossy().to_string();
                    if skips.contains(sub_name.as_str()) {
                        continue;
                    }
                    let sub_lower = sub_name.to_lowercase().replace('-', "_");
                    if let Some(&room_name) = fmap.get(sub_lower.as_str()) {
                        found_rooms.entry(room_name.to_string()).or_insert(sub_name);
                    }
                }
            }
        }
    }

    let mut rooms: Vec<RoomDef> = found_rooms
        .into_iter()
        .map(|(room_name, original)| RoomDef {
            keywords: vec![room_name.clone(), original.to_lowercase()],
            description: format!("Files from {}/", original),
            name: room_name,
        })
        .collect();

    rooms.sort_by(|a, b| a.name.cmp(&b.name));

    // Always add "general" as fallback
    if !rooms.iter().any(|r| r.name == "general") {
        rooms.push(RoomDef {
            name: "general".into(),
            description: "Files that don't fit other rooms".into(),
            keywords: vec![],
        });
    }

    rooms
}

/// Detect rooms from recurring filename patterns (fallback).
pub fn detect_rooms_from_files(project_dir: &str) -> Vec<RoomDef> {
    let project_path = Path::new(project_dir);
    let fmap = folder_room_map();
    let skips = skip_dirs();
    let mut keyword_counts: HashMap<String, usize> = HashMap::new();

    fn walk(
        dir: &Path,
        fmap: &HashMap<&str, &str>,
        skips: &std::collections::HashSet<&str>,
        counts: &mut HashMap<String, usize>,
    ) {
        let entries = match std::fs::read_dir(dir) {
            Ok(e) => e,
            Err(_) => return,
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let name = entry.file_name().to_string_lossy().to_string();
            if path.is_dir() {
                if !skips.contains(name.as_str()) {
                    walk(&path, fmap, skips, counts);
                }
            } else {
                let name_lower = name.to_lowercase().replace(['-', ' '], "_");
                for (&keyword, &room) in fmap.iter() {
                    if name_lower.contains(keyword) {
                        *counts.entry(room.to_string()).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    walk(project_path, &fmap, &skips, &mut keyword_counts);

    let mut rooms: Vec<RoomDef> = keyword_counts
        .into_iter()
        .filter(|(_, count)| *count >= 2)
        .map(|(room, _)| RoomDef {
            keywords: vec![room.clone()],
            description: format!("Files related to {}", room),
            name: room,
        })
        .collect();

    rooms.sort_by(|a, b| b.name.cmp(&a.name)); // sort for consistency
    rooms.truncate(6);

    if rooms.is_empty() {
        rooms.push(RoomDef {
            name: "general".into(),
            description: "All project files".into(),
            keywords: vec![],
        });
    }

    rooms
}

/// Detect the room for a file based on its path and content.
pub fn detect_room(
    filepath: &Path,
    content: &str,
    rooms: &[RoomDef],
    project_path: &Path,
) -> String {
    let relative = filepath
        .strip_prefix(project_path)
        .unwrap_or(filepath)
        .to_string_lossy()
        .to_lowercase();
    let filename = filepath
        .file_stem()
        .map(|s| s.to_string_lossy().to_lowercase())
        .unwrap_or_default();
    let content_lower: String = content
        .chars()
        .take(2000)
        .collect::<String>()
        .to_lowercase();

    // Priority 1: folder path contains room name
    let normalized = relative.replace('\\', "/");
    let path_parts: Vec<&str> = normalized.split('/').collect();
    if path_parts.len() > 1 {
        for part in &path_parts[..path_parts.len() - 1] {
            for room in rooms {
                let rname = room.name.to_lowercase();
                if rname.contains(part) || part.contains(&rname) {
                    return room.name.clone();
                }
            }
        }
    }

    // Priority 2: filename matches room name
    for room in rooms {
        let rname = room.name.to_lowercase();
        if rname.contains(&filename) || filename.contains(&rname) {
            return room.name.clone();
        }
    }

    // Priority 3: keyword scoring
    let mut scores: HashMap<String, usize> = HashMap::new();
    for room in rooms {
        let mut keywords: Vec<String> = room.keywords.clone();
        keywords.push(room.name.clone());
        let score: usize = keywords
            .iter()
            .map(|kw| content_lower.matches(&kw.to_lowercase()).count())
            .sum();
        if score > 0 {
            scores.insert(room.name.clone(), score);
        }
    }

    if let Some((best, _)) = scores.iter().max_by_key(|(_, &v)| v) {
        return best.clone();
    }

    "general".to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_folder_room_map_has_entries() {
        let map = folder_room_map();
        assert!(map.len() > 60);
        assert_eq!(map.get("frontend"), Some(&"frontend"));
        assert_eq!(map.get("backend"), Some(&"backend"));
        assert_eq!(map.get("docs"), Some(&"documentation"));
    }

    #[test]
    fn test_detect_rooms_empty_dir() {
        let tmp = TempDir::new().unwrap();
        let rooms = detect_rooms_from_folders(tmp.path().to_str().unwrap());
        // Should have at least "general"
        assert!(rooms.iter().any(|r| r.name == "general"));
    }

    #[test]
    fn test_detect_rooms_with_known_folders() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir(tmp.path().join("frontend")).unwrap();
        std::fs::create_dir(tmp.path().join("backend")).unwrap();
        std::fs::create_dir(tmp.path().join("docs")).unwrap();
        let rooms = detect_rooms_from_folders(tmp.path().to_str().unwrap());
        let names: Vec<&str> = rooms.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"frontend"));
        assert!(names.contains(&"backend"));
        assert!(names.contains(&"documentation"));
    }

    #[test]
    fn test_detect_room_by_path() {
        let rooms = vec![
            RoomDef {
                name: "frontend".into(),
                description: "".into(),
                keywords: vec![],
            },
            RoomDef {
                name: "backend".into(),
                description: "".into(),
                keywords: vec![],
            },
        ];
        let project = Path::new("/project");
        let filepath = Path::new("/project/frontend/app.js");
        assert_eq!(detect_room(filepath, "", &rooms, project), "frontend");
    }

    #[test]
    fn test_detect_room_by_filename() {
        let rooms = vec![RoomDef {
            name: "testing".into(),
            description: "".into(),
            keywords: vec![],
        }];
        let project = Path::new("/project");
        let filepath = Path::new("/project/testing_utils.py");
        assert_eq!(detect_room(filepath, "", &rooms, project), "testing");
    }

    #[test]
    fn test_detect_room_by_content_keywords() {
        let rooms = vec![
            RoomDef {
                name: "backend".into(),
                description: "".into(),
                keywords: vec!["api".into(), "server".into()],
            },
            RoomDef {
                name: "frontend".into(),
                description: "".into(),
                keywords: vec!["react".into(), "css".into()],
            },
        ];
        let project = Path::new("/project");
        let filepath = Path::new("/project/misc/notes.txt");
        assert_eq!(
            detect_room(
                filepath,
                "setting up the api server with express",
                &rooms,
                project
            ),
            "backend"
        );
    }

    #[test]
    fn test_detect_room_fallback_general() {
        let rooms = vec![RoomDef {
            name: "backend".into(),
            description: "".into(),
            keywords: vec!["api".into()],
        }];
        let project = Path::new("/project");
        let filepath = Path::new("/project/random.txt");
        assert_eq!(
            detect_room(filepath, "nothing relevant here", &rooms, project),
            "general"
        );
    }

    #[test]
    fn test_detect_rooms_from_files_empty() {
        let tmp = TempDir::new().unwrap();
        let rooms = detect_rooms_from_files(tmp.path().to_str().unwrap());
        assert!(rooms.iter().any(|r| r.name == "general"));
    }

    #[test]
    fn test_detect_rooms_nested_folders() {
        let tmp = TempDir::new().unwrap();
        std::fs::create_dir_all(tmp.path().join("src").join("tests")).unwrap();
        let rooms = detect_rooms_from_folders(tmp.path().to_str().unwrap());
        let names: Vec<&str> = rooms.iter().map(|r| r.name.as_str()).collect();
        assert!(names.contains(&"testing"));
    }

    #[test]
    fn test_skip_dirs() {
        let skips = skip_dirs();
        assert!(skips.contains(".git"));
        assert!(skips.contains("node_modules"));
        assert!(skips.contains("__pycache__"));
    }
}

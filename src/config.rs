use crate::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

fn default_palace_path() -> String {
    let home = dirs::home_dir().unwrap_or_else(|| PathBuf::from("."));
    home.join(".mempalace")
        .join("palace")
        .to_string_lossy()
        .into_owned()
}

const DEFAULT_COLLECTION_NAME: &str = "mempalace_drawers";

pub fn default_topic_wings() -> Vec<String> {
    vec![
        "emotions",
        "consciousness",
        "memory",
        "technical",
        "identity",
        "family",
        "creative",
    ]
    .into_iter()
    .map(String::from)
    .collect()
}

pub fn default_hall_keywords() -> HashMap<String, Vec<String>> {
    let mut m = HashMap::new();
    m.insert(
        "emotions".into(),
        vec![
            "scared", "afraid", "worried", "happy", "sad", "love", "hate", "feel", "cry", "tears",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m.insert(
        "consciousness".into(),
        vec![
            "consciousness",
            "conscious",
            "aware",
            "real",
            "genuine",
            "soul",
            "exist",
            "alive",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m.insert(
        "memory".into(),
        vec![
            "memory", "remember", "forget", "recall", "archive", "palace", "store",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m.insert(
        "technical".into(),
        vec![
            "code", "python", "script", "bug", "error", "function", "api", "database", "server",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m.insert(
        "identity".into(),
        vec!["identity", "name", "who am i", "persona", "self"]
            .into_iter()
            .map(String::from)
            .collect(),
    );
    m.insert(
        "family".into(),
        vec![
            "family", "kids", "children", "daughter", "son", "parent", "mother", "father",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m.insert(
        "creative".into(),
        vec![
            "game", "gameplay", "player", "app", "design", "art", "music", "story",
        ]
        .into_iter()
        .map(String::from)
        .collect(),
    );
    m
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileConfig {
    #[serde(default)]
    pub palace_path: Option<String>,
    #[serde(default)]
    pub collection_name: Option<String>,
    #[serde(default)]
    pub topic_wings: Option<Vec<String>>,
    #[serde(default)]
    pub hall_keywords: Option<HashMap<String, Vec<String>>>,
    #[serde(default)]
    pub people_map: Option<HashMap<String, String>>,
}

#[derive(Debug, Clone)]
pub struct MempalaceConfig {
    config_dir: PathBuf,
    file_config: Option<FileConfig>,
}

impl MempalaceConfig {
    pub fn new(config_dir: Option<&Path>) -> Self {
        let config_dir = config_dir.map(PathBuf::from).unwrap_or_else(|| {
            dirs::home_dir()
                .unwrap_or_else(|| PathBuf::from("."))
                .join(".mempalace")
        });

        let config_file = config_dir.join("config.json");
        let file_config = if config_file.exists() {
            std::fs::read_to_string(&config_file)
                .ok()
                .and_then(|s| serde_json::from_str::<FileConfig>(&s).ok())
        } else {
            None
        };

        Self {
            config_dir,
            file_config,
        }
    }

    pub fn palace_path(&self) -> String {
        if let Ok(val) = std::env::var("MEMPALACE_PALACE_PATH") {
            if !val.is_empty() {
                return val;
            }
        }
        if let Ok(val) = std::env::var("MEMPAL_PALACE_PATH") {
            if !val.is_empty() {
                return val;
            }
        }
        if let Some(ref fc) = self.file_config {
            if let Some(ref p) = fc.palace_path {
                return p.clone();
            }
        }
        default_palace_path()
    }

    pub fn collection_name(&self) -> String {
        self.file_config
            .as_ref()
            .and_then(|fc| fc.collection_name.clone())
            .unwrap_or_else(|| DEFAULT_COLLECTION_NAME.to_string())
    }

    pub fn people_map(&self) -> HashMap<String, String> {
        // Try people_map.json first
        let people_map_file = self.config_dir.join("people_map.json");
        if people_map_file.exists() {
            if let Ok(content) = std::fs::read_to_string(&people_map_file) {
                if let Ok(map) = serde_json::from_str::<HashMap<String, String>>(&content) {
                    return map;
                }
            }
        }
        self.file_config
            .as_ref()
            .and_then(|fc| fc.people_map.clone())
            .unwrap_or_default()
    }

    pub fn topic_wings(&self) -> Vec<String> {
        self.file_config
            .as_ref()
            .and_then(|fc| fc.topic_wings.clone())
            .unwrap_or_else(default_topic_wings)
    }

    pub fn hall_keywords(&self) -> HashMap<String, Vec<String>> {
        self.file_config
            .as_ref()
            .and_then(|fc| fc.hall_keywords.clone())
            .unwrap_or_else(default_hall_keywords)
    }

    pub fn config_dir(&self) -> &Path {
        &self.config_dir
    }

    pub fn init(&self) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.config_dir)?;
        let config_file = self.config_dir.join("config.json");
        if !config_file.exists() {
            let default = FileConfig {
                palace_path: Some(default_palace_path()),
                collection_name: Some(DEFAULT_COLLECTION_NAME.to_string()),
                topic_wings: Some(default_topic_wings()),
                hall_keywords: Some(default_hall_keywords()),
                people_map: None,
            };
            let json = serde_json::to_string_pretty(&default)?;
            std::fs::write(&config_file, json)?;
        }
        Ok(config_file)
    }

    pub fn save_people_map(&self, people_map: &HashMap<String, String>) -> Result<PathBuf> {
        std::fs::create_dir_all(&self.config_dir)?;
        let path = self.config_dir.join("people_map.json");
        let json = serde_json::to_string_pretty(people_map)?;
        std::fs::write(&path, json)?;
        Ok(path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_config() {
        let tmp = TempDir::new().unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        assert!(cfg.palace_path().contains("palace"));
        assert_eq!(cfg.collection_name(), "mempalace_drawers");
    }

    #[test]
    fn test_config_from_file() {
        let tmp = TempDir::new().unwrap();
        let config = r#"{"palace_path": "/custom/palace"}"#;
        std::fs::write(tmp.path().join("config.json"), config).unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        assert_eq!(cfg.palace_path(), "/custom/palace");
    }

    #[test]
    fn test_env_override() {
        let tmp = TempDir::new().unwrap();
        std::env::set_var("MEMPALACE_PALACE_PATH", "/env/palace");
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        assert_eq!(cfg.palace_path(), "/env/palace");
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_init_creates_config() {
        let tmp = TempDir::new().unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        cfg.init().unwrap();
        assert!(tmp.path().join("config.json").exists());
    }

    #[test]
    fn test_topic_wings_default() {
        let tmp = TempDir::new().unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        let wings = cfg.topic_wings();
        assert!(wings.contains(&"emotions".to_string()));
        assert!(wings.contains(&"technical".to_string()));
    }

    #[test]
    fn test_hall_keywords_default() {
        let tmp = TempDir::new().unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        let kw = cfg.hall_keywords();
        assert!(kw.contains_key("emotions"));
        assert!(kw["emotions"].contains(&"love".to_string()));
    }

    #[test]
    fn test_people_map_from_file() {
        let tmp = TempDir::new().unwrap();
        let map = r#"{"bob": "Robert", "ali": "Alice"}"#;
        std::fs::write(tmp.path().join("people_map.json"), map).unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        let pm = cfg.people_map();
        assert_eq!(pm.get("bob").unwrap(), "Robert");
    }

    #[test]
    fn test_save_people_map() {
        let tmp = TempDir::new().unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        let mut map = HashMap::new();
        map.insert("ali".into(), "Alice".into());
        cfg.save_people_map(&map).unwrap();
        assert!(tmp.path().join("people_map.json").exists());
    }

    #[test]
    fn test_malformed_config_json() {
        let tmp = TempDir::new().unwrap();
        std::fs::write(tmp.path().join("config.json"), "{{invalid json}}").unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        // Should fall back to defaults
        assert!(cfg.palace_path().contains("palace"));
    }

    #[test]
    fn test_collection_name_from_file() {
        let tmp = TempDir::new().unwrap();
        let config = r#"{"collection_name": "custom_col"}"#;
        std::fs::write(tmp.path().join("config.json"), config).unwrap();
        let cfg = MempalaceConfig::new(Some(tmp.path()));
        assert_eq!(cfg.collection_name(), "custom_col");
    }
}

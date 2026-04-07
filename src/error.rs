use thiserror::Error;

#[derive(Error, Debug)]
pub enum MempalaceError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),

    #[error("Database error: {0}")]
    Database(#[from] rusqlite::Error),

    #[error("JSON error: {0}")]
    Json(#[from] serde_json::Error),

    #[error("YAML error: {0}")]
    Yaml(#[from] serde_yaml::Error),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Search error: {0}")]
    Search(String),

    #[error("Parse error: {0}")]
    Parse(String),

    #[error("Not found: {0}")]
    NotFound(String),
}

pub type Result<T> = std::result::Result<T, MempalaceError>;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_io_error_display() {
        let err = MempalaceError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "file missing",
        ));
        assert!(err.to_string().contains("file missing"));
    }

    #[test]
    fn test_config_error_display() {
        let err = MempalaceError::Config("bad config".into());
        assert_eq!(err.to_string(), "Configuration error: bad config");
    }

    #[test]
    fn test_search_error_display() {
        let err = MempalaceError::Search("no results".into());
        assert_eq!(err.to_string(), "Search error: no results");
    }

    #[test]
    fn test_not_found_display() {
        let err = MempalaceError::NotFound("drawer_123".into());
        assert_eq!(err.to_string(), "Not found: drawer_123");
    }

    #[test]
    fn test_parse_error_display() {
        let err = MempalaceError::Parse("invalid format".into());
        assert_eq!(err.to_string(), "Parse error: invalid format");
    }

    #[test]
    fn test_from_io_error() {
        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let err: MempalaceError = io_err.into();
        matches!(err, MempalaceError::Io(_));
    }

    #[test]
    fn test_from_json_error() {
        let json_err = serde_json::from_str::<serde_json::Value>("{{bad").unwrap_err();
        let err: MempalaceError = json_err.into();
        matches!(err, MempalaceError::Json(_));
    }
}

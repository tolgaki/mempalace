//! normalize.rs — Convert any chat export format to MemPalace transcript format.
//!
//! Supported formats:
//!   - Plain text with > markers (pass through)
//!   - Claude Code JSONL (`{"type":"human","message":{"content":"..."}}` per line)
//!   - Claude.ai JSON (`{"messages":[{"role":"user","content":"..."}]}`)
//!   - ChatGPT conversations.json (mapping tree with parent/children)
//!   - Slack JSON (`[{"type":"message","user":"...","text":"..."}]`)
//!
//! No API key. No internet. Everything local.

use crate::error::{MempalaceError, Result};
use serde_json::Value;
use std::collections::{HashMap, HashSet};
use std::path::Path;

/// Load a file and normalize to transcript format if it's a chat export.
/// Plain text files pass through unchanged.
pub fn normalize(filepath: &str) -> Result<String> {
    let content = std::fs::read_to_string(filepath).map_err(|e| {
        MempalaceError::Io(std::io::Error::new(
            e.kind(),
            format!("Could not read {}: {}", filepath, e),
        ))
    })?;

    if content.trim().is_empty() {
        return Ok(content);
    }

    // Already has >= 3 lines starting with ">" — pass through
    let quote_count = content
        .lines()
        .filter(|line| line.trim_start().starts_with('>'))
        .count();
    if quote_count >= 3 {
        return Ok(content);
    }

    // Try JSON normalization based on extension or content shape
    let ext = Path::new(filepath)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    let trimmed = content.trim();
    let first_char = trimmed.chars().next().unwrap_or('\0');

    if ext == "json" || ext == "jsonl" || first_char == '{' || first_char == '[' {
        if let Some(normalized) = try_normalize_json(&content) {
            return Ok(normalized);
        }
    }

    Ok(content)
}

/// Detect the format of the given content string.
///
/// Returns one of:
///   - `"claude_code_jsonl"` — Claude Code JSONL sessions
///   - `"claude_ai_json"` — Claude.ai JSON conversations
///   - `"chatgpt_json"` — ChatGPT conversations with mapping tree
///   - `"slack_json"` — Slack message exports
///   - `"plain"` — plain text or unrecognized format
pub fn detect_format(content: &str) -> &'static str {
    let trimmed = content.trim();
    if trimmed.is_empty() {
        return "plain";
    }

    // Try JSONL (Claude Code): each line is a JSON object with "type" and "message"
    {
        let mut jsonl_count = 0;
        for line in trimmed.lines() {
            let line = line.trim();
            if line.is_empty() {
                continue;
            }
            if let Ok(val) = serde_json::from_str::<Value>(line) {
                if let Some(obj) = val.as_object() {
                    if obj.contains_key("type") && obj.contains_key("message") {
                        jsonl_count += 1;
                    }
                }
            } else {
                break;
            }
        }
        if jsonl_count >= 2 {
            return "claude_code_jsonl";
        }
    }

    // Try single JSON value
    if let Ok(data) = serde_json::from_str::<Value>(trimmed) {
        // ChatGPT: object with "mapping" key
        if let Some(obj) = data.as_object() {
            if obj.contains_key("mapping") {
                return "chatgpt_json";
            }
        }

        // Claude.ai: object with "messages" or "chat_messages" key
        if let Some(obj) = data.as_object() {
            if obj.contains_key("messages") || obj.contains_key("chat_messages") {
                return "claude_ai_json";
            }
        }

        // Array formats
        if let Some(arr) = data.as_array() {
            if !arr.is_empty() {
                // Slack: array of objects with "type":"message"
                let slack_count = arr
                    .iter()
                    .filter(|item| {
                        item.get("type").and_then(|v| v.as_str()) == Some("message")
                            && (item.get("user").is_some() || item.get("username").is_some())
                    })
                    .count();
                if slack_count >= 2 {
                    return "slack_json";
                }

                // Claude.ai array format: array of objects with "role" key
                let role_count = arr.iter().filter(|item| item.get("role").is_some()).count();
                if role_count >= 2 {
                    return "claude_ai_json";
                }
            }
        }
    }

    "plain"
}

fn try_normalize_json(content: &str) -> Option<String> {
    // Try JSONL first (Claude Code sessions)
    if let Some(result) = try_claude_code_jsonl(content) {
        return Some(result);
    }

    // Try parsing as a single JSON value
    let data: Value = serde_json::from_str(content).ok()?;

    for parser in &[try_claude_ai_json, try_chatgpt_json, try_slack_json] {
        if let Some(result) = parser(&data) {
            return Some(result);
        }
    }

    None
}

fn try_claude_code_jsonl(content: &str) -> Option<String> {
    let mut messages: Vec<(String, String)> = Vec::new();

    for line in content.trim().lines() {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        let entry: Value = match serde_json::from_str(line) {
            Ok(v) => v,
            Err(_) => continue,
        };
        let obj = entry.as_object()?;
        let msg_type = obj.get("type").and_then(|v| v.as_str()).unwrap_or("");
        let message = obj.get("message").and_then(|v| v.as_object());

        if let Some(message) = message {
            let content_val = message.get("content").cloned().unwrap_or(Value::Null);
            let text = extract_content(&content_val);
            if !text.is_empty() {
                match msg_type {
                    "human" => messages.push(("user".to_string(), text)),
                    "assistant" => messages.push(("assistant".to_string(), text)),
                    _ => {}
                }
            }
        }
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_claude_ai_json(data: &Value) -> Option<String> {
    let items = if let Some(arr) = data.as_array() {
        arr.clone()
    } else if let Some(obj) = data.as_object() {
        obj.get("messages")
            .or_else(|| obj.get("chat_messages"))
            .and_then(|v| v.as_array())
            .cloned()?
    } else {
        return None;
    };

    let mut messages: Vec<(String, String)> = Vec::new();

    for item in &items {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        let role = obj.get("role").and_then(|v| v.as_str()).unwrap_or("");
        let content_val = obj.get("content").cloned().unwrap_or(Value::Null);
        let text = extract_content(&content_val);
        if text.is_empty() {
            continue;
        }
        match role {
            "user" | "human" => messages.push(("user".to_string(), text)),
            "assistant" | "ai" => messages.push(("assistant".to_string(), text)),
            _ => {}
        }
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_chatgpt_json(data: &Value) -> Option<String> {
    let obj = data.as_object()?;
    let mapping = obj.get("mapping")?.as_object()?;

    // Find root: prefer node with parent=null AND no message (synthetic root)
    let mut root_id: Option<&str> = None;
    let mut fallback_root: Option<&str> = None;

    for (node_id, node) in mapping.iter() {
        let parent = node.get("parent");
        let is_null_parent = parent.is_none() || parent.is_some_and(|p| p.is_null());
        if is_null_parent {
            let has_no_message =
                node.get("message").is_none() || node.get("message").is_some_and(|m| m.is_null());
            if has_no_message {
                root_id = Some(node_id.as_str());
                break;
            } else if fallback_root.is_none() {
                fallback_root = Some(node_id.as_str());
            }
        }
    }

    let root_id = root_id.or(fallback_root)?;
    let mut current_id = Some(root_id.to_string());
    let mut visited = HashSet::new();
    let mut messages: Vec<(String, String)> = Vec::new();

    while let Some(ref cid) = current_id {
        if visited.contains(cid) {
            break;
        }
        visited.insert(cid.clone());

        let node = match mapping.get(cid.as_str()) {
            Some(n) => n,
            None => break,
        };

        if let Some(msg) = node.get("message") {
            if !msg.is_null() {
                let role = msg
                    .get("author")
                    .and_then(|a| a.get("role"))
                    .and_then(|r| r.as_str())
                    .unwrap_or("");
                let content = msg.get("content");
                let parts = content
                    .and_then(|c| c.as_object())
                    .and_then(|c| c.get("parts"))
                    .and_then(|p| p.as_array());

                let text = if let Some(parts) = parts {
                    parts
                        .iter()
                        .filter_map(|p| p.as_str())
                        .filter(|s| !s.is_empty())
                        .collect::<Vec<_>>()
                        .join(" ")
                        .trim()
                        .to_string()
                } else {
                    String::new()
                };

                if !text.is_empty() {
                    match role {
                        "user" => messages.push(("user".to_string(), text)),
                        "assistant" => messages.push(("assistant".to_string(), text)),
                        _ => {}
                    }
                }
            }
        }

        let children = node.get("children").and_then(|c| c.as_array());
        current_id = children
            .and_then(|c| c.first())
            .and_then(|c| c.as_str())
            .map(|s| s.to_string());
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn try_slack_json(data: &Value) -> Option<String> {
    let items = data.as_array()?;

    let mut messages: Vec<(String, String)> = Vec::new();
    let mut seen_users: HashMap<String, String> = HashMap::new();
    let mut last_role: Option<String> = None;

    for item in items {
        let obj = match item.as_object() {
            Some(o) => o,
            None => continue,
        };
        if obj.get("type").and_then(|v| v.as_str()) != Some("message") {
            continue;
        }
        let user_id = obj
            .get("user")
            .or_else(|| obj.get("username"))
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let text = obj
            .get("text")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string();

        if text.is_empty() || user_id.is_empty() {
            continue;
        }

        if !seen_users.contains_key(user_id) {
            let role = if seen_users.is_empty() {
                "user".to_string()
            } else if last_role.as_deref() == Some("user") {
                "assistant".to_string()
            } else {
                "user".to_string()
            };
            seen_users.insert(user_id.to_string(), role);
        }

        let role = seen_users[user_id].clone();
        last_role = Some(role.clone());
        messages.push((role, text));
    }

    if messages.len() >= 2 {
        Some(messages_to_transcript(&messages))
    } else {
        None
    }
}

fn extract_content(content: &Value) -> String {
    match content {
        Value::String(s) => s.trim().to_string(),
        Value::Array(arr) => {
            let parts: Vec<String> = arr
                .iter()
                .filter_map(|item| {
                    if let Some(s) = item.as_str() {
                        Some(s.to_string())
                    } else if let Some(obj) = item.as_object() {
                        if obj.get("type").and_then(|v| v.as_str()) == Some("text") {
                            obj.get("text")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string())
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                })
                .collect();
            parts.join(" ").trim().to_string()
        }
        Value::Object(obj) => obj
            .get("text")
            .or_else(|| obj.get("content"))
            .or_else(|| obj.get("value"))
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .trim()
            .to_string(),
        _ => String::new(),
    }
}

fn messages_to_transcript(messages: &[(String, String)]) -> String {
    let mut lines: Vec<String> = Vec::new();
    let mut i = 0;

    while i < messages.len() {
        let (ref role, ref text) = messages[i];
        if role == "user" {
            lines.push(format!("> {}", text));
            if i + 1 < messages.len() && messages[i + 1].0 == "assistant" {
                lines.push(messages[i + 1].1.clone());
                i += 2;
            } else {
                i += 1;
            }
        } else {
            lines.push(text.clone());
            i += 1;
        }
        lines.push(String::new());
    }

    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn write_temp_file(name: &str, content: &str) -> String {
        let dir = std::env::temp_dir().join("mempalace_test_normalize");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut f = std::fs::File::create(&path).unwrap();
        f.write_all(content.as_bytes()).unwrap();
        path.to_string_lossy().to_string()
    }

    #[test]
    fn test_plain_text_passthrough() {
        let content = "Hello world.\nThis is plain text.\nNothing special here.";
        let path = write_temp_file("plain.txt", content);
        let result = normalize(&path).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_empty_file() {
        let path = write_temp_file("empty.txt", "");
        let result = normalize(&path).unwrap();
        assert_eq!(result, "");
    }

    #[test]
    fn test_whitespace_only_file() {
        let path = write_temp_file("whitespace.txt", "   \n  \n  ");
        let result = normalize(&path).unwrap();
        assert_eq!(result, "   \n  \n  ");
    }

    #[test]
    fn test_already_has_quote_markers() {
        let content =
            "> question 1\nanswer 1\n\n> question 2\nanswer 2\n\n> question 3\nanswer 3\n";
        let path = write_temp_file("quoted.txt", content);
        let result = normalize(&path).unwrap();
        assert_eq!(result, content);
    }

    #[test]
    fn test_file_not_found() {
        let result = normalize("/nonexistent/path/to/file.txt");
        assert!(result.is_err());
    }

    #[test]
    fn test_claude_code_jsonl() {
        let content = r#"{"type":"human","message":{"content":"What is Rust?"}}
{"type":"assistant","message":{"content":"Rust is a systems programming language."}}
{"type":"human","message":{"content":"Why use it?"}}
{"type":"assistant","message":{"content":"For memory safety without garbage collection."}}"#;
        let path = write_temp_file("claude_code.jsonl", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> What is Rust?"));
        assert!(result.contains("Rust is a systems programming language."));
        assert!(result.contains("> Why use it?"));
        assert!(result.contains("For memory safety without garbage collection."));
    }

    #[test]
    fn test_claude_code_jsonl_with_block_content() {
        let content = r#"{"type":"human","message":{"content":[{"type":"text","text":"Hello there"}]}}
{"type":"assistant","message":{"content":[{"type":"text","text":"Hi! How can I help?"}]}}"#;
        let path = write_temp_file("claude_code_blocks.jsonl", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Hello there"));
        assert!(result.contains("Hi! How can I help?"));
    }

    #[test]
    fn test_claude_ai_json_messages_key() {
        let content = r#"{"messages":[
            {"role":"user","content":"Explain monads"},
            {"role":"assistant","content":"Monads are a design pattern."}
        ]}"#;
        let path = write_temp_file("claude_ai.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Explain monads"));
        assert!(result.contains("Monads are a design pattern."));
    }

    #[test]
    fn test_claude_ai_json_array_format() {
        let content = r#"[
            {"role":"human","content":"What is FP?"},
            {"role":"ai","content":"Functional programming is a paradigm."}
        ]"#;
        let path = write_temp_file("claude_ai_arr.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> What is FP?"));
        assert!(result.contains("Functional programming is a paradigm."));
    }

    #[test]
    fn test_chatgpt_json() {
        let content = r#"{
            "mapping": {
                "root": {
                    "parent": null,
                    "message": null,
                    "children": ["msg1"]
                },
                "msg1": {
                    "parent": "root",
                    "message": {
                        "author": {"role": "user"},
                        "content": {"parts": ["Tell me about trees"]}
                    },
                    "children": ["msg2"]
                },
                "msg2": {
                    "parent": "msg1",
                    "message": {
                        "author": {"role": "assistant"},
                        "content": {"parts": ["Trees are perennial plants."]}
                    },
                    "children": []
                }
            }
        }"#;
        let path = write_temp_file("chatgpt.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Tell me about trees"));
        assert!(result.contains("Trees are perennial plants."));
    }

    #[test]
    fn test_slack_json() {
        let content = r#"[
            {"type":"message","user":"U001","text":"Hey, how's the project?"},
            {"type":"message","user":"U002","text":"Going well, almost done."},
            {"type":"message","user":"U001","text":"Great, when's the deadline?"},
            {"type":"message","user":"U002","text":"Next Friday."}
        ]"#;
        let path = write_temp_file("slack.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Hey, how's the project?"));
        assert!(result.contains("Going well, almost done."));
        assert!(result.contains("> Great, when's the deadline?"));
        assert!(result.contains("Next Friday."));
    }

    #[test]
    fn test_slack_skips_non_message_types() {
        let content = r#"[
            {"type":"message","user":"U001","text":"Hello"},
            {"type":"channel_join","user":"U003","text":"joined"},
            {"type":"message","user":"U002","text":"Hi there"}
        ]"#;
        let path = write_temp_file("slack_filtered.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Hello"));
        assert!(result.contains("Hi there"));
        assert!(!result.contains("joined"));
    }

    #[test]
    fn test_malformed_json() {
        let content = r#"{"this is not valid json"#;
        let path = write_temp_file("bad.json", content);
        let result = normalize(&path).unwrap();
        // Falls through to passthrough since JSON parsing fails
        assert_eq!(result, content);
    }

    #[test]
    fn test_json_with_insufficient_messages() {
        let content = r#"[{"role":"user","content":"Hello"}]"#;
        let path = write_temp_file("single_msg.json", content);
        let result = normalize(&path).unwrap();
        // Only one message, not enough for transcript — passes through
        assert_eq!(result, content);
    }

    #[test]
    fn test_extract_content_string() {
        let val = Value::String("hello world".to_string());
        assert_eq!(extract_content(&val), "hello world");
    }

    #[test]
    fn test_extract_content_array_of_blocks() {
        let val = serde_json::json!([
            {"type": "text", "text": "first"},
            {"type": "image", "url": "http://example.com"},
            {"type": "text", "text": "second"}
        ]);
        assert_eq!(extract_content(&val), "first second");
    }

    #[test]
    fn test_extract_content_array_of_strings() {
        let val = serde_json::json!(["hello", "world"]);
        assert_eq!(extract_content(&val), "hello world");
    }

    #[test]
    fn test_extract_content_dict_with_text() {
        let val = serde_json::json!({"text": "some content"});
        assert_eq!(extract_content(&val), "some content");
    }

    #[test]
    fn test_extract_content_null() {
        assert_eq!(extract_content(&Value::Null), "");
    }

    #[test]
    fn test_messages_to_transcript_pairing() {
        let messages = vec![
            ("user".to_string(), "Q1".to_string()),
            ("assistant".to_string(), "A1".to_string()),
            ("user".to_string(), "Q2".to_string()),
            ("assistant".to_string(), "A2".to_string()),
        ];
        let result = messages_to_transcript(&messages);
        assert_eq!(result, "> Q1\nA1\n\n> Q2\nA2\n");
    }

    #[test]
    fn test_messages_to_transcript_unpaired_user() {
        let messages = vec![
            ("user".to_string(), "Q1".to_string()),
            ("user".to_string(), "Q2".to_string()),
            ("assistant".to_string(), "A2".to_string()),
        ];
        let result = messages_to_transcript(&messages);
        assert!(result.contains("> Q1\n\n"));
        assert!(result.contains("> Q2\nA2\n"));
    }

    #[test]
    fn test_messages_to_transcript_leading_assistant() {
        let messages = vec![
            ("assistant".to_string(), "Prelude".to_string()),
            ("user".to_string(), "Question".to_string()),
            ("assistant".to_string(), "Answer".to_string()),
        ];
        let result = messages_to_transcript(&messages);
        assert!(result.starts_with("Prelude\n\n"));
        assert!(result.contains("> Question\nAnswer\n"));
    }

    #[test]
    fn test_chatgpt_fallback_root() {
        // Root node has a message (no synthetic root) — should use fallback
        let content = r#"{
            "mapping": {
                "root": {
                    "parent": null,
                    "message": {
                        "author": {"role": "system"},
                        "content": {"parts": ["System prompt"]}
                    },
                    "children": ["msg1"]
                },
                "msg1": {
                    "parent": "root",
                    "message": {
                        "author": {"role": "user"},
                        "content": {"parts": ["Hi"]}
                    },
                    "children": ["msg2"]
                },
                "msg2": {
                    "parent": "msg1",
                    "message": {
                        "author": {"role": "assistant"},
                        "content": {"parts": ["Hello!"]}
                    },
                    "children": []
                }
            }
        }"#;
        let path = write_temp_file("chatgpt_fallback.json", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Hi"));
        assert!(result.contains("Hello!"));
    }

    #[test]
    fn test_json_content_detected_without_extension() {
        // File with .txt extension but JSON content should still be detected
        let content = r#"[
            {"role":"user","content":"Test question"},
            {"role":"assistant","content":"Test answer"}
        ]"#;
        let path = write_temp_file("sneaky.txt", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Test question"));
        assert!(result.contains("Test answer"));
    }

    #[test]
    fn test_claude_code_jsonl_skips_invalid_lines() {
        let content = r#"{"type":"human","message":{"content":"Valid question"}}
not valid json at all
{"type":"assistant","message":{"content":"Valid answer"}}"#;
        let path = write_temp_file("partial_jsonl.jsonl", content);
        let result = normalize(&path).unwrap();
        assert!(result.contains("> Valid question"));
        assert!(result.contains("Valid answer"));
    }
}

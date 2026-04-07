//! Integration test: MCP server protocol handling
//!
//! Tests the JSON-RPC protocol handler for the MCP server,
//! verifying initialize, tools/list, and tools/call responses.

use mempalace::config::MempalaceConfig;
use mempalace::knowledge_graph::KnowledgeGraph;
use mempalace::mcp_server;
use serde_json::json;
use tempfile::TempDir;

fn setup() -> (TempDir, MempalaceConfig, KnowledgeGraph) {
    let tmp = TempDir::new().unwrap();
    let palace_path = tmp.path().join("palace.sqlite3");

    // Create a minimal config pointing to our tmp palace
    std::fs::create_dir_all(tmp.path().join(".mempalace")).unwrap();
    let config_json = json!({
        "palace_path": palace_path.to_str().unwrap(),
    });
    std::fs::write(
        tmp.path().join(".mempalace").join("config.json"),
        config_json.to_string(),
    )
    .unwrap();

    let config = MempalaceConfig::new(Some(tmp.path().join(".mempalace").as_path()));
    let kg_path = tmp.path().join("kg.sqlite3");
    let kg = KnowledgeGraph::new(Some(kg_path.to_str().unwrap())).unwrap();

    (tmp, config, kg)
}

#[test]
fn test_mcp_initialize() {
    let (_tmp, config, kg) = setup();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 1,
        "method": "initialize",
        "params": {}
    });

    let response = mcp_server::handle_request(&request, &config, &kg);
    assert!(response.is_some());
    let resp = response.unwrap();
    assert_eq!(resp["jsonrpc"], "2.0");
    assert_eq!(resp["id"], 1);
    assert!(resp["result"]["capabilities"].is_object());
    assert!(resp["result"]["serverInfo"]["name"]
        .as_str()
        .unwrap()
        .contains("mempalace"));
}

#[test]
fn test_mcp_tools_list() {
    let (_tmp, config, kg) = setup();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 2,
        "method": "tools/list",
        "params": {}
    });

    let response = mcp_server::handle_request(&request, &config, &kg);
    assert!(response.is_some());
    let resp = response.unwrap();
    let tools = resp["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 19, "Expected 19 MCP tools");

    // Check a few key tools are present
    let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
    assert!(names.contains(&"mempalace_status"));
    assert!(names.contains(&"mempalace_search"));
    assert!(names.contains(&"mempalace_add_drawer"));
    assert!(names.contains(&"mempalace_kg_query"));
    assert!(names.contains(&"mempalace_diary_write"));
}

#[test]
fn test_mcp_tools_call_status() {
    let (_tmp, config, kg) = setup();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 3,
        "method": "tools/call",
        "params": {
            "name": "mempalace_status",
            "arguments": {}
        }
    });

    let response = mcp_server::handle_request(&request, &config, &kg);
    assert!(response.is_some());
    let resp = response.unwrap();
    // Should have result with content
    assert!(resp["result"].is_object());
}

#[test]
fn test_mcp_tools_call_add_and_search() {
    let (_tmp, config, kg) = setup();

    // Add a drawer
    let add_request = json!({
        "jsonrpc": "2.0",
        "id": 4,
        "method": "tools/call",
        "params": {
            "name": "mempalace_add_drawer",
            "arguments": {
                "wing": "test_wing",
                "room": "test_room",
                "content": "Rust is an amazing systems programming language with memory safety",
                "source_file": "test.md",
                "added_by": "mcp_test"
            }
        }
    });

    let add_resp = mcp_server::handle_request(&add_request, &config, &kg);
    assert!(add_resp.is_some());
    let add_result = add_resp.unwrap();
    let content = add_result["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(content).unwrap_or(json!({}));
    assert_eq!(parsed["success"], true);

    // Search for it
    let search_request = json!({
        "jsonrpc": "2.0",
        "id": 5,
        "method": "tools/call",
        "params": {
            "name": "mempalace_search",
            "arguments": {
                "query": "Rust programming",
                "limit": 5
            }
        }
    });

    let search_resp = mcp_server::handle_request(&search_request, &config, &kg);
    assert!(search_resp.is_some());
}

#[test]
fn test_mcp_tools_call_kg_operations() {
    let (_tmp, config, kg) = setup();

    // Add a triple
    let add_request = json!({
        "jsonrpc": "2.0",
        "id": 6,
        "method": "tools/call",
        "params": {
            "name": "mempalace_kg_add",
            "arguments": {
                "subject": "Alice",
                "predicate": "works_on",
                "object": "MemPalace"
            }
        }
    });

    let resp = mcp_server::handle_request(&add_request, &config, &kg).unwrap();
    let content = resp["result"]["content"][0]["text"].as_str().unwrap_or("");
    let parsed: serde_json::Value = serde_json::from_str(content).unwrap_or(json!({}));
    assert_eq!(parsed["success"], true);

    // Query the KG
    let query_request = json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "tools/call",
        "params": {
            "name": "mempalace_kg_query",
            "arguments": {
                "entity": "Alice"
            }
        }
    });

    let query_resp = mcp_server::handle_request(&query_request, &config, &kg).unwrap();
    let query_content = query_resp["result"]["content"][0]["text"]
        .as_str()
        .unwrap_or("");
    let query_parsed: serde_json::Value = serde_json::from_str(query_content).unwrap_or(json!({}));
    assert!(query_parsed["count"].as_u64().unwrap() >= 1);

    // Get stats
    let stats_request = json!({
        "jsonrpc": "2.0",
        "id": 8,
        "method": "tools/call",
        "params": {
            "name": "mempalace_kg_stats",
            "arguments": {}
        }
    });

    let stats_resp = mcp_server::handle_request(&stats_request, &config, &kg).unwrap();
    assert!(stats_resp["result"].is_object());
}

#[test]
fn test_mcp_unknown_tool() {
    let (_tmp, config, kg) = setup();
    let request = json!({
        "jsonrpc": "2.0",
        "id": 99,
        "method": "tools/call",
        "params": {
            "name": "nonexistent_tool",
            "arguments": {}
        }
    });

    let response = mcp_server::handle_request(&request, &config, &kg);
    assert!(response.is_some());
    let resp = response.unwrap();
    // Should have error at top level
    assert!(
        resp["error"]["message"]
            .as_str()
            .unwrap_or("")
            .contains("Unknown"),
        "Unknown tool should return error, got: {}",
        resp
    );
}

#[test]
fn test_mcp_notification_ignored() {
    let (_tmp, config, kg) = setup();
    // Notifications have no id
    let request = json!({
        "jsonrpc": "2.0",
        "method": "notifications/initialized",
        "params": {}
    });

    let response = mcp_server::handle_request(&request, &config, &kg);
    // Notifications should return None (no response needed)
    assert!(response.is_none());
}

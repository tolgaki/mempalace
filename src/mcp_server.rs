use crate::config::MempalaceConfig;
use crate::error::Result;
use crate::knowledge_graph::KnowledgeGraph;
use crate::palace_graph;
use crate::searcher;
use crate::store::{DrawerMetadata, PalaceStore, WhereFilter};
use md5::{Digest, Md5};
use serde_json::{json, Value};
use std::collections::HashMap;
use std::io::{self, BufRead, Write};

pub const PALACE_PROTOCOL: &str = r#"IMPORTANT — MemPalace Memory Protocol:
1. ON WAKE-UP: Call mempalace_status to load palace overview + AAAK spec.
2. BEFORE RESPONDING about any person, project, or past event: call mempalace_kg_query or mempalace_search FIRST. Never guess — verify.
3. IF UNSURE about a fact: say "let me check" and query the palace.
4. AFTER EACH SESSION: call mempalace_diary_write to record what happened.
5. WHEN FACTS CHANGE: call mempalace_kg_invalidate on the old fact, mempalace_kg_add for the new one."#;

pub const AAAK_SPEC: &str = r#"AAAK is a compressed memory dialect for efficient storage.
FORMAT:
  ENTITIES: 3-letter uppercase codes. ALC=Alice, JOR=Jordan.
  EMOTIONS: *action markers*. *warm*=joy, *fierce*=determined.
  STRUCTURE: Pipe-separated fields. FAM: family | PROJ: projects.
  DATES: ISO format. IMPORTANCE: ★ to ★★★★★.
  HALLS: hall_facts, hall_events, hall_discoveries, hall_preferences, hall_advice.
  ROOMS: Hyphenated slugs (e.g., chromadb-setup, gpu-pricing).
Read AAAK naturally — expand codes mentally, treat *markers* as emotional context."#;

fn get_store(config: &MempalaceConfig, create: bool) -> Option<PalaceStore> {
    let store = PalaceStore::open(&config.palace_path()).ok()?;
    if create || store.count().unwrap_or(0) > 0 {
        Some(store)
    } else {
        // Check if tables exist
        Some(store)
    }
}

fn no_palace(config: &MempalaceConfig) -> Value {
    json!({
        "error": "No palace found",
        "palace_path": config.palace_path(),
        "hint": "Run: mempalace init <dir> && mempalace mine <dir>"
    })
}

// ==================== READ TOOLS ====================

fn tool_status(config: &MempalaceConfig) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let count = store.count().unwrap_or(0);
    let mut wings: HashMap<String, usize> = HashMap::new();
    let mut rooms: HashMap<String, usize> = HashMap::new();

    if let Ok(drawers) = store.get(None, None) {
        for d in &drawers {
            *wings.entry(d.metadata.wing.clone()).or_insert(0) += 1;
            *rooms.entry(d.metadata.room.clone()).or_insert(0) += 1;
        }
    }

    json!({
        "total_drawers": count,
        "wings": wings,
        "rooms": rooms,
        "palace_path": config.palace_path(),
        "protocol": PALACE_PROTOCOL,
        "aaak_dialect": AAAK_SPEC,
    })
}

fn tool_list_wings(config: &MempalaceConfig) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let mut wings: HashMap<String, usize> = HashMap::new();
    if let Ok(drawers) = store.get(None, None) {
        for d in &drawers {
            *wings.entry(d.metadata.wing.clone()).or_insert(0) += 1;
        }
    }
    json!({"wings": wings})
}

fn tool_list_rooms(config: &MempalaceConfig, wing: Option<&str>) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let filter = wing.map(|w| WhereFilter::Wing(w.to_string()));
    let mut rooms: HashMap<String, usize> = HashMap::new();
    if let Ok(drawers) = store.get(filter.as_ref(), None) {
        for d in &drawers {
            *rooms.entry(d.metadata.room.clone()).or_insert(0) += 1;
        }
    }
    json!({"wing": wing.unwrap_or("all"), "rooms": rooms})
}

fn tool_get_taxonomy(config: &MempalaceConfig) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let mut taxonomy: HashMap<String, HashMap<String, usize>> = HashMap::new();
    if let Ok(drawers) = store.get(None, None) {
        for d in &drawers {
            let wing_entry = taxonomy.entry(d.metadata.wing.clone()).or_default();
            *wing_entry.entry(d.metadata.room.clone()).or_insert(0) += 1;
        }
    }
    json!({"taxonomy": taxonomy})
}

fn tool_search(
    config: &MempalaceConfig,
    query: &str,
    limit: usize,
    wing: Option<&str>,
    room: Option<&str>,
) -> Value {
    match searcher::search_memories(query, &config.palace_path(), wing, room, limit) {
        Ok(v) => v,
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_check_duplicate(config: &MempalaceConfig, content: &str, threshold: f64) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    match store.query(content, 5, None) {
        Ok(results) => {
            let duplicates: Vec<Value> = results
                .iter()
                .filter(|r| r.score >= threshold)
                .map(|r| {
                    let preview = if r.content.len() > 200 {
                        format!("{}...", &r.content[..200])
                    } else {
                        r.content.clone()
                    };
                    json!({
                        "id": r.id,
                        "wing": r.metadata.wing,
                        "room": r.metadata.room,
                        "similarity": r.score,
                        "content": preview,
                    })
                })
                .collect();
            json!({
                "is_duplicate": !duplicates.is_empty(),
                "matches": duplicates,
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_get_aaak_spec() -> Value {
    json!({"aaak_spec": AAAK_SPEC})
}

// ==================== WRITE TOOLS ====================

fn tool_add_drawer(
    config: &MempalaceConfig,
    wing: &str,
    room: &str,
    content: &str,
    source_file: Option<&str>,
    added_by: Option<&str>,
) -> Value {
    let store = match get_store(config, true) {
        Some(s) => s,
        None => return no_palace(config),
    };

    // Duplicate check
    let dup = tool_check_duplicate(config, content, 0.9);
    if dup
        .get("is_duplicate")
        .and_then(|v| v.as_bool())
        .unwrap_or(false)
    {
        return json!({
            "success": false,
            "reason": "duplicate",
            "matches": dup.get("matches"),
        });
    }

    let now = chrono::Utc::now().to_rfc3339();
    let mut hasher = Md5::new();
    hasher.update(format!("{}{}", &content[..content.len().min(100)], &now));
    let hash = format!("{:x}", hasher.finalize());
    let drawer_id = format!("drawer_{}_{}_{}", wing, room, &hash[..16]);

    let metadata = DrawerMetadata {
        wing: wing.to_string(),
        room: room.to_string(),
        hall: None,
        chunk_index: 0,
        source_file: source_file.unwrap_or("").to_string(),
        date: None,
        importance: None,
        emotional_weight: None,
        added_by: Some(added_by.unwrap_or("mcp").to_string()),
        filed_at: Some(now),
        extra: HashMap::new(),
    };

    match store.add(&drawer_id, content, &metadata) {
        Ok(true) => json!({"success": true, "drawer_id": drawer_id, "wing": wing, "room": room}),
        Ok(false) => json!({"success": false, "error": "Drawer already exists"}),
        Err(e) => json!({"success": false, "error": e.to_string()}),
    }
}

fn tool_delete_drawer(config: &MempalaceConfig, drawer_id: &str) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    match store.delete(drawer_id) {
        Ok(true) => json!({"success": true, "drawer_id": drawer_id}),
        Ok(false) => json!({"success": false, "error": format!("Drawer not found: {}", drawer_id)}),
        Err(e) => json!({"success": false, "error": e.to_string()}),
    }
}

// ==================== KNOWLEDGE GRAPH TOOLS ====================

fn tool_kg_query(kg: &KnowledgeGraph, entity: &str, as_of: Option<&str>, direction: &str) -> Value {
    match kg.query_entity(entity, as_of, direction) {
        Ok(facts) => {
            json!({"entity": entity, "as_of": as_of, "facts": facts, "count": facts.len()})
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_kg_add(
    kg: &KnowledgeGraph,
    subject: &str,
    predicate: &str,
    object: &str,
    valid_from: Option<&str>,
    source_closet: Option<&str>,
) -> Value {
    match kg.add_triple(
        subject,
        predicate,
        object,
        valid_from,
        None,
        Some(1.0),
        source_closet,
        None,
    ) {
        Ok(id) => json!({
            "success": true,
            "triple_id": id,
            "fact": format!("{} → {} → {}", subject, predicate, object),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_kg_invalidate(
    kg: &KnowledgeGraph,
    subject: &str,
    predicate: &str,
    object: &str,
    ended: Option<&str>,
) -> Value {
    match kg.invalidate(subject, predicate, object, ended) {
        Ok(()) => json!({
            "success": true,
            "fact": format!("{} → {} → {}", subject, predicate, object),
            "ended": ended.unwrap_or("today"),
        }),
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_kg_timeline(kg: &KnowledgeGraph, entity: Option<&str>) -> Value {
    match kg.timeline(entity) {
        Ok(results) => {
            json!({"entity": entity.unwrap_or("all"), "timeline": results, "count": results.len()})
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_kg_stats(kg: &KnowledgeGraph) -> Value {
    match kg.stats() {
        Ok(s) => s,
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ==================== NAVIGATION TOOLS ====================

fn tool_traverse_graph(config: &MempalaceConfig, start_room: &str, max_hops: usize) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    match palace_graph::traverse(start_room, &store, max_hops) {
        Ok(v) => v,
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_find_tunnels(
    config: &MempalaceConfig,
    wing_a: Option<&str>,
    wing_b: Option<&str>,
) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    match palace_graph::find_tunnels(wing_a, wing_b, &store) {
        Ok(v) => v,
        Err(e) => json!({"error": e.to_string()}),
    }
}

fn tool_graph_stats(config: &MempalaceConfig) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    match palace_graph::graph_stats(&store) {
        Ok(v) => v,
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ==================== DIARY TOOLS ====================

fn tool_diary_write(config: &MempalaceConfig, agent_name: &str, entry: &str, topic: &str) -> Value {
    let store = match get_store(config, true) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let wing = format!("wing_{}", agent_name.to_lowercase().replace(' ', "_"));
    let now = chrono::Utc::now();
    let mut hasher = Md5::new();
    hasher.update(&entry[..entry.len().min(50)]);
    let hash = format!("{:x}", hasher.finalize());
    let entry_id = format!(
        "diary_{}_{}_{}",
        wing,
        now.format("%Y%m%d_%H%M%S"),
        &hash[..8]
    );

    let mut extra = HashMap::new();
    extra.insert("topic".to_string(), topic.to_string());
    extra.insert("type".to_string(), "diary_entry".to_string());
    extra.insert("agent".to_string(), agent_name.to_string());

    let metadata = DrawerMetadata {
        wing: wing.clone(),
        room: "diary".into(),
        hall: Some("hall_diary".into()),
        chunk_index: 0,
        source_file: String::new(),
        date: Some(now.format("%Y-%m-%d").to_string()),
        importance: None,
        emotional_weight: None,
        added_by: Some(agent_name.to_string()),
        filed_at: Some(now.to_rfc3339()),
        extra,
    };

    match store.add(&entry_id, entry, &metadata) {
        Ok(_) => json!({
            "success": true,
            "entry_id": entry_id,
            "agent": agent_name,
            "topic": topic,
            "timestamp": now.to_rfc3339(),
        }),
        Err(e) => json!({"success": false, "error": e.to_string()}),
    }
}

fn tool_diary_read(config: &MempalaceConfig, agent_name: &str, last_n: usize) -> Value {
    let store = match get_store(config, false) {
        Some(s) => s,
        None => return no_palace(config),
    };
    let wing = format!("wing_{}", agent_name.to_lowercase().replace(' ', "_"));
    let filter = WhereFilter::WingAndRoom(wing, "diary".into());

    match store.get(Some(&filter), None) {
        Ok(drawers) => {
            let mut entries: Vec<Value> = drawers
                .iter()
                .map(|d| {
                    json!({
                        "date": d.metadata.date,
                        "timestamp": d.metadata.filed_at,
                        "topic": d.metadata.extra.get("topic").cloned().unwrap_or_default(),
                        "content": d.content,
                    })
                })
                .collect();
            entries.sort_by(|a, b| {
                let ta = a["timestamp"].as_str().unwrap_or("");
                let tb = b["timestamp"].as_str().unwrap_or("");
                tb.cmp(ta)
            });
            entries.truncate(last_n);
            let total = entries.len();
            json!({
                "agent": agent_name,
                "entries": entries,
                "total": total,
                "showing": entries.len(),
            })
        }
        Err(e) => json!({"error": e.to_string()}),
    }
}

// ==================== TOOL DEFINITIONS ====================

fn tool_definitions() -> Vec<Value> {
    vec![
        json!({"name": "mempalace_status", "description": "Palace overview — total drawers, wing and room counts", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_list_wings", "description": "List all wings with drawer counts", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_list_rooms", "description": "List rooms within a wing", "inputSchema": {"type": "object", "properties": {"wing": {"type": "string"}}}}),
        json!({"name": "mempalace_get_taxonomy", "description": "Full taxonomy: wing → room → drawer count", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_get_aaak_spec", "description": "Get the AAAK dialect specification", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_search", "description": "Semantic search. Returns verbatim drawer content.", "inputSchema": {"type": "object", "properties": {"query": {"type": "string"}, "limit": {"type": "integer"}, "wing": {"type": "string"}, "room": {"type": "string"}}, "required": ["query"]}}),
        json!({"name": "mempalace_check_duplicate", "description": "Check if content already exists", "inputSchema": {"type": "object", "properties": {"content": {"type": "string"}, "threshold": {"type": "number"}}, "required": ["content"]}}),
        json!({"name": "mempalace_add_drawer", "description": "File verbatim content into the palace", "inputSchema": {"type": "object", "properties": {"wing": {"type": "string"}, "room": {"type": "string"}, "content": {"type": "string"}, "source_file": {"type": "string"}, "added_by": {"type": "string"}}, "required": ["wing", "room", "content"]}}),
        json!({"name": "mempalace_delete_drawer", "description": "Delete a drawer by ID", "inputSchema": {"type": "object", "properties": {"drawer_id": {"type": "string"}}, "required": ["drawer_id"]}}),
        json!({"name": "mempalace_kg_query", "description": "Query knowledge graph for entity relationships", "inputSchema": {"type": "object", "properties": {"entity": {"type": "string"}, "as_of": {"type": "string"}, "direction": {"type": "string"}}, "required": ["entity"]}}),
        json!({"name": "mempalace_kg_add", "description": "Add a fact to the knowledge graph", "inputSchema": {"type": "object", "properties": {"subject": {"type": "string"}, "predicate": {"type": "string"}, "object": {"type": "string"}, "valid_from": {"type": "string"}, "source_closet": {"type": "string"}}, "required": ["subject", "predicate", "object"]}}),
        json!({"name": "mempalace_kg_invalidate", "description": "Mark a fact as no longer true", "inputSchema": {"type": "object", "properties": {"subject": {"type": "string"}, "predicate": {"type": "string"}, "object": {"type": "string"}, "ended": {"type": "string"}}, "required": ["subject", "predicate", "object"]}}),
        json!({"name": "mempalace_kg_timeline", "description": "Chronological timeline of facts", "inputSchema": {"type": "object", "properties": {"entity": {"type": "string"}}}}),
        json!({"name": "mempalace_kg_stats", "description": "Knowledge graph overview", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_traverse", "description": "Walk the palace graph from a room", "inputSchema": {"type": "object", "properties": {"start_room": {"type": "string"}, "max_hops": {"type": "integer"}}, "required": ["start_room"]}}),
        json!({"name": "mempalace_find_tunnels", "description": "Find rooms that bridge two wings", "inputSchema": {"type": "object", "properties": {"wing_a": {"type": "string"}, "wing_b": {"type": "string"}}}}),
        json!({"name": "mempalace_graph_stats", "description": "Palace graph overview", "inputSchema": {"type": "object", "properties": {}}}),
        json!({"name": "mempalace_diary_write", "description": "Write to your personal agent diary in AAAK format", "inputSchema": {"type": "object", "properties": {"agent_name": {"type": "string"}, "entry": {"type": "string"}, "topic": {"type": "string"}}, "required": ["agent_name", "entry"]}}),
        json!({"name": "mempalace_diary_read", "description": "Read your recent diary entries", "inputSchema": {"type": "object", "properties": {"agent_name": {"type": "string"}, "last_n": {"type": "integer"}}, "required": ["agent_name"]}}),
    ]
}

// ==================== REQUEST HANDLER ====================

pub fn handle_request(
    request: &Value,
    config: &MempalaceConfig,
    kg: &KnowledgeGraph,
) -> Option<Value> {
    let method = request["method"].as_str().unwrap_or("");
    let params = &request["params"];
    let req_id = &request["id"];

    match method {
        "initialize" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {
                "protocolVersion": "2024-11-05",
                "capabilities": {"tools": {}},
                "serverInfo": {"name": "mempalace", "version": "3.0.0"},
            }
        })),

        "notifications/initialized" => None,

        "tools/list" => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "result": {"tools": tool_definitions()}
        })),

        "tools/call" => {
            let tool_name = params["name"].as_str().unwrap_or("");
            let args = &params["arguments"];

            let result = match tool_name {
                "mempalace_status" => tool_status(config),
                "mempalace_list_wings" => tool_list_wings(config),
                "mempalace_list_rooms" => tool_list_rooms(config, args["wing"].as_str()),
                "mempalace_get_taxonomy" => tool_get_taxonomy(config),
                "mempalace_get_aaak_spec" => tool_get_aaak_spec(),
                "mempalace_search" => tool_search(
                    config,
                    args["query"].as_str().unwrap_or(""),
                    args["limit"].as_u64().unwrap_or(5) as usize,
                    args["wing"].as_str(),
                    args["room"].as_str(),
                ),
                "mempalace_check_duplicate" => tool_check_duplicate(
                    config,
                    args["content"].as_str().unwrap_or(""),
                    args["threshold"].as_f64().unwrap_or(0.9),
                ),
                "mempalace_add_drawer" => tool_add_drawer(
                    config,
                    args["wing"].as_str().unwrap_or(""),
                    args["room"].as_str().unwrap_or(""),
                    args["content"].as_str().unwrap_or(""),
                    args["source_file"].as_str(),
                    args["added_by"].as_str(),
                ),
                "mempalace_delete_drawer" => {
                    tool_delete_drawer(config, args["drawer_id"].as_str().unwrap_or(""))
                }
                "mempalace_kg_query" => tool_kg_query(
                    kg,
                    args["entity"].as_str().unwrap_or(""),
                    args["as_of"].as_str(),
                    args["direction"].as_str().unwrap_or("both"),
                ),
                "mempalace_kg_add" => tool_kg_add(
                    kg,
                    args["subject"].as_str().unwrap_or(""),
                    args["predicate"].as_str().unwrap_or(""),
                    args["object"].as_str().unwrap_or(""),
                    args["valid_from"].as_str(),
                    args["source_closet"].as_str(),
                ),
                "mempalace_kg_invalidate" => tool_kg_invalidate(
                    kg,
                    args["subject"].as_str().unwrap_or(""),
                    args["predicate"].as_str().unwrap_or(""),
                    args["object"].as_str().unwrap_or(""),
                    args["ended"].as_str(),
                ),
                "mempalace_kg_timeline" => tool_kg_timeline(kg, args["entity"].as_str()),
                "mempalace_kg_stats" => tool_kg_stats(kg),
                "mempalace_traverse" => tool_traverse_graph(
                    config,
                    args["start_room"].as_str().unwrap_or(""),
                    args["max_hops"].as_u64().unwrap_or(2) as usize,
                ),
                "mempalace_find_tunnels" => {
                    tool_find_tunnels(config, args["wing_a"].as_str(), args["wing_b"].as_str())
                }
                "mempalace_graph_stats" => tool_graph_stats(config),
                "mempalace_diary_write" => tool_diary_write(
                    config,
                    args["agent_name"].as_str().unwrap_or(""),
                    args["entry"].as_str().unwrap_or(""),
                    args["topic"].as_str().unwrap_or("general"),
                ),
                "mempalace_diary_read" => tool_diary_read(
                    config,
                    args["agent_name"].as_str().unwrap_or(""),
                    args["last_n"].as_u64().unwrap_or(10) as usize,
                ),
                _ => {
                    return Some(json!({
                        "jsonrpc": "2.0",
                        "id": req_id,
                        "error": {"code": -32601, "message": format!("Unknown tool: {}", tool_name)}
                    }))
                }
            };

            Some(json!({
                "jsonrpc": "2.0",
                "id": req_id,
                "result": {"content": [{"type": "text", "text": serde_json::to_string_pretty(&result).unwrap_or_default()}]}
            }))
        }

        _ => Some(json!({
            "jsonrpc": "2.0",
            "id": req_id,
            "error": {"code": -32601, "message": format!("Unknown method: {}", method)}
        })),
    }
}

/// Run the MCP server on stdin/stdout.
pub fn run_server() -> Result<()> {
    let config = MempalaceConfig::new(None);
    let kg = KnowledgeGraph::new(None)?;

    eprintln!("MemPalace MCP Server starting...");

    let stdin = io::stdin();
    let stdout = io::stdout();

    for line in stdin.lock().lines() {
        let line = match line {
            Ok(l) => l,
            Err(_) => break,
        };
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: Value = match serde_json::from_str(trimmed) {
            Ok(v) => v,
            Err(e) => {
                eprintln!("JSON parse error: {}", e);
                continue;
            }
        };

        if let Some(response) = handle_request(&request, &config, &kg) {
            let mut out = stdout.lock();
            writeln!(
                out,
                "{}",
                serde_json::to_string(&response).unwrap_or_default()
            )?;
            out.flush()?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_config() -> (TempDir, MempalaceConfig) {
        let tmp = TempDir::new().unwrap();
        std::env::set_var(
            "MEMPALACE_PALACE_PATH",
            tmp.path().join("palace").to_str().unwrap(),
        );
        let config = MempalaceConfig::new(Some(tmp.path()));
        (tmp, config)
    }

    #[test]
    fn test_handle_initialize() {
        let (_tmp, config) = test_config();
        let kg = KnowledgeGraph::new(Some(
            config.config_dir().join("kg.sqlite3").to_str().unwrap(),
        ))
        .unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 1, "method": "initialize", "params": {}});
        let resp = handle_request(&req, &config, &kg).unwrap();
        assert_eq!(resp["result"]["serverInfo"]["name"], "mempalace");
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_handle_tools_list() {
        let (_tmp, config) = test_config();
        let kg = KnowledgeGraph::new(Some(
            config.config_dir().join("kg.sqlite3").to_str().unwrap(),
        ))
        .unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 2, "method": "tools/list", "params": {}});
        let resp = handle_request(&req, &config, &kg).unwrap();
        let tools = resp["result"]["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 19);
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_handle_unknown_method() {
        let (_tmp, config) = test_config();
        let kg = KnowledgeGraph::new(Some(
            config.config_dir().join("kg.sqlite3").to_str().unwrap(),
        ))
        .unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 3, "method": "unknown/method", "params": {}});
        let resp = handle_request(&req, &config, &kg).unwrap();
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown method"));
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_handle_unknown_tool() {
        let (_tmp, config) = test_config();
        let kg = KnowledgeGraph::new(Some(
            config.config_dir().join("kg.sqlite3").to_str().unwrap(),
        ))
        .unwrap();
        let req = json!({"jsonrpc": "2.0", "id": 4, "method": "tools/call", "params": {"name": "nonexistent", "arguments": {}}});
        let resp = handle_request(&req, &config, &kg).unwrap();
        assert!(resp["error"]["message"]
            .as_str()
            .unwrap()
            .contains("Unknown tool"));
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_notifications_initialized_returns_none() {
        let (_tmp, config) = test_config();
        let kg = KnowledgeGraph::new(Some(
            config.config_dir().join("kg.sqlite3").to_str().unwrap(),
        ))
        .unwrap();
        let req = json!({"jsonrpc": "2.0", "method": "notifications/initialized"});
        assert!(handle_request(&req, &config, &kg).is_none());
        std::env::remove_var("MEMPALACE_PALACE_PATH");
    }

    #[test]
    fn test_tool_get_aaak_spec() {
        let result = tool_get_aaak_spec();
        assert!(result["aaak_spec"].as_str().unwrap().contains("AAAK"));
    }

    #[test]
    fn test_tool_status_no_palace() {
        let tmp = TempDir::new().unwrap();
        let config = MempalaceConfig::new(Some(tmp.path()));
        let result = tool_status(&config);
        // Should return either error or zero drawers (store creates on open)
        assert!(result.get("total_drawers").is_some() || result.get("error").is_some());
    }

    #[test]
    fn test_tool_definitions_count() {
        let defs = tool_definitions();
        assert_eq!(defs.len(), 19);
    }

    #[test]
    fn test_tool_kg_query() {
        let tmp = TempDir::new().unwrap();
        let kg =
            KnowledgeGraph::new(Some(tmp.path().join("kg.sqlite3").to_str().unwrap())).unwrap();
        kg.add_triple(
            "Alice",
            "loves",
            "chess",
            Some("2025-01-01"),
            None,
            Some(1.0),
            None,
            None,
        )
        .unwrap();
        let result = tool_kg_query(&kg, "Alice", None, "both");
        assert!(result["count"].as_u64().unwrap() >= 1);
    }

    #[test]
    fn test_tool_kg_stats() {
        let tmp = TempDir::new().unwrap();
        let kg =
            KnowledgeGraph::new(Some(tmp.path().join("kg.sqlite3").to_str().unwrap())).unwrap();
        let result = tool_kg_stats(&kg);
        assert!(result.get("entities").is_some());
    }

    #[test]
    fn test_palace_protocol_content() {
        assert!(PALACE_PROTOCOL.contains("WAKE-UP"));
        assert!(PALACE_PROTOCOL.contains("mempalace_status"));
    }
}

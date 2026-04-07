//! palace_graph.rs — Graph traversal layer for MemPalace
//!
//! Builds a navigable graph from the palace structure:
//!   - Nodes = rooms (named ideas)
//!   - Edges = shared rooms across wings (tunnels)
//!   - Edge types = halls (the corridors)
//!
//! No external graph DB needed — built from SQLite metadata.

use std::collections::{HashMap, HashSet, VecDeque};

use crate::error::Result;
use crate::store::PalaceStore;

// ── Data types ─────────────────────────────────────────────────────────────

/// A node in the palace graph, representing a room.
#[derive(Debug, Clone)]
pub struct RoomNode {
    pub wings: Vec<String>,
    pub halls: Vec<String>,
    pub count: usize,
    pub dates: Vec<String>,
}

/// An edge in the palace graph, connecting two wings through a shared room.
#[derive(Debug, Clone)]
pub struct Edge {
    pub room: String,
    pub wing_a: String,
    pub wing_b: String,
    pub hall: String,
    pub count: usize,
}

// ── Build graph ────────────────────────────────────────────────────────────

/// Build the palace graph from drawer metadata.
///
/// Returns `(nodes, edges)` where nodes map room names to `RoomNode` and
/// edges represent tunnels between wings through shared rooms.
pub fn build_graph(store: &PalaceStore) -> Result<(HashMap<String, RoomNode>, Vec<Edge>)> {
    let drawers = store.get(None, None)?;

    // Accumulate per-room data
    let mut room_wings: HashMap<String, HashSet<String>> = HashMap::new();
    let mut room_halls: HashMap<String, HashSet<String>> = HashMap::new();
    let mut room_dates: HashMap<String, HashSet<String>> = HashMap::new();
    let mut room_counts: HashMap<String, usize> = HashMap::new();

    for drawer in &drawers {
        let room = &drawer.metadata.room;
        let wing = &drawer.metadata.wing;

        if room.is_empty() || room == "general" || wing.is_empty() {
            continue;
        }

        room_wings
            .entry(room.clone())
            .or_default()
            .insert(wing.clone());

        if let Some(ref hall) = drawer.metadata.hall {
            if !hall.is_empty() {
                room_halls
                    .entry(room.clone())
                    .or_default()
                    .insert(hall.clone());
            }
        }

        if let Some(ref date) = drawer.metadata.date {
            if !date.is_empty() {
                room_dates
                    .entry(room.clone())
                    .or_default()
                    .insert(date.clone());
            }
        }

        *room_counts.entry(room.clone()).or_default() += 1;
    }

    // Build nodes
    let mut nodes: HashMap<String, RoomNode> = HashMap::new();
    for (room, wings_set) in &room_wings {
        let mut wings: Vec<String> = wings_set.iter().cloned().collect();
        wings.sort();

        let mut halls: Vec<String> = room_halls
            .get(room)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        halls.sort();

        let mut dates: Vec<String> = room_dates
            .get(room)
            .map(|s| s.iter().cloned().collect())
            .unwrap_or_default();
        dates.sort();
        // Keep only the last 5 dates
        if dates.len() > 5 {
            dates = dates[dates.len() - 5..].to_vec();
        }

        let count = room_counts.get(room).copied().unwrap_or(0);

        nodes.insert(
            room.clone(),
            RoomNode {
                wings,
                halls,
                count,
                dates,
            },
        );
    }

    // Build edges from rooms that span multiple wings
    let mut edges = Vec::new();
    for (room, node) in &nodes {
        if node.wings.len() < 2 {
            continue;
        }
        for (i, wa) in node.wings.iter().enumerate() {
            for wb in &node.wings[i + 1..] {
                if node.halls.is_empty() {
                    // Still record the edge even without a hall
                    edges.push(Edge {
                        room: room.clone(),
                        wing_a: wa.clone(),
                        wing_b: wb.clone(),
                        hall: String::new(),
                        count: node.count,
                    });
                } else {
                    for hall in &node.halls {
                        edges.push(Edge {
                            room: room.clone(),
                            wing_a: wa.clone(),
                            wing_b: wb.clone(),
                            hall: hall.clone(),
                            count: node.count,
                        });
                    }
                }
            }
        }
    }

    Ok((nodes, edges))
}

// ── Traversal ──────────────────────────────────────────────────────────────

/// BFS traversal from a starting room. Finds connected rooms through shared wings.
/// Returns a JSON array of visited rooms with hop distances.
pub fn traverse(
    start_room: &str,
    store: &PalaceStore,
    max_hops: usize,
) -> Result<serde_json::Value> {
    let (nodes, _) = build_graph(store)?;

    if !nodes.contains_key(start_room) {
        let suggestions = fuzzy_match(start_room, &nodes, 5);
        return Ok(serde_json::json!({
            "error": format!("Room '{}' not found", start_room),
            "suggestions": suggestions,
        }));
    }

    let start = &nodes[start_room];
    let mut visited: HashSet<String> = HashSet::new();
    visited.insert(start_room.to_string());

    let mut results = vec![serde_json::json!({
        "room": start_room,
        "wings": start.wings,
        "halls": start.halls,
        "count": start.count,
        "hop": 0,
    })];

    let mut frontier: VecDeque<(String, usize)> = VecDeque::new();
    frontier.push_back((start_room.to_string(), 0));

    while let Some((current_room, depth)) = frontier.pop_front() {
        if depth >= max_hops {
            continue;
        }

        let current_wings: HashSet<String> = nodes
            .get(&current_room)
            .map(|n| n.wings.iter().cloned().collect())
            .unwrap_or_default();

        for (room, data) in &nodes {
            if visited.contains(room) {
                continue;
            }

            let room_wings: HashSet<String> = data.wings.iter().cloned().collect();
            let shared: Vec<String> = current_wings.intersection(&room_wings).cloned().collect();

            if !shared.is_empty() {
                visited.insert(room.clone());
                let entry = serde_json::json!({
                    "room": room,
                    "wings": data.wings,
                    "halls": data.halls,
                    "count": data.count,
                    "hop": depth + 1,
                    "connected_via": shared,
                });
                results.push(entry);

                if depth + 1 < max_hops {
                    frontier.push_back((room.clone(), depth + 1));
                }
            }
        }
    }

    // Sort by hop distance then by count descending
    results.sort_by(|a, b| {
        let hop_a = a["hop"].as_u64().unwrap_or(0);
        let hop_b = b["hop"].as_u64().unwrap_or(0);
        let count_a = a["count"].as_u64().unwrap_or(0);
        let count_b = b["count"].as_u64().unwrap_or(0);
        hop_a.cmp(&hop_b).then(count_b.cmp(&count_a))
    });

    // Cap at 50 results
    results.truncate(50);
    Ok(serde_json::json!(results))
}

// ── Find tunnels ───────────────────────────────────────────────────────────

/// Find rooms that connect two wings (tunnels).
/// If no wings are specified, returns all multi-wing rooms.
pub fn find_tunnels(
    wing_a: Option<&str>,
    wing_b: Option<&str>,
    store: &PalaceStore,
) -> Result<serde_json::Value> {
    let (nodes, _) = build_graph(store)?;

    let mut tunnels: Vec<serde_json::Value> = Vec::new();
    for (room, data) in &nodes {
        if data.wings.len() < 2 {
            continue;
        }

        if let Some(wa) = wing_a {
            if !data.wings.contains(&wa.to_string()) {
                continue;
            }
        }
        if let Some(wb) = wing_b {
            if !data.wings.contains(&wb.to_string()) {
                continue;
            }
        }

        let recent = data.dates.last().cloned().unwrap_or_default();
        tunnels.push(serde_json::json!({
            "room": room,
            "wings": data.wings,
            "halls": data.halls,
            "count": data.count,
            "recent": recent,
        }));
    }

    tunnels.sort_by(|a, b| {
        let ca = a["count"].as_u64().unwrap_or(0);
        let cb = b["count"].as_u64().unwrap_or(0);
        cb.cmp(&ca)
    });

    tunnels.truncate(50);
    Ok(serde_json::json!(tunnels))
}

// ── Graph stats ────────────────────────────────────────────────────────────

/// Summary statistics about the palace graph.
pub fn graph_stats(store: &PalaceStore) -> Result<serde_json::Value> {
    let (nodes, edges) = build_graph(store)?;

    let tunnel_rooms = nodes.values().filter(|n| n.wings.len() >= 2).count();

    let mut wing_counts: HashMap<String, usize> = HashMap::new();
    for data in nodes.values() {
        for w in &data.wings {
            *wing_counts.entry(w.clone()).or_default() += 1;
        }
    }

    // Sort wings by count descending
    let mut wing_pairs: Vec<(String, usize)> = wing_counts.into_iter().collect();
    wing_pairs.sort_by(|a, b| b.1.cmp(&a.1));
    let rooms_per_wing: serde_json::Value = wing_pairs
        .iter()
        .map(|(w, c)| (w.clone(), serde_json::json!(c)))
        .collect::<serde_json::Map<String, serde_json::Value>>()
        .into();

    // Top tunnels: rooms with the most wings
    let mut tunnel_list: Vec<(&String, &RoomNode)> =
        nodes.iter().filter(|(_, n)| n.wings.len() >= 2).collect();
    tunnel_list.sort_by(|a, b| b.1.wings.len().cmp(&a.1.wings.len()));
    tunnel_list.truncate(10);

    let top_tunnels: Vec<serde_json::Value> = tunnel_list
        .iter()
        .map(|(room, data)| {
            serde_json::json!({
                "room": room,
                "wings": data.wings,
                "count": data.count,
            })
        })
        .collect();

    Ok(serde_json::json!({
        "total_rooms": nodes.len(),
        "tunnel_rooms": tunnel_rooms,
        "total_edges": edges.len(),
        "rooms_per_wing": rooms_per_wing,
        "top_tunnels": top_tunnels,
    }))
}

// ── Fuzzy matching ─────────────────────────────────────────────────────────

/// Find rooms that approximately match a query string using substring matching.
pub fn fuzzy_match(query: &str, nodes: &HashMap<String, RoomNode>, n: usize) -> Vec<String> {
    let query_lower = query.to_lowercase();
    let mut scored: Vec<(String, f64)> = Vec::new();

    for room in nodes.keys() {
        let room_lower = room.to_lowercase();
        if room_lower.contains(&query_lower) || query_lower.contains(&room_lower) {
            scored.push((room.clone(), 1.0));
        } else {
            let query_parts: Vec<&str> = query_lower.split('-').collect();
            if query_parts.iter().any(|part| room_lower.contains(part))
                || room_lower.split('-').any(|part| query_lower.contains(part))
            {
                scored.push((room.clone(), 0.5));
            }
        }
    }

    scored.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap());
    scored.truncate(n);
    scored.into_iter().map(|(r, _)| r).collect()
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::store::{DrawerMetadata, PalaceStore};
    use std::collections::HashMap as StdHashMap;
    use tempfile::TempDir;

    fn meta(wing: &str, room: &str, hall: Option<&str>, date: Option<&str>) -> DrawerMetadata {
        DrawerMetadata {
            wing: wing.into(),
            room: room.into(),
            hall: hall.map(|s| s.into()),
            chunk_index: 0,
            source_file: "test.md".into(),
            date: date.map(|s| s.into()),
            importance: None,
            emotional_weight: None,
            added_by: None,
            filed_at: None,
            extra: StdHashMap::new(),
        }
    }

    fn setup_graph_store() -> (PalaceStore, TempDir) {
        let tmp = TempDir::new().unwrap();
        let store = PalaceStore::open(tmp.path().to_str().unwrap()).unwrap();

        // "rust" room appears in both "technical" and "myproject" wings => tunnel
        store
            .add(
                "d1",
                "rust basics",
                &meta("technical", "rust", Some("intro"), Some("2026-01-01")),
            )
            .unwrap();
        store
            .add(
                "d2",
                "rust in myproject",
                &meta("myproject", "rust", Some("impl"), Some("2026-02-01")),
            )
            .unwrap();

        // "python" only in "technical"
        store
            .add(
                "d3",
                "python basics",
                &meta("technical", "python", Some("intro"), None),
            )
            .unwrap();

        // "deployment" in both "myproject" and "devops" => tunnel
        store
            .add(
                "d4",
                "deploy myproject",
                &meta("myproject", "deployment", Some("ci"), Some("2026-03-01")),
            )
            .unwrap();
        store
            .add(
                "d5",
                "deploy infra",
                &meta("devops", "deployment", Some("ci"), Some("2026-03-15")),
            )
            .unwrap();

        // "feelings" only in "emotions"
        store
            .add(
                "d6",
                "feeling good today",
                &meta("emotions", "feelings", None, Some("2026-01-10")),
            )
            .unwrap();

        // "design" in "myproject" only
        store
            .add(
                "d7",
                "ui design",
                &meta("myproject", "design", Some("frontend"), None),
            )
            .unwrap();

        // Another drawer for "rust" room in "myproject" to bump count
        store
            .add(
                "d8",
                "more rust work",
                &meta("myproject", "rust", Some("impl"), Some("2026-02-15")),
            )
            .unwrap();

        (store, tmp)
    }

    #[test]
    fn test_build_graph_nodes() {
        let (store, _tmp) = setup_graph_store();
        let (nodes, _) = build_graph(&store).unwrap();
        assert!(nodes.contains_key("rust"));
        assert!(nodes.contains_key("python"));
        assert!(nodes.contains_key("deployment"));
        assert!(nodes.contains_key("feelings"));
    }

    #[test]
    fn test_build_graph_tunnel_edges() {
        let (store, _tmp) = setup_graph_store();
        let (_, edges) = build_graph(&store).unwrap();
        // "rust" and "deployment" are tunnels
        let rust_edges: Vec<_> = edges.iter().filter(|e| e.room == "rust").collect();
        assert!(!rust_edges.is_empty());
        let deploy_edges: Vec<_> = edges.iter().filter(|e| e.room == "deployment").collect();
        assert!(!deploy_edges.is_empty());
    }

    #[test]
    fn test_build_graph_skips_general_room() {
        let tmp = TempDir::new().unwrap();
        let store = PalaceStore::open(tmp.path().to_str().unwrap()).unwrap();
        store
            .add(
                "g1",
                "general content",
                &meta("tech", "general", None, None),
            )
            .unwrap();
        let (nodes, _) = build_graph(&store).unwrap();
        assert!(!nodes.contains_key("general"));
    }

    #[test]
    fn test_build_graph_counts() {
        let (store, _tmp) = setup_graph_store();
        let (nodes, _) = build_graph(&store).unwrap();
        // "rust" has 3 drawers (d1, d2, d8)
        assert_eq!(nodes["rust"].count, 3);
        // "python" has 1 drawer
        assert_eq!(nodes["python"].count, 1);
    }

    #[test]
    fn test_build_graph_wings_sorted() {
        let (store, _tmp) = setup_graph_store();
        let (nodes, _) = build_graph(&store).unwrap();
        let rust_wings = &nodes["rust"].wings;
        assert_eq!(rust_wings, &["myproject", "technical"]);
    }

    #[test]
    fn test_traverse_from_rust() {
        let (store, _tmp) = setup_graph_store();
        let result = traverse("rust", &store, 2).unwrap();
        let arr = result.as_array().unwrap();
        // Should find "rust" itself at hop 0
        assert!(arr.iter().any(|v| v["room"] == "rust" && v["hop"] == 0));
        // Should find rooms connected via shared wings (e.g., "python" via "technical",
        // "deployment" via "myproject", "design" via "myproject")
        let rooms: Vec<&str> = arr.iter().map(|v| v["room"].as_str().unwrap()).collect();
        assert!(
            rooms.contains(&"python") || rooms.contains(&"deployment") || rooms.contains(&"design")
        );
    }

    #[test]
    fn test_traverse_unknown_room_gives_suggestions() {
        let (store, _tmp) = setup_graph_store();
        let result = traverse("rusty", &store, 2).unwrap();
        assert!(result["error"].is_string());
        // "rusty" should fuzzy-match "rust"
        let suggestions = result["suggestions"].as_array().unwrap();
        assert!(suggestions.iter().any(|s| s.as_str().unwrap() == "rust"));
    }

    #[test]
    fn test_traverse_max_hops_zero() {
        let (store, _tmp) = setup_graph_store();
        let result = traverse("rust", &store, 0).unwrap();
        let arr = result.as_array().unwrap();
        // With max_hops=0, should only return the start room
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["room"].as_str().unwrap(), "rust");
    }

    #[test]
    fn test_find_tunnels_all() {
        let (store, _tmp) = setup_graph_store();
        let result = find_tunnels(None, None, &store).unwrap();
        let tunnels = result.as_array().unwrap();
        // "rust" and "deployment" are the tunnel rooms
        let rooms: Vec<&str> = tunnels
            .iter()
            .map(|t| t["room"].as_str().unwrap())
            .collect();
        assert!(rooms.contains(&"rust"));
        assert!(rooms.contains(&"deployment"));
        assert!(!rooms.contains(&"python")); // python is single-wing
    }

    #[test]
    fn test_find_tunnels_filtered_by_wing() {
        let (store, _tmp) = setup_graph_store();
        let result = find_tunnels(Some("devops"), None, &store).unwrap();
        let tunnels = result.as_array().unwrap();
        // Only "deployment" spans into devops
        assert_eq!(tunnels.len(), 1);
        assert_eq!(tunnels[0]["room"].as_str().unwrap(), "deployment");
    }

    #[test]
    fn test_find_tunnels_filtered_by_both_wings() {
        let (store, _tmp) = setup_graph_store();
        let result = find_tunnels(Some("technical"), Some("myproject"), &store).unwrap();
        let tunnels = result.as_array().unwrap();
        // Only "rust" is in both technical and myproject
        let rooms: Vec<&str> = tunnels
            .iter()
            .map(|t| t["room"].as_str().unwrap())
            .collect();
        assert!(rooms.contains(&"rust"));
        assert!(!rooms.contains(&"deployment"));
    }

    #[test]
    fn test_graph_stats() {
        let (store, _tmp) = setup_graph_store();
        let stats = graph_stats(&store).unwrap();
        assert_eq!(stats["total_rooms"].as_u64().unwrap(), 5); // rust, python, deployment, feelings, design
        assert_eq!(stats["tunnel_rooms"].as_u64().unwrap(), 2); // rust, deployment
        assert!(stats["total_edges"].as_u64().unwrap() >= 2);
        assert!(stats["rooms_per_wing"].is_object());
        assert!(stats["top_tunnels"].is_array());
    }

    #[test]
    fn test_fuzzy_match_exact_substring() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "rust-setup".to_string(),
            RoomNode {
                wings: vec!["tech".into()],
                halls: vec![],
                count: 1,
                dates: vec![],
            },
        );
        nodes.insert(
            "python-basics".to_string(),
            RoomNode {
                wings: vec!["tech".into()],
                halls: vec![],
                count: 1,
                dates: vec![],
            },
        );
        let matches = fuzzy_match("rust", &nodes, 5);
        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0], "rust-setup");
    }

    #[test]
    fn test_fuzzy_match_partial() {
        let mut nodes = HashMap::new();
        nodes.insert(
            "college-apps".to_string(),
            RoomNode {
                wings: vec!["personal".into()],
                halls: vec![],
                count: 1,
                dates: vec![],
            },
        );
        nodes.insert(
            "mobile-apps".to_string(),
            RoomNode {
                wings: vec!["tech".into()],
                halls: vec![],
                count: 1,
                dates: vec![],
            },
        );
        let matches = fuzzy_match("college-apps", &nodes, 5);
        assert!(matches.iter().any(|m| m == "college-apps"));
    }

    #[test]
    fn test_empty_store_graph() {
        let tmp = TempDir::new().unwrap();
        let store = PalaceStore::open(tmp.path().to_str().unwrap()).unwrap();
        let (nodes, edges) = build_graph(&store).unwrap();
        assert!(nodes.is_empty());
        assert!(edges.is_empty());
    }
}

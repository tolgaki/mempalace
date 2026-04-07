use crate::entity_detector::{detect_entities, scan_for_detection};
use crate::entity_registry::{EntityRegistry, PersonEntry};
use crate::error::Result;
use std::collections::HashMap;
use std::io::{self, Write};
use std::path::{Path, PathBuf};

/// Default wing taxonomies by mode.
pub fn default_wings(mode: &str) -> Vec<String> {
    match mode {
        "work" => vec!["projects", "clients", "team", "decisions", "research"],
        "personal" => vec![
            "family",
            "health",
            "creative",
            "reflections",
            "relationships",
        ],
        "combo" => vec![
            "family",
            "work",
            "health",
            "creative",
            "projects",
            "reflections",
        ],
        _ => vec!["general"],
    }
    .into_iter()
    .map(String::from)
    .collect()
}

fn hr() {
    println!("\n{}", "─".repeat(58));
}

fn header(text: &str) {
    println!("\n{}", "=".repeat(58));
    println!("  {}", text);
    println!("{}", "=".repeat(58));
}

fn ask(prompt: &str, default: Option<&str>) -> String {
    if let Some(def) = default {
        print!("  {} [{}]: ", prompt, def);
    } else {
        print!("  {}: ", prompt);
    }
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_string();
    if trimmed.is_empty() {
        default.unwrap_or("").to_string()
    } else {
        trimmed
    }
}

fn yn(prompt: &str, default_yes: bool) -> bool {
    let hint = if default_yes { "Y/n" } else { "y/N" };
    print!("  {} [{}]: ", prompt, hint);
    io::stdout().flush().ok();
    let mut input = String::new();
    io::stdin().read_line(&mut input).ok();
    let trimmed = input.trim().to_lowercase();
    if trimmed.is_empty() {
        default_yes
    } else {
        trimmed.starts_with('y')
    }
}

/// Run the full interactive onboarding flow.
pub fn run_onboarding(
    directory: &str,
    config_dir: Option<&Path>,
    auto_detect: bool,
) -> Result<EntityRegistry> {
    // Step 1: Mode
    header("Welcome to MemPalace");
    println!(
        "\n  MemPalace is a personal memory system. To work well, it needs to know\n  \
         a little about your world.\n"
    );
    println!("  How are you using MemPalace?\n");
    println!("    [1]  Work     — notes, projects, clients, colleagues, decisions");
    println!("    [2]  Personal — diary, family, health, relationships, reflections");
    println!("    [3]  Both     — personal and professional mixed\n");

    let mode = loop {
        let choice = ask("Your choice [1/2/3]", None);
        match choice.as_str() {
            "1" => break "work",
            "2" => break "personal",
            "3" => break "combo",
            _ => println!("  Please enter 1, 2, or 3."),
        }
    };

    // Step 2: People
    let mut people = Vec::new();
    let mut aliases = HashMap::new();

    if mode == "personal" || mode == "combo" {
        hr();
        println!("\n  Personal world — who are the important people?\n");
        println!("  Format: name, relationship (e.g. \"Riley, daughter\")");
        println!("  Type 'done' when finished.\n");
        loop {
            let entry = ask("Person", None);
            if entry.is_empty() || entry.to_lowercase() == "done" {
                break;
            }
            let parts: Vec<&str> = entry.splitn(2, ',').collect();
            let name = parts[0].trim().to_string();
            let relationship = parts
                .get(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                let nick = ask(&format!("Nickname for {}? (or enter to skip)", name), None);
                if !nick.is_empty() {
                    aliases.insert(nick, name.clone());
                }
                people.push(PersonEntry {
                    name,
                    relationship,
                    context: "personal".into(),
                });
            }
        }
    }

    if mode == "work" || mode == "combo" {
        hr();
        println!("\n  Work world — colleagues, clients, collaborators?\n");
        loop {
            let entry = ask("Person", None);
            if entry.is_empty() || entry.to_lowercase() == "done" {
                break;
            }
            let parts: Vec<&str> = entry.splitn(2, ',').collect();
            let name = parts[0].trim().to_string();
            let role = parts
                .get(1)
                .map(|s| s.trim().to_string())
                .unwrap_or_default();
            if !name.is_empty() {
                people.push(PersonEntry {
                    name,
                    relationship: role,
                    context: "work".into(),
                });
            }
        }
    }

    // Step 3: Projects
    let mut projects = Vec::new();
    if mode != "personal" {
        hr();
        println!("\n  What are your main projects?\n");
        loop {
            let proj = ask("Project", None);
            if proj.is_empty() || proj.to_lowercase() == "done" {
                break;
            }
            projects.push(proj);
        }
    }

    // Step 4: Wings
    let defaults = default_wings(mode);
    hr();
    println!(
        "\n  Wings are the top-level categories.\n  Suggested: {}\n",
        defaults.join(", ")
    );
    let custom = ask("Wings (or enter to keep defaults)", None);
    let wings: Vec<String> = if custom.is_empty() {
        defaults
    } else {
        custom
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect()
    };

    // Step 5: Auto-detect
    if auto_detect && yn("Scan your files for additional names?", true) {
        let scan_dir = ask("Directory to scan", Some(directory));
        let files = scan_for_detection(&scan_dir, 10);
        if !files.is_empty() {
            let detected = detect_entities(&files, 10);
            if !detected.people.is_empty() {
                hr();
                println!(
                    "\n  Found {} additional name candidates:\n",
                    detected.people.len()
                );
                for e in &detected.people {
                    println!("    {:20} confidence={:.0}%", e.name, e.confidence * 100.0);
                }
            }
        }
    }

    // Build and save registry
    let mut registry = EntityRegistry::load(config_dir.map(PathBuf::from).as_deref());
    registry.seed(mode, &people, &projects, &aliases);

    // Generate AAAK bootstrap files
    generate_aaak_bootstrap(&people, &projects, &wings, mode, config_dir);

    header("Setup Complete");
    println!("\n  {}", registry.summary());
    println!("\n  Wings: {}", wings.join(", "));
    println!("\n  Your AI will know your world from the first session.\n");

    Ok(registry)
}

/// Non-interactive setup for tests and scripts.
pub fn quick_setup(
    mode: &str,
    people: &[PersonEntry],
    projects: &[String],
    aliases: &HashMap<String, String>,
    config_dir: Option<&Path>,
) -> Result<EntityRegistry> {
    let mut registry = EntityRegistry::load(config_dir.map(|p| p.to_path_buf()).as_deref());
    registry.seed(mode, people, projects, aliases);
    Ok(registry)
}

fn generate_aaak_bootstrap(
    people: &[PersonEntry],
    projects: &[String],
    wings: &[String],
    mode: &str,
    config_dir: Option<&Path>,
) {
    let mempalace_dir = config_dir
        .map(PathBuf::from)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".mempalace"));
    std::fs::create_dir_all(&mempalace_dir).ok();

    // Build entity codes
    let mut entity_codes: HashMap<String, String> = HashMap::new();
    let mut used_codes: std::collections::HashSet<String> = std::collections::HashSet::new();
    for p in people {
        let mut code: String = p.name.chars().take(3).collect::<String>().to_uppercase();
        if used_codes.contains(&code) {
            code = p.name.chars().take(4).collect::<String>().to_uppercase();
        }
        used_codes.insert(code.clone());
        entity_codes.insert(p.name.clone(), code);
    }

    // AAAK entity registry
    let mut lines = vec![
        "# AAAK Entity Registry".to_string(),
        "# Auto-generated by mempalace init.".to_string(),
        String::new(),
        "## People".to_string(),
    ];
    for p in people {
        let code = entity_codes.get(&p.name).cloned().unwrap_or_default();
        if p.relationship.is_empty() {
            lines.push(format!("  {}={}", code, p.name));
        } else {
            lines.push(format!("  {}={} ({})", code, p.name, p.relationship));
        }
    }
    if !projects.is_empty() {
        lines.push(String::new());
        lines.push("## Projects".to_string());
        for proj in projects {
            let code: String = proj.chars().take(4).collect::<String>().to_uppercase();
            lines.push(format!("  {}={}", code, proj));
        }
    }

    std::fs::write(mempalace_dir.join("aaak_entities.md"), lines.join("\n")).ok();

    // Critical facts bootstrap
    let mut facts = vec!["# Critical Facts (bootstrap)".to_string(), String::new()];
    let personal: Vec<_> = people.iter().filter(|p| p.context == "personal").collect();
    let work: Vec<_> = people.iter().filter(|p| p.context == "work").collect();

    if !personal.is_empty() {
        facts.push("## People (personal)".to_string());
        for p in &personal {
            let code = entity_codes.get(&p.name).cloned().unwrap_or_default();
            facts.push(format!("- **{}** ({})", p.name, code));
        }
        facts.push(String::new());
    }
    if !work.is_empty() {
        facts.push("## People (work)".to_string());
        for p in &work {
            let code = entity_codes.get(&p.name).cloned().unwrap_or_default();
            facts.push(format!("- **{}** ({})", p.name, code));
        }
        facts.push(String::new());
    }
    if !projects.is_empty() {
        facts.push("## Projects".to_string());
        for proj in projects {
            facts.push(format!("- **{}**", proj));
        }
        facts.push(String::new());
    }
    facts.push(format!("Wings: {}", wings.join(", ")));
    facts.push(format!("Mode: {}", mode));

    std::fs::write(mempalace_dir.join("critical_facts.md"), facts.join("\n")).ok();
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn test_default_wings_work() {
        let wings = default_wings("work");
        assert!(wings.contains(&"projects".to_string()));
        assert!(wings.contains(&"team".to_string()));
    }

    #[test]
    fn test_default_wings_personal() {
        let wings = default_wings("personal");
        assert!(wings.contains(&"family".to_string()));
        assert!(wings.contains(&"health".to_string()));
    }

    #[test]
    fn test_default_wings_combo() {
        let wings = default_wings("combo");
        assert!(wings.len() >= 5);
    }

    #[test]
    fn test_quick_setup() {
        let tmp = TempDir::new().unwrap();
        let people = vec![PersonEntry {
            name: "Alice".into(),
            relationship: "friend".into(),
            context: "personal".into(),
        }];
        let projects = vec!["TestProject".into()];
        let aliases = HashMap::new();
        let registry =
            quick_setup("personal", &people, &projects, &aliases, Some(tmp.path())).unwrap();
        assert_eq!(registry.mode(), "personal");
    }

    #[test]
    fn test_generate_aaak_bootstrap() {
        let tmp = TempDir::new().unwrap();
        let people = vec![PersonEntry {
            name: "Bob".into(),
            relationship: "partner".into(),
            context: "personal".into(),
        }];
        generate_aaak_bootstrap(
            &people,
            &[],
            &["family".into()],
            "personal",
            Some(tmp.path()),
        );
        assert!(tmp.path().join("aaak_entities.md").exists());
        assert!(tmp.path().join("critical_facts.md").exists());
    }
}

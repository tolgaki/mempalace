use anyhow::Result;
use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(name = "mempalace")]
#[command(about = "Give your AI a memory. No API key required.")]
struct Cli {
    /// Where the palace lives (default: from config or ~/.mempalace/palace)
    #[arg(long)]
    palace: Option<String>,

    #[command(subcommand)]
    command: Option<Commands>,
}

#[derive(Subcommand)]
enum Commands {
    /// Detect rooms from your folder structure
    Init {
        /// Project directory to set up
        dir: String,
        /// Auto-accept all detected entities
        #[arg(long)]
        yes: bool,
    },
    /// Mine files into the palace
    Mine {
        /// Directory to mine
        dir: String,
        /// Ingest mode: 'projects' or 'convos'
        #[arg(long, default_value = "projects")]
        mode: String,
        /// Wing name (default: directory name)
        #[arg(long)]
        wing: Option<String>,
        /// Your name — recorded on every drawer
        #[arg(long, default_value = "mempalace")]
        agent: String,
        /// Max files to process (0 = all)
        #[arg(long, default_value = "0")]
        limit: usize,
        /// Show what would be filed without filing
        #[arg(long)]
        dry_run: bool,
        /// Extraction strategy for convos mode
        #[arg(long, default_value = "exchange")]
        extract: String,
    },
    /// Find anything, exact words
    Search {
        /// What to search for
        query: String,
        /// Limit to one project
        #[arg(long)]
        wing: Option<String>,
        /// Limit to one room
        #[arg(long)]
        room: Option<String>,
        /// Number of results
        #[arg(long, default_value = "5")]
        results: usize,
    },
    /// Compress drawers using AAAK Dialect
    Compress {
        /// Wing to compress (default: all)
        #[arg(long)]
        wing: Option<String>,
        /// Preview without storing
        #[arg(long)]
        dry_run: bool,
        /// Entity config JSON
        #[arg(long)]
        config: Option<String>,
    },
    /// Show L0 + L1 wake-up context
    #[command(name = "wake-up")]
    WakeUp {
        /// Wake-up for a specific project/wing
        #[arg(long)]
        wing: Option<String>,
    },
    /// Split concatenated transcript mega-files
    Split {
        /// Directory containing transcript files
        dir: String,
        /// Write split files here
        #[arg(long)]
        output_dir: Option<String>,
        /// Show what would happen without writing
        #[arg(long)]
        dry_run: bool,
        /// Only split files with at least N sessions
        #[arg(long, default_value = "2")]
        min_sessions: usize,
    },
    /// Show what's been filed
    Status,
    /// Run the MCP server (JSON-RPC over stdin/stdout)
    Serve,
}

fn main() -> Result<()> {
    let cli = Cli::parse();
    let config = mempalace::MempalaceConfig::new(None);
    let palace_path = cli.palace.unwrap_or_else(|| config.palace_path());

    match cli.command {
        Some(Commands::Init { dir, yes: _ }) => {
            // Run entity detection + room detection
            println!("\n  Scanning for entities in: {}", dir);
            let files = mempalace::entity_detector::scan_for_detection(&dir, 10);
            if !files.is_empty() {
                println!("  Reading {} files...", files.len());
                let detected = mempalace::entity_detector::detect_entities(&files, 10);
                let total =
                    detected.people.len() + detected.projects.len() + detected.uncertain.len();
                if total > 0 {
                    println!("  Detected {} entities", total);
                } else {
                    println!("  No entities detected — proceeding with directory-based rooms.");
                }
            }

            let rooms = mempalace::room_detector::detect_rooms_from_folders(&dir);
            println!("  Detected {} rooms", rooms.len());
            for room in &rooms {
                println!("    ROOM: {}", room.name);
            }

            // Save config
            let project_path = std::path::Path::new(&dir)
                .canonicalize()
                .unwrap_or_else(|_| std::path::PathBuf::from(&dir));
            let project_name = project_path
                .file_name()
                .map(|n| n.to_string_lossy().to_lowercase().replace(['-', ' '], "_"))
                .unwrap_or_else(|| "project".into());

            let yaml_config = serde_yaml::to_string(&serde_json::json!({
                "wing": project_name,
                "rooms": rooms.iter().map(|r| serde_json::json!({"name": r.name, "description": r.description})).collect::<Vec<_>>(),
            }))?;
            std::fs::write(project_path.join("mempalace.yaml"), yaml_config)?;
            println!("  Config saved: {}/mempalace.yaml", dir);

            config.init()?;
        }

        Some(Commands::Mine {
            dir,
            mode,
            wing,
            agent,
            limit,
            dry_run,
            extract,
        }) => {
            if mode == "convos" {
                mempalace::convo_miner::mine_convos(
                    &dir,
                    &palace_path,
                    wing.as_deref(),
                    &agent,
                    limit,
                    dry_run,
                    &extract,
                )?;
            } else {
                mempalace::miner::mine(
                    &dir,
                    &palace_path,
                    wing.as_deref(),
                    &agent,
                    limit,
                    dry_run,
                )?;
            }
        }

        Some(Commands::Search {
            query,
            wing,
            room,
            results,
        }) => {
            mempalace::searcher::search_print(
                &query,
                &palace_path,
                wing.as_deref(),
                room.as_deref(),
                results,
            )?;
        }

        Some(Commands::Compress {
            wing,
            dry_run,
            config: config_path,
        }) => {
            use mempalace::dialect::Dialect;
            use mempalace::store::PalaceStore;

            let dialect = if let Some(ref path) = config_path {
                Dialect::from_config(path)?
            } else {
                Dialect::new(None, None)
            };

            let store = PalaceStore::open(&palace_path)?;
            let filter = wing
                .as_ref()
                .map(|w| mempalace::store::WhereFilter::Wing(w.clone()));
            let drawers = store.get(filter.as_ref(), None)?;

            if drawers.is_empty() {
                println!("\n  No drawers found.");
                return Ok(());
            }

            println!("\n  Compressing {} drawers...\n", drawers.len());
            let mut total_original = 0usize;
            let mut total_compressed = 0usize;

            for d in &drawers {
                let mut meta_map = std::collections::HashMap::new();
                meta_map.insert("wing".into(), d.metadata.wing.clone());
                meta_map.insert("room".into(), d.metadata.room.clone());
                meta_map.insert("source_file".into(), d.metadata.source_file.clone());
                if let Some(ref date) = d.metadata.date {
                    meta_map.insert("date".into(), date.clone());
                }

                let compressed = dialect.compress(&d.content, Some(&meta_map));
                let stats = Dialect::compression_stats(&d.content, &compressed);
                total_original += stats.original_chars;
                total_compressed += stats.compressed_chars;

                if dry_run {
                    println!(
                        "  [{}/{}] {}",
                        d.metadata.wing, d.metadata.room, d.metadata.source_file
                    );
                    println!(
                        "    {}t -> {}t ({:.1}x)",
                        stats.original_tokens, stats.compressed_tokens, stats.ratio
                    );
                    println!("    {}\n", compressed);
                }
            }

            let ratio = total_original as f64 / total_compressed.max(1) as f64;
            println!(
                "  Total: {}t -> {}t ({:.1}x compression)",
                Dialect::count_tokens(&"x".repeat(total_original)),
                Dialect::count_tokens(&"x".repeat(total_compressed)),
                ratio
            );
            if dry_run {
                println!("  (dry run -- nothing stored)");
            }
        }

        Some(Commands::WakeUp { wing }) => {
            let mut stack = mempalace::layers::MemoryStack::new(Some(&palace_path), None);
            let text = stack.wake_up(wing.as_deref());
            let tokens = text.len() / 4;
            println!("Wake-up text (~{} tokens):", tokens);
            println!("{}", "=".repeat(50));
            println!("{}", text);
        }

        Some(Commands::Split {
            dir,
            output_dir,
            dry_run,
            min_sessions: _,
        }) => {
            let path = std::path::Path::new(&dir);
            let out = output_dir.as_deref().map(std::path::Path::new);

            if path.is_file() {
                mempalace::split_mega_files::split_file(path, out, dry_run)?;
            } else {
                // Scan directory for .txt files
                for entry in std::fs::read_dir(path)?.flatten() {
                    if entry.path().extension().is_some_and(|e| e == "txt") {
                        mempalace::split_mega_files::split_file(&entry.path(), out, dry_run)?;
                    }
                }
            }
        }

        Some(Commands::Status) => {
            mempalace::miner::status(&palace_path)?;
        }

        Some(Commands::Serve) => {
            mempalace::mcp_server::run_server()?;
        }

        None => {
            println!("MemPalace — Give your AI a memory. No API key required.");
            println!("Run `mempalace --help` for available commands.");
        }
    }

    Ok(())
}

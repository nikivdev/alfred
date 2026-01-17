use std::path::PathBuf;

use clap::{Parser, Subcommand};
use flow_alfred::{discover_repos, discover_repos_structured, expand_path, fuzzy_match, fuzzy_sort, reload_workflow, Icon, Item, Output};

#[derive(Parser)]
#[command(name = "flow-alfred")]
#[command(about = "Alfred workflow tools")]
struct Cli {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Search git repositories under ~/code
    Code {
        /// Search query
        #[arg(default_value = "")]
        query: String,

        /// Root directory to scan
        #[arg(long, default_value = "~/code")]
        root: String,
    },

    /// Search git repositories under ~/repos (owner/repo structure)
    Repos {
        /// Search query
        #[arg(default_value = "")]
        query: String,

        /// Root directory to scan
        #[arg(long, default_value = "~/repos")]
        root: String,
    },

    /// Link workflow to Alfred (for development)
    Link {
        /// Path to workflow directory
        #[arg(default_value = "Flow.alfredworkflow")]
        workflow_dir: String,

        /// Bundle ID
        #[arg(long, default_value = "nikiv.dev.flow")]
        bundle_id: String,
    },

    /// Unlink workflow from Alfred
    Unlink {
        /// Bundle ID
        #[arg(long, default_value = "nikiv.dev.flow")]
        bundle_id: String,
    },

    /// Pack workflow into .alfredworkflow file
    Pack {
        /// Path to workflow directory
        #[arg(default_value = "Flow.alfredworkflow")]
        workflow_dir: String,

        /// Output file path
        #[arg(short, long)]
        output: Option<String>,
    },

    /// Install workflow (open .alfredworkflow file)
    Install {
        /// Path to .alfredworkflow file
        workflow_file: String,
    },

    /// Reload workflow in Alfred (refresh canvas without restart)
    Reload {
        /// Bundle ID
        #[arg(long, default_value = "nikiv.dev.flow")]
        bundle_id: String,
    },

    /// Watch workflow directory and reload on changes
    Watch {
        /// Path to workflow directory
        #[arg(default_value = "Flow.alfredworkflow")]
        workflow_dir: String,

        /// Bundle ID
        #[arg(long, default_value = "nikiv.dev.flow")]
        bundle_id: String,
    },

    /// List AI sessions for a project (Alfred JSON output)
    Sessions {
        /// Query to filter sessions
        query: String,

        /// Project path
        #[arg(long)]
        path: String,
    },

    /// Get session content for clipboard
    SessionContent {
        /// Session ID
        #[arg(long)]
        id: String,

        /// Project path
        #[arg(long)]
        path: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Code { query, root } => run_code_search(&query, &root),
        Commands::Repos { query, root } => run_repos_search(&query, &root),
        Commands::Link {
            workflow_dir,
            bundle_id,
        } => run_link(&workflow_dir, &bundle_id),
        Commands::Unlink { bundle_id } => run_unlink(&bundle_id),
        Commands::Pack {
            workflow_dir,
            output,
        } => run_pack(&workflow_dir, output),
        Commands::Install { workflow_file } => run_install(&workflow_file),
        Commands::Reload { bundle_id } => run_reload(&bundle_id),
        Commands::Watch { workflow_dir, bundle_id } => run_watch(&workflow_dir, &bundle_id),
        Commands::Sessions { query, path } => run_sessions(&query, &path),
        Commands::SessionContent { id, path } => run_session_content(&id, &path),
    }
}

fn run_code_search(query: &str, root: &str) {
    let root_path = expand_path(root);

    if !root_path.exists() {
        Output::new(vec![Item::new(
            format!("No directory found at {}", root),
            "Check your code_root setting",
        )
        .valid(false)
        .icon(Icon::path(
            "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns",
        ))])
        .print();
        return;
    }

    let repos = discover_repos(&root_path);
    if repos.is_empty() {
        Output::new(vec![Item::new("No git repositories found", format!("in {}", root))
            .valid(false)
            .icon(Icon::path("/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns"))])
            .print();
        return;
    }

    let mut items: Vec<Item> = repos
        .iter()
        .filter(|e| query.is_empty() || fuzzy_match(query, &e.display))
        .map(|entry| {
            let path_str = entry.path.to_string_lossy().to_string();
            let relative_path = format!("{}/{}", root, &entry.display);
            // Condense when last two path segments are the same (e.g., "org/gitedit/gitedit" -> "org/gitedit")
            let parts: Vec<&str> = entry.display.rsplitn(2, '/').collect();
            let display = if parts.len() == 2 && parts[0] == parts[1].rsplit('/').next().unwrap_or("") {
                entry.display.rsplit_once('/').map(|(prefix, _)| prefix.to_string()).unwrap_or_else(|| entry.display.clone())
            } else {
                entry.display.clone()
            };
            Item::title_only(&display)
                .uid(&path_str)
                .arg(&path_str)
                .match_field(&entry.display)
                .autocomplete(&entry.display)
                .file_type()
                .icon(Icon::fileicon(&path_str))
                .quicklook(&path_str)
                .copy_text(&relative_path)
                .cmd_mod(&relative_path, "Paste path")
                .alt_mod(&path_str, "Browse sessions")
        })
        .collect();

    if !query.is_empty() {
        fuzzy_sort(&mut items, query, |item| &item.title);
    }

    Output::new(items).print();
}

fn run_repos_search(query: &str, root: &str) {
    let root_path = expand_path(root);

    if !root_path.exists() {
        Output::new(vec![Item::new(
            format!("No directory found at {}", root),
            "Check your repos_root setting",
        )
        .valid(false)
        .icon(Icon::path(
            "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns",
        ))])
        .print();
        return;
    }

    let repos = discover_repos_structured(&root_path);
    if repos.is_empty() {
        Output::new(vec![Item::new("No git repositories found", format!("in {}", root))
            .valid(false)
            .icon(Icon::path("/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns"))])
            .print();
        return;
    }

    let mut items: Vec<Item> = repos
        .iter()
        .filter(|e| query.is_empty() || fuzzy_match(query, &e.display))
        .map(|entry| {
            let path_str = entry.path.to_string_lossy().to_string();
            let relative_path = format!("{}/{}", root, &entry.display);
            // Condense when last two path segments are the same (e.g., "org/gitedit/gitedit" -> "org/gitedit")
            let parts: Vec<&str> = entry.display.rsplitn(2, '/').collect();
            let display = if parts.len() == 2 && parts[0] == parts[1].rsplit('/').next().unwrap_or("") {
                entry.display.rsplit_once('/').map(|(prefix, _)| prefix.to_string()).unwrap_or_else(|| entry.display.clone())
            } else {
                entry.display.clone()
            };
            Item::title_only(&display)
                .uid(&path_str)
                .arg(&path_str)  // Full path for opening
                .match_field(&entry.display)  // Keep full path for matching
                .autocomplete(&entry.display)
                .icon(Icon::fileicon(&path_str))
                .quicklook(&path_str)
                .copy_text(&relative_path)  // Relative path for copy
                .cmd_mod(&relative_path, "Paste path")
                .alt_mod(&path_str, "Browse sessions")
        })
        .collect();

    if !query.is_empty() {
        fuzzy_sort(&mut items, query, |item| &item.title);
    }

    Output::new(items).print();
}

fn run_link(workflow_dir: &str, bundle_id: &str) {
    let workflow_path = PathBuf::from(workflow_dir).canonicalize().unwrap_or_else(|_| {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(workflow_dir)
    });

    if !workflow_path.exists() {
        eprintln!("Workflow directory not found: {:?}", workflow_path);
        std::process::exit(1);
    }

    match flow_alfred::link_workflow(&workflow_path, bundle_id) {
        Ok(dest) => {
            println!("Linked {:?} -> {:?}", workflow_path, dest);
            // Reload workflow in Alfred
            if let Err(e) = reload_workflow(bundle_id) {
                eprintln!("Warning: Failed to reload workflow: {}", e);
            } else {
                println!("Reloaded workflow in Alfred");
            }
        }
        Err(e) => {
            eprintln!("Failed to link: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_unlink(bundle_id: &str) {
    match flow_alfred::unlink_workflow(bundle_id) {
        Ok(()) => println!("Unlinked {}", bundle_id),
        Err(e) => {
            eprintln!("Failed to unlink: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_pack(workflow_dir: &str, output: Option<String>) {
    let workflow_path = PathBuf::from(workflow_dir);
    if !workflow_path.exists() {
        eprintln!("Workflow directory not found: {:?}", workflow_path);
        std::process::exit(1);
    }

    let output_path = output
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("Flow-Workflow.alfredworkflow"));

    match flow_alfred::pack_workflow(&workflow_path, &output_path) {
        Ok(()) => println!("Created {:?}", output_path),
        Err(e) => {
            eprintln!("Failed to pack: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_install(workflow_file: &str) {
    let path = PathBuf::from(workflow_file);
    if !path.exists() {
        eprintln!("Workflow file not found: {:?}", path);
        std::process::exit(1);
    }

    match flow_alfred::install_workflow(&path) {
        Ok(()) => println!("Opening {:?} for installation...", path),
        Err(e) => {
            eprintln!("Failed to install: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_reload(bundle_id: &str) {
    match reload_workflow(bundle_id) {
        Ok(()) => println!("Reloaded workflow: {}", bundle_id),
        Err(e) => {
            eprintln!("Failed to reload: {}", e);
            std::process::exit(1);
        }
    }
}

fn run_watch(workflow_dir: &str, bundle_id: &str) {
    use std::io::{BufRead, BufReader};
    use std::process::{Command, Stdio};

    let workflow_path = PathBuf::from(workflow_dir).canonicalize().unwrap_or_else(|_| {
        let cwd = std::env::current_dir().unwrap_or_default();
        cwd.join(workflow_dir)
    });

    if !workflow_path.exists() {
        eprintln!("Workflow directory not found: {:?}", workflow_path);
        std::process::exit(1);
    }

    println!("Watching {:?} for changes...", workflow_path);
    println!("Press Ctrl+C to stop");

    let mut child = Command::new("fswatch")
        .args(["-o", &workflow_path.to_string_lossy()])
        .stdout(Stdio::piped())
        .spawn()
        .unwrap_or_else(|e| {
            eprintln!("Failed to start fswatch: {}. Install with: brew install fswatch", e);
            std::process::exit(1);
        });

    let stdout = child.stdout.take().expect("Failed to get stdout");
    let reader = BufReader::new(stdout);

    for _line in reader.lines().map_while(Result::ok) {
        println!("Change detected, reloading...");
        if let Err(e) = reload_workflow(bundle_id) {
            eprintln!("Failed to reload: {}", e);
        } else {
            println!("Reloaded");
        }
    }
}

fn run_sessions(query: &str, project_path: &str) {
    use serde_json::Value;
    use std::fs;

    let claude_dir = dirs::home_dir()
        .map(|h| h.join(".claude").join("projects"))
        .unwrap_or_default();

    // Convert path to Claude's folder naming: /Users/nikiv/code/alfred -> -Users-nikiv-code-alfred
    let project_folder = project_path.replace('/', "-");
    let sessions_dir = claude_dir.join(&project_folder);

    if !sessions_dir.exists() {
        Output::new(vec![Item::new("No sessions found", &format!("for {}", project_path))
            .valid(false)])
            .print();
        return;
    }

    // Find all .jsonl files
    let mut sessions: Vec<(String, String, String, i64)> = Vec::new(); // (id, first_msg, timestamp_str, timestamp_unix)

    if let Ok(entries) = fs::read_dir(&sessions_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().map(|e| e == "jsonl").unwrap_or(false) {
                if let Some(session_id) = path.file_stem().and_then(|s| s.to_str()) {
                    // Read first user message and last timestamp
                    if let Ok(content) = fs::read_to_string(&path) {
                        let mut first_user_msg = String::new();
                        let mut last_timestamp: i64 = 0;
                        let mut last_timestamp_str = String::new();

                        for line in content.lines() {
                            if let Ok(json) = serde_json::from_str::<Value>(line) {
                                // Get first user message
                                if first_user_msg.is_empty() {
                                    if json.get("type").and_then(|t| t.as_str()) == Some("user") {
                                        if let Some(msg) = json.get("message")
                                            .and_then(|m| m.get("content"))
                                            .and_then(|c| c.as_str())
                                        {
                                            first_user_msg = msg.chars().take(80).collect();
                                            first_user_msg = first_user_msg.lines().next().unwrap_or("").to_string();
                                        }
                                    }
                                }

                                // Track last timestamp
                                if let Some(ts) = json.get("timestamp").and_then(|t| t.as_str()) {
                                    if let Ok(dt) = chrono::DateTime::parse_from_rfc3339(ts) {
                                        let unix = dt.timestamp();
                                        if unix > last_timestamp {
                                            last_timestamp = unix;
                                            last_timestamp_str = format_relative_time(unix);
                                        }
                                    }
                                }
                            }
                        }

                        if !first_user_msg.is_empty() && last_timestamp > 0 {
                            sessions.push((
                                session_id.to_string(),
                                first_user_msg,
                                last_timestamp_str,
                                last_timestamp,
                            ));
                        }
                    }
                }
            }
        }
    }

    // Sort by timestamp descending (most recent first)
    sessions.sort_by(|a, b| b.3.cmp(&a.3));

    if sessions.is_empty() {
        Output::new(vec![Item::new("No sessions found", &format!("for {}", project_path))
            .valid(false)])
            .print();
        return;
    }

    let items: Vec<Item> = sessions
        .iter()
        .filter(|(_, msg, _, _)| query.is_empty() || msg.to_lowercase().contains(&query.to_lowercase()))
        .map(|(id, msg, time, _)| {
            let arg = format!("{}|{}", id, project_path);
            Item::new(msg, time)
                .uid(id)
                .arg(&arg)
                .match_field(msg)
        })
        .collect();

    Output::new(items).print();
}

fn format_relative_time(unix_timestamp: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let diff = now - unix_timestamp;

    if diff < 60 {
        "just now".to_string()
    } else if diff < 3600 {
        format!("{}m ago", diff / 60)
    } else if diff < 86400 {
        format!("{}h ago", diff / 3600)
    } else if diff < 604800 {
        format!("{}d ago", diff / 86400)
    } else {
        format!("{}w ago", diff / 604800)
    }
}

fn run_session_content(session_id: &str, project_path: &str) {
    use serde_json::Value;
    use std::fs;

    let claude_dir = dirs::home_dir()
        .map(|h| h.join(".claude").join("projects"))
        .unwrap_or_default();

    let project_folder = project_path.replace('/', "-");
    let session_file = claude_dir.join(&project_folder).join(format!("{}.jsonl", session_id));

    if !session_file.exists() {
        eprintln!("Session file not found: {:?}", session_file);
        return;
    }

    let content = match fs::read_to_string(&session_file) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("Failed to read session: {}", e);
            return;
        }
    };

    let mut output = String::new();

    for line in content.lines() {
        if let Ok(json) = serde_json::from_str::<Value>(line) {
            let msg_type = json.get("type").and_then(|t| t.as_str()).unwrap_or("");

            if msg_type == "user" {
                if let Some(msg) = json.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_str()) {
                    output.push_str("\n## User\n\n");
                    output.push_str(msg);
                    output.push_str("\n");
                }
            } else if msg_type == "assistant" {
                if let Some(content_arr) = json.get("message").and_then(|m| m.get("content")).and_then(|c| c.as_array()) {
                    for item in content_arr {
                        if item.get("type").and_then(|t| t.as_str()) == Some("text") {
                            if let Some(text) = item.get("text").and_then(|t| t.as_str()) {
                                output.push_str("\n## Assistant\n\n");
                                output.push_str(text);
                                output.push_str("\n");
                            }
                        }
                    }
                }
            }
        }
    }

    // Output the content (will be captured by Alfred for clipboard)
    print!("{}", output.trim());
}

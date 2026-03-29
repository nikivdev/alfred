use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;

use clap::{Parser, Subcommand};
use flow_alfred::{
    discover_repos_cached, discover_repos_structured_cached, expand_path, fuzzy_match, fuzzy_score,
    reload_workflow, CodeEntry, Icon, Item, Output,
};

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

    /// Search git repositories under ~/repos and ~/code together
    ReposCode {
        /// Search query
        #[arg(default_value = "")]
        query: String,

        /// Root directory to scan for structured repos
        #[arg(long, default_value = "~/repos")]
        repos_root: String,

        /// Root directory to scan for code repos
        #[arg(long, default_value = "~/code")]
        code_root: String,
    },

    /// Search Codex recipe notes and copy the paste-ready payload
    Recipes {
        /// Search query
        #[arg(default_value = "")]
        query: String,

        /// Root directory to scan
        #[arg(long, default_value = "~/docs/codex/recipes")]
        root: String,
    },

    /// Get recipe clipboard content from a markdown file
    RecipeContent {
        /// Recipe markdown file path
        #[arg(long)]
        path: String,
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

    /// List windows of frontmost app (Alfred JSON output)
    Windows {
        /// Query to filter windows
        #[arg(default_value = "")]
        query: String,
    },

    /// Raise a window by index
    RaiseWindow {
        /// JSON with app name and window index
        arg: String,
    },
}

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Commands::Code { query, root } => run_code_search(&query, &root),
        Commands::Repos { query, root } => run_repos_search(&query, &root),
        Commands::ReposCode {
            query,
            repos_root,
            code_root,
        } => run_repos_code_search(&query, &repos_root, &code_root),
        Commands::Recipes { query, root } => run_recipe_search(&query, &root),
        Commands::RecipeContent { path } => run_recipe_content(&path),
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
        Commands::Watch {
            workflow_dir,
            bundle_id,
        } => run_watch(&workflow_dir, &bundle_id),
        Commands::Sessions { query, path } => run_sessions(&query, &path),
        Commands::SessionContent { id, path } => run_session_content(&id, &path),
        Commands::Windows { query } => run_windows(&query),
        Commands::RaiseWindow { arg } => run_raise_window(&arg),
    }
}

#[derive(Debug, Clone)]
struct RepoSearchEntry {
    display: String,
    path: PathBuf,
    root: String,
    subtitle: Option<String>,
    search_text: String,
}

fn run_code_search(query: &str, root: &str) {
    let root_path = expand_path(root);

    if !root_path.exists() {
        print_missing_root(root, "Check your code_root setting");
        return;
    }

    let repos = discover_repos_cached(&root_path);
    if repos.is_empty() {
        print_no_repos(&format!("in {}", root));
        return;
    }

    let items = build_repo_items(
        query,
        repos
            .into_iter()
            .map(|entry| repo_search_entry(entry, root, None))
            .collect(),
    );

    Output::new(items).print();
}

fn run_repos_search(query: &str, root: &str) {
    let root_path = expand_path(root);

    if !root_path.exists() {
        print_missing_root(root, "Check your repos_root setting");
        return;
    }

    let repos = discover_repos_structured_cached(&root_path);
    if repos.is_empty() {
        print_no_repos(&format!("in {}", root));
        return;
    }

    let items = build_repo_items(
        query,
        repos
            .into_iter()
            .map(|entry| repo_search_entry(entry, root, None))
            .collect(),
    );

    Output::new(items).print();
}

fn run_repos_code_search(query: &str, repos_root: &str, code_root: &str) {
    let repos_root_path = expand_path(repos_root);
    let code_root_path = expand_path(code_root);

    let repos_exists = repos_root_path.exists();
    let code_exists = code_root_path.exists();

    if !repos_exists && !code_exists {
        print_missing_root(
            &format!("{repos_root} or {code_root}"),
            "Check your repos_root and code_root settings",
        );
        return;
    }

    let repos_handle = repos_exists.then(|| {
        let root_path = repos_root_path.clone();
        thread::spawn(move || discover_repos_structured_cached(&root_path))
    });
    let code_handle = code_exists.then(|| {
        let root_path = code_root_path.clone();
        thread::spawn(move || discover_repos_cached(&root_path))
    });

    let mut entries = Vec::new();
    if let Some(handle) = repos_handle {
        if let Ok(repos) = handle.join() {
            entries.extend(
                repos
                    .into_iter()
                    .map(|entry| repo_search_entry(entry, repos_root, Some("repos"))),
            );
        }
    }
    if let Some(handle) = code_handle {
        if let Ok(repos) = handle.join() {
            entries.extend(
                repos
                    .into_iter()
                    .map(|entry| repo_search_entry(entry, code_root, Some("code"))),
            );
        }
    }

    if entries.is_empty() {
        let scope = match (repos_exists, code_exists) {
            (true, true) => format!("in {repos_root} and {code_root}"),
            (true, false) => format!("in {repos_root}"),
            (false, true) => format!("in {code_root}"),
            (false, false) => unreachable!(),
        };
        print_no_repos(&scope);
        return;
    }

    let items = build_repo_items(query, entries);
    Output::new(items).print();
}

fn repo_search_entry(entry: CodeEntry, root: &str, subtitle: Option<&str>) -> RepoSearchEntry {
    let subtitle = subtitle.map(|text| text.to_string());
    let search_text = subtitle
        .as_ref()
        .map(|text| format!("{} {}", entry.display, text))
        .unwrap_or_else(|| entry.display.clone());

    RepoSearchEntry {
        display: entry.display,
        path: entry.path,
        root: root.to_string(),
        subtitle,
        search_text,
    }
}

fn build_repo_items(query: &str, entries: Vec<RepoSearchEntry>) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut entries: Vec<RepoSearchEntry> = entries
        .into_iter()
        .filter(|entry| seen.insert(entry.path.clone()))
        .filter(|entry| query.is_empty() || fuzzy_match(query, &entry.search_text))
        .collect();

    if !query.is_empty() {
        entries.sort_by(|left, right| {
            let left_score = fuzzy_score(query, &left.search_text);
            let right_score = fuzzy_score(query, &right.search_text);
            right_score
                .cmp(&left_score)
                .then_with(|| left.display.cmp(&right.display))
        });
    } else {
        entries.sort_by(|left, right| left.display.cmp(&right.display));
    }

    entries
        .into_iter()
        .map(|entry| build_repo_item(entry))
        .collect()
}

fn build_repo_item(entry: RepoSearchEntry) -> Item {
    let path_str = entry.path.to_string_lossy().to_string();
    let relative_path = format!("{}/{}", entry.root, entry.display);
    let display = condensed_repo_display(&entry.display);

    let item = if let Some(subtitle) = entry.subtitle.as_deref() {
        Item::new(&display, subtitle)
    } else {
        Item::title_only(&display)
    };

    item.uid(&path_str)
        .arg(&path_str)
        .match_field(&entry.search_text)
        .autocomplete(&entry.display)
        .file_type()
        .icon(Icon::fileicon(&path_str))
        .quicklook(&path_str)
        .copy_text(&relative_path)
        .cmd_mod(&relative_path, "Paste path")
        .alt_mod(&path_str, "Browse sessions")
}

fn condensed_repo_display(display: &str) -> String {
    let parts: Vec<&str> = display.rsplitn(2, '/').collect();
    if parts.len() == 2 && parts[0] == parts[1].rsplit('/').next().unwrap_or("") {
        display
            .rsplit_once('/')
            .map(|(prefix, _)| prefix.to_string())
            .unwrap_or_else(|| display.to_string())
    } else {
        display.to_string()
    }
}

fn print_missing_root(root: &str, hint: &str) {
    Output::new(vec![Item::new(
        format!("No directory found at {}", root),
        hint,
    )
    .valid(false)
    .icon(Icon::path(
        "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns",
    ))])
    .print();
}

fn print_no_repos(scope: &str) {
    Output::new(vec![Item::new("No git repositories found", scope)
        .valid(false)
        .icon(Icon::path(
            "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns",
        ))])
    .print();
}

#[derive(Debug, Clone)]
struct RecipeEntry {
    path: PathBuf,
    relative_path: String,
    title: String,
    summary: String,
    paste_text: String,
    search_text: String,
}

fn run_recipe_search(query: &str, root: &str) {
    let root_path = expand_path(root);

    if !root_path.exists() {
        Output::new(vec![Item::new(
            format!("No directory found at {}", root),
            "Check your recipes root",
        )
        .valid(false)
        .icon(Icon::path(
            "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns",
        ))])
        .print();
        return;
    }

    let recipes = discover_recipe_entries(&root_path);
    if recipes.is_empty() {
        Output::new(vec![Item::new("No recipes found", format!("in {}", root))
            .valid(false)
            .icon(Icon::path(
                "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns",
            ))])
        .print();
        return;
    }

    let query_lower = query.to_lowercase();
    let mut filtered = recipes
        .into_iter()
        .filter(|entry| query.is_empty() || fuzzy_match(query, &entry.search_text))
        .collect::<Vec<_>>();

    if !query.is_empty() {
        filtered.sort_by(|left, right| {
            let left_score = fuzzy_score(&query_lower, &left.search_text);
            let right_score = fuzzy_score(&query_lower, &right.search_text);
            right_score
                .cmp(&left_score)
                .then_with(|| left.title.cmp(&right.title))
        });
    }

    let items = filtered
        .into_iter()
        .map(|entry| {
            let path_str = entry.path.to_string_lossy().to_string();
            let subtitle = if entry.summary.is_empty() {
                entry.relative_path.clone()
            } else {
                format!("{}  |  {}", entry.summary, entry.relative_path)
            };
            Item::new(&entry.title, subtitle)
                .uid(&path_str)
                .arg(&path_str)
                .match_field(&entry.search_text)
                .autocomplete(&entry.title)
                .quicklook(&path_str)
                .icon(Icon::fileicon(&path_str))
                .copy_text(&entry.paste_text)
                .cmd_mod(&path_str, "Open recipe in Zed Preview")
        })
        .collect::<Vec<_>>();

    Output::new(items).print();
}

fn run_recipe_content(path: &str) {
    let expanded = expand_path(path);
    match load_recipe_entry(&expanded, expanded.parent()) {
        Some(recipe) => print!("{}", recipe.paste_text),
        None => {
            eprintln!(
                "Recipe file not found or unreadable: {}",
                expanded.display()
            );
            std::process::exit(1);
        }
    }
}

fn discover_recipe_entries(root: &std::path::Path) -> Vec<RecipeEntry> {
    let mut pending = vec![root.to_path_buf()];
    let mut entries = Vec::new();

    while let Some(dir) = pending.pop() {
        let read_dir = match std::fs::read_dir(&dir) {
            Ok(value) => value,
            Err(_) => continue,
        };
        for entry in read_dir.flatten() {
            let path = entry.path();
            if path.is_dir() {
                pending.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("md") {
                continue;
            }
            if path.file_name().and_then(|value| value.to_str()) == Some("README.md") {
                continue;
            }
            if let Some(recipe) = load_recipe_entry(&path, Some(root)) {
                entries.push(recipe);
            }
        }
    }

    entries
}

fn load_recipe_entry(
    path: &std::path::Path,
    root: Option<&std::path::Path>,
) -> Option<RecipeEntry> {
    let raw = std::fs::read_to_string(path).ok()?;
    let title = parse_recipe_title(&raw).unwrap_or_else(|| {
        path.file_stem()
            .and_then(|value| value.to_str())
            .unwrap_or("recipe")
            .replace('-', " ")
    });
    let summary = parse_recipe_summary(&raw);
    let paste_text = parse_recipe_paste_text(&raw).unwrap_or_else(|| raw.trim().to_string());
    let relative_path = root
        .and_then(|base| path.strip_prefix(base).ok())
        .map(|value| value.to_string_lossy().to_string())
        .unwrap_or_else(|| path.to_string_lossy().to_string());
    let search_text = format!(
        "{} {} {} {}",
        title,
        summary,
        relative_path,
        paste_text.lines().take(4).collect::<Vec<_>>().join(" ")
    );

    Some(RecipeEntry {
        path: path.to_path_buf(),
        relative_path,
        title,
        summary,
        paste_text,
        search_text,
    })
}

fn parse_recipe_title(raw: &str) -> Option<String> {
    raw.lines().find_map(|line| {
        line.strip_prefix("# ")
            .map(|value| value.trim().to_string())
    })
}

fn parse_recipe_summary(raw: &str) -> String {
    let mut in_code_block = false;
    let mut paragraph = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            continue;
        }
        if in_code_block {
            continue;
        }
        if trimmed.starts_with('#') {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        if trimmed.is_empty() {
            if !paragraph.is_empty() {
                break;
            }
            continue;
        }
        paragraph.push(trimmed.to_string());
    }

    paragraph.join(" ")
}

fn parse_recipe_paste_text(raw: &str) -> Option<String> {
    let mut in_paste_section = false;
    let mut in_code_block = false;
    let mut paste_lines = Vec::new();

    for line in raw.lines() {
        let trimmed = line.trim();
        if let Some(heading) = trimmed.strip_prefix("## ") {
            if in_paste_section && !paste_lines.is_empty() {
                break;
            }
            in_paste_section = heading.to_ascii_lowercase().contains("paste");
            continue;
        }
        if !in_paste_section {
            continue;
        }
        if trimmed.starts_with("```") {
            in_code_block = !in_code_block;
            if !in_code_block && !paste_lines.is_empty() {
                break;
            }
            continue;
        }
        if in_code_block {
            paste_lines.push(line.to_string());
        }
    }

    let paste_text = paste_lines.join("\n").trim().to_string();
    if paste_text.is_empty() {
        None
    } else {
        Some(paste_text)
    }
}

fn run_link(workflow_dir: &str, bundle_id: &str) {
    let workflow_path = PathBuf::from(workflow_dir)
        .canonicalize()
        .unwrap_or_else(|_| {
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

    let workflow_path = PathBuf::from(workflow_dir)
        .canonicalize()
        .unwrap_or_else(|_| {
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
            eprintln!(
                "Failed to start fswatch: {}. Install with: brew install fswatch",
                e
            );
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
        Output::new(vec![Item::new(
            "No sessions found",
            &format!("for {}", project_path),
        )
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
                                        if let Some(msg) = json
                                            .get("message")
                                            .and_then(|m| m.get("content"))
                                            .and_then(|c| c.as_str())
                                        {
                                            first_user_msg = msg.chars().take(80).collect();
                                            first_user_msg = first_user_msg
                                                .lines()
                                                .next()
                                                .unwrap_or("")
                                                .to_string();
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
        Output::new(vec![Item::new(
            "No sessions found",
            &format!("for {}", project_path),
        )
        .valid(false)])
        .print();
        return;
    }

    let items: Vec<Item> = sessions
        .iter()
        .filter(|(_, msg, _, _)| {
            query.is_empty() || msg.to_lowercase().contains(&query.to_lowercase())
        })
        .map(|(id, msg, time, _)| {
            let arg = format!("{}|{}", id, project_path);
            Item::new(msg, time).uid(id).arg(&arg).match_field(msg)
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
    let session_file = claude_dir
        .join(&project_folder)
        .join(format!("{}.jsonl", session_id));

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
                if let Some(msg) = json
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_str())
                {
                    output.push_str("\n## User\n\n");
                    output.push_str(msg);
                    output.push_str("\n");
                }
            } else if msg_type == "assistant" {
                if let Some(content_arr) = json
                    .get("message")
                    .and_then(|m| m.get("content"))
                    .and_then(|c| c.as_array())
                {
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

fn run_windows(query: &str) {
    use std::fs;
    use std::path::PathBuf;
    use std::process::Command;
    use std::time::{Duration, SystemTime};

    // Cache directory for Alfred workflow
    let cache_dir = dirs::home_dir()
        .map(|h| {
            h.join("Library/Caches/com.runningwithcrayons.Alfred/Workflow Data/nikiv.dev.flow")
        })
        .unwrap_or_else(|| PathBuf::from("/tmp"));

    let cache_file = cache_dir.join("windows.json");
    let _ = fs::create_dir_all(&cache_dir);

    // Find Swift helper - check multiple locations
    let swift_helper = {
        let locations = [
            // Installed in workflow directory
            std::env::var("alfred_workflow_dir")
                .map(|d| PathBuf::from(d).join("bin/flow-windows"))
                .ok(),
            // Development location
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin/flow-windows")),
            // Fallback
            dirs::home_dir().map(|h| h.join(".cargo/bin/flow-windows")),
        ];
        locations.into_iter().flatten().find(|p| p.exists())
    };

    // Check cache age (valid for 2 seconds)
    let cache_age = cache_file
        .metadata()
        .ok()
        .and_then(|m| m.modified().ok())
        .and_then(|t| SystemTime::now().duration_since(t).ok())
        .unwrap_or(Duration::from_secs(999));

    let cache_valid = cache_age < Duration::from_secs(2);
    let cached_data = fs::read_to_string(&cache_file).ok();

    // If we have valid cache, return it immediately
    if cache_valid {
        if let Some(data) = &cached_data {
            let mut json: serde_json::Value = serde_json::from_str(data).unwrap_or_default();
            // Filter by query if provided
            if !query.is_empty() {
                if let Some(items) = json.get_mut("items").and_then(|i| i.as_array_mut()) {
                    let query_lower = query.to_lowercase();
                    items.retain(|item| {
                        item.get("title")
                            .and_then(|t| t.as_str())
                            .map(|t| t.to_lowercase().contains(&query_lower))
                            .unwrap_or(true)
                    });
                }
            }
            print!("{}", serde_json::to_string(&json).unwrap_or_default());
            return;
        }
    }

    // If cache is stale but exists, return it with rerun flag
    let should_rerun = cached_data.is_some() && !cache_valid;

    // Get fresh data from Swift helper
    let fresh_data = if let Some(helper) = swift_helper {
        Command::new(&helper)
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    } else {
        // Fallback to JXA if Swift helper not found
        let jxa = r#"ObjC.import('Cocoa');const f=Application('System Events').processes.whose({frontmost:true})[0];const n=f.name();const w=f.windows();const i=[];for(let j=0;j<w.length;j++){try{const t=w[j].name();if(t&&t.length>0)i.push({title:t,subtitle:n,arg:JSON.stringify({app:n,window:j,title:t}),match:t,icon:{type:'fileicon',path:f.applicationFile().posixPath()}});}catch(e){}}if(i.length===0)i.push({title:'No windows',subtitle:n,valid:false});JSON.stringify({items:i});"#;
        Command::new("osascript")
            .args(["-l", "JavaScript", "-e", jxa])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).to_string())
    };

    // Update cache with fresh data
    if let Some(ref data) = fresh_data {
        let _ = fs::write(&cache_file, data);
    }

    // Use fresh data if available, otherwise cached
    let output_data = fresh_data.or(cached_data).unwrap_or_else(|| {
        r#"{"items":[{"title":"Error","subtitle":"Could not get windows","valid":false}]}"#
            .to_string()
    });

    // Parse and filter
    let mut json: serde_json::Value = serde_json::from_str(&output_data).unwrap_or_default();

    // Add rerun flag if we returned stale cache
    if should_rerun {
        if let Some(obj) = json.as_object_mut() {
            obj.insert("rerun".to_string(), serde_json::json!(0.1));
        }
    }

    // Filter by query
    if !query.is_empty() {
        if let Some(items) = json.get_mut("items").and_then(|i| i.as_array_mut()) {
            let query_lower = query.to_lowercase();
            items.retain(|item| {
                item.get("title")
                    .and_then(|t| t.as_str())
                    .map(|t| t.to_lowercase().contains(&query_lower))
                    .unwrap_or(true)
            });
        }
    }

    print!("{}", serde_json::to_string(&json).unwrap_or_default());
}

fn run_raise_window(arg: &str) {
    use std::process::Command;

    // Find Swift helper - check multiple locations
    let swift_helper = {
        let locations = [
            std::env::var("alfred_workflow_dir")
                .map(|d| PathBuf::from(d).join("bin/flow-raise-window"))
                .ok(),
            Some(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("bin/flow-raise-window")),
            dirs::home_dir().map(|h| h.join(".cargo/bin/flow-raise-window")),
        ];
        locations.into_iter().flatten().find(|p| p.exists())
    };

    if let Some(helper) = swift_helper {
        let _ = Command::new(&helper).arg(arg).output();
    } else {
        // Fallback to JXA
        let parsed: Result<serde_json::Value, _> = serde_json::from_str(arg);
        let (app_name, window_index) = match parsed {
            Ok(json) => {
                let app = json.get("app").and_then(|a| a.as_str()).unwrap_or("");
                let idx = json.get("window").and_then(|w| w.as_i64()).unwrap_or(0);
                (app.to_string(), idx)
            }
            Err(_) => return,
        };

        let jxa = format!(
            r#"const se=Application('System Events');const p=se.processes.byName('{}');p.frontmost=true;const w=p.windows[{}];if(w)try{{w.actions.byName('AXRaise').perform();}}catch(e){{}}"#,
            app_name, window_index
        );
        let _ = Command::new("osascript")
            .args(["-l", "JavaScript", "-e", &jxa])
            .output();
    }
}

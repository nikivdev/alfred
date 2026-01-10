use std::path::PathBuf;

use clap::{Parser, Subcommand};
use flow_alfred::{discover_repos, discover_repos_structured, expand_path, fuzzy_match, fuzzy_sort, Icon, Item, Output};

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
            Item::title_only(&entry.display)
                .uid(&path_str)
                .arg(&path_str)
                .match_field(&entry.display)
                .autocomplete(&entry.display)
                .file_type()
                .icon(Icon::fileicon(&path_str))
                .quicklook(&path_str)
                .copy_text(&relative_path)
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
            Item::title_only(&entry.display)
                .uid(&path_str)
                .arg(&path_str)  // Full path for opening
                .match_field(&entry.display)
                .autocomplete(&entry.display)
                .icon(Icon::fileicon(&path_str))
                .quicklook(&path_str)
                .copy_text(&relative_path)  // Relative path for copy
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
        Ok(dest) => println!("Linked {:?} -> {:?}", workflow_path, dest),
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

use std::collections::HashSet;
use std::path::PathBuf;
use std::thread;

use flow_alfred::{
    discover_repos_cached, discover_repos_structured_cached, expand_path, fuzzy_match, fuzzy_score,
    CodeEntry, Icon, Item, Output,
};

#[derive(Debug, Clone)]
struct RepoSearchEntry {
    display: String,
    path: PathBuf,
    root: String,
    subtitle: Option<String>,
    search_text: String,
}

fn main() {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let repos_root = std::env::var("repos_root").unwrap_or_else(|_| "~/repos".to_string());
    let code_root = std::env::var("code_root").unwrap_or_else(|_| "~/code".to_string());

    run_repos_code_search(&query, &repos_root, &code_root);
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

    entries.into_iter().map(build_repo_item).collect()
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

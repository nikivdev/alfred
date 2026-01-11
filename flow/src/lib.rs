//! Alfred workflow library for Rust
//!
//! Build Alfred workflows fast with type-safe Rust.
//!
//! # Example
//! ```no_run
//! use flow_alfred::{Item, Output};
//!
//! let items = vec![
//!     Item::new("Title", "subtitle").arg("/path/to/file"),
//!     Item::new("Another", "item").valid(false),
//! ];
//! Output::new(items).print();
//! ```

use serde::Serialize;
use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Alfred JSON output wrapper
#[derive(Debug, Serialize)]
pub struct Output {
    pub items: Vec<Item>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rerun: Option<f64>,
}

impl Output {
    pub fn new(items: Vec<Item>) -> Self {
        Self { items, rerun: None }
    }

    pub fn empty() -> Self {
        Self {
            items: vec![],
            rerun: None,
        }
    }

    /// Set rerun interval in seconds (Alfred will re-query)
    pub fn rerun(mut self, seconds: f64) -> Self {
        self.rerun = Some(seconds);
        self
    }

    /// Print JSON to stdout for Alfred
    pub fn print(&self) {
        println!("{}", serde_json::to_string(self).unwrap_or_default());
    }

    /// Get JSON string
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_default()
    }
}

/// Alfred list item
#[derive(Debug, Clone, Serialize)]
pub struct Item {
    pub uid: Option<String>,
    pub title: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<Icon>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub autocomplete: Option<String>,
    #[serde(rename = "match", skip_serializing_if = "Option::is_none")]
    pub match_field: Option<String>,
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub item_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mods: Option<Mods>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<Text>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub quicklookurl: Option<String>,
}

impl Item {
    pub fn new(title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        let title = title.into();
        let subtitle = subtitle.into();
        Self {
            uid: None,
            title,
            subtitle: Some(subtitle),
            arg: None,
            icon: None,
            valid: None,
            autocomplete: None,
            match_field: None,
            item_type: None,
            mods: None,
            text: None,
            quicklookurl: None,
        }
    }

    pub fn title_only(title: impl Into<String>) -> Self {
        Self {
            uid: None,
            title: title.into(),
            subtitle: None,
            arg: None,
            icon: None,
            valid: None,
            autocomplete: None,
            match_field: None,
            item_type: None,
            mods: None,
            text: None,
            quicklookurl: None,
        }
    }

    pub fn uid(mut self, uid: impl Into<String>) -> Self {
        self.uid = Some(uid.into());
        self
    }

    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.arg = Some(arg.into());
        self
    }

    pub fn icon(mut self, icon: Icon) -> Self {
        self.icon = Some(icon);
        self
    }

    pub fn icon_path(mut self, path: impl Into<String>) -> Self {
        self.icon = Some(Icon::path(path));
        self
    }

    pub fn icon_file(mut self, path: impl Into<String>) -> Self {
        self.icon = Some(Icon::fileicon(path));
        self
    }

    pub fn valid(mut self, valid: bool) -> Self {
        self.valid = Some(valid);
        self
    }

    pub fn autocomplete(mut self, autocomplete: impl Into<String>) -> Self {
        self.autocomplete = Some(autocomplete.into());
        self
    }

    pub fn match_field(mut self, match_field: impl Into<String>) -> Self {
        self.match_field = Some(match_field.into());
        self
    }

    pub fn file_type(mut self) -> Self {
        self.item_type = Some("file".to_string());
        self
    }

    pub fn quicklook(mut self, url: impl Into<String>) -> Self {
        self.quicklookurl = Some(url.into());
        self
    }

    pub fn copy_text(mut self, text: impl Into<String>) -> Self {
        let text_val = text.into();
        self.text = Some(Text {
            copy: Some(text_val.clone()),
            largetype: None,
        });
        self
    }

    pub fn largetype(mut self, text: impl Into<String>) -> Self {
        if let Some(ref mut t) = self.text {
            t.largetype = Some(text.into());
        } else {
            self.text = Some(Text {
                copy: None,
                largetype: Some(text.into()),
            });
        }
        self
    }

    /// Set cmd modifier action (Cmd+Return)
    pub fn cmd_mod(mut self, arg: impl Into<String>, subtitle: impl Into<String>) -> Self {
        let mods = self.mods.get_or_insert_with(Mods::default);
        mods.cmd = Some(ModItem {
            valid: Some(true),
            arg: Some(arg.into()),
            subtitle: Some(subtitle.into()),
        });
        self
    }

    /// Set alt modifier action (Alt/Option+Return)
    pub fn alt_mod(mut self, arg: impl Into<String>, subtitle: impl Into<String>) -> Self {
        let mods = self.mods.get_or_insert_with(Mods::default);
        mods.alt = Some(ModItem {
            valid: Some(true),
            arg: Some(arg.into()),
            subtitle: Some(subtitle.into()),
        });
        self
    }
}

/// Icon for Alfred item
#[derive(Debug, Clone, Serialize)]
pub struct Icon {
    #[serde(rename = "type", skip_serializing_if = "Option::is_none")]
    pub icon_type: Option<String>,
    pub path: String,
}

impl Icon {
    pub fn path(path: impl Into<String>) -> Self {
        Self {
            icon_type: None,
            path: path.into(),
        }
    }

    pub fn fileicon(path: impl Into<String>) -> Self {
        Self {
            icon_type: Some("fileicon".to_string()),
            path: path.into(),
        }
    }

    pub fn filetype(uti: impl Into<String>) -> Self {
        Self {
            icon_type: Some("filetype".to_string()),
            path: uti.into(),
        }
    }
}

/// Modifier key actions
#[derive(Debug, Clone, Serialize, Default)]
pub struct Mods {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cmd: Option<ModItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub alt: Option<ModItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ctrl: Option<ModItem>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shift: Option<ModItem>,
}

/// Modifier item override
#[derive(Debug, Clone, Serialize)]
pub struct ModItem {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub valid: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub arg: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
}

/// Text for copy/largetype
#[derive(Debug, Clone, Serialize)]
pub struct Text {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub copy: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub largetype: Option<String>,
}

// ============================================================================
// Workflow Management
// ============================================================================

/// Get Alfred workflows directory
pub fn workflows_dir() -> Option<PathBuf> {
    let home = dirs_home()?;

    // Check for sync folder first (via defaults)
    if let Ok(output) = std::process::Command::new("defaults")
        .args(["read", "com.runningwithcrayons.Alfred-Preferences", "syncfolder"])
        .output()
    {
        if output.status.success() {
            let sync_folder = String::from_utf8_lossy(&output.stdout).trim().to_string();
            let expanded = expand_path(&sync_folder);
            let path = expanded.join("Alfred.alfredpreferences").join("workflows");
            if path.exists() {
                return Some(path);
            }
        }
    }

    // Fall back to default location
    let path = home
        .join("Library")
        .join("Application Support")
        .join("Alfred")
        .join("Alfred.alfredpreferences")
        .join("workflows");
    if path.exists() {
        Some(path)
    } else {
        // Create it if parent exists
        let parent = path.parent()?;
        if parent.exists() {
            fs::create_dir_all(&path).ok()?;
            Some(path)
        } else {
            None
        }
    }
}

/// Link a workflow directory into Alfred
pub fn link_workflow(workflow_dir: &Path, bundle_id: &str) -> Result<PathBuf, String> {
    let workflows = workflows_dir().ok_or("Alfred workflows directory not found")?;
    let dest = workflows.join(bundle_id);

    if dest.exists() {
        // Remove existing symlink or directory
        if dest.is_symlink() {
            fs::remove_file(&dest).map_err(|e| format!("Failed to remove symlink: {}", e))?;
        } else {
            return Err(format!("Destination exists and is not a symlink: {:?}", dest));
        }
    }

    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(workflow_dir, &dest)
            .map_err(|e| format!("Failed to create symlink: {}", e))?;
    }

    Ok(dest)
}

/// Unlink a workflow from Alfred
pub fn unlink_workflow(bundle_id: &str) -> Result<(), String> {
    let workflows = workflows_dir().ok_or("Alfred workflows directory not found")?;
    let dest = workflows.join(bundle_id);

    if dest.exists() && dest.is_symlink() {
        fs::remove_file(&dest).map_err(|e| format!("Failed to remove symlink: {}", e))?;
    }
    Ok(())
}

/// Reload a workflow in Alfred (refreshes canvas without restart)
pub fn reload_workflow(bundle_id: &str) -> Result<(), String> {
    Command::new("osascript")
        .args(["-e", &format!("tell application \"Alfred\" to reload workflow \"{}\"", bundle_id)])
        .output()
        .map_err(|e| format!("Failed to reload workflow: {}", e))?;
    Ok(())
}

/// Pack a workflow directory into .alfredworkflow file
pub fn pack_workflow(workflow_dir: &Path, output_path: &Path) -> Result<(), String> {
    Command::new("zip")
        .arg("-r")
        .arg(output_path)
        .arg(".")
        .current_dir(workflow_dir)
        .output()
        .map_err(|e| format!("Failed to create workflow package: {}", e))?;
    Ok(())
}

/// Open a .alfredworkflow file to install it
pub fn install_workflow(workflow_path: &Path) -> Result<(), String> {
    Command::new("open")
        .arg(workflow_path)
        .output()
        .map_err(|e| format!("Failed to open workflow: {}", e))?;
    Ok(())
}

// ============================================================================
// Fuzzy Matching
// ============================================================================

/// Check if query matches target fuzzily
pub fn fuzzy_match(query: &str, target: &str) -> bool {
    if query.is_empty() {
        return true;
    }
    let query = query.to_lowercase();
    let target = target.to_lowercase();

    let mut query_chars = query.chars().peekable();
    for c in target.chars() {
        if query_chars.peek() == Some(&c) {
            query_chars.next();
        }
        if query_chars.peek().is_none() {
            return true;
        }
    }
    query_chars.peek().is_none()
}

/// Score a fuzzy match (higher is better)
pub fn fuzzy_score(query: &str, target: &str) -> i32 {
    if query.is_empty() {
        return 0;
    }
    let query = query.to_lowercase();
    let target = target.to_lowercase();

    let mut score = 0;
    let mut query_chars = query.chars().peekable();
    let mut last_match_pos: Option<usize> = None;
    let mut consecutive = 0;

    for (i, c) in target.chars().enumerate() {
        if query_chars.peek() == Some(&c) {
            query_chars.next();

            // Bonus for consecutive matches
            if let Some(last) = last_match_pos {
                if i == last + 1 {
                    consecutive += 1;
                    score += consecutive * 10;
                } else {
                    consecutive = 0;
                }
            }

            // Bonus for matching at start
            if i == 0 {
                score += 20;
            }

            // Bonus for matching after separator
            if i > 0 {
                let prev = target.chars().nth(i - 1);
                if prev == Some('/') || prev == Some('-') || prev == Some('_') || prev == Some(' ')
                {
                    score += 15;
                }
            }

            last_match_pos = Some(i);
            score += 5;
        }
    }

    if query_chars.peek().is_some() {
        return -1; // Didn't match all chars
    }

    score
}

/// Sort items by fuzzy score
pub fn fuzzy_sort<T, F>(items: &mut [T], query: &str, get_str: F)
where
    F: Fn(&T) -> &str,
{
    items.sort_by(|a, b| {
        let score_a = fuzzy_score(query, get_str(a));
        let score_b = fuzzy_score(query, get_str(b));
        score_b.cmp(&score_a)
    });
}

// ============================================================================
// Code/Project Discovery
// ============================================================================

/// Entry representing a discovered code repository
pub struct CodeEntry {
    /// Display name (relative path from root)
    pub display: String,
    /// Full path to the repository
    pub path: PathBuf,
}

/// Discover git repositories under a root directory
pub fn discover_repos(root: &Path) -> Vec<CodeEntry> {
    let mut repos = Vec::new();
    let mut seen = HashSet::new();
    let mut stack = vec![root.to_path_buf()];

    while let Some(dir) = stack.pop() {
        let entries = match fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for entry in entries.flatten() {
            let path = entry.path();
            let file_type = match entry.file_type() {
                Ok(ft) => ft,
                Err(_) => continue,
            };
            if !file_type.is_dir() {
                continue;
            }

            let name = entry.file_name().to_string_lossy().to_string();
            if should_skip_dir(&name) {
                continue;
            }

            let git_dir = path.join(".git");
            if git_dir.is_dir() || git_dir.is_file() {
                let display = path
                    .strip_prefix(root)
                    .unwrap_or(&path)
                    .to_string_lossy()
                    .to_string();
                let key = path.to_string_lossy().to_string();
                if seen.insert(key) {
                    repos.push(CodeEntry { display, path });
                }
                continue;
            }

            stack.push(path);
        }
    }

    repos.sort_by(|a, b| a.display.cmp(&b.display));
    repos
}

/// Discover git repositories in owner/repo structure (like ~/repos)
pub fn discover_repos_structured(root: &Path) -> Vec<CodeEntry> {
    let mut repos = Vec::new();

    // Read owner directories
    let owners = match fs::read_dir(root) {
        Ok(entries) => entries,
        Err(_) => return repos,
    };

    for owner_entry in owners.flatten() {
        let owner_path = owner_entry.path();
        if !owner_path.is_dir() {
            continue;
        }

        let owner_name = match owner_path.file_name() {
            Some(name) => name.to_string_lossy().to_string(),
            None => continue,
        };

        // Skip hidden directories
        if owner_name.starts_with('.') {
            continue;
        }

        // Read repo directories under each owner
        let repo_entries = match fs::read_dir(&owner_path) {
            Ok(entries) => entries,
            Err(_) => continue,
        };

        for repo_entry in repo_entries.flatten() {
            let repo_path = repo_entry.path();
            if !repo_path.is_dir() {
                continue;
            }

            let repo_name = match repo_path.file_name() {
                Some(name) => name.to_string_lossy().to_string(),
                None => continue,
            };

            // Skip hidden directories
            if repo_name.starts_with('.') {
                continue;
            }

            // Check if it's a git repo
            if repo_path.join(".git").exists() {
                repos.push(CodeEntry {
                    display: format!("{}/{}", owner_name, repo_name),
                    path: repo_path,
                });
            }
        }
    }

    repos.sort_by(|a, b| a.display.cmp(&b.display));
    repos
}

fn should_skip_dir(name: &str) -> bool {
    if name.starts_with('.') {
        return true;
    }
    matches!(
        name,
        "node_modules"
            | "target"
            | "dist"
            | "build"
            | "__pycache__"
            | ".pytest_cache"
            | ".mypy_cache"
            | "venv"
            | ".venv"
            | "vendor"
            | "Pods"
            | ".cargo"
            | ".rustup"
            | ".next"
            | ".turbo"
            | ".cache"
    )
}

// ============================================================================
// Workflow Object Builders (for info.plist generation)
// ============================================================================

/// Argument type for Script Filters
#[derive(Debug, Clone, Copy, Default)]
pub enum ArgumentType {
    /// Argument is required (argumenttype = 0)
    Required,
    /// Argument is optional (argumenttype = 1) - USE THIS for external triggers
    #[default]
    Optional,
    /// No argument accepted (argumenttype = 2)
    None,
}

impl ArgumentType {
    pub fn to_plist_value(&self) -> u8 {
        match self {
            ArgumentType::Required => 0,
            ArgumentType::Optional => 1,
            ArgumentType::None => 2,
        }
    }
}

/// Script Filter configuration
#[derive(Debug, Clone)]
pub struct ScriptFilter {
    pub uid: String,
    pub keyword: String,
    pub title: String,
    pub subtitle: String,
    pub running_subtext: String,
    pub script: String,
    pub argument_type: ArgumentType,
    pub with_space: bool,
    pub alfred_filters_results: bool,
    pub queue_delay_immediately: bool,
}

impl ScriptFilter {
    pub fn new(uid: &str, keyword: &str) -> Self {
        Self {
            uid: uid.to_string(),
            keyword: keyword.to_string(),
            title: String::new(),
            subtitle: String::new(),
            running_subtext: "Loading...".to_string(),
            script: String::new(),
            argument_type: ArgumentType::Optional, // Default to optional for external trigger support
            with_space: false,
            alfred_filters_results: false,
            queue_delay_immediately: true,
        }
    }

    pub fn title(mut self, title: &str) -> Self {
        self.title = title.to_string();
        self
    }

    pub fn subtitle(mut self, subtitle: &str) -> Self {
        self.subtitle = subtitle.to_string();
        self
    }

    pub fn running_subtext(mut self, text: &str) -> Self {
        self.running_subtext = text.to_string();
        self
    }

    pub fn script(mut self, script: &str) -> Self {
        self.script = script.to_string();
        self
    }

    pub fn argument_type(mut self, arg_type: ArgumentType) -> Self {
        self.argument_type = arg_type;
        self
    }

    pub fn with_space(mut self, with_space: bool) -> Self {
        self.with_space = with_space;
        self
    }

    pub fn alfred_filters_results(mut self, filters: bool) -> Self {
        self.alfred_filters_results = filters;
        self
    }

    /// Generate plist XML for this Script Filter object
    pub fn to_plist_object(&self) -> String {
        let script_escaped = xml_escape(&self.script);
        format!(
            r#"<dict>
    <key>config</key>
    <dict>
        <key>alfredfiltersresults</key>
        <{alfredfiltersresults}/>
        <key>alfredfiltersresultsmatchmode</key>
        <integer>2</integer>
        <key>argumenttreatemptyqueryasnil</key>
        <false/>
        <key>argumenttrimmode</key>
        <integer>0</integer>
        <key>argumenttype</key>
        <integer>{argumenttype}</integer>
        <key>escaping</key>
        <integer>102</integer>
        <key>keyword</key>
        <string>{keyword}</string>
        <key>queuedelaycustom</key>
        <integer>1</integer>
        <key>queuedelayimmediatelyinitially</key>
        <{queuedelayimmediately}/>
        <key>queuedelaymode</key>
        <integer>0</integer>
        <key>queuemode</key>
        <integer>1</integer>
        <key>runningsubtext</key>
        <string>{runningsubtext}</string>
        <key>script</key>
        <string>{script}</string>
        <key>scriptargtype</key>
        <integer>1</integer>
        <key>scriptfile</key>
        <string></string>
        <key>subtext</key>
        <string>{subtitle}</string>
        <key>title</key>
        <string>{title}</string>
        <key>type</key>
        <integer>0</integer>
        <key>withspace</key>
        <{withspace}/>
    </dict>
    <key>type</key>
    <string>alfred.workflow.input.scriptfilter</string>
    <key>uid</key>
    <string>{uid}</string>
    <key>version</key>
    <integer>3</integer>
</dict>"#,
            alfredfiltersresults = if self.alfred_filters_results { "true" } else { "false" },
            argumenttype = self.argument_type.to_plist_value(),
            keyword = xml_escape(&self.keyword),
            queuedelayimmediately = if self.queue_delay_immediately { "true" } else { "false" },
            runningsubtext = xml_escape(&self.running_subtext),
            script = script_escaped,
            subtitle = xml_escape(&self.subtitle),
            title = xml_escape(&self.title),
            withspace = if self.with_space { "true" } else { "false" },
            uid = &self.uid,
        )
    }
}

/// External Trigger configuration
#[derive(Debug, Clone)]
pub struct ExternalTrigger {
    pub uid: String,
    pub trigger_id: String,
    pub available_via_url: bool,
}

impl ExternalTrigger {
    pub fn new(uid: &str, trigger_id: &str) -> Self {
        Self {
            uid: uid.to_string(),
            trigger_id: trigger_id.to_string(),
            available_via_url: false,
        }
    }

    pub fn available_via_url(mut self, available: bool) -> Self {
        self.available_via_url = available;
        self
    }

    /// Generate plist XML for this External Trigger object
    pub fn to_plist_object(&self) -> String {
        format!(
            r#"<dict>
    <key>config</key>
    <dict>
        <key>availableviaurlhandler</key>
        <{available_via_url}/>
        <key>triggerid</key>
        <string>{trigger_id}</string>
    </dict>
    <key>type</key>
    <string>alfred.workflow.trigger.external</string>
    <key>uid</key>
    <string>{uid}</string>
    <key>version</key>
    <integer>1</integer>
</dict>"#,
            available_via_url = if self.available_via_url { "true" } else { "false" },
            trigger_id = xml_escape(&self.trigger_id),
            uid = &self.uid,
        )
    }
}

/// Open File action configuration
#[derive(Debug, Clone)]
pub struct OpenFileAction {
    pub uid: String,
    pub open_with: Option<String>,
}

impl OpenFileAction {
    pub fn new(uid: &str) -> Self {
        Self {
            uid: uid.to_string(),
            open_with: None,
        }
    }

    pub fn open_with(mut self, app_bundle_id: &str) -> Self {
        self.open_with = Some(app_bundle_id.to_string());
        self
    }

    /// Generate plist XML for this Open File action
    pub fn to_plist_object(&self) -> String {
        let open_with = self.open_with.as_deref().unwrap_or("");
        format!(
            r#"<dict>
    <key>config</key>
    <dict>
        <key>openwith</key>
        <string>{open_with}</string>
        <key>sourcefile</key>
        <string>{{query}}</string>
    </dict>
    <key>type</key>
    <string>alfred.workflow.action.openfile</string>
    <key>uid</key>
    <string>{uid}</string>
    <key>version</key>
    <integer>3</integer>
</dict>"#,
            open_with = xml_escape(open_with),
            uid = &self.uid,
        )
    }
}

/// Connection between workflow objects
#[derive(Debug, Clone)]
pub struct Connection {
    pub source_uid: String,
    pub dest_uid: String,
    pub modifiers: u32, // 0 = none, 1048576 = cmd
}

impl Connection {
    pub fn new(source: &str, dest: &str) -> Self {
        Self {
            source_uid: source.to_string(),
            dest_uid: dest.to_string(),
            modifiers: 0,
        }
    }

    pub fn with_cmd(mut self) -> Self {
        self.modifiers = 1048576;
        self
    }
}

/// UI position for workflow canvas
#[derive(Debug, Clone)]
pub struct UIPosition {
    pub uid: String,
    pub x: f64,
    pub y: f64,
}

impl UIPosition {
    pub fn new(uid: &str, x: f64, y: f64) -> Self {
        Self {
            uid: uid.to_string(),
            x,
            y,
        }
    }
}

/// Helper to escape XML special characters
fn xml_escape(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
        .replace('"', "&quot;")
        .replace('\'', "&apos;")
}

// ============================================================================
// Utilities
// ============================================================================

/// Expand ~ to home directory
pub fn expand_path(path: &str) -> PathBuf {
    if path.starts_with("~/") {
        if let Some(home) = dirs_home() {
            return home.join(&path[2..]);
        }
    }
    PathBuf::from(path)
}

fn dirs_home() -> Option<PathBuf> {
    std::env::var("HOME").ok().map(PathBuf::from)
}

/// Get environment variable set by Alfred
pub fn env(name: &str) -> Option<String> {
    std::env::var(format!("alfred_{}", name)).ok()
}

/// Check if running inside Alfred
pub fn in_alfred() -> bool {
    std::env::var("alfred_version").is_ok()
}

/// Get workflow bundle ID from environment
pub fn bundle_id() -> Option<String> {
    std::env::var("alfred_workflow_bundleid").ok()
}

/// Get workflow data directory
pub fn data_dir() -> Option<PathBuf> {
    std::env::var("alfred_workflow_data").ok().map(PathBuf::from)
}

/// Get workflow cache directory
pub fn cache_dir() -> Option<PathBuf> {
    std::env::var("alfred_workflow_cache")
        .ok()
        .map(PathBuf::from)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_item_builder() {
        let item = Item::new("Title", "Subtitle")
            .arg("/path")
            .uid("123")
            .valid(true);

        assert_eq!(item.title, "Title");
        assert_eq!(item.subtitle, Some("Subtitle".to_string()));
        assert_eq!(item.arg, Some("/path".to_string()));
        assert_eq!(item.uid, Some("123".to_string()));
        assert_eq!(item.valid, Some(true));
    }

    #[test]
    fn test_output_json() {
        let output = Output::new(vec![Item::new("Test", "Sub").arg("val")]);
        let json = output.to_json();
        assert!(json.contains("\"title\":\"Test\""));
        assert!(json.contains("\"arg\":\"val\""));
    }

    #[test]
    fn test_fuzzy_match() {
        assert!(fuzzy_match("fc", "flow-code"));
        assert!(fuzzy_match("fl", "flow"));
        assert!(fuzzy_match("", "anything"));
        assert!(!fuzzy_match("xyz", "abc"));
    }

    #[test]
    fn test_fuzzy_score() {
        // Exact prefix should score higher
        let score_prefix = fuzzy_score("fl", "flow");
        let score_middle = fuzzy_score("fl", "alfred");
        assert!(score_prefix > score_middle);
    }
}

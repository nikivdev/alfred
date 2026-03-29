use std::collections::HashSet;
use std::fs::{self, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use flow_alfred::{
    cache_dir, discover_repos_structured_with_config, discover_repos_with_config, expand_path,
    CodeEntry, Config, Icon, Item, Output,
};
use rusqlite::{params, Connection, OpenFlags, OptionalExtension};

const INDEX_SCHEMA_VERSION: i64 = 1;
const INDEX_SOFT_TTL: Duration = Duration::from_secs(10 * 60);
const REFRESH_LOCK_STALE_AFTER: Duration = Duration::from_secs(10 * 60);
const INDEX_FILENAME: &str = "repos-code-index.sqlite3";
const REFRESH_LOCK_HELD_ENV: &str = "FLOW_REPOS_CODE_LOCK_HELD";
const INDEX_PATH_ENV: &str = "FLOW_REPOS_CODE_INDEX_PATH";
const INDEX_RERUN_SECS: f64 = 0.2;
const MAX_EMPTY_QUERY_ITEMS: usize = 32;
const GENERIC_FOLDER_ICON_PATH: &str =
    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericFolderIcon.icns";

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum RootKind {
    Repos,
    Code,
}

impl RootKind {
    fn as_str(self) -> &'static str {
        match self {
            Self::Repos => "repos",
            Self::Code => "code",
        }
    }

    fn discovery_mode(self) -> &'static str {
        match self {
            Self::Repos => "structured",
            Self::Code => "flat",
        }
    }

    fn from_str(value: &str) -> Option<Self> {
        match value {
            "repos" => Some(Self::Repos),
            "code" => Some(Self::Code),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
struct RepoSearchEntry {
    display: String,
    path: PathBuf,
    root_kind: RootKind,
    match_text_lower: String,
}

#[derive(Debug)]
struct SearchContext {
    repos_root: String,
    code_root: String,
    repos_root_path: PathBuf,
    code_root_path: PathBuf,
    repos_exists: bool,
    code_exists: bool,
    config: Config,
    config_hash: String,
    db_path: PathBuf,
    lock_path: PathBuf,
}

#[derive(Debug)]
struct IndexSnapshot {
    entries: Vec<RepoSearchEntry>,
    last_full_refresh_at: Option<u64>,
    config_hash: Option<String>,
}

#[derive(Debug)]
enum IndexReadError {
    Missing,
    #[allow(dead_code)]
    Broken(String),
}

#[derive(Debug)]
struct RefreshLock {
    path: PathBuf,
    release_on_drop: bool,
}

#[derive(Debug)]
struct RootScanResult {
    kind: RootKind,
    root_path: PathBuf,
    started_at: u64,
    finished_at: u64,
    status: &'static str,
    error: Option<String>,
    entries: Vec<CodeEntry>,
}

fn main() {
    let args = std::env::args().skip(1).collect::<Vec<_>>();
    let context = SearchContext::load();

    if args.len() == 1 && args[0] == "--refresh-index" {
        run_refresh_index_command(&context);
        return;
    }

    let query = args.join(" ");
    run_repos_code_search(&query, &context);
}

impl SearchContext {
    fn load() -> Self {
        let repos_root = std::env::var("repos_root").unwrap_or_else(|_| "~/repos".to_string());
        let code_root = std::env::var("code_root").unwrap_or_else(|_| "~/code".to_string());
        let config = Config::load();
        let repos_root_path = expand_path(&repos_root);
        let code_root_path = expand_path(&code_root);
        let db_path = index_db_path();
        let lock_path = refresh_lock_path(&db_path);

        Self::new(repos_root, code_root, config, repos_root_path, code_root_path, db_path, lock_path)
    }

    #[cfg(test)]
    fn for_test(
        repos_root: String,
        code_root: String,
        config: Config,
        db_path: PathBuf,
    ) -> Self {
        let repos_root_path = expand_path(&repos_root);
        let code_root_path = expand_path(&code_root);
        let lock_path = refresh_lock_path(&db_path);
        Self::new(repos_root, code_root, config, repos_root_path, code_root_path, db_path, lock_path)
    }

    fn new(
        repos_root: String,
        code_root: String,
        config: Config,
        repos_root_path: PathBuf,
        code_root_path: PathBuf,
        db_path: PathBuf,
        lock_path: PathBuf,
    ) -> Self {
        let config_hash =
            compute_config_hash(&repos_root_path, &code_root_path, &config, INDEX_SCHEMA_VERSION);

        Self {
            repos_exists: repos_root_path.exists(),
            code_exists: code_root_path.exists(),
            repos_root,
            code_root,
            repos_root_path,
            code_root_path,
            config,
            config_hash,
            db_path,
            lock_path,
        }
    }

    fn scope_description(&self) -> String {
        match (self.repos_exists, self.code_exists) {
            (true, true) => format!("in {} and {}", self.repos_root, self.code_root),
            (true, false) => format!("in {}", self.repos_root),
            (false, true) => format!("in {}", self.code_root),
            (false, false) => format!("in {} or {}", self.repos_root, self.code_root),
        }
    }

    fn copy_root(&self, kind: RootKind) -> &str {
        match kind {
            RootKind::Repos => &self.repos_root,
            RootKind::Code => &self.code_root,
        }
    }
}

impl IndexSnapshot {
    fn is_stale(&self, context: &SearchContext) -> bool {
        if self.config_hash.as_deref() != Some(context.config_hash.as_str()) {
            return true;
        }

        match self.last_full_refresh_at {
            Some(last_refresh_at) => now_unix_timestamp().saturating_sub(last_refresh_at)
                > INDEX_SOFT_TTL.as_secs(),
            None => true,
        }
    }
}

impl RefreshLock {
    fn acquire(path: &Path) -> Option<Self> {
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }

        match OpenOptions::new().write(true).create_new(true).open(path) {
            Ok(_) => Some(Self {
                path: path.to_path_buf(),
                release_on_drop: true,
            }),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                if lock_is_stale(path) {
                    let _ = fs::remove_file(path);
                    return Self::acquire(path);
                }
                None
            }
            Err(_) => None,
        }
    }

    fn assume_held(path: PathBuf) -> Self {
        Self {
            path,
            release_on_drop: true,
        }
    }

    fn disarm(mut self) {
        self.release_on_drop = false;
    }
}

impl Drop for RefreshLock {
    fn drop(&mut self) {
        if self.release_on_drop {
            let _ = fs::remove_file(&self.path);
        }
    }
}

fn run_repos_code_search(query: &str, context: &SearchContext) {
    if !context.repos_exists && !context.code_exists {
        print_missing_root(
            &format!("{} or {}", context.repos_root, context.code_root),
            "Check your repos_root and code_root settings",
        );
        return;
    }

    match read_index_snapshot(context) {
        Ok(snapshot) => {
            let is_stale = snapshot.is_stale(context);

            if snapshot.entries.is_empty() {
                if is_stale && maybe_spawn_background_refresh(context) {
                    print_refreshing_index(context);
                    return;
                }

                let entries = discover_entries_from_disk(context);
                render_entries(query, context, entries, None);
                return;
            }

            let rerun = if is_stale && maybe_spawn_background_refresh(context) {
                Some(INDEX_RERUN_SECS)
            } else {
                None
            };

            render_entries(query, context, snapshot.entries, rerun);
        }
        Err(IndexReadError::Broken(_)) => {
            quarantine_index_files(&context.db_path);
            if maybe_spawn_background_refresh(context) {
                print_refreshing_index(context);
                return;
            }

            let entries = discover_entries_from_disk(context);
            render_entries(query, context, entries, None);
        }
        Err(IndexReadError::Missing) => {
            if maybe_spawn_background_refresh(context) {
                print_refreshing_index(context);
                return;
            }

            let entries = discover_entries_from_disk(context);
            render_entries(query, context, entries, None);
        }
    }
}

fn run_refresh_index_command(context: &SearchContext) {
    if !context.repos_exists && !context.code_exists {
        return;
    }

    let held_lock = std::env::var_os(REFRESH_LOCK_HELD_ENV)
        .map(|_| RefreshLock::assume_held(context.lock_path.clone()));
    let lock = match held_lock.or_else(|| RefreshLock::acquire(&context.lock_path)) {
        Some(lock) => lock,
        None => return,
    };

    let _ = refresh_index_locked(context, lock);
}

#[cfg(test)]
fn load_or_bootstrap_snapshot(context: &SearchContext) -> Result<IndexSnapshot, String> {
    let mut bootstrapped = false;

    loop {
        match read_index_snapshot(context) {
            Ok(snapshot) => return Ok(snapshot),
            Err(IndexReadError::Missing) if !bootstrapped => {
                bootstrapped = true;
            }
            Err(IndexReadError::Broken(_)) if !bootstrapped => {
                bootstrapped = true;
                quarantine_index_files(&context.db_path);
            }
            Err(IndexReadError::Broken(err)) => return Err(err),
            Err(IndexReadError::Missing) => return Err("index missing".to_string()),
        }

        refresh_index_sync(context)?;
    }
}

fn read_index_snapshot(context: &SearchContext) -> Result<IndexSnapshot, IndexReadError> {
    if !context.db_path.exists() {
        return Err(IndexReadError::Missing);
    }

    let conn = Connection::open_with_flags(
        &context.db_path,
        OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
    )
    .map_err(|err| IndexReadError::Broken(err.to_string()))?;

    conn.busy_timeout(Duration::from_millis(25))
        .map_err(|err| IndexReadError::Broken(err.to_string()))?;

    let entries = load_index_entries(&conn).map_err(|err| IndexReadError::Broken(err.to_string()))?;
    let last_full_refresh_at = load_meta_value(&conn, "last_full_refresh_at")
        .map_err(|err| IndexReadError::Broken(err.to_string()))?
        .and_then(|value| value.parse::<u64>().ok());
    let config_hash = load_meta_value(&conn, "config_hash")
        .map_err(|err| IndexReadError::Broken(err.to_string()))?;
    let schema_version = load_meta_value(&conn, "schema_version")
        .map_err(|err| IndexReadError::Broken(err.to_string()))?;

    if schema_version
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default()
        != INDEX_SCHEMA_VERSION
    {
        return Err(IndexReadError::Broken("schema version mismatch".to_string()));
    }

    Ok(IndexSnapshot {
        entries,
        last_full_refresh_at,
        config_hash,
    })
}

fn load_index_entries(conn: &Connection) -> rusqlite::Result<Vec<RepoSearchEntry>> {
    let mut stmt = conn.prepare(
        "SELECT display, path, root_kind, match_text_lower
         FROM entries
         WHERE exists_now = 1",
    )?;

    let rows = stmt.query_map([], |row| {
        let root_kind_raw: String = row.get(2)?;
        let root_kind = RootKind::from_str(&root_kind_raw).ok_or_else(|| {
            rusqlite::Error::InvalidParameterName(format!("invalid root kind: {}", root_kind_raw))
        })?;

        let path: String = row.get(1)?;
        Ok(RepoSearchEntry {
            display: row.get(0)?,
            path: PathBuf::from(path),
            root_kind,
            match_text_lower: row.get(3)?,
        })
    })?;

    rows.collect()
}

fn load_meta_value(conn: &Connection, key: &str) -> rusqlite::Result<Option<String>> {
    conn.query_row("SELECT value FROM meta WHERE key = ?1", [key], |row| row.get(0))
        .optional()
}

#[cfg(test)]
fn refresh_index_sync(context: &SearchContext) -> Result<(), String> {
    let Some(lock) = RefreshLock::acquire(&context.lock_path) else {
        return Err("refresh already running".to_string());
    };

    refresh_index_locked(context, lock)
}

fn refresh_index_locked(context: &SearchContext, _lock: RefreshLock) -> Result<(), String> {
    if let Some(parent) = context.db_path.parent() {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }

    let mut conn = Connection::open(&context.db_path).map_err(|err| err.to_string())?;
    conn.busy_timeout(Duration::from_millis(250))
        .map_err(|err| err.to_string())?;
    conn.pragma_update(None, "journal_mode", "WAL")
        .map_err(|err| err.to_string())?;
    conn.pragma_update(None, "synchronous", "NORMAL")
        .map_err(|err| err.to_string())?;
    initialize_schema(&conn).map_err(|err| err.to_string())?;

    let generation = load_meta_value(&conn, "index_generation")
        .map_err(|err| err.to_string())?
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or_default()
        + 1;

    let root_results = scan_roots(context);
    let any_success = root_results.iter().any(|result| result.status == "ok");
    let finished_at = now_unix_timestamp();

    let tx = conn.transaction().map_err(|err| err.to_string())?;
    initialize_schema(&tx).map_err(|err| err.to_string())?;

    for result in &root_results {
        write_root_status(&tx, result, &context.config_hash).map_err(|err| err.to_string())?;

        if result.status != "ok" {
            continue;
        }

        for entry in &result.entries {
            let match_text = format!("{}/{}", result.kind.as_str(), entry.display);
            tx.execute(
                "INSERT INTO entries (
                    path,
                    root_kind,
                    root_path,
                    display,
                    match_text,
                    match_text_lower,
                    exists_now,
                    last_seen_generation,
                    last_seen_at
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 1, ?7, ?8)
                 ON CONFLICT(path) DO UPDATE SET
                    root_kind = excluded.root_kind,
                    root_path = excluded.root_path,
                    display = excluded.display,
                    match_text = excluded.match_text,
                    match_text_lower = excluded.match_text_lower,
                    exists_now = 1,
                    last_seen_generation = excluded.last_seen_generation,
                    last_seen_at = excluded.last_seen_at",
                params![
                    entry.path.to_string_lossy().to_string(),
                    result.kind.as_str(),
                    result.root_path.to_string_lossy().to_string(),
                    entry.display,
                    match_text,
                    match_text.to_lowercase(),
                    generation,
                    finished_at as i64,
                ],
            )
            .map_err(|err| err.to_string())?;
        }

        tx.execute(
            "UPDATE entries
             SET exists_now = 0
             WHERE root_kind = ?1
               AND (root_path <> ?2 OR last_seen_generation <> ?3)",
            params![
                result.kind.as_str(),
                result.root_path.to_string_lossy().to_string(),
                generation,
            ],
        )
        .map_err(|err| err.to_string())?;
    }

    if any_success {
        set_meta_value(&tx, "schema_version", INDEX_SCHEMA_VERSION.to_string())
            .map_err(|err| err.to_string())?;
        set_meta_value(&tx, "config_hash", context.config_hash.clone())
            .map_err(|err| err.to_string())?;
        set_meta_value(&tx, "last_full_refresh_at", finished_at.to_string())
            .map_err(|err| err.to_string())?;
        set_meta_value(&tx, "index_generation", generation.to_string())
            .map_err(|err| err.to_string())?;
    }

    tx.commit().map_err(|err| err.to_string())
}

fn write_root_status(
    tx: &Connection,
    result: &RootScanResult,
    config_hash: &str,
) -> rusqlite::Result<()> {
    tx.execute(
        "INSERT INTO roots (
            root_kind,
            root_path,
            discovery_mode,
            config_hash,
            last_scan_started_at,
            last_scan_finished_at,
            last_scan_status,
            last_scan_error
         ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)
         ON CONFLICT(root_kind) DO UPDATE SET
            root_path = excluded.root_path,
            discovery_mode = excluded.discovery_mode,
            config_hash = excluded.config_hash,
            last_scan_started_at = excluded.last_scan_started_at,
            last_scan_finished_at = excluded.last_scan_finished_at,
            last_scan_status = excluded.last_scan_status,
            last_scan_error = excluded.last_scan_error",
        params![
            result.kind.as_str(),
            result.root_path.to_string_lossy().to_string(),
            result.kind.discovery_mode(),
            config_hash,
            result.started_at as i64,
            result.finished_at as i64,
            result.status,
            result.error.clone(),
        ],
    )?;

    Ok(())
}

fn set_meta_value(conn: &Connection, key: &str, value: String) -> rusqlite::Result<()> {
    conn.execute(
        "INSERT INTO meta (key, value) VALUES (?1, ?2)
         ON CONFLICT(key) DO UPDATE SET value = excluded.value",
        params![key, value],
    )?;
    Ok(())
}

fn initialize_schema(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS meta (
            key TEXT PRIMARY KEY,
            value TEXT NOT NULL
        );

        CREATE TABLE IF NOT EXISTS roots (
            root_kind TEXT PRIMARY KEY,
            root_path TEXT NOT NULL,
            discovery_mode TEXT NOT NULL,
            config_hash TEXT NOT NULL,
            last_scan_started_at INTEGER,
            last_scan_finished_at INTEGER,
            last_scan_status TEXT NOT NULL,
            last_scan_error TEXT
        );

        CREATE TABLE IF NOT EXISTS entries (
            path TEXT PRIMARY KEY,
            root_kind TEXT NOT NULL,
            root_path TEXT NOT NULL,
            display TEXT NOT NULL,
            match_text TEXT NOT NULL,
            match_text_lower TEXT NOT NULL,
            exists_now INTEGER NOT NULL DEFAULT 1,
            last_seen_generation INTEGER NOT NULL,
            last_seen_at INTEGER NOT NULL
        );

        CREATE INDEX IF NOT EXISTS entries_exists_root_kind_idx
            ON entries (exists_now, root_kind);
        CREATE INDEX IF NOT EXISTS entries_last_seen_generation_idx
            ON entries (last_seen_generation);",
    )
}

fn maybe_spawn_background_refresh(context: &SearchContext) -> bool {
    let Some(lock) = RefreshLock::acquire(&context.lock_path) else {
        return true;
    };

    let Ok(current_exe) = std::env::current_exe() else {
        return false;
    };

    let spawn_result = Command::new(current_exe)
        .arg("--refresh-index")
        .env(REFRESH_LOCK_HELD_ENV, "1")
        .env("repos_root", &context.repos_root)
        .env("code_root", &context.code_root)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();

    match spawn_result {
        Ok(_) => {
            lock.disarm();
            true
        }
        Err(_) => false,
    }
}

fn scan_roots(context: &SearchContext) -> Vec<RootScanResult> {
    thread::scope(|scope| {
        let repos_scan = scope.spawn(|| {
            scan_root(
                RootKind::Repos,
                &context.repos_root_path,
                context.repos_exists,
                &context.config,
            )
        });
        let code_scan = scope.spawn(|| {
            scan_root(
                RootKind::Code,
                &context.code_root_path,
                context.code_exists,
                &context.config,
            )
        });

        vec![repos_scan.join().unwrap(), code_scan.join().unwrap()]
    })
}

fn scan_root(kind: RootKind, root_path: &Path, exists: bool, config: &Config) -> RootScanResult {
    let started_at = now_unix_timestamp();

    if !exists {
        return RootScanResult {
            kind,
            root_path: root_path.to_path_buf(),
            started_at,
            finished_at: now_unix_timestamp(),
            status: "missing",
            error: Some("root does not exist".to_string()),
            entries: Vec::new(),
        };
    }

    let entries = match kind {
        RootKind::Repos => discover_repos_structured_with_config(root_path, config),
        RootKind::Code => discover_repos_with_config(root_path, config),
    };

    RootScanResult {
        kind,
        root_path: root_path.to_path_buf(),
        started_at,
        finished_at: now_unix_timestamp(),
        status: "ok",
        error: None,
        entries,
    }
}

fn discover_entries_from_disk(context: &SearchContext) -> Vec<RepoSearchEntry> {
    let mut entries = Vec::new();

    if context.repos_exists {
        entries.extend(
            discover_repos_structured_with_config(&context.repos_root_path, &context.config)
                .into_iter()
                .map(|entry| repo_search_entry(entry, RootKind::Repos)),
        );
    }

    if context.code_exists {
        entries.extend(
            discover_repos_with_config(&context.code_root_path, &context.config)
                .into_iter()
                .map(|entry| repo_search_entry(entry, RootKind::Code)),
        );
    }

    entries
}

fn render_entries(
    query: &str,
    context: &SearchContext,
    entries: Vec<RepoSearchEntry>,
    rerun: Option<f64>,
) {
    if entries.is_empty() {
        print_no_repos(&context.scope_description());
        return;
    }

    let items = build_repo_items(query, entries, context);
    let output = if let Some(seconds) = rerun {
        Output::new(items).rerun(seconds)
    } else {
        Output::new(items)
    };
    output.print();
}

fn repo_search_entry(entry: CodeEntry, root_kind: RootKind) -> RepoSearchEntry {
    let match_text = format!("{}/{}", root_kind.as_str(), entry.display);

    RepoSearchEntry {
        display: entry.display,
        path: entry.path,
        root_kind,
        match_text_lower: match_text.to_lowercase(),
    }
}

fn build_repo_items(query: &str, entries: Vec<RepoSearchEntry>, context: &SearchContext) -> Vec<Item> {
    let mut seen = HashSet::new();
    let mut entries: Vec<RepoSearchEntry> = entries
        .into_iter()
        .filter(|entry| seen.insert(entry.path.clone()))
        .collect();

    let query = query.trim();
    if query.is_empty() {
        entries.sort_by(|left, right| left.display.cmp(&right.display));
        if entries.len() > MAX_EMPTY_QUERY_ITEMS {
            entries.truncate(MAX_EMPTY_QUERY_ITEMS);
        }
    } else {
        entries = filter_and_sort_simple(entries, query);
    }

    entries
        .into_iter()
        .map(|entry| build_repo_item(entry, context))
        .collect()
}

fn filter_and_sort_simple(entries: Vec<RepoSearchEntry>, query: &str) -> Vec<RepoSearchEntry> {
    let pattern = SimplePattern::new(query);
    let mut entries: Vec<_> = entries
        .into_iter()
        .filter_map(|entry| {
            simple_pattern_score(&pattern, &entry.match_text_lower).map(|score| (entry, score))
        })
        .collect();

    entries.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(&left_score)
            .then_with(|| left.display.cmp(&right.display))
    });

    entries.into_iter().map(|(entry, _)| entry).collect()
}

#[derive(Debug, Clone)]
struct SimplePattern {
    tokens: Vec<String>,
}

impl SimplePattern {
    fn new(query: &str) -> Self {
        let tokens = query
            .split_whitespace()
            .filter(|token| !token.is_empty())
            .map(|token| token.to_lowercase())
            .collect();
        Self { tokens }
    }
}

fn simple_pattern_score(pattern: &SimplePattern, target_lower: &str) -> Option<i32> {
    if pattern.tokens.is_empty() {
        return Some(0);
    }

    let mut score = 0;
    for token in &pattern.tokens {
        let token_score = simple_fuzzy_score_lower(token, target_lower);
        if token_score < 0 {
            return None;
        }
        score += token_score;
    }

    if pattern.tokens.len() > 1 {
        score += (pattern.tokens.len() as i32 - 1) * 10;
    }

    Some(score)
}

fn simple_fuzzy_score_lower(query_lower: &str, target_lower: &str) -> i32 {
    if query_lower.is_empty() {
        return 0;
    }

    let mut score = 0;
    let mut query_chars = query_lower.chars().peekable();
    let mut last_match_pos: Option<usize> = None;
    let mut consecutive = 0;
    let target_chars: Vec<char> = target_lower.chars().collect();

    for (i, c) in target_chars.iter().copied().enumerate() {
        if query_chars.peek() == Some(&c) {
            query_chars.next();

            if let Some(last) = last_match_pos {
                if i == last + 1 {
                    consecutive += 1;
                    score += consecutive * 10;
                } else {
                    consecutive = 0;
                }
            }

            if i == 0 {
                score += 20;
            } else if matches!(target_chars[i - 1], '/' | '-' | '_' | ' ') {
                score += 15;
            }

            last_match_pos = Some(i);
            score += 5;
        }
    }

    if query_chars.peek().is_some() {
        return -1;
    }

    score
}

fn build_repo_item(entry: RepoSearchEntry, context: &SearchContext) -> Item {
    let path_str = entry.path.to_string_lossy().to_string();
    let relative_path = format!("{}/{}", context.copy_root(entry.root_kind), entry.display);
    let display = condensed_repo_display(&entry.display);

    Item::new(&display, entry.root_kind.as_str())
        .uid(&path_str)
        .arg(&path_str)
        .autocomplete(&entry.display)
        .file_type()
        .icon(Icon::path(GENERIC_FOLDER_ICON_PATH))
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

fn print_refreshing_index(context: &SearchContext) {
    Output::new(vec![Item::new("Indexing repos and code…", context.scope_description())
        .valid(false)
        .icon(Icon::path(GENERIC_FOLDER_ICON_PATH))])
    .rerun(INDEX_RERUN_SECS)
    .print();
}

fn index_db_path() -> PathBuf {
    if let Some(path) = std::env::var_os(INDEX_PATH_ENV) {
        return PathBuf::from(path);
    }

    index_root().join(INDEX_FILENAME)
}

fn refresh_lock_path(db_path: &Path) -> PathBuf {
    db_path.with_extension("refresh.lock")
}

fn index_root() -> PathBuf {
    cache_dir().unwrap_or_else(|| {
        dirs::home_dir()
            .map(|home| {
                home.join("Library/Caches/com.runningwithcrayons.Alfred/Workflow Data")
                    .join("nikiv.dev.flow")
            })
            .unwrap_or_else(|| std::env::temp_dir().join("nikiv.dev.flow"))
    })
}

fn quarantine_index_files(db_path: &Path) {
    let timestamp = now_unix_timestamp();
    for suffix in ["", "-shm", "-wal"] {
        let source = PathBuf::from(format!("{}{}", db_path.display(), suffix));
        if !source.exists() {
            continue;
        }

        let target = PathBuf::from(format!("{}.corrupt-{}{}", db_path.display(), timestamp, suffix));
        let _ = fs::rename(&source, target);
    }
}

fn lock_is_stale(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };

    let Ok(modified) = metadata.modified() else {
        return false;
    };

    modified
        .elapsed()
        .map(|age| age > REFRESH_LOCK_STALE_AFTER)
        .unwrap_or(false)
}

fn compute_config_hash(
    repos_root_path: &Path,
    code_root_path: &Path,
    config: &Config,
    schema_version: i64,
) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    schema_version.hash(&mut hasher);
    repos_root_path.hash(&mut hasher);
    code_root_path.hash(&mut hasher);
    for pattern in &config.exclude {
        pattern.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

fn now_unix_timestamp() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs()
}

#[cfg(test)]
mod tests {
    use super::{
        load_or_bootstrap_snapshot, read_index_snapshot, refresh_index_sync, simple_pattern_score,
        Config, SearchContext, SimplePattern,
    };
    use std::fs;
    use std::path::PathBuf;

    struct TestDirs {
        root: PathBuf,
        repos_root: PathBuf,
        code_root: PathBuf,
        db_path: PathBuf,
    }

    impl TestDirs {
        fn new(name: &str) -> Self {
            let root = std::env::temp_dir().join(format!(
                "flow-repos-code-{}-{}-{}",
                name,
                std::process::id(),
                super::now_unix_timestamp()
            ));
            let repos_root = root.join("repos");
            let code_root = root.join("code");
            let db_path = root.join("repos-code-index.sqlite3");

            fs::create_dir_all(&repos_root).unwrap();
            fs::create_dir_all(&code_root).unwrap();

            Self {
                root,
                repos_root,
                code_root,
                db_path,
            }
        }

        fn context(&self) -> SearchContext {
            SearchContext::for_test(
                self.repos_root.to_string_lossy().to_string(),
                self.code_root.to_string_lossy().to_string(),
                Config::default(),
                self.db_path.clone(),
            )
        }
    }

    impl Drop for TestDirs {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.root);
        }
    }

    #[test]
    fn multi_word_query_matches_scope_and_name() {
        let pattern = SimplePattern::new("code al");
        assert!(simple_pattern_score(&pattern, "code/alfred").is_some());
    }

    #[test]
    fn multi_word_query_rejects_missing_token() {
        let pattern = SimplePattern::new("code al");
        assert!(simple_pattern_score(&pattern, "repos/alfred").is_none());
    }

    #[test]
    fn bootstrap_refresh_populates_sqlite_index() {
        let dirs = TestDirs::new("bootstrap");
        fs::create_dir_all(dirs.repos_root.join("owner/repo/.git")).unwrap();
        fs::create_dir_all(dirs.code_root.join("flow/.git")).unwrap();

        let context = dirs.context();
        refresh_index_sync(&context).unwrap();
        let snapshot = load_or_bootstrap_snapshot(&context).unwrap();
        let displays: Vec<_> = snapshot.entries.iter().map(|entry| entry.display.as_str()).collect();

        assert!(displays.contains(&"owner/repo"));
        assert!(displays.contains(&"flow"));
    }

    #[test]
    fn refresh_marks_removed_entries_inactive() {
        let dirs = TestDirs::new("deletes");
        let repo_path = dirs.code_root.join("flow");
        fs::create_dir_all(repo_path.join(".git")).unwrap();

        let context = dirs.context();
        refresh_index_sync(&context).unwrap();
        fs::remove_dir_all(&repo_path).unwrap();
        refresh_index_sync(&context).unwrap();

        let snapshot = read_index_snapshot(&context).unwrap();
        assert!(snapshot.entries.is_empty());
    }
}

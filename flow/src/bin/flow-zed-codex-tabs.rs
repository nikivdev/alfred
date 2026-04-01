use std::{
    collections::{BTreeSet, HashMap, HashSet},
    fs,
    io::{BufRead, BufReader},
    path::{Path, PathBuf},
    process::Command,
};

use chrono::NaiveDateTime;
use flow_alfred::{expand_path, fuzzy_score, Icon, Item, Output};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};

const DEFAULT_SNAPSHOT_PATH: &str =
    "~/Library/Application Support/Zed/state/open-codex-sessions.json";
const ALERT_ICON_PATH: &str =
    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns";
const GENERIC_DOC_ICON_PATH: &str =
    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericDocumentIcon.icns";
const EMPTY_QUERY_RESULT_LIMIT: usize = 32;
const OPEN_CODEX_SESSIONS_SNAPSHOT_FILE: &str = "open-codex-sessions.json";
const OPEN_CODEX_SESSIONS_CONTENT_CACHE_FILE: &str = "open-codex-sessions-content-cache.json";
const KNOWN_ZED_DATA_DIRS: &[&str] = &["ZedNikiv", "Zed", "Zed Preview", "Zed Debug", "Zed Dev"];
const SNAPSHOT_CACHE_FRESHNESS_MS: u64 = 2_000;
const SESSION_CONTENT_REFRESH_MS: u64 = 2_000;
const SESSION_CONTENT_CHAR_BUDGET: usize = 16_000;
const SESSION_CONTENT_SEGMENT_CHAR_LIMIT: usize = 1_600;
const DETAIL_SNIPPET_CHAR_LIMIT: usize = 140;
const TITLE_CONTEXT_CHAR_LIMIT: usize = 72;
const MIN_CONTEXT_SNIPPET_CHARS: usize = 8;

#[derive(Debug, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenCodexSessionsSnapshot {
    generated_at_unix_ms: u64,
    items: Vec<OpenCodexSessionItem>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct OpenCodexSessionItem {
    uid: String,
    session_id: String,
    project_path: String,
    project_name: String,
    working_directory: Option<String>,
    tab_title: String,
    window_title: Option<String>,
    custom_title: Option<String>,
    jump_url: String,
    workspace_id: Option<i64>,
    window_id: u64,
    item_id: u64,
    active_window: bool,
    active_workspace: bool,
    active_item: bool,
    last_focused_unix_ms: u64,
    #[serde(skip)]
    content: Option<SessionContentEntry>,
}

#[derive(Debug, Clone)]
struct SearchResult {
    item: OpenCodexSessionItem,
    score: i32,
    search_text: String,
}

#[derive(Debug, Clone, Default, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionContentCache {
    #[serde(default)]
    entries: HashMap<String, SessionContentEntry>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
struct SessionContentEntry {
    session_id: String,
    session_path: String,
    session_file_modified_unix_ms: u64,
    indexed_at_unix_ms: u64,
    #[serde(default)]
    first_user_message: Option<String>,
    #[serde(default)]
    last_user_message: Option<String>,
    #[serde(default)]
    completion_summary: Option<String>,
    #[serde(default)]
    search_text: String,
}

fn main() {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let snapshot_path = std::env::var("zed_open_codex_sessions_snapshot")
        .unwrap_or_else(|_| DEFAULT_SNAPSHOT_PATH.to_string());
    run_search(&query, &snapshot_path);
}

fn run_search(query: &str, snapshot_path: &str) {
    let snapshot = match load_snapshot_from_available_sources(snapshot_path) {
        Ok(snapshot) => snapshot,
        Err(snapshot_resolution) => {
            Output::new(build_missing_snapshot_items(&snapshot_resolution)).print();
            return;
        }
    };

    if snapshot.items.is_empty() {
        Output::new(vec![Item::new(
            "No open Codex tabs found",
            "Open Codex in Zed first",
        )
        .valid(false)
        .icon(Icon::path(GENERIC_DOC_ICON_PATH))])
        .print();
        return;
    }

    let results = filter_and_sort(query, &snapshot);
    if results.is_empty() {
        Output::new(vec![Item::new(
            "No open Codex tabs matched",
            "Try a broader query",
        )
        .valid(false)
        .icon(Icon::path(GENERIC_DOC_ICON_PATH))])
        .print();
        return;
    }

    let items = build_items(query, &snapshot, results);
    Output::new(items).print();
}

fn load_snapshot(path: &Path) -> Result<OpenCodexSessionsSnapshot, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str::<OpenCodexSessionsSnapshot>(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

#[derive(Debug, Clone)]
struct SnapshotPathResolution {
    existing_path: Option<PathBuf>,
    preferred_missing_path: PathBuf,
    detected_profiles: Vec<String>,
}

fn resolve_snapshot_path(configured_path: &str) -> SnapshotPathResolution {
    let configured_path = expand_path(configured_path);
    if configured_path.exists() {
        return SnapshotPathResolution {
            existing_path: Some(configured_path.clone()),
            preferred_missing_path: configured_path,
            detected_profiles: Vec::new(),
        };
    }

    let mut candidate_paths = Vec::new();
    for profile in discover_app_support_zed_profiles() {
        candidate_paths.push(snapshot_path_for_profile(&profile));
    }
    for profile in KNOWN_ZED_DATA_DIRS {
        candidate_paths.push(snapshot_path_for_profile(profile));
    }

    let deduped_candidates = dedupe_paths(&candidate_paths);

    if let Some(existing_path) = deduped_candidates
        .iter()
        .find(|path| path.exists())
        .cloned()
    {
        return SnapshotPathResolution {
            existing_path: Some(existing_path.clone()),
            preferred_missing_path: existing_path,
            detected_profiles: Vec::new(),
        };
    }

    let detected_profiles = detect_running_zed_profiles();
    for profile in &detected_profiles {
        candidate_paths.push(snapshot_path_for_profile(profile));
    }

    let deduped_candidates = dedupe_paths(&candidate_paths);

    let preferred_missing_path = deduped_candidates
        .into_iter()
        .next()
        .unwrap_or(configured_path);

    SnapshotPathResolution {
        existing_path: None,
        preferred_missing_path,
        detected_profiles,
    }
}

fn dedupe_paths(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut seen = BTreeSet::new();
    let mut deduped = Vec::new();
    for path in paths {
        if seen.insert(path.clone()) {
            deduped.push(path.clone());
        }
    }
    deduped
}

fn load_snapshot_from_available_sources(
    configured_path: &str,
) -> Result<OpenCodexSessionsSnapshot, SnapshotPathResolution> {
    let resolution = resolve_snapshot_path(configured_path);
    let snapshot_reference_path = resolution
        .existing_path
        .clone()
        .unwrap_or_else(|| resolution.preferred_missing_path.clone());
    let db_path = first_existing_db_path(&resolution);
    if let Some(existing_path) = resolution.existing_path.as_deref() {
        if db_is_newer_than_snapshot(db_path.as_deref(), existing_path) {
            if let Some(snapshot) = load_db_fallback_snapshot(&resolution) {
                let _ = write_snapshot_cache(&snapshot, existing_path);
                let mut snapshot = snapshot;
                enrich_snapshot_with_session_content(&mut snapshot, &snapshot_reference_path);
                return Ok(snapshot);
            }
        }
        return match load_snapshot(existing_path) {
            Ok(mut snapshot) => {
                enrich_snapshot_with_session_content(&mut snapshot, &snapshot_reference_path);
                Ok(snapshot)
            }
            Err(_) => {
                if let Some(snapshot) = load_db_fallback_snapshot(&resolution) {
                    let _ = write_snapshot_cache(&snapshot, existing_path);
                    let mut snapshot = snapshot;
                    enrich_snapshot_with_session_content(&mut snapshot, &snapshot_reference_path);
                    Ok(snapshot)
                } else {
                    Err(resolution)
                }
            }
        };
    }

    if let Some(snapshot) = load_db_fallback_snapshot(&resolution) {
        let _ = write_snapshot_cache(&snapshot, &resolution.preferred_missing_path);
        let mut snapshot = snapshot;
        enrich_snapshot_with_session_content(&mut snapshot, &snapshot_reference_path);
        return Ok(snapshot);
    }

    Err(resolution)
}

fn build_missing_snapshot_items(resolution: &SnapshotPathResolution) -> Vec<Item> {
    let mut items = vec![Item::new(
        "Open Codex tabs snapshot not found",
        resolution.preferred_missing_path.display().to_string(),
    )
    .valid(false)
    .icon(Icon::path(ALERT_ICON_PATH))];

    if let Some(profile) = resolution.detected_profiles.first() {
        items.push(
            Item::new(
                format!("Detected running Zed profile: {profile}"),
                "Relaunch the rebuilt fork so it writes state/open-codex-sessions.json.",
            )
            .valid(false)
            .icon(Icon::path(GENERIC_DOC_ICON_PATH)),
        );
    } else {
        items.push(
            Item::new(
                "Build and run the forked Zed with the exporter",
                "The fork writes open-codex-sessions.json while Zed is running.",
            )
            .valid(false)
            .icon(Icon::path(GENERIC_DOC_ICON_PATH)),
        );
    }

    items
}

fn enrich_snapshot_with_session_content(
    snapshot: &mut OpenCodexSessionsSnapshot,
    snapshot_reference_path: &Path,
) {
    let session_ids = snapshot
        .items
        .iter()
        .map(|item| item.session_id.clone())
        .collect::<HashSet<_>>();
    if session_ids.is_empty() {
        return;
    }

    let cache_path = snapshot_reference_path.with_file_name(OPEN_CODEX_SESSIONS_CONTENT_CACHE_FILE);
    let mut cache = load_session_content_cache(&cache_path).unwrap_or_default();
    let session_paths = resolve_session_paths_for_ids(&session_ids, &cache);
    let mut cache_changed = false;

    for item in &mut snapshot.items {
        let Some(session_path) = session_paths.get(&item.session_id) else {
            continue;
        };
        if let Some((content, changed)) = resolve_session_content_entry(
            &item.session_id,
            session_path,
            cache.entries.get(&item.session_id),
        ) {
            if changed {
                cache
                    .entries
                    .insert(item.session_id.clone(), content.clone());
                cache_changed = true;
            }
            item.content = Some(content);
        }
    }

    if cache_changed {
        let _ = write_session_content_cache(&cache_path, &cache);
    }
}

fn load_session_content_cache(path: &Path) -> Result<SessionContentCache, String> {
    let raw = fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str::<SessionContentCache>(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn write_session_content_cache(path: &Path, cache: &SessionContentCache) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let payload = serde_json::to_vec(cache).map_err(|err| {
        format!(
            "failed to encode session content cache {}: {err}",
            path.display()
        )
    })?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, payload).map_err(|err| {
        format!(
            "failed to write session content cache {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path).map_err(|err| {
        format!(
            "failed to install session content cache {}: {err}",
            path.display()
        )
    })?;
    Ok(())
}

fn resolve_session_paths_for_ids(
    session_ids: &HashSet<String>,
    cache: &SessionContentCache,
) -> HashMap<String, PathBuf> {
    let mut resolved = HashMap::new();
    let mut missing = HashSet::new();

    for session_id in session_ids {
        if let Some(entry) = cache.entries.get(session_id) {
            let cached_path = PathBuf::from(&entry.session_path);
            if cached_path.exists() {
                resolved.insert(session_id.clone(), cached_path);
                continue;
            }
        }
        missing.insert(session_id.clone());
    }

    if !missing.is_empty() {
        resolved.extend(scan_codex_session_paths(&missing));
    }

    resolved
}

fn scan_codex_session_paths(target_ids: &HashSet<String>) -> HashMap<String, PathBuf> {
    let Some(home_dir) = dirs::home_dir() else {
        return HashMap::new();
    };
    let root = home_dir.join(".codex").join("sessions");
    if !root.exists() {
        return HashMap::new();
    }

    let mut remaining = target_ids.clone();
    let mut resolved = HashMap::new();
    let mut stack = vec![root];

    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
                continue;
            }
            if path.extension().and_then(|value| value.to_str()) != Some("jsonl") {
                continue;
            }
            let Some(file_name) = path.file_name().and_then(|value| value.to_str()) else {
                continue;
            };
            let Some(session_id) = extract_session_id_from_rollout_filename(file_name) else {
                continue;
            };
            if remaining.remove(&session_id) {
                resolved.insert(session_id, path);
                if remaining.is_empty() {
                    return resolved;
                }
            }
        }
    }

    resolved
}

fn extract_session_id_from_rollout_filename(file_name: &str) -> Option<String> {
    let stem = file_name.strip_suffix(".jsonl")?;
    let candidate = stem.get(stem.len().checked_sub(36)?..)?;
    if looks_like_session_id(candidate) {
        Some(candidate.to_string())
    } else {
        None
    }
}

fn looks_like_session_id(value: &str) -> bool {
    if value.len() != 36 {
        return false;
    }
    for (index, ch) in value.chars().enumerate() {
        if matches!(index, 8 | 13 | 18 | 23) {
            if ch != '-' {
                return false;
            }
            continue;
        }
        if !ch.is_ascii_hexdigit() {
            return false;
        }
    }
    true
}

fn resolve_session_content_entry(
    session_id: &str,
    session_path: &Path,
    existing: Option<&SessionContentEntry>,
) -> Option<(SessionContentEntry, bool)> {
    let modified_unix_ms = path_modified_unix_ms(session_path)?;
    if let Some(existing) = existing {
        let existing_path_matches = Path::new(&existing.session_path) == session_path;
        let recently_indexed =
            unix_now_ms().saturating_sub(existing.indexed_at_unix_ms) <= SESSION_CONTENT_REFRESH_MS;
        if existing_path_matches && existing.session_file_modified_unix_ms == modified_unix_ms {
            return Some((existing.clone(), false));
        }
        if existing_path_matches && recently_indexed {
            return Some((existing.clone(), false));
        }
    }

    build_session_content_entry(session_id, session_path, modified_unix_ms)
        .map(|entry| (entry, true))
}

fn build_session_content_entry(
    session_id: &str,
    session_path: &Path,
    modified_unix_ms: u64,
) -> Option<SessionContentEntry> {
    let file = fs::File::open(session_path).ok()?;
    let reader = BufReader::new(file);
    let mut first_user_message = None;
    let mut last_user_message = None;
    let mut completion_summary = None;
    let mut search_segments = Vec::new();
    let mut search_len = 0usize;

    for line in reader.lines().map_while(Result::ok) {
        if line.contains(r#""type":"event_msg""#) && line.contains(r#""type":"user_message""#) {
            if let Some(message) = extract_session_user_message(&line) {
                if first_user_message.is_none() {
                    first_user_message = Some(message.clone());
                }
                last_user_message = Some(message.clone());
                push_search_segment(&mut search_segments, &mut search_len, &message);
            }
            continue;
        }

        if line.contains(r#""type":"task_complete""#) {
            if let Some(message) = extract_task_complete_summary(&line) {
                completion_summary = Some(message.clone());
                push_search_segment(&mut search_segments, &mut search_len, &message);
            }
        }
    }

    if first_user_message.is_none()
        && last_user_message.is_none()
        && completion_summary.is_none()
        && search_segments.is_empty()
    {
        return None;
    }

    Some(SessionContentEntry {
        session_id: session_id.to_string(),
        session_path: session_path.to_string_lossy().into_owned(),
        session_file_modified_unix_ms: modified_unix_ms,
        indexed_at_unix_ms: unix_now_ms(),
        first_user_message,
        last_user_message,
        completion_summary,
        search_text: search_segments.join(" "),
    })
}

fn extract_session_user_message(line: &str) -> Option<String> {
    let json = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let payload = json.get("payload")?;
    if payload.get("type")?.as_str()? != "user_message" {
        return None;
    }
    normalize_session_text(
        payload.get("message")?.as_str()?,
        SESSION_CONTENT_SEGMENT_CHAR_LIMIT,
    )
}

fn extract_task_complete_summary(line: &str) -> Option<String> {
    let json = serde_json::from_str::<serde_json::Value>(line).ok()?;
    let payload = json.get("payload")?;
    if payload.get("type")?.as_str()? != "task_complete" {
        return None;
    }
    normalize_session_text(
        payload.get("last_agent_message")?.as_str()?,
        SESSION_CONTENT_SEGMENT_CHAR_LIMIT,
    )
}

fn normalize_session_text(text: &str, max_chars: usize) -> Option<String> {
    let normalized = text.split_whitespace().collect::<Vec<_>>().join(" ");
    let trimmed = normalized.trim();
    if trimmed.is_empty() {
        return None;
    }
    Some(truncate_with_ellipsis(trimmed, max_chars))
}

fn push_search_segment(segments: &mut Vec<String>, total_len: &mut usize, text: &str) {
    if text.is_empty() || *total_len >= SESSION_CONTENT_CHAR_BUDGET {
        return;
    }
    let remaining = SESSION_CONTENT_CHAR_BUDGET.saturating_sub(*total_len);
    let segment = truncate_with_ellipsis(text, remaining.min(SESSION_CONTENT_SEGMENT_CHAR_LIMIT));
    if segment.is_empty() {
        return;
    }
    *total_len += segment.len();
    segments.push(segment);
}

fn path_modified_unix_ms(path: &Path) -> Option<u64> {
    fs::metadata(path)
        .ok()?
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()
        .map(|duration| duration.as_millis() as u64)
}

fn load_db_fallback_snapshot(
    resolution: &SnapshotPathResolution,
) -> Option<OpenCodexSessionsSnapshot> {
    for path in db_candidates(resolution) {
        if !path.exists() {
            continue;
        }
        if let Ok(snapshot) = load_db_snapshot(&path) {
            if !snapshot.items.is_empty() {
                return Some(snapshot);
            }
        }
    }

    None
}

fn db_candidates(resolution: &SnapshotPathResolution) -> Vec<PathBuf> {
    let mut candidates = Vec::new();
    for profile in &resolution.detected_profiles {
        candidates.push(db_path_for_profile(profile));
    }
    for profile in discover_app_support_zed_profiles() {
        candidates.push(db_path_for_profile(&profile));
    }
    for profile in KNOWN_ZED_DATA_DIRS {
        candidates.push(db_path_for_profile(profile));
    }

    let mut seen = BTreeSet::new();
    candidates
        .into_iter()
        .filter(|path| seen.insert(path.clone()))
        .collect()
}

fn first_existing_db_path(resolution: &SnapshotPathResolution) -> Option<PathBuf> {
    db_candidates(resolution)
        .into_iter()
        .find(|path| path.exists())
}

fn db_is_newer_than_snapshot(db_path: Option<&Path>, snapshot_path: &Path) -> bool {
    let Some(db_path) = db_path else {
        return false;
    };
    let now = std::time::SystemTime::now();
    let Ok(db_modified) = fs::metadata(db_path).and_then(|metadata| metadata.modified()) else {
        return false;
    };
    let Ok(snapshot_modified) =
        fs::metadata(snapshot_path).and_then(|metadata| metadata.modified())
    else {
        return true;
    };
    if let Ok(snapshot_age) = now.duration_since(snapshot_modified) {
        if snapshot_age.as_millis() as u64 <= SNAPSHOT_CACHE_FRESHNESS_MS {
            return false;
        }
    }
    db_modified > snapshot_modified
}

fn write_snapshot_cache(snapshot: &OpenCodexSessionsSnapshot, path: &Path) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|err| format!("failed to create {}: {err}", parent.display()))?;
    }

    let payload = serde_json::to_vec(snapshot)
        .map_err(|err| format!("failed to encode snapshot cache {}: {err}", path.display()))?;
    let temp_path = path.with_extension(format!("json.tmp-{}", std::process::id()));
    fs::write(&temp_path, payload).map_err(|err| {
        format!(
            "failed to write snapshot cache {}: {err}",
            temp_path.display()
        )
    })?;
    fs::rename(&temp_path, path)
        .map_err(|err| format!("failed to install snapshot cache {}: {err}", path.display()))?;
    Ok(())
}

fn detect_running_zed_profiles() -> Vec<String> {
    let output = Command::new("ps").args(["-axo", "command="]).output();
    let Ok(output) = output else {
        return Vec::new();
    };
    if !output.status.success() {
        return Vec::new();
    }

    extract_zed_profiles_from_ps_output(&String::from_utf8_lossy(&output.stdout))
}

fn extract_zed_profiles_from_ps_output(output: &str) -> Vec<String> {
    let mut seen = BTreeSet::new();
    let mut profiles = Vec::new();
    let needle = "/Library/Application Support/";

    for line in output.lines() {
        let mut search_start = 0;
        while let Some(relative_index) = line[search_start..].find(needle) {
            let start = search_start + relative_index + needle.len();
            let Some(rest) = line.get(start..) else {
                break;
            };
            let Some(profile) = rest.split('/').next() else {
                break;
            };
            if profile.starts_with("Zed") && seen.insert(profile.to_string()) {
                profiles.push(profile.to_string());
            }
            search_start = start.saturating_add(profile.len());
        }
    }

    profiles
}

fn discover_app_support_zed_profiles() -> Vec<String> {
    let Some(home_dir) = dirs::home_dir() else {
        return Vec::new();
    };
    let app_support_dir = home_dir.join("Library").join("Application Support");
    let Ok(entries) = std::fs::read_dir(app_support_dir) else {
        return Vec::new();
    };

    let mut profiles = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            if !path.is_dir() {
                return None;
            }
            let name = entry.file_name();
            let name = name.to_string_lossy();
            if !name.starts_with("Zed") {
                return None;
            }
            Some(name.into_owned())
        })
        .collect::<Vec<_>>();
    profiles.sort();
    profiles
}

fn snapshot_path_for_profile(profile: &str) -> PathBuf {
    let Some(home_dir) = dirs::home_dir() else {
        return expand_path(DEFAULT_SNAPSHOT_PATH);
    };
    app_support_dir_for_profile(&home_dir, profile)
        .join("state")
        .join(OPEN_CODEX_SESSIONS_SNAPSHOT_FILE)
}

fn db_path_for_profile(profile: &str) -> PathBuf {
    let Some(home_dir) = dirs::home_dir() else {
        return PathBuf::from("~/Library/Application Support/Zed/db/0-stable/db.sqlite");
    };
    app_support_dir_for_profile(&home_dir, profile)
        .join("db")
        .join("0-stable")
        .join("db.sqlite")
}

fn app_support_dir_for_profile(home_dir: &Path, profile: &str) -> PathBuf {
    home_dir
        .join("Library")
        .join("Application Support")
        .join(profile)
}

#[derive(Debug)]
struct DbCodexTabRow {
    workspace_id: i64,
    window_id: Option<i64>,
    workspace_timestamp: String,
    workspace_paths: String,
    pane_active: bool,
    item_id: i64,
    item_active: bool,
    working_directory_path: Option<String>,
    custom_title: Option<String>,
    codex_session_id: String,
}

fn load_db_snapshot(path: &Path) -> Result<OpenCodexSessionsSnapshot, String> {
    let connection = Connection::open_with_flags(path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
        .map_err(|err| format!("failed to open {}: {err}", path.display()))?;

    let current_session_id = connection
        .query_row(
            "SELECT session_id FROM workspaces WHERE session_id IS NOT NULL ORDER BY timestamp DESC LIMIT 1",
            [],
            |row| row.get::<_, String>(0),
        )
        .optional()
        .map_err(|err| format!("failed to query current Zed session from {}: {err}", path.display()))?;

    let Some(current_session_id) = current_session_id else {
        return Ok(OpenCodexSessionsSnapshot {
            generated_at_unix_ms: unix_now_ms(),
            items: Vec::new(),
        });
    };

    let mut statement = connection
        .prepare(
            "
            SELECT
                w.workspace_id,
                w.window_id,
                w.timestamp,
                w.paths,
                COALESCE(p.active, 0) AS pane_active,
                i.item_id,
                i.position,
                i.active,
                t.working_directory_path,
                t.custom_title,
                t.codex_session_id
            FROM workspaces w
            JOIN items i
              ON i.workspace_id = w.workspace_id
            LEFT JOIN panes p
              ON p.workspace_id = i.workspace_id
             AND p.pane_id = i.pane_id
            JOIN terminals t
              ON t.workspace_id = i.workspace_id
             AND t.item_id = i.item_id
            WHERE w.session_id = ?1
              AND t.codex_session_id IS NOT NULL
              AND t.codex_session_id != ''
            ORDER BY w.timestamp DESC, w.window_id, w.workspace_id, i.position
            ",
        )
        .map_err(|err| {
            format!(
                "failed to prepare Zed DB query for {}: {err}",
                path.display()
            )
        })?;

    let rows = statement
        .query_map(params![current_session_id], |row| {
            Ok(DbCodexTabRow {
                workspace_id: row.get(0)?,
                window_id: row.get(1)?,
                workspace_timestamp: row.get(2)?,
                workspace_paths: row.get(3)?,
                pane_active: row.get::<_, i64>(4)? != 0,
                item_id: row.get(5)?,
                item_active: row.get::<_, i64>(7)? != 0,
                working_directory_path: row.get(8)?,
                custom_title: row.get(9)?,
                codex_session_id: row.get(10)?,
            })
        })
        .map_err(|err| format!("failed to load Zed DB rows from {}: {err}", path.display()))?;

    let rows = rows.collect::<Result<Vec<_>, _>>().map_err(|err| {
        format!(
            "failed to decode Zed DB rows from {}: {err}",
            path.display()
        )
    })?;

    if rows.is_empty() {
        return Ok(OpenCodexSessionsSnapshot {
            generated_at_unix_ms: unix_now_ms(),
            items: Vec::new(),
        });
    }

    let active_workspace_id = rows.first().map(|row| row.workspace_id);
    let active_window_id = rows.first().and_then(|row| row.window_id);
    let items = rows
        .into_iter()
        .map(|row| db_row_to_snapshot_item(row, active_workspace_id, active_window_id))
        .collect::<Vec<_>>();

    Ok(OpenCodexSessionsSnapshot {
        generated_at_unix_ms: unix_now_ms(),
        items,
    })
}

fn db_row_to_snapshot_item(
    row: DbCodexTabRow,
    active_workspace_id: Option<i64>,
    active_window_id: Option<i64>,
) -> OpenCodexSessionItem {
    let workspace_paths = split_workspace_paths(&row.workspace_paths);
    let project_path = workspace_paths
        .first()
        .cloned()
        .or_else(|| row.working_directory_path.clone())
        .unwrap_or_default();
    let project_name = path_tail(&project_path).unwrap_or_else(|| project_path.clone());
    let tab_title = row
        .custom_title
        .as_deref()
        .filter(|value| !value.trim().is_empty())
        .map(ToOwned::to_owned)
        .or_else(|| row.working_directory_path.as_deref().and_then(path_tail))
        .unwrap_or_else(|| project_name.clone());
    let last_focused_unix_ms = parse_timestamp_ms(&row.workspace_timestamp).unwrap_or(0);

    OpenCodexSessionItem {
        uid: format!(
            "{}:{}",
            row.window_id.unwrap_or_default().max(0),
            row.item_id.max(0)
        ),
        session_id: row.codex_session_id.clone(),
        project_path: project_path.clone(),
        project_name,
        working_directory: row.working_directory_path.clone(),
        tab_title,
        window_title: row
            .window_id
            .map(|window_id| format!("window {}", window_id.max(0))),
        custom_title: row.custom_title,
        jump_url: build_jump_url(&row.codex_session_id, &project_path),
        workspace_id: Some(row.workspace_id),
        window_id: row.window_id.unwrap_or_default().max(0) as u64,
        item_id: row.item_id.max(0) as u64,
        active_window: active_window_id == row.window_id,
        active_workspace: active_workspace_id == Some(row.workspace_id),
        active_item: row.pane_active && row.item_active,
        last_focused_unix_ms,
        content: None,
    }
}

fn split_workspace_paths(raw_paths: &str) -> Vec<String> {
    if raw_paths.contains('\0') {
        return raw_paths
            .split('\0')
            .filter(|path| !path.is_empty())
            .map(ToOwned::to_owned)
            .collect();
    }
    raw_paths
        .lines()
        .filter(|path| !path.trim().is_empty())
        .map(|path| path.trim().to_string())
        .collect()
}

fn path_tail(path: &str) -> Option<String> {
    Path::new(path)
        .file_name()
        .and_then(|name| name.to_str())
        .map(ToOwned::to_owned)
        .filter(|value| !value.is_empty())
}

fn parse_timestamp_ms(timestamp: &str) -> Option<u64> {
    NaiveDateTime::parse_from_str(timestamp, "%Y-%m-%d %H:%M:%S")
        .ok()
        .map(|value| value.and_utc().timestamp_millis().max(0) as u64)
}

fn build_jump_url(session_id: &str, project_path: &str) -> String {
    format!(
        "zed://codex/session/{}?project={}",
        session_id,
        percent_encode(project_path)
    )
}

fn percent_encode(value: &str) -> String {
    let mut encoded = String::with_capacity(value.len());
    for byte in value.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'.' | b'_' | b'~') {
            encoded.push(byte as char);
        } else {
            encoded.push('%');
            encoded.push_str(&format!("{:02X}", byte));
        }
    }
    encoded
}

fn filter_and_sort(query: &str, snapshot: &OpenCodexSessionsSnapshot) -> Vec<SearchResult> {
    let query = query.trim();
    let query_lower = query.to_lowercase();
    let tokens = query_tokens(&query_lower);
    let allow_fuzzy = tokens.len() <= 1;
    let mut results = snapshot
        .items
        .iter()
        .filter_map(|item| {
            let search_text = build_search_text(item);
            let score = if tokens.is_empty() {
                base_rank(item)
            } else {
                score_item(item, &search_text, &query_lower, &tokens, allow_fuzzy)?
            };
            Some(SearchResult {
                item: item.clone(),
                score,
                search_text,
            })
        })
        .collect::<Vec<_>>();

    results.sort_by(|left, right| {
        right
            .score
            .cmp(&left.score)
            .then_with(|| right.item.active_window.cmp(&left.item.active_window))
            .then_with(|| right.item.active_workspace.cmp(&left.item.active_workspace))
            .then_with(|| right.item.active_item.cmp(&left.item.active_item))
            .then_with(|| {
                right
                    .item
                    .last_focused_unix_ms
                    .cmp(&left.item.last_focused_unix_ms)
            })
            .then_with(|| left.item.tab_title.cmp(&right.item.tab_title))
            .then_with(|| left.item.project_name.cmp(&right.item.project_name))
            .then_with(|| left.item.window_id.cmp(&right.item.window_id))
            .then_with(|| left.item.item_id.cmp(&right.item.item_id))
    });

    results
}

fn score_item(
    item: &OpenCodexSessionItem,
    search_text: &str,
    query_lower: &str,
    tokens: &[String],
    allow_fuzzy: bool,
) -> Option<i32> {
    let tab_title = item.tab_title.to_lowercase();
    let custom_title = item
        .custom_title
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let project_name = item.project_name.to_lowercase();
    let project_path = item.project_path.to_lowercase();
    let window_title = item
        .window_title
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let working_directory = item
        .working_directory
        .as_deref()
        .unwrap_or_default()
        .to_lowercase();
    let session_id = item.session_id.to_lowercase();
    let content_search = item
        .content
        .as_ref()
        .map(|content| content.search_text.to_lowercase())
        .unwrap_or_default();

    let fields = [
        tab_title.as_str(),
        custom_title.as_str(),
        project_name.as_str(),
        project_path.as_str(),
        window_title.as_str(),
        working_directory.as_str(),
        session_id.as_str(),
    ];

    let mut score = 0;
    for token in tokens {
        let mut token_score = fields
            .iter()
            .map(|field| score_field(token, field, allow_fuzzy))
            .max()
            .unwrap_or(-1);
        if token_score < 0 && !content_search.is_empty() {
            token_score = score_content_field(token, &content_search);
        }
        if token_score < 0 {
            return None;
        }
        score += token_score;
    }

    if tab_title == query_lower || custom_title == query_lower {
        score += 400;
    } else if tab_title.starts_with(query_lower) || custom_title.starts_with(query_lower) {
        score += 260;
    } else if project_name == query_lower {
        score += 220;
    } else if project_name.starts_with(query_lower) {
        score += 150;
    }

    if search_text.contains(query_lower) {
        score += 40;
    }

    Some(score + base_rank(item))
}

fn score_field(token: &str, field: &str, allow_fuzzy: bool) -> i32 {
    if token.is_empty() || field.is_empty() {
        return -1;
    }
    if field == token {
        return 180;
    }
    if field.starts_with(token) {
        return 140;
    }
    if field
        .split(|c: char| !c.is_ascii_alphanumeric())
        .any(|segment| !segment.is_empty() && segment.starts_with(token))
    {
        return 110;
    }
    if field.contains(token) {
        return 85;
    }
    if allow_fuzzy {
        let score = fuzzy_score(token, field);
        if score >= 0 {
            return 40 + score;
        }
    }
    -1
}

fn score_content_field(token: &str, field: &str) -> i32 {
    if token.is_empty() || field.is_empty() {
        return -1;
    }
    if field.contains(token) {
        return 55;
    }
    -1
}

fn base_rank(item: &OpenCodexSessionItem) -> i32 {
    let mut score = 0;
    if item.active_window {
        score += 90;
    }
    if item.active_workspace {
        score += 70;
    }
    if item.active_item {
        score += 50;
    }
    score + ((item.last_focused_unix_ms / 1_000) % 1_000_000) as i32
}

fn build_search_text(item: &OpenCodexSessionItem) -> String {
    let mut fields = Vec::with_capacity(8);
    fields.push(item.tab_title.to_lowercase());
    if let Some(custom_title) = item.custom_title.as_deref() {
        fields.push(custom_title.to_lowercase());
    }
    fields.push(item.project_name.to_lowercase());
    fields.push(item.project_path.to_lowercase());
    if let Some(window_title) = item.window_title.as_deref() {
        fields.push(window_title.to_lowercase());
    }
    if let Some(working_directory) = item.working_directory.as_deref() {
        fields.push(working_directory.to_lowercase());
    }
    fields.push(item.session_id.to_lowercase());
    if let Some(content) = item.content.as_ref() {
        if !content.search_text.is_empty() {
            fields.push(content.search_text.to_lowercase());
        }
    }
    fields.join(" ")
}

fn build_items(
    query: &str,
    snapshot: &OpenCodexSessionsSnapshot,
    results: Vec<SearchResult>,
) -> Vec<Item> {
    let limit = if query.trim().is_empty() {
        EMPTY_QUERY_RESULT_LIMIT
    } else {
        results.len()
    };

    let hidden_count = results.len().saturating_sub(limit);
    let mut title_counts = HashMap::new();
    for result in results.iter().take(limit) {
        *title_counts
            .entry(result.item.tab_title.clone())
            .or_insert(0usize) += 1;
    }
    let mut items = results
        .into_iter()
        .take(limit)
        .map(|result| {
            let duplicate_title = title_counts
                .get(&result.item.tab_title)
                .copied()
                .unwrap_or_default()
                > 1;
            build_item(result, query, duplicate_title)
        })
        .collect::<Vec<_>>();

    if query.trim().is_empty() && hidden_count > 0 {
        items.push(
            Item::new(
                format!("{hidden_count} more open Codex tabs"),
                format!(
                    "Type to narrow. Snapshot updated at {}.",
                    relative_snapshot_time(snapshot.generated_at_unix_ms)
                ),
            )
            .valid(false)
            .icon(Icon::path(GENERIC_DOC_ICON_PATH)),
        );
    }

    items
}

fn build_item(result: SearchResult, query: &str, duplicate_title: bool) -> Item {
    let title = display_title(&result.item, duplicate_title);
    let subtitle = build_subtitle(&result.item, query);
    let quicklook_target = result
        .item
        .working_directory
        .clone()
        .unwrap_or_else(|| result.item.project_path.clone());

    Item::new(title, subtitle)
        .uid(&result.item.uid)
        .arg(&result.item.jump_url)
        .autocomplete(&result.item.tab_title)
        .match_field(&result.search_text)
        .quicklook(&quicklook_target)
        .icon(Icon::path(GENERIC_DOC_ICON_PATH))
        .cmd_mod(&result.item.project_path, "Copy project path")
        .alt_mod(
            &result.item.session_id,
            format!("Copy {}", short_session_id(&result.item.session_id)),
        )
}

fn display_title(item: &OpenCodexSessionItem, duplicate_title: bool) -> String {
    let mut title = item.tab_title.clone();
    if item.tab_title.eq_ignore_ascii_case("codex") {
        if let Some(content_title) = item
            .content
            .as_ref()
            .and_then(|content| content.first_user_message.as_deref())
            .filter(|value| value.chars().count() >= MIN_CONTEXT_SNIPPET_CHARS)
        {
            title = truncate_with_ellipsis(content_title, TITLE_CONTEXT_CHAR_LIMIT);
        }
    }
    if duplicate_title {
        return format!("{title}  {}", short_session_id(&item.session_id));
    }
    title
}

fn build_subtitle(item: &OpenCodexSessionItem, query: &str) -> String {
    let mut parts = Vec::new();
    parts.push(home_relative_path(&item.project_path));
    if let Some(detail) = detail_snippet(item, query) {
        parts.push(detail);
    }
    parts.push(short_session_id(&item.session_id));
    parts.join("  |  ")
}

fn detail_snippet(item: &OpenCodexSessionItem, query: &str) -> Option<String> {
    let content = item.content.as_ref()?;
    let query_tokens = query_tokens(query);
    let mut candidates: Vec<&str> = Vec::new();
    if let Some(message) = content
        .last_user_message
        .as_deref()
        .filter(|value| value.chars().count() >= MIN_CONTEXT_SNIPPET_CHARS)
    {
        candidates.push(message);
    }
    if let Some(summary) = content
        .completion_summary
        .as_deref()
        .filter(|value| value.chars().count() >= MIN_CONTEXT_SNIPPET_CHARS)
    {
        candidates.push(summary);
    }
    if let Some(message) = content
        .first_user_message
        .as_deref()
        .filter(|value| value.chars().count() >= MIN_CONTEXT_SNIPPET_CHARS)
    {
        candidates.push(message);
    }

    if !query_tokens.is_empty() {
        for candidate in &candidates {
            let candidate_lower = candidate.to_lowercase();
            if query_tokens
                .iter()
                .all(|token| candidate_lower.contains(token))
            {
                return Some(truncate_with_ellipsis(candidate, DETAIL_SNIPPET_CHAR_LIMIT));
            }
        }
    }

    candidates
        .into_iter()
        .next()
        .map(|candidate| truncate_with_ellipsis(candidate, DETAIL_SNIPPET_CHAR_LIMIT))
}

fn short_session_id(session_id: &str) -> String {
    session_id.chars().take(13).collect()
}

fn home_relative_path(path: &str) -> String {
    let Some(home_dir) = dirs::home_dir() else {
        return path.to_string();
    };
    let home = home_dir.to_string_lossy();
    if path == home {
        "~".to_string()
    } else if let Some(stripped) = path.strip_prefix(&format!("{home}/")) {
        format!("~/{stripped}")
    } else {
        path.to_string()
    }
}

fn relative_snapshot_time(generated_at_unix_ms: u64) -> String {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or(generated_at_unix_ms);
    let age_ms = now_ms.saturating_sub(generated_at_unix_ms);
    if age_ms < 1_000 {
        "just now".to_string()
    } else {
        format!("{}s ago", age_ms / 1_000)
    }
}

fn query_tokens(query_lower: &str) -> Vec<String> {
    query_lower
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn unix_now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|duration| duration.as_millis() as u64)
        .unwrap_or_default()
}

fn truncate_with_ellipsis(value: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    let mut out = String::new();
    for ch in value.chars().take(max_chars) {
        out.push(ch);
    }
    if value.chars().count() > max_chars && max_chars > 1 {
        out.pop();
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn item(
        uid: &str,
        tab_title: &str,
        project_name: &str,
        session_id: &str,
        active_window: bool,
        active_workspace: bool,
        active_item: bool,
        last_focused_unix_ms: u64,
    ) -> OpenCodexSessionItem {
        OpenCodexSessionItem {
            uid: uid.to_string(),
            session_id: session_id.to_string(),
            project_path: format!("/tmp/{project_name}"),
            project_name: project_name.to_string(),
            working_directory: Some(format!("/tmp/{project_name}")),
            tab_title: tab_title.to_string(),
            window_title: Some(format!("{project_name} window")),
            custom_title: None,
            jump_url: format!("zed://codex/session/{session_id}?project=/tmp/{project_name}"),
            workspace_id: Some(1),
            window_id: 1,
            item_id: uid.parse().unwrap_or(1),
            active_window,
            active_workspace,
            active_item,
            last_focused_unix_ms,
            content: None,
        }
    }

    fn snapshot(items: Vec<OpenCodexSessionItem>) -> OpenCodexSessionsSnapshot {
        OpenCodexSessionsSnapshot {
            generated_at_unix_ms: 1_700_000_000_000,
            items,
        }
    }

    #[test]
    fn multiword_query_requires_meaningful_matches() {
        let snapshot = snapshot(vec![
            item(
                "1",
                "new agent",
                "run",
                "019d1111-aaaa-bbbb-cccc-000000000001",
                false,
                false,
                false,
                10,
            ),
            item(
                "2",
                "designer",
                "agent-context",
                "019d2222-aaaa-bbbb-cccc-000000000002",
                true,
                true,
                true,
                20,
            ),
        ]);

        let results = filter_and_sort("new agent", &snapshot);
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].item.tab_title, "new agent");
    }

    #[test]
    fn empty_query_prefers_active_and_recent_tabs() {
        let snapshot = snapshot(vec![
            item(
                "1",
                "older tab",
                "flow",
                "019d1111-aaaa-bbbb-cccc-000000000001",
                false,
                false,
                false,
                1_000,
            ),
            item(
                "2",
                "current tab",
                "zed",
                "019d2222-aaaa-bbbb-cccc-000000000002",
                true,
                true,
                true,
                2_000,
            ),
        ]);

        let results = filter_and_sort("", &snapshot);
        assert_eq!(results[0].item.tab_title, "current tab");
        assert_eq!(results[1].item.tab_title, "older tab");
    }

    #[test]
    fn extracts_running_zed_profiles_from_process_output() {
        let output = r#"
/Applications/Zed.app/Contents/MacOS/zed
/Users/nikitavoloboev/.local/share/fnm/node-versions/v25/bin/node /Users/nikitavoloboev/Library/Application Support/ZedNikiv/languages/vtsls/node_modules/@vtsls/language-server/bin/vtsls.js --stdio
/Users/nikitavoloboev/Library/Application Support/Zed Preview/languages/foo/bar
"#;

        let profiles = extract_zed_profiles_from_ps_output(output);
        assert_eq!(profiles, vec!["ZedNikiv", "Zed Preview"]);
    }

    #[test]
    fn extracts_session_id_from_rollout_filename() {
        let file_name = "rollout-2026-03-31T14-36-54-019d441c-8a4b-79f2-9387-2440e28edc7f.jsonl";
        assert_eq!(
            extract_session_id_from_rollout_filename(file_name).as_deref(),
            Some("019d441c-8a4b-79f2-9387-2440e28edc7f")
        );
    }

    #[test]
    fn builds_session_content_entry_from_jsonl() {
        let temp_dir =
            std::env::temp_dir().join(format!("flow-zed-codex-tabs-{}", std::process::id()));
        let _ = fs::create_dir_all(&temp_dir);
        let file_path =
            temp_dir.join("rollout-2026-04-01T00-00-00-019d4148-1a04-7760-9c5f-5c2c1b307ebf.jsonl");
        let mut file = fs::File::create(&file_path).unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-04-01T00:00:00.000Z","type":"event_msg","payload":{{"type":"user_message","message":"search openclaw operating model"}}}}"#
        )
        .unwrap();
        writeln!(
            file,
            r#"{{"timestamp":"2026-04-01T00:01:00.000Z","type":"event_msg","payload":{{"type":"task_complete","turn_id":"019d","last_agent_message":"Plan written for OpenClaw operator shell."}}}}"#
        )
        .unwrap();

        let entry = build_session_content_entry(
            "019d4148-1a04-7760-9c5f-5c2c1b307ebf",
            &file_path,
            path_modified_unix_ms(&file_path).unwrap(),
        )
        .unwrap();
        assert_eq!(
            entry.first_user_message.as_deref(),
            Some("search openclaw operating model")
        );
        assert!(entry.search_text.contains("openclaw"));
        assert_eq!(
            entry.completion_summary.as_deref(),
            Some("Plan written for OpenClaw operator shell.")
        );

        let _ = fs::remove_file(&file_path);
        let _ = fs::remove_dir(&temp_dir);
    }

    #[test]
    fn build_item_uses_user_query_for_detail_snippet() {
        let mut item = item(
            "1",
            "codex",
            "run",
            "019d4148-1a04-7760-9c5f-5c2c1b307ebf",
            true,
            true,
            true,
            2_000,
        );
        item.content = Some(SessionContentEntry {
            session_id: item.session_id.clone(),
            session_path: "/tmp/session.jsonl".to_string(),
            session_file_modified_unix_ms: 1,
            indexed_at_unix_ms: 2,
            first_user_message: Some("inspect the designer lane".to_string()),
            last_user_message: Some("search openclaw operating model".to_string()),
            completion_summary: Some("Plan written for OpenClaw operator shell.".to_string()),
            search_text: "inspect the designer lane search openclaw operating model Plan written for OpenClaw operator shell.".to_string(),
        });

        let rendered = build_item(
            SearchResult {
                item,
                score: 500,
                search_text: "codex run 019d4148".to_string(),
            },
            "plan written",
            false,
        );

        let subtitle = rendered.subtitle.unwrap_or_default();
        assert!(subtitle.contains("Plan written for OpenClaw operator shell."));
    }
}

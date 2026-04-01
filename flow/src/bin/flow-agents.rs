use std::path::{Path, PathBuf};

use flow_alfred::{expand_path, fuzzy_score, Icon, Item, Output};
use serde::Deserialize;

const DEFAULT_CATALOG_PATH: &str = "~/run/.ai/artifacts/codex/run-agent-skills.json";
const ALERT_ICON_PATH: &str =
    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/AlertStopIcon.icns";
const GENERIC_DOC_ICON_PATH: &str =
    "/System/Library/CoreServices/CoreTypes.bundle/Contents/Resources/GenericDocumentIcon.icns";
const EMPTY_QUERY_RESULT_LIMIT: usize = 24;

#[derive(Debug, Deserialize)]
struct AgentCatalog {
    #[serde(rename = "repoRoot")]
    repo_root: String,
    agents: Vec<AgentEntry>,
}

#[derive(Debug, Clone, Deserialize)]
struct AgentEntry {
    id: String,
    #[serde(rename = "displayName")]
    display_name: String,
    mention: String,
    #[serde(rename = "attachablePath")]
    attachable_path: Option<String>,
    #[serde(rename = "attachableRepoPath")]
    attachable_repo_path: Option<String>,
    #[serde(rename = "skillPath")]
    skill_path: String,
    #[serde(rename = "skillAbsolutePath")]
    skill_absolute_path: Option<String>,
    #[serde(rename = "fileContextAbsolutePath")]
    file_context_absolute_path: Option<String>,
    #[serde(rename = "shortDescription")]
    short_description: String,
    purpose: String,
    #[serde(rename = "searchText")]
    search_text: String,
    #[serde(rename = "missingFields", default)]
    missing_fields: Vec<String>,
}

#[derive(Debug, Clone)]
struct AgentResult {
    entry: AgentEntry,
    attachable_path: PathBuf,
    source_path: PathBuf,
    source_subtitle: String,
    search_text: String,
}

fn main() {
    let query = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let catalog_path =
        std::env::var("agents_catalog").unwrap_or_else(|_| DEFAULT_CATALOG_PATH.to_string());
    run_agent_search(&query, &catalog_path);
}

fn run_agent_search(query: &str, catalog_path: &str) {
    let expanded_catalog_path = expand_path(catalog_path);

    if !expanded_catalog_path.exists() {
        Output::new(vec![
            Item::new(
                "Run-agent catalog not found",
                expanded_catalog_path.display().to_string(),
            )
            .valid(false)
            .icon(Icon::path(ALERT_ICON_PATH)),
            Item::new(
                "Run codex-skills-sync first",
                "f run --config ~/run/flow.toml codex-skills-sync",
            )
            .valid(false)
            .icon(Icon::path(GENERIC_DOC_ICON_PATH)),
        ])
        .print();
        return;
    }

    let catalog = match load_catalog(&expanded_catalog_path) {
        Ok(catalog) => catalog,
        Err(err) => {
            Output::new(vec![Item::new("Could not read run-agent catalog", err)
                .valid(false)
                .icon(Icon::path(ALERT_ICON_PATH))])
            .print();
            return;
        }
    };

    let results = filter_and_sort_agents(query, &catalog);
    if results.is_empty() {
        Output::new(vec![Item::new("No agents found", "Try a broader query")
            .valid(false)
            .icon(Icon::path(GENERIC_DOC_ICON_PATH))])
        .print();
        return;
    }

    let items = build_agent_items(query, results);
    Output::new(items).print();
}

fn load_catalog(path: &Path) -> Result<AgentCatalog, String> {
    let raw = std::fs::read_to_string(path)
        .map_err(|err| format!("failed to read {}: {err}", path.display()))?;
    serde_json::from_str::<AgentCatalog>(&raw)
        .map_err(|err| format!("failed to parse {}: {err}", path.display()))
}

fn filter_and_sort_agents(query: &str, catalog: &AgentCatalog) -> Vec<AgentResult> {
    let repo_root = PathBuf::from(&catalog.repo_root);
    let query = query.trim();
    let query_lower = query.to_lowercase();
    let query_tokens = query_tokens(&query_lower);
    let mut results = catalog
        .agents
        .iter()
        .cloned()
        .filter_map(|entry| {
            let attachable_path = resolve_catalog_path(
                &repo_root,
                entry.attachable_path.as_deref(),
                entry
                    .skill_absolute_path
                    .as_deref()
                    .or(Some(entry.skill_path.as_str())),
            );
            let (source_path, source_subtitle) = match entry.file_context_absolute_path.as_deref() {
                Some(path) => (
                    resolve_catalog_path(&repo_root, Some(path), None),
                    "Open focused context source in Zed".to_string(),
                ),
                None => (
                    resolve_catalog_path(
                        &repo_root,
                        entry.skill_absolute_path.as_deref(),
                        Some(entry.skill_path.as_str()),
                    ),
                    "Open source skill file in Zed".to_string(),
                ),
            };
            let search_text = format!(
                "{} {} {} {} {}",
                entry.id,
                entry.display_name,
                entry.mention,
                entry.short_description,
                entry.search_text
            );
            let result = AgentResult {
                entry,
                attachable_path,
                source_path,
                source_subtitle,
                search_text,
            };
            let score = if query.is_empty() {
                0
            } else {
                agent_score(&query_lower, &query_tokens, &result)?
            };
            Some((result, score))
        })
        .collect::<Vec<_>>();

    if query.is_empty() {
        results.sort_by(|(left, _), (right, _)| {
            left.entry
                .display_name
                .cmp(&right.entry.display_name)
                .then_with(|| left.entry.id.cmp(&right.entry.id))
        });
        return results.into_iter().map(|(result, _)| result).collect();
    }

    results.sort_by(|(left, left_score), (right, right_score)| {
        right_score
            .cmp(left_score)
            .then_with(|| left.entry.display_name.cmp(&right.entry.display_name))
            .then_with(|| left.entry.id.cmp(&right.entry.id))
    });

    results.into_iter().map(|(result, _)| result).collect()
}

fn resolve_catalog_path(repo_root: &Path, path: Option<&str>, fallback: Option<&str>) -> PathBuf {
    let selected = path
        .filter(|value| !value.trim().is_empty())
        .or_else(|| fallback.filter(|value| !value.trim().is_empty()));
    let Some(path) = selected else {
        return repo_root.to_path_buf();
    };

    let raw = PathBuf::from(path);
    if raw.is_absolute() {
        raw
    } else {
        repo_root.join(raw)
    }
}

fn agent_score(query_lower: &str, query_tokens: &[&str], result: &AgentResult) -> Option<i32> {
    let id_lower = result.entry.id.to_lowercase();
    let display_lower = result.entry.display_name.to_lowercase();
    let mention_lower = result.entry.mention.to_lowercase();
    let mention_without_prefix = mention_lower.trim_start_matches('$');
    let relative_lower = result
        .entry
        .attachable_repo_path
        .as_deref()
        .unwrap_or(result.entry.skill_path.as_str())
        .to_lowercase();
    let short_lower = result.entry.short_description.to_lowercase();
    let purpose_lower = result.entry.purpose.to_lowercase();
    let search_lower = result.search_text.to_lowercase();

    let allow_fuzzy_primary = query_tokens.len() == 1;
    let mut score = 0;
    for token in query_tokens {
        score += token_score(
            token,
            &[
                display_lower.as_str(),
                id_lower.as_str(),
                mention_lower.as_str(),
                mention_without_prefix,
                relative_lower.as_str(),
            ],
            &[
                short_lower.as_str(),
                purpose_lower.as_str(),
                search_lower.as_str(),
            ],
            allow_fuzzy_primary,
        )?;
    }

    if id_lower == query_lower {
        score += 700;
    }
    if display_lower == query_lower {
        score += 700;
    }
    if mention_lower == query_lower || mention_without_prefix == query_lower {
        score += 700;
    }
    if display_lower.starts_with(query_lower) {
        score += 260;
    }
    if id_lower.starts_with(query_lower) || mention_without_prefix.starts_with(query_lower) {
        score += 240;
    }
    if display_lower.contains(query_lower) {
        score += 180;
    }
    if id_lower.contains(query_lower) || mention_without_prefix.contains(query_lower) {
        score += 160;
    }
    if short_lower.contains(query_lower) {
        score += 80;
    }
    if purpose_lower.contains(query_lower) {
        score += 80;
    }

    Some(score)
}

fn query_tokens<'a>(query_lower: &'a str) -> Vec<&'a str> {
    query_lower
        .split_whitespace()
        .filter(|token| !token.is_empty())
        .collect()
}

fn token_score(
    token: &str,
    primary_fields: &[&str],
    secondary_fields: &[&str],
    allow_fuzzy_primary: bool,
) -> Option<i32> {
    let mut best = -1;

    for field in primary_fields {
        best = best.max(field_score(token, field, allow_fuzzy_primary));
    }
    for field in secondary_fields {
        best = best.max(field_score(token, field, false));
    }

    (best >= 0).then_some(best)
}

fn field_score(token: &str, field: &str, allow_fuzzy: bool) -> i32 {
    if field.is_empty() {
        return -1;
    }

    if field == token {
        return 420;
    }
    if field.starts_with(token) {
        return 320;
    }
    if let Some(index) = field.find(token) {
        let boundary_bonus = if index == 0 {
            40
        } else if matches!(
            field.as_bytes()[index - 1],
            b'/' | b'-' | b'_' | b' ' | b'$'
        ) {
            25
        } else {
            0
        };
        let distance_penalty = (index as i32).min(80);
        return 220 + boundary_bonus - distance_penalty;
    }
    if allow_fuzzy {
        let score = fuzzy_score(token, field);
        if score >= 0 {
            return 80 + score;
        }
    }

    -1
}

fn build_agent_item(result: AgentResult) -> Item {
    let attachable_path = result.attachable_path.to_string_lossy().to_string();
    let paste_path = home_relative_path(&result.attachable_path);
    let source_path = result.source_path.to_string_lossy().to_string();
    let subtitle = build_subtitle(&result);

    Item::new(&result.entry.display_name, subtitle)
        .uid(&result.entry.id)
        .arg(&paste_path)
        .autocomplete(&result.entry.id)
        .quicklook(&attachable_path)
        .icon(Icon::path(GENERIC_DOC_ICON_PATH))
        .cmd_mod(&source_path, &result.source_subtitle)
        .alt_mod(
            &result.entry.mention,
            format!("Copy {}", result.entry.mention),
        )
}

fn build_agent_items(query: &str, results: Vec<AgentResult>) -> Vec<Item> {
    if !query.trim().is_empty() || results.len() <= EMPTY_QUERY_RESULT_LIMIT {
        return results.into_iter().map(build_agent_item).collect();
    }

    let hidden_count = results.len() - EMPTY_QUERY_RESULT_LIMIT;
    let mut items = results
        .into_iter()
        .take(EMPTY_QUERY_RESULT_LIMIT)
        .map(build_agent_item)
        .collect::<Vec<_>>();
    items.push(
        Item::new(
            format!("Type to search {} more agents", hidden_count),
            format!(
                "Showing the first {} agents for instant open",
                EMPTY_QUERY_RESULT_LIMIT
            ),
        )
        .valid(false)
        .icon(Icon::path(GENERIC_DOC_ICON_PATH)),
    );
    items
}

fn build_subtitle(result: &AgentResult) -> String {
    let relative_path = result
        .entry
        .attachable_repo_path
        .clone()
        .unwrap_or_else(|| result.entry.skill_path.clone());
    let mut pieces = vec![
        result.entry.mention.clone(),
        result.entry.short_description.clone(),
        relative_path,
    ];
    if !result.entry.missing_fields.is_empty() {
        pieces.push(format!(
            "missing {}",
            result.entry.missing_fields.join(", ")
        ));
    }
    pieces.join("  |  ")
}

fn home_relative_path(path: &Path) -> String {
    let Some(home) = dirs::home_dir() else {
        return path.to_string_lossy().to_string();
    };

    path.strip_prefix(&home)
        .ok()
        .map(|relative| {
            let relative = relative.to_string_lossy();
            if relative.is_empty() {
                "~".to_string()
            } else {
                format!("~/{}", relative)
            }
        })
        .unwrap_or_else(|| path.to_string_lossy().to_string())
}

#[cfg(test)]
mod tests {
    use super::{filter_and_sort_agents, home_relative_path, AgentCatalog, AgentEntry};
    use std::path::Path;

    fn sample_agent(
        id: &str,
        display_name: &str,
        mention: &str,
        short_description: &str,
        purpose: &str,
    ) -> AgentEntry {
        AgentEntry {
            id: id.to_string(),
            display_name: display_name.to_string(),
            mention: mention.to_string(),
            attachable_path: Some(format!("/tmp/{id}/run.md")),
            attachable_repo_path: Some(format!("agent-context/{id}/run.md")),
            skill_path: format!(".ai/skills/{id}/SKILL.md"),
            skill_absolute_path: Some(format!("/tmp/{id}/SKILL.md")),
            file_context_absolute_path: None,
            short_description: short_description.to_string(),
            purpose: purpose.to_string(),
            search_text: format!("{display_name} {short_description} {purpose}"),
            missing_fields: Vec::new(),
        }
    }

    #[test]
    fn multi_word_queries_require_meaningful_token_hits() {
        let catalog = AgentCatalog {
            repo_root: "/tmp".to_string(),
            agents: vec![
                sample_agent(
                    "new",
                    "New",
                    "$new",
                    "Create one new run-owned agent bundle completely.",
                    "Create a new agent bundle in ~/run.",
                ),
                sample_agent(
                    "update",
                    "Update",
                    "$update",
                    "Update one existing run-owned agent bundle accurately.",
                    "Update an existing run-owned agent.",
                ),
                sample_agent(
                    "designer",
                    "Designer",
                    "$designer",
                    "Answer grounded questions about Prom Designer.",
                    "Designer workflow help.",
                ),
            ],
        };

        let results = filter_and_sort_agents("new agent", &catalog);
        let titles = results
            .into_iter()
            .map(|result| result.entry.display_name)
            .collect::<Vec<_>>();

        assert_eq!(titles, vec!["New".to_string()]);
    }

    #[test]
    fn single_word_queries_still_rank_primary_matches_first() {
        let catalog = AgentCatalog {
            repo_root: "/tmp".to_string(),
            agents: vec![
                sample_agent(
                    "designer",
                    "Designer",
                    "$designer",
                    "Answer grounded questions about Prom Designer.",
                    "Designer workflow help.",
                ),
                sample_agent(
                    "designer-dev",
                    "Designer Dev",
                    "$designer-dev",
                    "Automate the Prom Designer dev loop.",
                    "Designer dev workflow.",
                ),
                sample_agent(
                    "ci",
                    "CI",
                    "$ci",
                    "Plan and validate remote CI and release readiness for Prom Designer.",
                    "CI workflow.",
                ),
            ],
        };

        let results = filter_and_sort_agents("designer", &catalog);
        let titles = results
            .into_iter()
            .map(|result| result.entry.display_name)
            .collect::<Vec<_>>();

        assert_eq!(titles[0], "Designer");
        assert_eq!(titles[1], "Designer Dev");
    }

    #[test]
    fn home_relative_path_uses_tilde_for_home_children() {
        let home = dirs::home_dir().unwrap();
        let path = home.join("run/agent-context/designer/run.md");

        assert_eq!(
            home_relative_path(Path::new(&path)),
            "~/run/agent-context/designer/run.md"
        );
    }
}

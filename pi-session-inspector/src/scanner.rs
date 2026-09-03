use crate::models::*;
use std::collections::HashMap;
use std::fs;
use std::io::{self, BufRead};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct ScanOptions {
    pub sessions_dir: String,
    pub project_regex: Option<regex::Regex>,
    pub path_regex: Option<regex::Regex>,
    pub provider_filter: Option<String>,
    pub model_filter: Option<String>,
    pub name_regex: Option<regex::Regex>,
}

#[derive(Debug, Clone)]
pub struct SessionSummary {
    pub session_id: String,
    pub session_file: PathBuf,
    pub cwd: String,
    pub started_at: String,
    pub display_name: Option<String>,
    pub total_tokens: u64,
    pub total_cost: f64,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    // Price (cost) broken down by billing component. `cost_output` already
    // includes reasoning tokens (reasoning is billed as output for deepseek).
    pub cost_input: f64,
    pub cost_output: f64,
    pub cost_cached: f64,
    pub duration_secs: i64,
    pub turns: usize,
    pub message_count: usize,
    pub assistant_count: usize,
    pub user_count: usize,
    pub tool_result_count: usize,
    pub compactions: Vec<CompactionUsage>,
    pub branch_summaries: Vec<CompactionUsage>,
    /// Number of times the prompt cache was invalidated during the session.
    /// Counts a request that loses more than 3% of the previously-matched
    /// cache (cache_read drops by more than 3% from a prior hit), i.e. the
    /// cached prefix stopped matching and had to be rebuilt at full input price.
    /// A complete miss (cache_read == 0) is just the extreme case (100% loss).
    pub cache_invalidations: u64,
    /// Total input tokens lost to cache invalidation. At each invalidation the
    /// previously-cached tokens that no longer matched (the drop in cache_read
    /// from the preceding request) had to be re-processed at full input price
    /// instead of the discounted cache rate.
    pub cache_lost_tokens: u64,
    /// Dollar cost of the cache-invalidation losses: the lost tokens re-processed
    /// at the session's effective full-input rate minus what they would have cost
    /// at the effective cache-read rate.
    pub cache_lost_cost: f64,
    /// Total context size (fresh `input` + `cache_read`) sent to the model on the
    /// final assistant request — i.e. the context being used at the end of the
    /// session.
    pub context_tokens_at_end: u64,
    pub provider_breakdown: HashMap<String, ProviderUsage>,
    pub model_breakdown: HashMap<String, ProviderUsage>,
    pub tool_breakdown: HashMap<String, ToolUsage>,
}

#[derive(Debug, Clone)]
pub struct CompactionUsage {
    pub tokens: u64,
    pub cost: f64,
    #[allow(dead_code)]
    pub from_hook: bool,
}

#[derive(Debug, Clone)]
pub struct ProviderUsage {
    pub tokens: u64,
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
    pub reasoning: u64,
    pub cost: f64,
    pub calls: usize,
}

/// Per-tool usage, attributed from the assistant turns that issued each call.
/// Only output/reasoning tokens are reported (an even split of the turn's output
/// across the tool calls it made). Input/consumed tokens are intentionally NOT
/// attributed here because they do not map cleanly to individual tools.
#[derive(Debug, Clone, Default)]
pub struct ToolUsage {
    /// Number of times the tool was called.
    pub calls: usize,
    /// Output tokens produced to issue calls to this tool.
    pub output: u64,
    /// Reasoning (thinking) tokens produced while issuing calls to this tool.
    pub reasoning: u64,
}

pub fn scan(options: &ScanOptions) -> io::Result<Vec<SessionSummary>> {
    let sessions_dir = Path::new(&options.sessions_dir);

    if !sessions_dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Sessions directory not found: {}", options.sessions_dir),
        ));
    }

    let mut summaries = Vec::new();

    // Walk all subdirectories (project folders)
    for project_entry in fs::read_dir(sessions_dir)? {
        let project_dir = project_entry?.path();
        if !project_dir.is_dir() {
            continue;
        }

        // Walk session files in each project dir
        for session_entry in fs::read_dir(&project_dir)? {
            let session_file = session_entry?.path();
            if session_file.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }

            match parse_session_file(&session_file) {
                Ok(mut summary) => {
                    // Apply filters
                    if let Some(ref rx) = options.project_regex {
                        if !rx.is_match(&summary.cwd) {
                            continue;
                        }
                    }

                    // Filter by the session's file path (includes the project folder under
                    // sessions_dir plus the filename), e.g. to target a specific session file.
                    let session_path = summary.session_file.to_string_lossy().to_string();
                    if let Some(ref rx) = options.path_regex {
                        if !rx.is_match(&session_path) {
                            continue;
                        }
                    }

                    if let Some(ref prov) = options.provider_filter {
                        // Check if any usage matches provider
                        let has_provider = summary
                            .provider_breakdown
                            .keys()
                            .any(|p| p.to_lowercase().contains(&prov.to_lowercase()));
                        if !has_provider {
                            continue;
                        }
                    }

                    if let Some(ref mdl) = options.model_filter {
                        let has_model = summary
                            .model_breakdown
                            .keys()
                            .any(|m| m.to_lowercase().contains(&mdl.to_lowercase()));
                        if !has_model {
                            continue;
                        }
                    }

                    if let Some(ref rx) = options.name_regex {
                        let name_matches = summary
                            .display_name
                            .as_deref()
                            .map(|n| rx.is_match(n))
                            .unwrap_or(false);
                        if !name_matches {
                            continue;
                        }
                    }

                    // If filtering by provider/model, remove non-matching breakdowns
                    if let Some(ref prov) = options.provider_filter {
                        let prov_lower = prov.to_lowercase();
                        summary
                            .provider_breakdown
                            .retain(|k, _| k.to_lowercase().contains(&prov_lower));
                    }
                    if let Some(ref mdl) = options.model_filter {
                        let mdl_lower = mdl.to_lowercase();
                        summary
                            .model_breakdown
                            .retain(|k, _| k.to_lowercase().contains(&mdl_lower));
                    }

                    summaries.push(summary);
                }
                Err(e) => {
                    eprintln!("Warning: Failed to parse {}: {}", session_file.display(), e);
                }
            }
        }
    }

    Ok(summaries)
}

/// Shorten an absolute project path by replacing the home directory with `~`.
fn short_project(cwd: &str) -> String {
    let home = dirs::home_dir()
        .map(|h| h.to_string_lossy().to_string())
        .unwrap_or_default();
    if cwd.starts_with(&home) {
        cwd.replacen(&home, "~", 1)
    } else {
        cwd.to_string()
    }
}

/// Collect all distinct named session names present in the sessions directory.
/// Optionally filter the resulting names by a regex.
pub fn list_named_sessions(
    sessions_dir: &str,
    name_regex: Option<&regex::Regex>,
) -> io::Result<Vec<String>> {
    let dir = Path::new(sessions_dir);
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Sessions directory not found: {}", sessions_dir),
        ));
    }

    let mut names = std::collections::HashSet::new();

    for project_entry in fs::read_dir(dir)? {
        let project_dir = project_entry?.path();
        if !project_dir.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(&project_dir)? {
            let session_file = session_entry?.path();
            if session_file.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }
            if let Ok(summary) = parse_session_file(&session_file) {
                if let Some(name) = summary.display_name {
                    if let Some(rx) = name_regex {
                        if rx.is_match(&name) {
                            names.insert(name);
                        }
                    } else {
                        names.insert(name);
                    }
                }
            }
        }
    }

    let mut names: Vec<String> = names.into_iter().collect();
    names.sort();
    names.sort_by_key(|n| n.to_lowercase());
    Ok(names)
}

/// Collect all distinct session project paths (cwd) present in the sessions directory.
/// Optionally filter the resulting projects by a regex.
pub fn list_projects(sessions_dir: &str, regex: Option<&regex::Regex>) -> io::Result<Vec<String>> {
    let dir = Path::new(sessions_dir);
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Sessions directory not found: {}", sessions_dir),
        ));
    }

    let mut projects = std::collections::HashSet::new();

    for project_entry in fs::read_dir(dir)? {
        let project_dir = project_entry?.path();
        if !project_dir.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(&project_dir)? {
            let session_file = session_entry?.path();
            if session_file.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }
            if let Ok(summary) = parse_session_file(&session_file) {
                let proj = short_project(&summary.cwd);
                if let Some(rx) = regex {
                    if rx.is_match(&proj) || rx.is_match(&summary.cwd) {
                        projects.insert(proj);
                    }
                } else {
                    projects.insert(proj);
                }
            }
        }
    }

    let mut projects: Vec<String> = projects.into_iter().collect();
    projects.sort_by_key(|p| p.to_lowercase());
    Ok(projects)
}

/// Collect all distinct session jsonl file paths present in the sessions directory.
/// Optionally filter the resulting paths by a regex.
pub fn list_session_paths(
    sessions_dir: &str,
    regex: Option<&regex::Regex>,
) -> io::Result<Vec<String>> {
    let dir = Path::new(sessions_dir);
    if !dir.is_dir() {
        return Err(io::Error::new(
            io::ErrorKind::NotFound,
            format!("Sessions directory not found: {}", sessions_dir),
        ));
    }

    let mut paths = Vec::new();
    for project_entry in fs::read_dir(dir)? {
        let project_dir = project_entry?.path();
        if !project_dir.is_dir() {
            continue;
        }
        for session_entry in fs::read_dir(&project_dir)? {
            let session_file = session_entry?.path();
            if session_file.extension().map_or(true, |e| e != "jsonl") {
                continue;
            }
            let sp = session_file.to_string_lossy().to_string();
            if let Some(rx) = regex {
                if rx.is_match(&sp) {
                    paths.push(sp);
                }
            } else {
                paths.push(sp);
            }
        }
    }
    paths.sort_by_key(|p| p.to_lowercase());
    Ok(paths)
}

/// Parse every valid entry from a session `.jsonl` file in file order. Malformed
/// lines are skipped (matching the tolerant behaviour of the aggregate scan).
pub fn parse_entries(path: &Path) -> io::Result<Vec<SessionEntry>> {
    let file = fs::File::open(path)?;
    let reader = io::BufReader::new(file);
    let mut entries = Vec::new();
    for line in reader.lines() {
        let line = line?;
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if let Ok(entry) = serde_json::from_str::<SessionEntry>(trimmed) {
            entries.push(entry);
        }
    }
    Ok(entries)
}

fn parse_session_file(path: &Path) -> io::Result<SessionSummary> {
    let entries = parse_entries(path)?;

    let mut session_id = String::new();
    let mut cwd = String::new();
    let mut started_at = String::new();
    let mut display_name: Option<String> = None;
    let mut total_tokens: u64 = 0;
    let mut total_cost: f64 = 0.0;
    let mut input_tokens: u64 = 0;
    let mut cached_input_tokens: u64 = 0;
    let mut output_tokens: u64 = 0;
    let mut reasoning_tokens: u64 = 0;
    let mut cost_input: f64 = 0.0;
    let mut cost_output: f64 = 0.0;
    let mut cost_cached: f64 = 0.0;
    let mut start_epoch: Option<i64> = None;
    let mut end_epoch: Option<i64> = None;
    let mut turns: usize = 0;
    let mut message_count: usize = 0;
    let mut assistant_count: usize = 0;
    let mut user_count: usize = 0;
    let mut tool_result_count: usize = 0;
    let mut compactions: Vec<CompactionUsage> = Vec::new();
    let mut branch_summaries: Vec<CompactionUsage> = Vec::new();
    let mut provider_breakdown: HashMap<String, ProviderUsage> = HashMap::new();
    let mut model_breakdown: HashMap<String, ProviderUsage> = HashMap::new();
    let mut tool_breakdown: HashMap<String, ToolUsage> = HashMap::new();
    let mut cache_invalidations: u64 = 0;
    let mut cache_lost_tokens: u64 = 0;
    let mut cache_tracker = CacheTracker::default();
    // Context (fresh input + cached) sent on the most recent assistant request;
    // this is the last assistant message's total input, i.e. the context in use
    // at the end of the session.
    let mut context_tokens_at_end: u64 = 0;

    for entry in entries {
        match entry {
            SessionEntry::SessionHeader(header) => {
                session_id = header.id;
                cwd = header.cwd;
                started_at = header.timestamp.clone();
                start_epoch = parse_iso_epoch(&header.timestamp).or(start_epoch);
                if let Some(ts) = start_epoch {
                    end_epoch = Some(ts.max(end_epoch.unwrap_or(ts)));
                }
            }
            SessionEntry::Message(msg) => {
                message_count += 1;
                if let Some(ts) = parse_iso_epoch(&msg.timestamp) {
                    end_epoch = Some(ts.max(end_epoch.unwrap_or(ts)));
                }
                match &msg.message {
                    AgentMessage::Assistant(assistant) => {
                        assistant_count += 1;
                        if let Some(ref usage) = assistant.usage {
                            add_usage(
                                usage,
                                &mut total_tokens,
                                &mut total_cost,
                                &mut input_tokens,
                                &mut cached_input_tokens,
                                &mut output_tokens,
                                &mut reasoning_tokens,
                                &mut cost_input,
                                &mut cost_output,
                                &mut cost_cached,
                                &mut provider_breakdown,
                                &mut model_breakdown,
                                assistant.provider.as_deref(),
                                assistant.model.as_deref(),
                            );
                            // Attribute this turn's produced output/reasoning tokens
                            // to the tools it called (evenly split across the calls).
                            add_tool_usage(
                                assistant,
                                usage,
                                &mut tool_breakdown,
                            );
                            let obs = cache_tracker.record(usage);
                            if obs.is_invalidation {
                                cache_invalidations += 1;
                                cache_lost_tokens += obs.lost_tokens;
                            }
                            context_tokens_at_end = usage.input + usage.cache_read;
                        }
                    }
                    AgentMessage::User(_) => {
                        user_count += 1;
                        turns += 1;
                    }
                    AgentMessage::ToolResult(tr) => {
                        tool_result_count += 1;
                        if let Some(ref usage) = tr.usage {
                            add_usage(
                                usage,
                                &mut total_tokens,
                                &mut total_cost,
                                &mut input_tokens,
                                &mut cached_input_tokens,
                                &mut output_tokens,
                                &mut reasoning_tokens,
                                &mut cost_input,
                                &mut cost_output,
                                &mut cost_cached,
                                &mut provider_breakdown,
                                &mut model_breakdown,
                                None,
                                None,
                            );
                            let obs = cache_tracker.record(usage);
                            if obs.is_invalidation {
                                cache_invalidations += 1;
                                cache_lost_tokens += obs.lost_tokens;
                            }
                        }
                    }
                    _ => {}
                }
            }
            SessionEntry::Compaction(compaction) => {
                if let Some(ref usage) = compaction.usage {
                    total_tokens += usage.total_tokens;
                    total_cost += usage.cost.total;
                    input_tokens += usage.input;
                    cached_input_tokens += usage.cache_read + usage.cache_write;
                    output_tokens += usage.output;
                    reasoning_tokens += usage.reasoning;
                    cost_input += usage.cost.input;
                    cost_output += usage.cost.output;
                    cost_cached += usage.cost.cache_read + usage.cost.cache_write;
                    compactions.push(CompactionUsage {
                        tokens: usage.total_tokens,
                        cost: usage.cost.total,
                        from_hook: compaction.from_hook.unwrap_or(false),
                    });
                }
            }
            SessionEntry::BranchSummary(bs) => {
                if let Some(ref usage) = bs.usage {
                    total_tokens += usage.total_tokens;
                    total_cost += usage.cost.total;
                    input_tokens += usage.input;
                    cached_input_tokens += usage.cache_read + usage.cache_write;
                    output_tokens += usage.output;
                    reasoning_tokens += usage.reasoning;
                    cost_input += usage.cost.input;
                    cost_output += usage.cost.output;
                    cost_cached += usage.cost.cache_read + usage.cost.cache_write;
                    branch_summaries.push(CompactionUsage {
                        tokens: usage.total_tokens,
                        cost: usage.cost.total,
                        from_hook: bs.from_hook.unwrap_or(false),
                    });
                }
            }
            SessionEntry::SessionInfo(info) => {
                if let Some(name) = info.name {
                    display_name = Some(name);
                }
            }
            // Entries we collect info from but don't accumulate usage:
            SessionEntry::ModelChange(_)
            | SessionEntry::ThinkingLevelChange(_)
            | SessionEntry::Custom(_)
            | SessionEntry::CustomMessage(_)
            | SessionEntry::Label(_) => {}
        }
    }

    let duration_secs = match (start_epoch, end_epoch) {
        (Some(start), Some(end)) => (end - start).max(0),
        _ => 0,
    };

    // Cost of the tokens lost to invalidation: re-processed at the effective full
    // input rate, minus what they would have cost at the effective cache-read rate.
    // Rates are derived from the session's own totals so they reflect the models
    // actually used.
    let cache_lost_cost = if cache_lost_tokens > 0 && input_tokens > 0 && cached_input_tokens > 0 {
        let input_rate = cost_input / input_tokens as f64;
        let cache_rate = cost_cached / cached_input_tokens as f64;
        ((input_rate - cache_rate).max(0.0)) * cache_lost_tokens as f64
    } else {
        0.0
    };

    Ok(SessionSummary {
        session_id,
        session_file: path.to_path_buf(),
        cwd,
        started_at,
        display_name,
        total_tokens,
        total_cost,
        input_tokens,
        cached_input_tokens,
        output_tokens,
        reasoning_tokens,
        cost_input,
        cost_output,
        cost_cached,
        duration_secs,
        turns,
        message_count,
        assistant_count,
        user_count,
        tool_result_count,
        compactions,
        branch_summaries,
        cache_invalidations,
        cache_lost_tokens,
        cache_lost_cost,
        context_tokens_at_end,
        provider_breakdown,
        model_breakdown,
        tool_breakdown,
    })
}

/// Distribute an assistant turn's produced output (and reasoning) tokens evenly
/// across the tools called by that turn. Reasoning is billed as output for
/// deepseek, so both are attributed the same way. Only used to report produced
/// tokens; input/consumed tokens are deliberately not attributed per tool.
fn add_tool_usage(
    assistant: &crate::models::AssistantMessage,
    usage: &Usage,
    tool_breakdown: &mut HashMap<String, ToolUsage>,
) {
    let calls: Vec<&str> = assistant
        .content
        .iter()
        .filter_map(|c| match c {
            crate::models::ContentBlock::ToolCall { name, .. } => Some(name.as_str()),
            _ => None,
        })
        .collect();

    if calls.is_empty() {
        return;
    }

    let n = calls.len() as u64;
    let out_base = usage.output / n;
    let out_rem = usage.output % n;
    let rea_base = usage.reasoning / n;
    let rea_rem = usage.reasoning % n;

    for (i, name) in calls.into_iter().enumerate() {
        let entry = tool_breakdown.entry(name.to_string()).or_default();
        entry.calls += 1;
        // Give any integer division remainder to the first call so the per-tool
        // totals exactly match the session output/reasoning totals.
        entry.output += out_base + if i == 0 { out_rem } else { 0 };
        entry.reasoning += rea_base + if i == 0 { rea_rem } else { 0 };
    }
}

/// Tracks the running prompt-cache state across a session so that each request
/// can be classified as a cache hit or a cache invalidation, and so the HTML
/// report and the aggregate stats stay perfectly in sync.
#[derive(Debug, Default, Clone)]
pub struct CacheTracker {
    last_hit: bool,
    prev_cache_read: u64,
}

/// Threshold for what counts as a cache loss: a request is flagged as
/// invalidating the cache when more than this fraction of the previously-matched
/// prefix stops matching (i.e. `cache_read` drops by more than this fraction).
pub const CACHE_LOSS_THRESHOLD: f64 = 0.03; // 3%

/// The per-request outcome of running [`CacheTracker::record`].
#[derive(Debug, Clone, Copy)]
pub struct CacheObservation {
    /// True when this request lost more than 3% of the previously-matched cache.
    pub is_invalidation: bool,
    /// Number of tokens that dropped out of the cache (`prev - current`, >0 only
    /// when `is_invalidation`).
    pub lost_tokens: u64,
}

/// Track whether the prompt cache was invalidated by the request described by
/// `usage`. Also advances the running state used by later calls.
///
/// A cache invalidation is counted when a request that previously had a cache
/// hit (`cache_read > 0` or `cache_write > 0`) loses more than 3% of the
/// previously-matched prefix on the next request (its `cache_read` drops by more
/// than 3% from the preceding request's `cache_read`). At that point the cached
/// prefix stopped matching, so it had to be rebuilt — the full-price re-write.
/// cached prefix no longer matched, so it had to be rebuilt — the full-price
/// re-write. A complete miss (`cache_read == 0`) is just the 100%-loss extreme.
///
/// `lost_tokens` is the amount by which `cache_read` dropped (preceding minus
/// current), i.e. the cached tokens that stopped matching and had to be
/// re-processed at full input price instead of the discounted cache rate.
///
/// Note: some providers (e.g. deepinfra's DeepSeek) never populate `cache_write`;
/// for them this loss-of-cache transition is the only observable invalidation signal.
impl CacheTracker {
    pub fn record(&mut self, usage: &Usage) -> CacheObservation {
        let mut obs = CacheObservation {
            is_invalidation: false,
            lost_tokens: 0,
        };
        if self.last_hit && self.prev_cache_read > 0 {
            let lost = self.prev_cache_read.saturating_sub(usage.cache_read);
            if lost as f64 / self.prev_cache_read as f64 > CACHE_LOSS_THRESHOLD {
                obs.is_invalidation = true;
                obs.lost_tokens = lost;
            }
        }
        self.prev_cache_read = usage.cache_read;
        self.last_hit = usage.cache_read > 0 || usage.cache_write > 0;
        obs
    }
}

fn parse_iso_epoch(iso: &str) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(iso)
        .map(|dt| dt.timestamp())
        .ok()
        .or_else(|| {
            // Fallback for the "unix epoch" style that may appear in message timestamps.
            iso.parse::<i64>().ok()
        })
}

fn add_usage(
    usage: &Usage,
    total_tokens: &mut u64,
    total_cost: &mut f64,
    input_tokens: &mut u64,
    cached_input_tokens: &mut u64,
    output_tokens: &mut u64,
    reasoning_tokens: &mut u64,
    cost_input: &mut f64,
    cost_output: &mut f64,
    cost_cached: &mut f64,
    provider_breakdown: &mut HashMap<String, ProviderUsage>,
    model_breakdown: &mut HashMap<String, ProviderUsage>,
    provider: Option<&str>,
    model: Option<&str>,
) {
    *total_tokens += usage.total_tokens;
    *total_cost += usage.cost.total;
    *input_tokens += usage.input;
    *cached_input_tokens += usage.cache_read + usage.cache_write;
    *output_tokens += usage.output;
    *reasoning_tokens += usage.reasoning;
    *cost_input += usage.cost.input;
    *cost_output += usage.cost.output;
    *cost_cached += usage.cost.cache_read + usage.cost.cache_write;

    let prov_key = provider.unwrap_or("unknown").to_string();
    let prov = provider_breakdown
        .entry(prov_key.clone())
        .or_insert(ProviderUsage {
            tokens: 0,
            input: 0,
            cached_input: 0,
            output: 0,
            reasoning: 0,
            cost: 0.0,
            calls: 0,
        });
    prov.tokens += usage.total_tokens;
    prov.input += usage.input;
    prov.cached_input += usage.cache_read + usage.cache_write;
    prov.output += usage.output;
    prov.reasoning += usage.reasoning;
    prov.cost += usage.cost.total;
    prov.calls += 1;

    let model_key = model.unwrap_or("unknown").to_string();
    let mdl = model_breakdown.entry(model_key).or_insert(ProviderUsage {
        tokens: 0,
        input: 0,
        cached_input: 0,
        output: 0,
        reasoning: 0,
        cost: 0.0,
        calls: 0,
    });
    mdl.tokens += usage.total_tokens;
    mdl.input += usage.input;
    mdl.cached_input += usage.cache_read + usage.cache_write;
    mdl.output += usage.output;
    mdl.reasoning += usage.reasoning;
    mdl.cost += usage.cost.total;
    mdl.calls += 1;
}

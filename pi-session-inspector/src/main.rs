mod html;
mod models;
mod reporter;
mod scanner;

use clap::Parser;
use reporter::report;
use scanner::scan;

/// Pi Session Inspector — scan Pi sessions and summarize token consumption & pricing.
///
/// Full reporting guide with examples: see REPORTING.md in the project root.
/// Reporting modes: summary table (default), detail (-d), per-model (-M),
/// per-provider (-R), per-tool (-T), and CSV export (-f csv). Select by project
/// (-p), named session (-N), session file path (-S), provider (-P) or model (-m);
/// discover available values with --list-names / --list-projects / --list-session-paths.
#[derive(Parser)]
#[command(name = "pi-inspect")]
#[command(version, about, long_about)]
#[command(disable_help_flag = true)]
struct Cli {
    /// Path to the sessions directory (default: ~/.pi/agent/sessions)
    #[arg(long)]
    sessions_dir: Option<String>,

    /// Filter by project (cwd path). Value is a REGEX matched against the session cwd
    /// (e.g. "firstmate.*rm", "^/home.*/llm")
    #[arg(short, long)]
    project: Option<String>,

    /// Filter by provider (e.g., "openai-codex", "anthropic", "openai")
    #[arg(short = 'P', long)]
    provider: Option<String>,

    /// Filter by model (e.g., "gpt-5.6-sol", "claude-sonnet-4-5")
    #[arg(short, long)]
    model: Option<String>,

    /// Filter by named session. Value is a REGEX matched against the session name
    /// (e.g. "deepseek.*flash", "^implementation")
    #[arg(short = 'N', long)]
    name: Option<String>,

    /// Filter by session file path. Value is a REGEX matched against the session's
    /// jsonl path (includes the project folder under sessions_dir).
    /// e.g. --session-path 'firstmate.*2026-08-07'
    #[arg(short = 'S', long)]
    session_path: Option<String>,

    /// List all distinct named sessions (optionally filtered by --name regex) and exit
    #[arg(long, conflicts_with_all = ["detail", "per_model", "per_provider", "limit", "format"])]
    list_names: bool,

    /// List all distinct projects (cwd paths) and exit. Combine with --project regex to filter.
    #[arg(long, conflicts_with_all = ["detail", "per_model", "per_provider", "limit", "format"])]
    list_projects: bool,

    /// List all session jsonl file paths and exit. Combine with --session-path regex to filter.
    #[arg(long, conflicts_with_all = ["detail", "per_model", "per_provider", "limit", "format"])]
    list_session_paths: bool,

    /// Show per-session detail (not just summary)
    #[arg(short = 'd', long)]
    detail: bool,

    /// Show per-model breakdown within each session
    #[arg(short = 'M', long)]
    per_model: bool,

    /// Show per-provider breakdown
    #[arg(short = 'R', long)]
    per_provider: bool,

    /// Show per-tool breakdown (call counts + output/reasoning tokens produced)
    #[arg(short = 'T', long)]
    per_tool: bool,

    /// Sort summary: token-desc, token-asc, cost-desc, cost-asc, date-desc, date-asc
    #[arg(short = 's', long, default_value = "date-desc")]
    sort: String,

    /// Limit to N sessions (after filtering/sorting)
    #[arg(short = 'n', long)]
    limit: Option<usize>,

    /// Output format: table (default), csv
    #[arg(short = 'f', long, default_value = "table")]
    format: String,

    /// Write a self-contained HTML report (histogram + timeline) to this file
    /// for the matched session(s). Can be combined with the selector flags
    /// (e.g. -S) to target a single session file.
    #[arg(long)]
    html: Option<String>,

    /// Print help and the full reporting guide (REPORTING.md)
    #[arg(short = 'h', long)]
    help: bool,
}

/// The full reporting guide, embedded at build time so it always prints
/// regardless of the current working directory.
const REPORTING_GUIDE: &str = include_str!("../REPORTING.md");

fn main() {
    let cli = Cli::parse();

    // `--help` / `-h` prints the full reporting documentation page.
    if cli.help {
        println!("{}", REPORTING_GUIDE);
        return;
    }

    let sessions_dir = cli.sessions_dir.unwrap_or_else(default_sessions_dir);

    // Compile the name regex once up front so we can fail fast on a bad pattern.
    let name_regex = match cli.name.as_deref().map(regex::Regex::new) {
        Some(Err(e)) => {
            eprintln!("Invalid --name regex: {}", e);
            std::process::exit(1);
        }
        Some(Ok(rx)) => Some(rx),
        None => None,
    };

    // Compile the project regex once up front so we can fail fast on a bad pattern.
    let project_regex = match cli.project.as_deref().map(regex::Regex::new) {
        Some(Err(e)) => {
            eprintln!("Invalid --project regex: {}", e);
            std::process::exit(1);
        }
        Some(Ok(rx)) => Some(rx),
        None => None,
    };

    // Compile the session-path regex up front so we can fail fast on a bad pattern.
    let path_regex = match cli.session_path.as_deref().map(regex::Regex::new) {
        Some(Err(e)) => {
            eprintln!("Invalid --session-path regex: {}", e);
            std::process::exit(1);
        }
        Some(Ok(rx)) => Some(rx),
        None => None,
    };

    // `--list-names` short-circuits before the normal scan/report flow.
    if cli.list_names {
        match scanner::list_named_sessions(&sessions_dir, name_regex.as_ref()) {
            Ok(names) => {
                if names.is_empty() {
                    println!("No named sessions found.");
                } else {
                    println!("{} named session(s):", names.len());
                    for name in &names {
                        println!("  {}", name);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error listing named sessions: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // `--list-projects` short-circuits before the normal scan/report flow.
    if cli.list_projects {
        match scanner::list_projects(&sessions_dir, project_regex.as_ref()) {
            Ok(projects) => {
                if projects.is_empty() {
                    println!("No projects found.");
                } else {
                    println!("{} project(s):", projects.len());
                    for proj in &projects {
                        println!("  {}", proj);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error listing projects: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    // `--list-session-paths` short-circuits before the normal scan/report flow.
    if cli.list_session_paths {
        match scanner::list_session_paths(&sessions_dir, path_regex.as_ref()) {
            Ok(paths) => {
                if paths.is_empty() {
                    println!("No session paths found.");
                } else {
                    println!("{} session file(s):", paths.len());
                    for p in &paths {
                        println!("  {}", p);
                    }
                }
            }
            Err(e) => {
                eprintln!("Error listing session paths: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let options = scanner::ScanOptions {
        sessions_dir,
        project_regex,
        path_regex,
        provider_filter: cli.provider,
        model_filter: cli.model,
        name_regex,
    };

    let mut results = match scan(&options) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("Error scanning sessions: {}", e);
            std::process::exit(1);
        }
    };

    // Apply sort
    reporter::sort_results(&mut results, &cli.sort);

    // Apply limit
    if let Some(limit) = cli.limit {
        results.truncate(limit);
    }

    // HTML report mode: generate a self-contained report for the matched
    // session(s) and write it to the requested file, printing the path.
    if let Some(html_path) = cli.html {
        let path = std::path::Path::new(&html_path);
        match html::write_report(path, &results) {
            Ok(n) => {
                if n == 0 {
                    eprintln!("No sessions matched the filters; nothing written.");
                    std::process::exit(1);
                }
                println!("Wrote HTML report for {} session(s) -> {}", n, path.display());
            }
            Err(e) => {
                eprintln!("Error writing HTML report: {}", e);
                std::process::exit(1);
            }
        }
        return;
    }

    let report_opts = reporter::ReportOptions {
        detail: cli.detail,
        per_model: cli.per_model,
        per_provider: cli.per_provider,
        per_tool: cli.per_tool,
        format: reporter::Format::from_str(&cli.format),
    };

    report(&results, &report_opts);
}

fn default_sessions_dir() -> String {
    dirs::home_dir()
        .map(|h| {
            h.join(".pi")
                .join("agent")
                .join("sessions")
                .to_string_lossy()
                .to_string()
        })
        .unwrap_or_else(|| {
            eprintln!("Warning: Could not determine home directory, using current dir");
            ".pi/agent/sessions".to_string()
        })
}

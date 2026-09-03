use crate::scanner::SessionSummary;
use colored::*;

#[derive(Debug, Clone, Copy)]
pub enum Format {
    Table,
    Csv,
}

impl Format {
    pub fn from_str(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "csv" => Format::Csv,
            _ => Format::Table,
        }
    }
}

pub struct ReportOptions {
    pub detail: bool,
    pub per_model: bool,
    pub per_provider: bool,
    pub per_tool: bool,
    pub format: Format,
}

pub fn sort_results(results: &mut [SessionSummary], sort: &str) {
    match sort {
        "token-desc" => results.sort_by(|a, b| b.total_tokens.cmp(&a.total_tokens)),
        "token-asc" => results.sort_by(|a, b| a.total_tokens.cmp(&b.total_tokens)),
        "cost-desc" => results.sort_by(|a, b| {
            b.total_cost
                .partial_cmp(&a.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "cost-asc" => results.sort_by(|a, b| {
            a.total_cost
                .partial_cmp(&b.total_cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        }),
        "date-desc" => results.sort_by(|a, b| b.started_at.cmp(&a.started_at)),
        "date-asc" => results.sort_by(|a, b| a.started_at.cmp(&b.started_at)),
        _ => results.sort_by(|a, b| b.started_at.cmp(&a.started_at)),
    }
}

pub fn report(results: &[SessionSummary], opts: &ReportOptions) {
    match opts.format {
        Format::Table => report_table(results, opts),
        Format::Csv => report_csv(results, opts),
    }
}

fn report_table(results: &[SessionSummary], opts: &ReportOptions) {
    if results.is_empty() {
        println!("{}", "No sessions found matching the filters.".yellow());
        return;
    }

    // Header
    println!();
    println!(
        "{:<6} {:<21} {:<36} {:>9} {:>10} {:>9} {:>9} {:>11} {:>10}",
        "Idx",
        "Date",
        "Project / Name",
        "Input",
        "CachedIn",
        "Output",
        "Duration",
        "Tokens",
        "Cost ($)"
    );
    println!("{}", "-".repeat(132));

    for (i, s) in results.iter().enumerate() {
        let name = s.display_name.as_deref().unwrap_or_else(|| "—");
        let proj_info = format!(
            "{} {}",
            short_project(&s.cwd),
            if s.display_name.is_some() {
                format!("[{}]", name)
            } else {
                String::new()
            }
        );
        let date = short_date(&s.started_at);

        println!(
            "{:<6} {:<21} {:<36} {:>9} {:>10} {:>9} {:>9} {:>11} {:>10}",
            i + 1,
            date,
            truncate_str(&proj_info, 36),
            format_tokens(s.input_tokens),
            format_tokens(s.cached_input_tokens),
            format_tokens(s.output_tokens),
            format_duration(s.duration_secs),
            format_tokens(s.total_tokens),
            format_cost(s.total_cost),
        );
    }

    println!("{}", "-".repeat(132));

    // Grand total
    let grand_tokens: u64 = results.iter().map(|s| s.total_tokens).sum();
    let grand_cost: f64 = results.iter().map(|s| s.total_cost).sum();
    let grand_input: u64 = results.iter().map(|s| s.input_tokens).sum();
    let grand_cached: u64 = results.iter().map(|s| s.cached_input_tokens).sum();
    let grand_output: u64 = results.iter().map(|s| s.output_tokens).sum();
    let grand_messages: usize = results.iter().map(|s| s.message_count).sum();
    let grand_assistants: usize = results.iter().map(|s| s.assistant_count).sum();

    println!(
        "{:<97} {:>11} {:>10}",
        format!(
            "Total: {} sessions, {} messages, {} assistant calls ({} in / {} cached / {} out)",
            results.len(),
            grand_messages,
            grand_assistants,
            format_tokens(grand_input),
            format_tokens(grand_cached),
            format_tokens(grand_output)
        )
        .dimmed(),
        format_tokens(grand_tokens).bold(),
        format_cost(grand_cost).bold(),
    );

    // Grand provider breakdown
    if opts.per_provider {
        println!();
        println!("{}", "Provider breakdown:".bold().underline());
        let mut prov_totals: std::collections::HashMap<String, (u64, f64, usize)> =
            std::collections::HashMap::new();
        for s in results {
            for (prov, usage) in &s.provider_breakdown {
                let entry = prov_totals.entry(prov.clone()).or_insert((0, 0.0, 0));
                entry.0 += usage.tokens;
                entry.1 += usage.cost;
                entry.2 += usage.calls;
            }
        }
        let mut provs: Vec<_> = prov_totals.into_iter().collect();
        provs.sort_by(|a, b| {
            b.1 .1
                .partial_cmp(&a.1 .1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        println!(
            "{:<30} {:>12} {:>10} {:>8}",
            "Provider", "Tokens", "Cost ($)", "Calls"
        );
        println!("{}", "-".repeat(64));
        for (prov, (tokens, cost, calls)) in &provs {
            println!(
                "{:<30} {:>12} {:>10} {:>8}",
                truncate_str(prov, 30),
                format_tokens(*tokens),
                format_cost(*cost),
                calls
            );
        }
        println!(
            "{:<30} {:>12} {:>10}",
            "Total",
            format_tokens(provs.iter().map(|p| p.1 .0).sum()),
            format_cost(provs.iter().map(|p| p.1 .1).sum::<f64>())
        );
    }

    // Grand model breakdown
    if opts.per_model {
        println!();
        println!("{}", "Model breakdown:".bold().underline());
        let mut model_totals: std::collections::HashMap<String, (u64, f64, usize)> =
            std::collections::HashMap::new();
        for s in results {
            for (model, usage) in &s.model_breakdown {
                let entry = model_totals.entry(model.clone()).or_insert((0, 0.0, 0));
                entry.0 += usage.tokens;
                entry.1 += usage.cost;
                entry.2 += usage.calls;
            }
        }
        let mut models: Vec<_> = model_totals.into_iter().collect();
        models.sort_by(|a, b| {
            b.1 .1
                .partial_cmp(&a.1 .1)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        println!(
            "{:<40} {:>12} {:>10} {:>8}",
            "Model", "Tokens", "Cost ($)", "Calls"
        );
        println!("{}", "-".repeat(74));
        for (model, (tokens, cost, calls)) in &models {
            println!(
                "{:<40} {:>12} {:>10} {:>8}",
                truncate_str(model, 40),
                format_tokens(*tokens),
                format_cost(*cost),
                calls
            );
        }
        println!(
            "{:<40} {:>12} {:>10}",
            "Total",
            format_tokens(models.iter().map(|m| m.1 .0).sum()),
            format_cost(models.iter().map(|m| m.1 .1).sum::<f64>())
        );
    }

    // Per-session detail
    if opts.detail {
        for (i, s) in results.iter().enumerate() {
            print_session_detail(i + 1, s, opts);
        }
    }

    // Grand tool breakdown (call counts + produced output/reasoning tokens).
    // Input/consumed tokens are not attributed per tool (see ToolUsage docs).
    if opts.per_tool {
        println!();
        println!("{}", "Tool breakdown (all sessions):".bold().underline());
        let mut tool_totals: std::collections::HashMap<String, crate::scanner::ToolUsage> =
            std::collections::HashMap::new();
        for s in results {
            for (tool, usage) in &s.tool_breakdown {
                let entry = tool_totals.entry(tool.clone()).or_default();
                entry.calls += usage.calls;
                entry.output += usage.output;
                entry.reasoning += usage.reasoning;
            }
        }
        let mut tools: Vec<_> = tool_totals.into_iter().collect();
        tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));

        println!(
            "{:<32} {:>7} {:>12} {:>12} {:>12}",
            "Tool", "Calls", "Output", "Reasoning", "Out+Reas"
        );
        println!("{}", "-".repeat(80));
        for (tool, usage) in &tools {
            println!(
                "{:<32} {:>7} {:>12} {:>12} {:>12}",
                truncate_str(tool, 32),
                usage.calls,
                format_tokens(usage.output),
                format_tokens(usage.reasoning),
                format_tokens(usage.output + usage.reasoning)
            );
        }
        println!(
            "{:<32} {:>7} {:>12} {:>12} {:>12}",
            "Total",
            tools.iter().map(|t| t.1.calls).sum::<usize>(),
            format_tokens(tools.iter().map(|t| t.1.output).sum::<u64>()),
            format_tokens(tools.iter().map(|t| t.1.reasoning).sum::<u64>()),
            format_tokens(
                tools
                    .iter()
                    .map(|t| t.1.output + t.1.reasoning)
                    .sum::<u64>()
            )
        );
        println!(
            "{}",
            "Note: output/reasoning are the tokens the model produced to issue each \
             tool call (evenly split per turn). Consumed/input tokens are not attributed \
             per tool."
                .dimmed()
        );
    }
}

fn print_session_detail(idx: usize, s: &SessionSummary, _opts: &ReportOptions) {
    println!();
    println!("{}", format!("── Session {} ──", idx).bold().underline());
    println!("  ID:       {}", s.session_id);
    println!("  File:     {}", s.session_file.display());
    println!("  Project:  {}", s.cwd);
    println!("  Started:  {}", s.started_at);
    if let Some(ref name) = s.display_name {
        println!("  Name:     {}", name);
    }
    println!(
        "  Messages: {} ({} user, {} assistant, {} tool results)",
        s.message_count, s.user_count, s.assistant_count, s.tool_result_count
    );
    println!("  Turns:    {}", s.turns);
    println!("  Duration: {}", format_duration(s.duration_secs));
    println!("  Cache invalidations: {} ({}, ${:.6})",
        s.cache_invalidations,
        format_tokens(s.cache_lost_tokens),
        s.cache_lost_cost);
    println!("  Context at end of session: {}", format_tokens(s.context_tokens_at_end));
    println!("  Tokens:");
    println!(
        "    Input tokens:      {:>12}",
        format_tokens(s.input_tokens)
    );
    println!(
        "    Cached input:      {:>12}",
        format_tokens(s.cached_input_tokens)
    );
    println!(
        "    Output tokens:     {:>12}",
        format_tokens(s.output_tokens)
    );
    println!(
        "    Reasoning tokens:  {:>12}",
        format_tokens(s.reasoning_tokens)
    );
    println!(
        "    Total:             {:>12}",
        format_tokens(s.total_tokens)
    );
    println!("  Cost (price breakdown):");
    println!(
        "    Input:        ${:<12.6}   (@ {:.4}/M)",
        s.cost_input,
        rate_per_m(s.cost_input, s.input_tokens)
    );
    println!(
        "    Cached:       ${:<12.6}   (@ {:.4}/M)",
        s.cost_cached,
        rate_per_m(s.cost_cached, s.cached_input_tokens)
    );
    println!(
        "    Output:       ${:<12.6}   (@ {:.4}/M)",
        s.cost_output,
        rate_per_m(s.cost_output, s.output_tokens)
    );
    if s.reasoning_tokens > 0 && s.output_tokens > 0 {
        // Reasoning is billed as output (deepseek). Estimate its share of the
        // output cost proportionally to its share of output tokens.
        let reasoning_cost = s.cost_output * (s.reasoning_tokens as f64 / s.output_tokens as f64);
        println!(
            "      └ includes ~${:.6} reasoning ({} billed as output)",
            reasoning_cost,
            format_tokens(s.reasoning_tokens)
        );
    }
    println!("    Total:        ${:.6}", s.total_cost);

    if !s.compactions.is_empty() {
        let ct: u64 = s.compactions.iter().map(|c| c.tokens).sum();
        let cc: f64 = s.compactions.iter().map(|c| c.cost).sum();
        println!(
            "  Compactions: {} ({}, ${:.6})",
            s.compactions.len(),
            format_tokens(ct),
            cc
        );
    }
    if !s.branch_summaries.is_empty() {
        let bt: u64 = s.branch_summaries.iter().map(|b| b.tokens).sum();
        let bc: f64 = s.branch_summaries.iter().map(|b| b.cost).sum();
        println!(
            "  Branch summaries: {} ({}, ${:.6})",
            s.branch_summaries.len(),
            format_tokens(bt),
            bc
        );
    }

    if !s.model_breakdown.is_empty() {
        println!("  Models:");
        let mut models: Vec<_> = s.model_breakdown.iter().collect();
        models.sort_by(|a, b| {
            b.1.cost
                .partial_cmp(&a.1.cost)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for (model, usage) in &models {
            println!("    {:<35} {:>10} {:>10}  ({:>3} calls)   in {} / cached {} / out {} / reasoning {}",
                truncate_str(model, 35),
                format_tokens(usage.tokens),
                format_cost(usage.cost),
                usage.calls,
                format_tokens(usage.input),
                format_tokens(usage.cached_input),
                format_tokens(usage.output),
                format_tokens(usage.reasoning));
        }
    }

    if !s.tool_breakdown.is_empty() {
        println!("  Tool breakdown (calls + produced output/reasoning):");
        let mut tools: Vec<_> = s.tool_breakdown.iter().collect();
        tools.sort_by(|a, b| b.1.calls.cmp(&a.1.calls));
        for (tool, usage) in &tools {
            println!(
                "    {:<35} {:>5} calls  out {} / reasoning {}",
                truncate_str(tool, 35),
                usage.calls,
                format_tokens(usage.output),
                format_tokens(usage.reasoning)
            );
        }
        println!(
            "    {}",
            "(output/reasoning split evenly across the calls in each turn; \
             consumed/input tokens not attributed per tool)"
                .dimmed()
        );
    }
}

fn report_csv(results: &[SessionSummary], opts: &ReportOptions) {
    // CSV header
    let mut headers = vec![
        "index",
        "session_id",
        "file",
        "cwd",
        "display_name",
        "started_at",
        "total_tokens",
        "total_cost",
        "input_tokens",
        "cached_input_tokens",
        "output_tokens",
        "duration_secs",
        "turns",
        "message_count",
        "user_count",
        "assistant_count",
        "tool_result_count",
        "compaction_count",
        "branch_summary_count",
        "cache_invalidations",
        "cache_lost_tokens",
        "cache_lost_cost",
        "context_tokens_at_end",
    ];
    if opts.per_provider {
        headers.push("provider_breakdown");
    }
    if opts.per_model {
        headers.push("model_breakdown");
    }
    if opts.per_tool {
        headers.push("tool_breakdown");
    }
    println!("{}", headers.join(","));

    for (i, s) in results.iter().enumerate() {
        let _compaction_tokens: u64 = s.compactions.iter().map(|c| c.tokens).sum();
        let _branch_tokens: u64 = s.branch_summaries.iter().map(|c| c.tokens).sum();

        let mut fields = vec![
            (i + 1).to_string(),
            s.session_id.clone(),
            s.session_file.to_string_lossy().to_string(),
            escape_csv(&s.cwd),
            escape_csv(s.display_name.as_deref().unwrap_or("")),
            s.started_at.clone(),
            s.total_tokens.to_string(),
            format!("{:.6}", s.total_cost),
            s.input_tokens.to_string(),
            s.cached_input_tokens.to_string(),
            s.output_tokens.to_string(),
            s.duration_secs.to_string(),
            s.turns.to_string(),
            s.message_count.to_string(),
            s.user_count.to_string(),
            s.assistant_count.to_string(),
            s.tool_result_count.to_string(),
            s.compactions.len().to_string(),
            s.branch_summaries.len().to_string(),
            s.cache_invalidations.to_string(),
            s.cache_lost_tokens.to_string(),
            format!("{:.6}", s.cache_lost_cost),
            s.context_tokens_at_end.to_string(),
        ];

        if opts.per_provider {
            let pb: Vec<String> = s
                .provider_breakdown
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}t/{}i/{}c/{}o/{:.6}c/{}r",
                        k, v.tokens, v.input, v.cached_input, v.output, v.cost, v.calls
                    )
                })
                .collect();
            fields.push(pb.join("; "));
        }
        if opts.per_model {
            let mb: Vec<String> = s
                .model_breakdown
                .iter()
                .map(|(k, v)| {
                    format!(
                        "{}:{}t/{}i/{}c/{}o/{:.6}c/{}r",
                        k, v.tokens, v.input, v.cached_input, v.output, v.cost, v.calls
                    )
                })
                .collect();
            fields.push(mb.join("; "));
        }
        if opts.per_tool {
            let tb: Vec<String> = s
                .tool_breakdown
                .iter()
                .map(|(k, v)| format!("{}:{}c/{}o/{}r", k, v.calls, v.output, v.reasoning))
                .collect();
            fields.push(tb.join("; "));
        }
        println!("{}", fields.join(","));
    }
}

// ── Helper functions ──

/// Cost (dollars) per 1M tokens for a given cost/token pair.
/// Returns 0 when there are no tokens to divide by.
fn rate_per_m(cost: f64, tokens: u64) -> f64 {
    if tokens == 0 {
        0.0
    } else {
        cost * 1_000_000.0 / tokens as f64
    }
}

fn format_tokens(tokens: u64) -> String {
    if tokens >= 1_000_000 {
        format!("{:.1}M", tokens as f64 / 1_000_000.0)
    } else if tokens >= 1_000 {
        format!("{:.1}K", tokens as f64 / 1_000.0)
    } else {
        tokens.to_string()
    }
}

fn format_cost(cost: f64) -> String {
    if cost >= 10.0 {
        format!("${:.2}", cost)
    } else if cost >= 1.0 {
        format!("${:.4}", cost)
    } else {
        format!("${:.6}", cost)
    }
}

fn short_date(iso: &str) -> String {
    // "2026-07-31T18:47:26.527Z" -> "2026-07-31 18:47"
    if iso.len() >= 16 {
        iso[..16].replace('T', " ")
    } else {
        iso.to_string()
    }
}

fn format_duration(secs: i64) -> String {
    if secs <= 0 {
        return "—".to_string();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    let secs_left = secs % 60;
    if days > 0 {
        format!("{}d {:02}h {:02}m", days, hours, mins)
    } else if hours > 0 {
        format!("{}h {:02}m {:02}s", hours, mins, secs_left)
    } else if mins > 0 {
        format!("{}m {:02}s", mins, secs_left)
    } else {
        format!("{}s", secs_left)
    }
}

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

fn truncate_str(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..max - 1])
    }
}

fn escape_csv(s: &str) -> String {
    if s.contains(',') || s.contains('"') || s.contains('\n') {
        format!("\"{}\"", s.replace('"', "\"\""))
    } else {
        s.to_string()
    }
}

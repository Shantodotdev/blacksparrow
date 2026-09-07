//! # CLI Command Handlers
//!
//! Implements execution logic for `audit`, `inspect`, `mcp`, `report`, `list`,
//! `issues`, `check-ai`, `delete`, `clean`, and `schema` commands.

use crate::cli::args::{
    AuditArgs, CheckAiArgs, CleanArgs, Cli, Commands, DeleteArgs, InspectArgs, IssuesArgs,
    ListArgs, McpArgs, ReportArgs, SchemaArgs,
};
use crate::core::config::CrawlConfig;
use crate::core::models::{IssueCategory, Severity};
use crate::crawler::ai_check::audit_ai_readiness;
use crate::crawler::client::{FetchOptions, HttpClient};
use crate::crawler::engine::{run_crawl_with_options, CrawlResult, ProgressCallback};
use crate::crawler::inspector::inspect_url_with_options;
use crate::graph::{compute_pagerank, SiteGraph};
use crate::report::{
    create_crawl_progress_bar, export_csv_suite, export_json_report, export_markdown_report,
    finish_crawl_progress, print_ai_readiness_scorecard, print_audit_banner,
    print_executive_scorecard, print_historical_sessions, print_issues_matrix,
    print_page_inspection, print_schema_outcome, update_crawl_progress,
};
use crate::rules::page::schema_val::validate_raw_schema;
use crate::storage::{resolve_db_path, CrawlSessionInit, Database, IssueFilterCriteria};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

/// Executes the parsed CLI command.
pub async fn execute(cli: Cli) -> Result<(), Box<dyn std::error::Error>> {
    match cli.command {
        Commands::Audit(args) => handle_audit(args).await,
        Commands::Inspect(args) => handle_inspect(args).await,
        Commands::Mcp(args) => handle_mcp(args).await,
        Commands::Report(args) => handle_report(args).await,
        Commands::List(args) => handle_list(args).await,
        Commands::Issues(args) => handle_issues(args).await,
        Commands::CheckAi(args) => handle_check_ai(args).await,
        Commands::Delete(args) => handle_delete(args).await,
        Commands::Clean(args) => handle_clean(args).await,
        Commands::Schema(args) => handle_schema(args).await,
    }
}

/// Executes a full or partial website audit crawl.
async fn handle_audit(args: AuditArgs) -> Result<(), Box<dyn std::error::Error>> {
    let mut config = CrawlConfig::new(&args.url)?;
    config.max_pages = args.max_pages;
    config.max_depth = args.max_depth;
    config.concurrency = args.concurrency;
    config.delay_ms = args.delay;
    config.user_agent = args.user_agent;
    config.respect_robots = !args.no_robots;
    config.no_aimd = args.no_aimd;
    config.ephemeral = args.ephemeral;
    config.max_query_params = args.max_query_params;
    config.ignore_sorting_facets = args.ignore_sorting_facets;
    config.include_regex = args.include;
    config.exclude_regex = args.exclude;
    config.quiet = args.quiet;
    config.crawl_name = args.name;

    if let Some(sm) = args.sitemap {
        config.explicit_sitemaps.push(sm);
    }
    for h in args.headers {
        if let Some((k, v)) = h.split_once(':') {
            config
                .headers
                .push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    let db_path = resolve_db_path(args.db_path.clone(), args.local);
    config.db_path = Some(db_path.clone());

    let session_id = format!(
        "crawl_{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
    );
    config.session_id = Some(session_id.clone());

    let (writer_handle, writer_task) = if !args.ephemeral {
        let db = Database::open(&db_path)?;
        db.init_crawl_session(&CrawlSessionInit {
            session_id: session_id.clone(),
            target_url: config.start_url.clone(),
            max_pages: config.max_pages,
            max_depth: config.max_depth,
            respect_robots: config.respect_robots,
            render_js: config.render_js,
        })?;
        let (handle, task) = db.spawn_writer(&session_id, 50, Duration::from_millis(500))?;
        (Some(handle), Some(task))
    } else {
        (None, None)
    };

    if !config.quiet {
        print_audit_banner(
            &config.start_url,
            config.max_pages,
            config.concurrency,
            !config.no_aimd,
        );
    }

    let pb = if !config.quiet {
        Some(create_crawl_progress_bar(config.max_pages))
    } else {
        None
    };

    let progress_cb: Option<ProgressCallback> = if let Some(ref progress_bar) = pb {
        let pb_clone = progress_bar.clone();
        Some(Arc::new(move |update| {
            update_crawl_progress(&pb_clone, &update);
        }))
    } else {
        None
    };

    let crawl_result =
        run_crawl_with_options(&config, progress_cb, writer_handle.clone(), None).await?;

    if let Some(ref progress_bar) = pb {
        finish_crawl_progress(progress_bar);
    }

    if let (Some(handle), Some(task)) = (writer_handle, writer_task) {
        let _ = handle.shutdown().await;
        let _ = task.await;
    }

    if args.ephemeral && db_path.exists() {
        let _ = std::fs::remove_file(&db_path);
        let mut shm = db_path.clone();
        shm.set_extension("db-shm");
        let _ = std::fs::remove_file(shm);
        let mut wal = db_path.clone();
        wal.set_extension("db-wal");
        let _ = std::fs::remove_file(wal);
    }

    // Handle file exports
    let formats: Vec<&str> = args.format.split(',').map(|s| s.trim()).collect();
    let mut exported_artifacts = Vec::new();

    if formats.contains(&"md") || formats.contains(&"all") {
        if let Ok(path) = export_markdown_report(&crawl_result, &args.output_dir) {
            exported_artifacts.push(("Markdown", path));
        }
    }

    if formats.contains(&"json") || formats.contains(&"all") {
        if let Ok(path) = export_json_report(&crawl_result, &args.output_dir) {
            exported_artifacts.push(("JSON", path));
        }
    }

    if formats.contains(&"csv") || formats.contains(&"all") {
        if let Ok(paths) = export_csv_suite(&crawl_result, &args.output_dir) {
            if let Some(first) = paths.first() {
                if let Some(parent) = first.parent() {
                    exported_artifacts.push(("CSV Suite", parent.to_path_buf()));
                }
            }
        }
    }

    // Always display executive terminal scorecard if requested and not in quiet mode
    if (formats.contains(&"terminal") || formats.contains(&"all") || formats.is_empty())
        && !config.quiet
    {
        let ref_paths: Vec<(&str, &std::path::Path)> = exported_artifacts
            .iter()
            .map(|(fmt, p)| (*fmt, p.as_path()))
            .collect();
        print_executive_scorecard(&crawl_result, &ref_paths);
    }

    // Check CI/CD failure threshold
    let critical_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Critical)
        .count();
    let alert_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Alert)
        .count();
    let warning_count = crawl_result
        .issues
        .iter()
        .filter(|i| i.severity == Severity::Warning)
        .count();

    let should_fail = match args.fail_on.to_lowercase().as_str() {
        "critical" => critical_count > 0,
        "alert" => critical_count > 0 || alert_count > 0,
        "warning" => critical_count > 0 || alert_count > 0 || warning_count > 0,
        _ => false,
    };

    if should_fail {
        eprintln!(
            "❌ Audit failed CI/CD threshold policy (--fail-on {})",
            args.fail_on
        );
        std::process::exit(1);
    }

    Ok(())
}

/// Inspects a single page and prints its SEO metadata and audit issues.
async fn handle_inspect(args: InspectArgs) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(args.timeout);
    let mut custom_headers = Vec::new();
    for h in args.headers {
        if let Some((k, v)) = h.split_once(':') {
            custom_headers.push((k.trim().to_string(), v.trim().to_string()));
        }
    }

    match inspect_url_with_options(&args.url, &args.user_agent, timeout, custom_headers).await {
        Ok((page, fetch, issues)) => {
            if args.format.eq_ignore_ascii_case("json") {
                let json_output = serde_json::json!({
                    "url": fetch.url,
                    "final_url": fetch.final_url,
                    "status_code": fetch.status_code,
                    "ttfb_ms": fetch.ttfb_ms,
                    "content_type": fetch.content_type,
                    "title": page.title,
                    "meta_description": page.meta_description,
                    "h1": page.h1_primary,
                    "canonical_url": page.canonical_url,
                    "word_count": page.word_count,
                    "issues": issues,
                });
                println!("{}", serde_json::to_string_pretty(&json_output)?);
            } else if args.format.eq_ignore_ascii_case("md") {
                println!("# Page Inspection: {}\n", fetch.final_url);
                println!("- **Status**: {}", fetch.status_code);
                println!("- **TTFB**: {}ms", fetch.ttfb_ms);
                println!("- **Title**: {}", page.title.as_deref().unwrap_or("None"));
                println!("- **H1**: {}", page.h1_primary.as_deref().unwrap_or("None"));
                println!(
                    "- **Canonical**: {}",
                    page.canonical_url.as_deref().unwrap_or("None")
                );
                println!("\n## Detected Issues ({})", issues.len());
                for issue in &issues {
                    println!(
                        "- **[{:?}]** {}: {}",
                        issue.severity,
                        issue.code.as_str(),
                        issue.message
                    );
                }
            } else {
                print_page_inspection(&page, &fetch, &issues);
            }
            Ok(())
        }
        Err(err) => {
            eprintln!("❌ Failed to inspect URL '{}': {}", args.url, err);
            std::process::exit(2);
        }
    }
}

/// Launches the native Model Context Protocol (MCP) server.
async fn handle_mcp(args: McpArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(args.db_path, args.local);
    if args.transport.eq_ignore_ascii_case("stdio") {
        crate::mcp::run_mcp_server(Some(db_path)).await?;
    } else {
        eprintln!(
            "❌ Transport '{}' is not currently supported. Please use '--transport stdio'.",
            args.transport
        );
        std::process::exit(1);
    }
    Ok(())
}

/// Inspects or re-exports an existing audit session from persistence.
async fn handle_report(args: ReportArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(args.db_path, args.local);
    if !db_path.exists() {
        eprintln!(
            "❌ Persistence database not found at '{}'. Run an audit first.",
            db_path.display()
        );
        std::process::exit(1);
    }

    let session_id = match args.session.or(args.session_pos) {
        Some(s) => s,
        None => {
            eprintln!("❌ Missing session ID. Usage: seolens report <SESSION_ID> or seolens report --session <SESSION_ID>");
            std::process::exit(1);
        }
    };

    let db = Database::open(&db_path)?;
    let crawl = match db.get_crawl(&session_id)? {
        Some(c) => c,
        None => {
            eprintln!(
                "❌ Session '{}' was not found in '{}'. Use 'seolens list' to see saved sessions.",
                session_id,
                db_path.display()
            );
            std::process::exit(1);
        }
    };

    println!("📦 Loading session '{}' from SQLite...", session_id);
    let pages = db.get_crawl_pages(&session_id, 100_000, 0)?;
    let issues = db.get_crawl_issues(&session_id, None, None)?;

    let sitemap_urls: Vec<String> = pages
        .iter()
        .filter(|p| p.is_sitemap_url)
        .map(|p| p.url.clone())
        .collect();
    let graph = SiteGraph::from_pages(&pages, &sitemap_urls);
    let pagerank = compute_pagerank(&graph, 0.85, 100, 1e-6);

    let crawl_result = CrawlResult {
        target_url: crawl.target_url.clone(),
        pages,
        graph,
        pagerank,
        issues,
        duration: Duration::from_secs(0),
        sitemap_urls,
        aimd_delay_ms: 0,
        health_score: crawl.health_score,
    };

    let output_dir = args
        .output_dir
        .unwrap_or_else(|| PathBuf::from("./reports"));
    let format_str = args
        .format
        .unwrap_or_else(|| "terminal,json,md".to_string());
    let formats: Vec<&str> = format_str.split(',').map(|s| s.trim()).collect();
    let mut exported_artifacts = Vec::new();

    if formats.contains(&"md") || formats.contains(&"all") {
        if let Ok(path) = export_markdown_report(&crawl_result, &output_dir) {
            exported_artifacts.push(("Markdown", path));
        }
    }

    if formats.contains(&"json") || formats.contains(&"all") {
        if let Ok(path) = export_json_report(&crawl_result, &output_dir) {
            exported_artifacts.push(("JSON", path));
        }
    }

    if formats.contains(&"csv") || formats.contains(&"all") {
        if let Ok(paths) = export_csv_suite(&crawl_result, &output_dir) {
            if let Some(first) = paths.first() {
                if let Some(parent) = first.parent() {
                    exported_artifacts.push(("CSV Suite", parent.to_path_buf()));
                }
            }
        }
    }

    if formats.contains(&"terminal") || formats.contains(&"all") || formats.is_empty() {
        let ref_paths: Vec<(&str, &std::path::Path)> = exported_artifacts
            .iter()
            .map(|(fmt, p)| (*fmt, p.as_path()))
            .collect();
        print_executive_scorecard(&crawl_result, &ref_paths);
    }

    Ok(())
}

/// Lists historical audit sessions stored locally.
async fn handle_list(args: ListArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(args.db_path, args.local);
    if !db_path.exists() {
        if args.format.eq_ignore_ascii_case("json") {
            println!("[]");
        } else {
            print_historical_sessions(&db_path, &[]);
        }
        return Ok(());
    }

    let db = Database::open(&db_path)?;
    let crawls = db.list_crawls()?;

    if args.format.eq_ignore_ascii_case("json") {
        let displayed = if args.limit > 0 {
            crawls.into_iter().take(args.limit).collect::<Vec<_>>()
        } else {
            crawls
        };
        println!("{}", serde_json::to_string_pretty(&displayed)?);
    } else {
        let displayed = if args.limit > 0 {
            crawls.into_iter().take(args.limit).collect::<Vec<_>>()
        } else {
            crawls
        };
        print_historical_sessions(&db_path, &displayed);
    }

    Ok(())
}

/// Drill down and filter audit findings for a session.
async fn handle_issues(args: IssuesArgs) -> Result<(), Box<dyn std::error::Error>> {
    let session_id = match args.session.or(args.session_pos) {
        Some(s) => s,
        None => {
            eprintln!("❌ Missing session ID. Usage: seolens issues <SESSION_ID> [OPTIONS]");
            std::process::exit(1);
        }
    };

    let db_path = resolve_db_path(args.db_path, args.local);
    if !db_path.exists() {
        eprintln!(
            "❌ Database not found at '{}'. Run an audit first.",
            db_path.display()
        );
        std::process::exit(1);
    }

    let db = Database::open(&db_path)?;
    let sev_filter = args
        .severity
        .as_deref()
        .and_then(|s| match s.to_lowercase().as_str() {
            "critical" => Some(Severity::Critical),
            "alert" => Some(Severity::Alert),
            "warning" => Some(Severity::Warning),
            "notice" => Some(Severity::Notice),
            _ => None,
        });
    let cat_filter = args
        .category
        .as_deref()
        .and_then(IssueCategory::from_str_name);

    let criteria = IssueFilterCriteria {
        severity: sev_filter,
        category: cat_filter,
        code: args.code.as_deref(),
        url_substring: args.url.as_deref(),
        limit: args.limit,
        offset: args.offset,
    };

    let total = db.count_issues_filtered(&session_id, &criteria)?;
    let issues = db.query_issues_filtered(&session_id, &criteria)?;

    if args.format.eq_ignore_ascii_case("json") {
        let out = serde_json::json!({
            "session_id": session_id,
            "total_matching": total,
            "offset": args.offset,
            "limit": args.limit,
            "issues": issues,
        });
        println!("{}", serde_json::to_string_pretty(&out)?);
    } else if args.format.eq_ignore_ascii_case("md") {
        println!("# Audit Issues: {session_id}\n");
        println!(
            "Found {total} matching issues (showing {}..{}):\n",
            args.offset,
            (args.offset + issues.len()).min(total)
        );
        for (i, issue) in issues.iter().enumerate() {
            println!(
                "### {}. [{:?}] {}",
                args.offset + i + 1,
                issue.severity,
                issue.code.as_str()
            );
            println!("- **Target URL**: {}", issue.target_url);
            println!("- **Category**: {:?}", issue.category);
            println!("- **Message**: {}", issue.message);
            if let Some(ref s) = issue.source_page_url {
                println!("- **Source Page**: {}", s);
            }
            println!();
        }
    } else {
        print_issues_matrix(&session_id, &issues, total, args.offset, args.limit);
    }

    Ok(())
}

/// Check website readiness for AI search engines (ChatGPT Search, Perplexity, Claude) and /llms.txt.
async fn handle_check_ai(args: CheckAiArgs) -> Result<(), Box<dyn std::error::Error>> {
    let timeout = Duration::from_secs(args.timeout);
    let report = audit_ai_readiness(&args.url, &args.user_agent, timeout).await?;

    if args.format.eq_ignore_ascii_case("json") {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else if args.format.eq_ignore_ascii_case("md") {
        println!("# AI Search & GEO Readiness: {}\n", report.base_url);
        println!("- **Citation Risk**: {}", report.citation_search_risk);
        println!(
            "- **/llms.txt**: {}",
            if report.llms_txt_found {
                "Found (200 OK)"
            } else {
                "Missing (404)"
            }
        );
        println!(
            "- **/llms-full.txt**: {}",
            if report.llms_full_txt_found {
                "Found (200 OK)"
            } else {
                "Not published"
            }
        );
        println!("\n## Real-Time Search & Retrieval Bots");
        for (bot, status) in &report.retrieval_bots {
            println!("- **{bot}**: {status}");
        }
        println!("\n## AI Training Bots");
        for (bot, status) in &report.training_bots {
            println!("- **{bot}**: {status}");
        }
        if !report.recommendations.is_empty() {
            println!("\n## Recommendations");
            for rec in &report.recommendations {
                println!("- {rec}");
            }
        }
    } else {
        print_ai_readiness_scorecard(&report);
    }

    Ok(())
}

/// Delete a specific crawl session and its associated records.
async fn handle_delete(args: DeleteArgs) -> Result<(), Box<dyn std::error::Error>> {
    let session_id = match args.session.or(args.session_pos) {
        Some(s) => s,
        None => {
            eprintln!("❌ Missing session ID. Usage: seolens delete <SESSION_ID>");
            std::process::exit(1);
        }
    };

    let db_path = resolve_db_path(args.db_path, args.local);
    if !db_path.exists() {
        eprintln!("❌ Database not found at '{}'.", db_path.display());
        std::process::exit(1);
    }

    let db = Database::open(&db_path)?;
    let deleted = db.delete_crawl(&session_id)?;

    if deleted {
        println!("🗑️  Successfully deleted session '{session_id}' and all associated records.");
    } else {
        eprintln!("⚠️  Session '{session_id}' was not found in database.");
    }

    Ok(())
}

/// Clean historical crawl sessions from the database.
async fn handle_clean(args: CleanArgs) -> Result<(), Box<dyn std::error::Error>> {
    let db_path = resolve_db_path(args.db_path, args.local);
    if !db_path.exists() {
        println!(
            "Database file '{}' does not exist. Nothing to clean.",
            db_path.display()
        );
        return Ok(());
    }

    let db = Database::open(&db_path)?;

    if args.all {
        let count = db.clean_all_crawls()?;
        println!("🧹 Successfully purged all {count} audit sessions from database.");
    } else if let Some(days) = args.older_than {
        let count = db.clean_crawls_older_than(days)?;
        println!("🧹 Successfully purged {count} audit sessions older than {days} days.");
    } else {
        eprintln!("❌ Please specify either --older-than <days> or --all to clean sessions.");
        std::process::exit(1);
    }

    Ok(())
}

/// Validate JSON-LD / schema against Google Rich Results guidelines.
async fn handle_schema(args: SchemaArgs) -> Result<(), Box<dyn std::error::Error>> {
    let raw_content = if args.target.starts_with("http://") || args.target.starts_with("https://") {
        let client = HttpClient::new(FetchOptions {
            user_agent: args.user_agent,
            timeout: Duration::from_secs(15),
            ..Default::default()
        })?;
        let res = client.fetch(&args.target).await?;
        res.body
    } else {
        std::fs::read_to_string(&args.target)
            .map_err(|e| format!("Failed to read schema file '{}': {e}", args.target))?
    };

    let outcome = validate_raw_schema(&raw_content, args.expected_type.as_deref())?;

    if args.format.eq_ignore_ascii_case("json") {
        println!("{}", serde_json::to_string_pretty(&outcome)?);
    } else {
        print_schema_outcome(&outcome);
    }

    Ok(())
}

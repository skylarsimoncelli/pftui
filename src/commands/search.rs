use anyhow::Result;

/// A flattened CLI command entry for search.
#[derive(Debug, Clone)]
#[cfg_attr(test, derive(PartialEq))]
struct CommandEntry {
    /// Full dotted path, e.g. "journal prediction score-batch"
    path: String,
    /// The doc comment / about string from clap
    description: String,
}

/// Recursively walk a clap `Command` tree and collect every leaf (and branch)
/// into a flat list of `CommandEntry` items.
fn collect_commands(cmd: &clap::Command, prefix: &str) -> Vec<CommandEntry> {
    let mut entries = Vec::new();
    for sub in cmd.get_subcommands() {
        let name = sub.get_name();
        if name == "help" {
            continue; // skip built-in help subcommands
        }
        let path = if prefix.is_empty() {
            name.to_string()
        } else {
            format!("{prefix} {name}")
        };
        let description = sub.get_about().map(|s| s.to_string()).unwrap_or_default();
        entries.push(CommandEntry {
            path: path.clone(),
            description,
        });
        // Recurse into children
        entries.extend(collect_commands(sub, &path));
    }
    entries
}

/// Case-insensitive substring match against path or description.
fn matches_query(entry: &CommandEntry, terms: &[String]) -> bool {
    let path_lower = entry.path.to_lowercase();
    let desc_lower = entry.description.to_lowercase();
    // ALL terms must match (AND logic) — each term can match path or description
    terms
        .iter()
        .all(|t| path_lower.contains(t) || desc_lower.contains(t))
}

pub fn run(cli_cmd: clap::Command, query: &str, json: bool) -> Result<()> {
    let terms: Vec<String> = query.split_whitespace().map(|s| s.to_lowercase()).collect();
    if terms.is_empty() {
        anyhow::bail!("Usage: pftui system search <query>");
    }

    let all = collect_commands(&cli_cmd, "pftui");
    let mut results: Vec<&CommandEntry> = all.iter().filter(|e| matches_query(e, &terms)).collect();

    // Sort: exact path segment matches first, then alphabetically
    results.sort_by(|a, b| {
        let a_path_exact = terms.iter().any(|t| {
            a.path
                .to_lowercase()
                .split_whitespace()
                .any(|seg| seg == *t)
        });
        let b_path_exact = terms.iter().any(|t| {
            b.path
                .to_lowercase()
                .split_whitespace()
                .any(|seg| seg == *t)
        });
        b_path_exact
            .cmp(&a_path_exact)
            .then_with(|| a.path.cmp(&b.path))
    });

    if json {
        let items: Vec<serde_json::Value> = results
            .iter()
            .map(|e| {
                serde_json::json!({
                    "command": e.path,
                    "description": e.description,
                })
            })
            .collect();
        println!(
            "{}",
            serde_json::to_string_pretty(&serde_json::json!({
                "query": query,
                "count": items.len(),
                "results": items,
            }))?
        );
    } else {
        if results.is_empty() {
            println!("No commands matching \"{query}\"");
            println!();
            println!("Try broader terms or check `pftui --help` for the command tree.");
        } else {
            println!("Commands matching \"{query}\":\n");
            for entry in &results {
                if entry.description.is_empty() {
                    println!("  {}", entry.path);
                } else {
                    println!("  {:<50} {}", entry.path, entry.description);
                }
            }
            println!("\n  {} result(s)", results.len());
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_command() -> clap::Command {
        clap::Command::new("pftui")
            .subcommand(
                clap::Command::new("journal")
                    .about("Journal operations")
                    .subcommand(
                        clap::Command::new("prediction")
                            .about("Prediction tracking and scoring")
                            .subcommand(
                                clap::Command::new("score-batch")
                                    .about("Score multiple predictions at once"),
                            )
                            .subcommand(clap::Command::new("list").about("List predictions")),
                    )
                    .subcommand(
                        clap::Command::new("scenario")
                            .about("Scenario management")
                            .subcommand(clap::Command::new("list").about("List scenarios")),
                    ),
            )
            .subcommand(
                clap::Command::new("analytics")
                    .about("Multi-timeframe analytics")
                    .subcommand(
                        clap::Command::new("correlations")
                            .about("Correlation analysis")
                            .subcommand(
                                clap::Command::new("breaks")
                                    .about("Detect correlation breaks between asset pairs"),
                            ),
                    )
                    .subcommand(
                        clap::Command::new("conviction")
                            .about("Conviction tracking")
                            .subcommand(clap::Command::new("list").about("List convictions")),
                    ),
            )
    }

    #[test]
    fn test_collect_commands() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        // Should have journal, journal prediction, journal prediction score-batch, etc.
        assert!(entries.len() >= 8);
        assert!(entries
            .iter()
            .any(|e| e.path == "pftui journal prediction score-batch"));
    }

    #[test]
    fn test_search_batch() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        let terms = vec!["batch".to_string()];
        let results: Vec<_> = entries
            .iter()
            .filter(|e| matches_query(e, &terms))
            .collect();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "pftui journal prediction score-batch");
    }

    #[test]
    fn test_search_correlation() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        let terms = vec!["correlation".to_string()];
        let results: Vec<_> = entries
            .iter()
            .filter(|e| matches_query(e, &terms))
            .collect();
        assert!(results.len() >= 2); // correlations + correlations breaks
        assert!(results
            .iter()
            .any(|e| e.path.contains("correlations breaks")));
    }

    #[test]
    fn test_search_multiple_terms() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        let terms = vec!["prediction".to_string(), "score".to_string()];
        let results: Vec<_> = entries
            .iter()
            .filter(|e| matches_query(e, &terms))
            .collect();
        assert_eq!(results.len(), 1);
        assert!(results[0].path.contains("score-batch"));
    }

    #[test]
    fn test_search_description() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        let terms = vec!["scenario".to_string()];
        let results: Vec<_> = entries
            .iter()
            .filter(|e| matches_query(e, &terms))
            .collect();
        assert!(results.iter().any(|e| e.path.contains("scenario")));
    }

    #[test]
    fn test_search_no_results() {
        let cmd = test_command();
        let entries = collect_commands(&cmd, "pftui");
        let terms = vec!["nonexistent".to_string()];
        let results: Vec<_> = entries
            .iter()
            .filter(|e| matches_query(e, &terms))
            .collect();
        assert!(results.is_empty());
    }
}

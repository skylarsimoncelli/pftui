//! Doc-drift guard for the two primary operator-facing documents.
//!
//! Sibling of `tests/analyst_routine_commands.rs`: parses fenced ```bash
//! blocks in README.md and AGENTS.md and verifies that every literal
//! `pftui ...` invocation still parses against the current binary
//! (via `--help`, so no side effects and no DB access).
//!
//! Conventions (documented in docs/DATA-ARCHITECTURE.md):
//! - heredoc bodies are skipped (templates, not commands);
//! - a line whose trailing comment contains `# (illustrative)` is skipped —
//!   use it to mark intentionally-aspirational examples that should not be
//!   held to the current CLI surface.

use std::fs;
use std::path::Path;
use std::process::{Command, Output};

const DOC_FILES: [&str; 2] = ["README.md", "AGENTS.md"];

#[derive(Debug)]
struct DocCommand {
    file: String,
    line: usize,
    command: String,
}

#[test]
fn documented_pftui_commands_in_readme_and_agents_parse() {
    let commands = collect_doc_commands();
    assert!(
        commands.len() > 30,
        "doc command parser found unexpectedly few commands: {}",
        commands.len()
    );

    let mut failures = Vec::new();
    for doc in commands {
        let args = match sanitize_command(&doc.command) {
            Some(args) => args,
            None => continue,
        };
        let output = run_help(&args);
        if !output.status.success() {
            failures.push(format!(
                "{}:{}\n  {}\n  args: {:?}\n  stdout:\n{}\n  stderr:\n{}",
                doc.file,
                doc.line,
                doc.command,
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "documented commands in README.md/AGENTS.md failed to parse:\n\n{}",
        failures.join("\n\n")
    );
}

fn collect_doc_commands() -> Vec<DocCommand> {
    let mut commands = Vec::new();
    for file in DOC_FILES {
        let path = Path::new(file);
        let content = fs::read_to_string(path)
            .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
        let mut in_bash = false;
        let mut heredoc_end: Option<String> = None;
        for (idx, line) in content.lines().enumerate() {
            let trimmed = line.trim();
            if trimmed.starts_with("```") {
                in_bash = trimmed == "```bash" || trimmed == "```sh" || trimmed == "```shell";
                heredoc_end = None;
                continue;
            }
            if !in_bash {
                continue;
            }
            if let Some(end) = heredoc_end.as_deref() {
                if trimmed == end {
                    heredoc_end = None;
                }
                continue;
            }
            if let Some(end) = heredoc_marker(trimmed) {
                heredoc_end = Some(end.to_string());
                continue;
            }
            // Skip convention for intentionally-aspirational examples.
            if trimmed.contains("# (illustrative)") {
                continue;
            }
            let candidate = trimmed.trim_start_matches('$').trim();
            if candidate.starts_with("pftui ") {
                commands.push(DocCommand {
                    file: file.to_string(),
                    line: idx + 1,
                    command: candidate.to_string(),
                });
            }
        }
    }
    commands
}

fn heredoc_marker(line: &str) -> Option<&str> {
    let (_, marker) = line.split_once("<<")?;
    marker
        .split_whitespace()
        .next()
        .map(|value| value.trim_matches(['\'', '"']))
        .filter(|value| !value.is_empty())
}

fn sanitize_command(command: &str) -> Option<Vec<String>> {
    let mut command = command
        .split('#')
        .next()
        .unwrap_or(command)
        .trim()
        .to_string();
    if let Some((prefix, _)) = command.split_once('|') {
        command = prefix.trim().to_string();
    }
    if let Some((prefix, _)) = command.split_once("&&") {
        command = prefix.trim().to_string();
    }
    if let Some((prefix, _)) = command.split_once("2>") {
        command = prefix.trim().to_string();
    }
    if let Some((prefix, _)) = command.split_once(" > ") {
        command = prefix.trim().to_string();
    }
    // Shell substitutions become a plain placeholder value.
    while let (Some(start), Some(_)) = (command.find("$("), command.find(')')) {
        let end = command[start..].find(')').map(|i| start + i)?;
        command.replace_range(start..=end, "2026-05-29");
    }
    command = command.replace("$TARGET", "2026-08-27");
    command = command.replace("\"$DB\"", "db.sqlite");
    command = command.replace("<your_key>", "demo-key");
    command = command.replace("<key>", "demo-key");
    command = command.replace("<contract_id>", "1");
    command = command.replace("<id>", "1");
    command = command.replace('\\', " ");
    command = command.replace('[', "");
    command = command.replace(']', "");
    command = command.replace('<', "");
    command = command.replace('>', "");

    let mut args = shlex::split(&command)?;
    if args.first().map(String::as_str) != Some("pftui") {
        return None;
    }
    args.remove(0);
    args.push("--help".to_string());
    Some(args)
}

fn run_help(args: &[String]) -> Output {
    Command::new(env!("CARGO_BIN_EXE_pftui"))
        .args(args)
        .env("NO_COLOR", "1")
        .output()
        .unwrap_or_else(|err| panic!("failed to run pftui {args:?}: {err}"))
}

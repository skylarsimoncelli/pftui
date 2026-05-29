use std::fs;
use std::path::Path;
use std::process::{Command, Output};

#[derive(Debug)]
struct RoutineCommand {
    file: String,
    line: usize,
    command: String,
}

#[test]
fn documented_pftui_commands_in_routines_parse() {
    let commands = collect_routine_commands();
    assert!(
        commands.len() > 100,
        "routine command parser found unexpectedly few commands: {}",
        commands.len()
    );

    let mut failures = Vec::new();
    for routine in commands {
        let args = match sanitize_command(&routine.command) {
            Some(args) => args,
            None => continue,
        };
        let output = run_help(&args);
        if !output.status.success() {
            failures.push(format!(
                "{}:{}\n  {}\n  args: {:?}\n  stdout:\n{}\n  stderr:\n{}",
                routine.file,
                routine.line,
                routine.command,
                args,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            ));
        }
    }

    assert!(
        failures.is_empty(),
        "documented routine commands failed to parse:\n\n{}",
        failures.join("\n\n")
    );
}

fn collect_routine_commands() -> Vec<RoutineCommand> {
    let mut commands = Vec::new();
    let dir = Path::new("agents/routines");
    for entry in fs::read_dir(dir).unwrap_or_else(|err| panic!("failed to read {dir:?}: {err}")) {
        let entry = entry.unwrap_or_else(|err| panic!("failed to read routine dir entry: {err}"));
        let path = entry.path();
        if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
            continue;
        }
        let content = fs::read_to_string(&path)
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
            let candidate = trimmed.trim_start_matches('$').trim();
            if candidate.starts_with("pftui ") {
                commands.push(RoutineCommand {
                    file: path.display().to_string(),
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
    if let Some((prefix, _)) = command.split_once("2>") {
        command = prefix.trim().to_string();
    }
    command = command.replace("$(date +%Y-%m-%d)", "2026-05-29");
    command = command.replace("${DIGEST}", "digest");
    command = command.replace("[YYYY-MM-DD]", "2026-05-29");
    command = command.replace("[YYYY-MM-DDTHH:MM:SS]", "2026-05-29T09:00:00");
    command = command.replace("[0.X]", "0.5");
    command = command.replace("[X]", "50");
    command = command.replace("<new>", "50");
    command = command.replace("<id>", "1");
    command = command.replace("<ID>", "1");
    command = command.replace("[ID]", "1");
    command = command.replace("<message-id>", "1");
    command = command.replace("<trend-id>", "1");
    command = command.replace("<n>", "3");
    command = command.replace("[1-10]", "5");
    command = command.replace("[1-5]", "3");
    command = command.replace("<SYMBOL>", "BTC");
    command = command.replace("[SYM]", "BTC");
    command = command.replace("[symbol]", "BTC");
    command = command.replace("<topic>", "fed");
    command = command.replace("[topic]", "fed");
    command = command.replace("[level]", "normal");
    command = command.replace("[score]", "0.5");
    command = command.replace("[ids]", "1");
    command = command.replace("[news.id if article-derived]", "1");
    command = command.replace("[rising|stable|declining]", "stable");
    command = command.replace("[normal|high]", "normal");
    command = command.replace("[normal|elevated|critical]", "normal");
    command = command.replace("[low|normal|high|critical]", "normal");
    command = command.replace("<strengthens|weakens>", "strengthens");
    command = command.replace("<correct|wrong|partial>", "correct");
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

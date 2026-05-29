use std::collections::BTreeSet;
use std::process::{Command, Output};

#[test]
fn every_cli_subcommand_renders_help() {
    let mut visited = BTreeSet::new();
    walk_help(Vec::new(), &mut visited);
    assert!(
        visited.len() > 100,
        "help smoke visited unexpectedly few commands: {}",
        visited.len()
    );
}

fn walk_help(args: Vec<String>, visited: &mut BTreeSet<Vec<String>>) {
    if !visited.insert(args.clone()) {
        return;
    }

    let output = run_help(&args);
    assert_success(&format!("pftui {}", display_args(&args)), &output);
    let stdout = String::from_utf8_lossy(&output.stdout);

    for subcommand in parse_subcommands(&stdout) {
        let mut child = args.clone();
        child.push(subcommand);
        walk_help(child, visited);
    }
}

fn run_help(args: &[String]) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_pftui"));
    command.args(args).arg("--help").env("NO_COLOR", "1");
    command
        .output()
        .unwrap_or_else(|err| panic!("failed to run pftui {} --help: {err}", display_args(args)))
}

fn parse_subcommands(help: &str) -> Vec<String> {
    let mut in_commands = false;
    let mut commands = Vec::new();

    for line in help.lines() {
        let trimmed = line.trim_end();
        if trimmed == "Commands:" {
            in_commands = true;
            continue;
        }
        if !in_commands {
            continue;
        }
        if trimmed.is_empty() {
            continue;
        }
        if !line.starts_with("  ") || trimmed.ends_with(':') {
            break;
        }

        let Some(name) = trimmed.split_whitespace().next() else {
            continue;
        };
        if name != "help" {
            commands.push(name.to_string());
        }
    }

    commands
}

fn assert_success(label: &str, output: &Output) {
    if output.status.success() {
        return;
    }
    panic!(
        "{label} failed with status {}\nstdout:\n{}\nstderr:\n{}",
        output.status,
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

fn display_args(args: &[String]) -> String {
    if args.is_empty() {
        "<root>".to_string()
    } else {
        args.join(" ")
    }
}

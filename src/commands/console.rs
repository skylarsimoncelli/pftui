use std::cmp::Reverse;
use std::path::PathBuf;
use std::process::{Command as ProcessCommand, Stdio};
use std::sync::{Arc, Mutex};

use anyhow::{Context, Result};
use clap::CommandFactory;
use rustyline::completion::{Completer, Pair};
use rustyline::error::ReadlineError;
use rustyline::highlight::Highlighter;
use rustyline::history::DefaultHistory;
use rustyline::hint::Hinter;
use rustyline::validate::Validator;
use rustyline::{
    Cmd, ConditionalEventHandler, Config, Context as RustyContext, Editor, Event, EventContext,
    EventHandler, Helper, KeyCode, KeyEvent, Modifiers,
};

use crate::cli::Cli;

const CYAN: &str = "\x1b[36m";
const YELLOW: &str = "\x1b[33m";
const DIM: &str = "\x1b[2m";
const RESET: &str = "\x1b[0m";

#[derive(Debug, Clone)]
struct ConsoleOption {
    name: String,
    description: String,
}

#[derive(Debug, Clone)]
struct ConsoleNode {
    name: String,
    description: String,
    subcommands: Vec<ConsoleNode>,
    options: Vec<ConsoleOption>,
}

#[derive(Debug, Clone)]
struct BrowserEntry {
    name: String,
    description: String,
}

#[derive(Clone)]
struct ConsoleHelper {
    root: ConsoleNode,
    context_path: Arc<Mutex<Vec<String>>>,
}

struct EmptyBackspaceHandler {
    pop_requested: Arc<Mutex<bool>>,
}

impl Helper for ConsoleHelper {}
impl Hinter for ConsoleHelper {
    type Hint = String;
}
impl Highlighter for ConsoleHelper {}
impl Validator for ConsoleHelper {}

impl Completer for ConsoleHelper {
    type Candidate = Pair;

    fn complete(
        &self,
        line: &str,
        pos: usize,
        _ctx: &RustyContext<'_>,
    ) -> rustyline::Result<(usize, Vec<Pair>)> {
        let prefix = &line[..pos];
        let start = if prefix.ends_with(char::is_whitespace) {
            pos
        } else {
            prefix
                .rfind(char::is_whitespace)
                .map(|idx| idx + 1)
                .unwrap_or(0)
        };
        let current = &prefix[start..];
        let active_node = self.node_for_completion(prefix);
        let mut entries = browser_entries_for_completion(active_node, current);
        if current.starts_with('-') || current.is_empty() {
            entries.extend(option_entries_for_completion(active_node, current));
        }

        let mut seen = std::collections::HashSet::new();
        let mut pairs = Vec::new();
        for entry in entries {
            if !seen.insert(entry.name.clone()) {
                continue;
            }
            pairs.push(Pair {
                display: entry.name.clone(),
                replacement: entry.name,
            });
        }
        Ok((start, pairs))
    }
}

pub fn run(cached_only: bool) -> Result<()> {
    let root = build_console_tree(Cli::command());
    let context_path = Arc::new(Mutex::new(Vec::<String>::new()));
    let pop_requested = Arc::new(Mutex::new(false));
    let helper = ConsoleHelper {
        root: root.clone(),
        context_path: Arc::clone(&context_path),
    };

    let config = Config::builder()
        .history_ignore_dups(true)?
        .completion_type(rustyline::CompletionType::List)
        .build();
    let mut editor: Editor<ConsoleHelper, DefaultHistory> =
        Editor::with_config(config).context("failed to initialize console editor")?;
    editor.set_helper(Some(helper));
    editor.bind_sequence(
        Event::from(KeyEvent(KeyCode::Backspace, Modifiers::NONE)),
        EventHandler::Conditional(Box::new(EmptyBackspaceHandler {
            pop_requested: Arc::clone(&pop_requested),
        })),
    );

    let history_path = history_path();
    if let Some(parent) = history_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = editor.load_history(&history_path);

    print_banner();

    loop {
        let prompt = format_prompt(&context_path.lock().unwrap());
        match editor.readline(&prompt) {
            Ok(line) => {
                let trimmed = line.trim();
                if trimmed.is_empty() {
                    continue;
                }
                let _ = editor.add_history_entry(trimmed);

                match handle_console_line(trimmed, &root, &context_path, cached_only)? {
                    ConsoleAction::Continue => {}
                    ConsoleAction::Exit => break,
                }
            }
            Err(ReadlineError::Interrupted) => {
                println!();
                continue;
            }
            Err(ReadlineError::Eof) => {
                let mut pop = pop_requested.lock().unwrap();
                if *pop {
                    *pop = false;
                    let mut current_path = context_path.lock().unwrap();
                    if !current_path.is_empty() {
                        current_path.pop();
                        continue;
                    }
                }
                println!();
                break;
            }
            Err(err) => return Err(err).context("console input failed"),
        }
    }

    let _ = editor.save_history(&history_path);
    Ok(())
}

enum ConsoleAction {
    Continue,
    Exit,
}

fn handle_console_line(
    line: &str,
    root: &ConsoleNode,
    context_path: &Arc<Mutex<Vec<String>>>,
    cached_only: bool,
) -> Result<ConsoleAction> {
    let trimmed = line.trim();
    if trimmed == "exit" {
        let mut current_path = context_path.lock().unwrap();
        if current_path.is_empty() {
            return Ok(ConsoleAction::Exit);
        }
        current_path.pop();
        return Ok(ConsoleAction::Continue);
    }

    if matches!(trimmed, "?" | "help") {
        let current_path = context_path.lock().unwrap().clone();
        if let Some(node) = find_node(root, &current_path) {
            print_node_browser(node, &current_path);
        }
        return Ok(ConsoleAction::Continue);
    }

    if let Some(filter) = trimmed.strip_suffix('?') {
        let current_path = context_path.lock().unwrap().clone();
        if let Some(node) = find_node(root, &current_path) {
            print_node_browser_filtered(node, &current_path, filter.trim());
        }
        return Ok(ConsoleAction::Continue);
    }

    let tokens = parse_tokens(trimmed);
    if tokens.is_empty() {
        return Ok(ConsoleAction::Continue);
    }

    let current_path = context_path.lock().unwrap().clone();
    let current_node = find_node(root, &current_path).unwrap_or(root);

    if let Some(nav_path) = resolve_navigation_path(current_node, &tokens) {
        let target = find_node(current_node, &nav_path).unwrap_or(current_node);
        if !target.subcommands.is_empty() {
            let mut merged = current_path;
            merged.extend(nav_path);
            *context_path.lock().unwrap() = merged;
            return Ok(ConsoleAction::Continue);
        }
    }

    let current_path = context_path.lock().unwrap().clone();
    execute_command(&current_path, &tokens, cached_only)?;
    Ok(ConsoleAction::Continue)
}

fn execute_command(context: &[String], tokens: &[String], cached_only: bool) -> Result<()> {
    let exe = std::env::current_exe().context("failed to locate current executable")?;
    let mut cmd = ProcessCommand::new(exe);
    if cached_only {
        cmd.arg("--cached-only");
    }
    cmd.args(context);
    cmd.args(tokens);
    let status = cmd
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("failed to execute console command")?;

    if !status.success() {
        println!("{DIM}command exited with status {status}{RESET}");
    }
    Ok(())
}

fn print_banner() {
    println!("{CYAN}pftui console{RESET}");
    println!("{DIM}? for commands, Tab to complete, Backspace on empty prompt to go up, Ctrl-D to exit.{RESET}");
}

fn print_node_browser(node: &ConsoleNode, context_path: &[String]) {
    print_node_browser_filtered(node, context_path, "");
}

fn print_node_browser_filtered(node: &ConsoleNode, context_path: &[String], filter: &str) {
    let subcommands = browser_entries(node, Some(filter));
    let options = option_entries(node, Some(filter));
    let label = if context_path.is_empty() {
        "root".to_string()
    } else {
        context_path.join(" ")
    };
    println!();
    println!("{CYAN}{label}{RESET}");
    if !subcommands.is_empty() {
        println!("{DIM}subcommands{RESET}");
        for entry in subcommands {
            println!("  {CYAN}{:<24}{RESET} {}", entry.name, entry.description);
        }
    }
    if !options.is_empty() {
        println!("{DIM}flags{RESET}");
        for entry in options {
            println!("  {YELLOW}{:<24}{RESET} {}", entry.name, entry.description);
        }
    }
}

fn browser_entries(node: &ConsoleNode, filter: Option<&str>) -> Vec<BrowserEntry> {
    let mut entries: Vec<_> = node
        .subcommands
        .iter()
        .filter(|entry| matches_filter(filter.unwrap_or(""), &entry.name, &entry.description))
        .map(|entry| BrowserEntry {
            name: entry.name.clone(),
            description: entry.description.clone(),
        })
        .collect();
    entries.sort_by_key(|entry| {
        (
            Reverse(score_match(filter.unwrap_or(""), &entry.name, &entry.description)),
            entry.name.clone(),
        )
    });
    entries
}

fn option_entries(node: &ConsoleNode, filter: Option<&str>) -> Vec<BrowserEntry> {
    let mut entries: Vec<_> = node
        .options
        .iter()
        .filter(|entry| matches_filter(filter.unwrap_or(""), &entry.name, &entry.description))
        .map(|entry| BrowserEntry {
            name: entry.name.clone(),
            description: entry.description.clone(),
        })
        .collect();
    entries.sort_by_key(|entry| entry.name.clone());
    entries
}

fn build_console_tree(command: clap::Command) -> ConsoleNode {
    let description = command
        .get_about()
        .map(|text| text.to_string())
        .or_else(|| command.get_long_about().map(|text| text.to_string()))
        .unwrap_or_default();

    let subcommands = command
        .get_subcommands()
        .filter(|subcommand| !subcommand.is_hide_set())
        .cloned()
        .map(build_console_tree)
        .collect();

    let options = command
        .get_arguments()
        .filter(|arg| arg.get_long().is_some() || arg.get_short().is_some())
        .map(|arg| {
            let mut names = Vec::new();
            if let Some(long) = arg.get_long() {
                names.push(format!("--{long}"));
            }
            if let Some(short) = arg.get_short() {
                names.push(format!("-{short}"));
            }
            let description = arg.get_help().map(|text| text.to_string()).unwrap_or_default();
            ConsoleOption {
                name: names.join(", "),
                description,
            }
        })
        .collect();

    ConsoleNode {
        name: command.get_name().to_string(),
        description,
        subcommands,
        options,
    }
}

fn find_node<'a>(root: &'a ConsoleNode, path: &[String]) -> Option<&'a ConsoleNode> {
    let mut node = root;
    for segment in path {
        node = node.subcommands.iter().find(|child| child.name == *segment)?;
    }
    Some(node)
}

fn resolve_navigation_path(node: &ConsoleNode, tokens: &[String]) -> Option<Vec<String>> {
    let mut current = node;
    let mut path = Vec::new();
    for token in tokens {
        if token.starts_with('-') {
            return None;
        }
        let child = current.subcommands.iter().find(|candidate| candidate.name == *token)?;
        path.push(token.clone());
        current = child;
    }
    Some(path)
}

fn parse_tokens(line: &str) -> Vec<String> {
    if line.trim().is_empty() {
        return Vec::new();
    }
    shlex::split(line).unwrap_or_else(|| line.split_whitespace().map(ToString::to_string).collect())
}

fn format_prompt(context_path: &[String]) -> String {
    if context_path.is_empty() {
        "pftui> ".to_string()
    } else {
        format!("pftui {}> ", context_path.join(" "))
    }
}

fn history_path() -> PathBuf {
    if let Some(mut dir) = dirs::data_local_dir() {
        dir.push("pftui");
        dir.push("console_history");
        return dir;
    }
    PathBuf::from(".pftui_console_history")
}

fn matches_filter(filter: &str, name: &str, description: &str) -> bool {
    if filter.trim().is_empty() {
        return true;
    }
    score_match(filter, name, description) > 0
}

fn score_match(filter: &str, name: &str, description: &str) -> u8 {
    let filter = filter.trim().to_lowercase();
    if filter.is_empty() {
        return 1;
    }
    let name_lower = name.to_lowercase();
    let desc_lower = description.to_lowercase();

    if name_lower == filter {
        5
    } else if name_lower.starts_with(&filter) {
        4
    } else if name_lower.contains(&filter) {
        3
    } else if desc_lower.contains(&filter) {
        2
    } else {
        0
    }
}

fn score_match_name_only(filter: &str, name: &str) -> u8 {
    let filter = filter.trim().to_lowercase();
    if filter.is_empty() {
        return 1;
    }
    let name_lower = name.to_lowercase();
    if name_lower == filter {
        5
    } else if name_lower.starts_with(&filter) {
        4
    } else if name_lower.contains(&filter) {
        3
    } else {
        0
    }
}

fn browser_entries_for_completion(node: &ConsoleNode, filter: &str) -> Vec<BrowserEntry> {
    let mut entries: Vec<_> = node
        .subcommands
        .iter()
        .filter(|entry| score_match_name_only(filter, &entry.name) > 0)
        .map(|entry| BrowserEntry {
            name: entry.name.clone(),
            description: entry.description.clone(),
        })
        .collect();
    entries.sort_by_key(|entry| (Reverse(score_match_name_only(filter, &entry.name)), entry.name.clone()));
    entries
}

fn option_entries_for_completion(node: &ConsoleNode, filter: &str) -> Vec<BrowserEntry> {
    let mut entries: Vec<_> = node
        .options
        .iter()
        .filter(|entry| score_match_name_only(filter, &entry.name) > 0)
        .map(|entry| BrowserEntry {
            name: entry.name.clone(),
            description: entry.description.clone(),
        })
        .collect();
    entries.sort_by_key(|entry| (Reverse(score_match_name_only(filter, &entry.name)), entry.name.clone()));
    entries
}

impl ConsoleHelper {
    fn node_for_completion<'a>(&'a self, prefix: &str) -> &'a ConsoleNode {
        let current_path = self.context_path.lock().unwrap().clone();
        let mut node = find_node(&self.root, &current_path).unwrap_or(&self.root);
        let ends_with_space = prefix.ends_with(char::is_whitespace);
        let tokens = parse_tokens(prefix);
        let path_len = if ends_with_space {
            tokens.len()
        } else {
            tokens.len().saturating_sub(1)
        };

        for token in tokens.into_iter().take(path_len) {
            if let Some(child) = node.subcommands.iter().find(|entry| entry.name == token) {
                node = child;
            } else {
                break;
            }
        }
        node
    }
}

impl ConditionalEventHandler for EmptyBackspaceHandler {
    fn handle(
        &self,
        evt: &Event,
        _n: usize,
        _positive: bool,
        ctx: &EventContext,
    ) -> Option<Cmd> {
        if matches!(evt, Event::KeySeq(keys) if keys.len() == 1 && keys[0] == KeyEvent(KeyCode::Backspace, Modifiers::NONE))
            && ctx.line().is_empty()
            && ctx.pos() == 0
        {
            *self.pop_requested.lock().unwrap() = true;
            Some(Cmd::EndOfFile)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_contains_top_level_console_peers() {
        let root = build_console_tree(Cli::command());
        assert!(root.subcommands.iter().any(|node| node.name == "portfolio"));
        assert!(root.subcommands.iter().any(|node| node.name == "analytics"));
    }

    #[test]
    fn resolve_navigation_path_finds_nested_nodes() {
        let root = build_console_tree(Cli::command());
        let analytics = root
            .subcommands
            .iter()
            .find(|node| node.name == "analytics")
            .unwrap();
        let path = resolve_navigation_path(
            analytics,
            &[String::from("macro"), String::from("regime")],
        )
        .unwrap();
        assert_eq!(path, vec!["macro", "regime"]);
    }

    #[test]
    fn score_match_prefers_prefixes() {
        assert!(score_match("port", "portfolio", "") > score_match("port", "import", ""));
    }

    #[test]
    fn completion_matches_names_not_descriptions() {
        let node = ConsoleNode {
            name: "root".into(),
            description: String::new(),
            subcommands: vec![
                ConsoleNode {
                    name: "agent".into(),
                    description: "Agentic operations and inter-agent workflows".into(),
                    subcommands: vec![],
                    options: vec![],
                },
                ConsoleNode {
                    name: "data".into(),
                    description: "Data management operations".into(),
                    subcommands: vec![],
                    options: vec![],
                },
                ConsoleNode {
                    name: "system".into(),
                    description: "System/admin operations".into(),
                    subcommands: vec![],
                    options: vec![],
                },
            ],
            options: vec![],
        };

        let matches = browser_entries_for_completion(&node, "ag");
        let names: Vec<_> = matches.into_iter().map(|entry| entry.name).collect();
        assert_eq!(names, vec!["agent"]);
    }

    #[test]
    fn exit_from_root_leaves_console() {
        let root = build_console_tree(Cli::command());
        let context_path = Arc::new(Mutex::new(Vec::<String>::new()));
        let action = handle_console_line("exit", &root, &context_path, false).unwrap();
        assert!(matches!(action, ConsoleAction::Exit));
        assert!(context_path.lock().unwrap().is_empty());
    }

    #[test]
    fn exit_from_nested_context_moves_up_one_level() {
        let root = build_console_tree(Cli::command());
        let context_path = Arc::new(Mutex::new(vec![
            String::from("analytics"),
            String::from("macro"),
        ]));
        let action = handle_console_line("exit", &root, &context_path, false).unwrap();
        assert!(matches!(action, ConsoleAction::Continue));
        assert_eq!(
            *context_path.lock().unwrap(),
            vec![String::from("analytics")]
        );
    }
}

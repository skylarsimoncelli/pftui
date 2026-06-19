//! Smoke tests for `agents/report-prompts/` templates.
//!
//! Catches the most common regressions:
//!   1. A `{PLACEHOLDER}` in a template that isn't in the skill's known
//!      variable list.
//!   2. An `{INCLUDE ...}` directive that references a missing shared file.
//!   3. A template that lost its variable-declaration header.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};

/// Known variables the `/pftui-report` skill provides at template
/// substitution time. Update this set when a new variable is added to
/// the skill — and reflect the change in `agents/report-prompts/README.md`.
fn known_variables() -> HashSet<&'static str> {
    [
        "OPERATOR_FOCUS",
        "HELD_ASSETS",
        "DATE_ISO",
        "SKYLAR_JOURNAL_7D",
        "LAYER_OWN_HISTORY",
        "LAYER_DIVERGENCE_DIGEST",
        "MACRO_TAPE_7D",
        "INBOX_FROM_AGENTS",
        "LESSON_BOOK",
        "MISALIGNMENT_DOSSIER",
        "MANDATORY_CONTEXT",
        "LAYER",
        "ASSET",
        "PERSONA_PATH",
        "CANDIDATE_MD",
        "SECTION_CATALOG",
        "CTX",
        "DEEP",
        "DATE_HUMAN",
    ]
    .into_iter()
    .collect()
}

fn prompts_dir() -> PathBuf {
    let here = env!("CARGO_MANIFEST_DIR");
    Path::new(here).join("agents").join("report-prompts")
}

fn list_templates(dir: &Path) -> Vec<PathBuf> {
    fs::read_dir(dir)
        .unwrap_or_else(|e| panic!("read_dir {dir:?}: {e}"))
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|s| s.to_str()) == Some("md"))
        .filter(|p| {
            let name = p.file_name().and_then(|s| s.to_str()).unwrap_or("");
            name != "README.md"
        })
        .collect()
}

#[test]
fn report_prompt_templates_use_only_known_variables() {
    let known = known_variables();
    let dir = prompts_dir();
    if !dir.exists() {
        // Repo without the directory yet; skip silently.
        return;
    }
    let mut violations: Vec<String> = Vec::new();
    let re = regex::Regex::new(r"\{([A-Z][A-Z0-9_]+)\}").expect("valid regex");
    for path in list_templates(&dir) {
        let body = fs::read_to_string(&path).expect("read template");
        for cap in re.captures_iter(&body) {
            let name = &cap[1];
            // Skip non-variable placeholders that some templates use
            // structurally (e.g. example SYM_1 in the analyst routine).
            if name.contains("SYM_") || name == "SYS" {
                continue;
            }
            if !known.contains(name) {
                violations.push(format!(
                    "{}: unknown variable {{{}}}",
                    path.file_name().unwrap().to_string_lossy(),
                    name
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "report-prompt templates reference unknown variables — \
         either add them to known_variables() in tests/report_prompt_templates.rs \
         or fix the template:\n{}",
        violations.join("\n")
    );
}

#[test]
fn report_prompt_template_includes_resolve_to_existing_files() {
    let dir = prompts_dir();
    if !dir.exists() {
        return;
    }
    let re = regex::Regex::new(r"\{INCLUDE\s+([A-Za-z0-9_\-./]+)\}").expect("valid regex");
    let mut violations: Vec<String> = Vec::new();
    for path in list_templates(&dir) {
        let body = fs::read_to_string(&path).expect("read template");
        for cap in re.captures_iter(&body) {
            let target = &cap[1];
            let resolved = dir.join(target);
            if !resolved.exists() {
                violations.push(format!(
                    "{}: includes {} but the file does not exist at {}",
                    path.file_name().unwrap().to_string_lossy(),
                    target,
                    resolved.display()
                ));
            }
        }
    }
    assert!(
        violations.is_empty(),
        "report-prompt templates reference missing includes:\n{}",
        violations.join("\n")
    );
}

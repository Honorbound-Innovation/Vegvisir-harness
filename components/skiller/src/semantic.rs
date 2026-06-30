use regex::Regex;

/// Deterministic semantic-plausibility checks for Skiller's non-model path.
///
/// These checks intentionally catch only obvious false positives: markdown/list/table
/// fragments, programming syntax, prose/process instructions, and command strings
/// without a plausible executable token. They are not a substitute for provider or
/// human semantic review, but they keep deterministic extraction from promoting
/// nonsense into first-class operational skills.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlausibilityIssue {
    pub code: &'static str,
    pub message: String,
}

impl PlausibilityIssue {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CliCommandPlausibility {
    pub plausible: bool,
    pub issues: Vec<PlausibilityIssue>,
}

impl CliCommandPlausibility {
    pub fn issue_summary(&self) -> String {
        self.issues
            .iter()
            .map(|issue| format!("{}: {}", issue.code, issue.message))
            .collect::<Vec<_>>()
            .join("; ")
    }
}

pub fn assess_cli_command(command: &str) -> CliCommandPlausibility {
    let mut issues = Vec::new();
    let trimmed = command.trim();
    if trimmed.is_empty() {
        issues.push(PlausibilityIssue::new(
            "empty_command",
            "CLI target command is empty",
        ));
        return CliCommandPlausibility {
            plausible: false,
            issues,
        };
    }

    if looks_like_markdown_fragment(trimmed) {
        issues.push(PlausibilityIssue::new(
            "markdown_fragment",
            "looks like a markdown list, quote, heading, checkbox, or table row",
        ));
    }
    if starts_with_numbered_list_marker(trimmed) {
        issues.push(PlausibilityIssue::new(
            "numbered_process_step",
            "starts with a numbered-list marker rather than an executable token",
        ));
    }
    if looks_like_programming_syntax(trimmed) {
        issues.push(PlausibilityIssue::new(
            "programming_syntax",
            "looks like programming-language syntax rather than a shell command",
        ));
    }
    if looks_like_process_instruction(trimmed) {
        issues.push(PlausibilityIssue::new(
            "process_instruction",
            "looks like a process/procedure instruction rather than a CLI command",
        ));
    }

    match cli_tool_name_if_plausible(trimmed) {
        Some(tool) => {
            if !is_plausible_executable_token(&tool) {
                issues.push(PlausibilityIssue::new(
                    "implausible_executable",
                    format!("tool token `{tool}` is not a plausible executable name"),
                ));
            }
        }
        None => issues.push(PlausibilityIssue::new(
            "missing_executable",
            "does not begin with a plausible executable/tool token",
        )),
    }

    if looks_like_prose_sentence(trimmed) {
        issues.push(PlausibilityIssue::new(
            "prose_sentence",
            "looks like prose rather than a command invocation",
        ));
    }

    CliCommandPlausibility {
        plausible: issues.is_empty(),
        issues,
    }
}

pub fn is_plausible_cli_command(command: &str) -> bool {
    assess_cli_command(command).plausible
}

pub fn suspicious_cli_command_reason(command: &str) -> Option<String> {
    let report = assess_cli_command(command);
    (!report.plausible).then(|| report.issue_summary())
}

pub fn cli_tool_name_if_plausible(command: &str) -> Option<String> {
    let candidate = command
        .trim()
        .strip_prefix('$')
        .map(str::trim)
        .unwrap_or(command.trim());
    let first = candidate.split_whitespace().next()?.trim();
    let tool = first
        .trim_matches('`')
        .trim_start_matches("sudo ")
        .trim_start_matches("env ")
        .trim_start_matches("command ")
        .trim_start_matches("./")
        .to_string();
    if tool.is_empty() {
        return None;
    }
    Some(tool)
}

pub fn looks_like_weak_title(title: &str) -> bool {
    let trimmed = title.trim();
    if trimmed.is_empty() {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    starts_with_numbered_list_marker(trimmed)
        || trimmed.starts_with('|')
        || trimmed.contains(" | ")
        || lower.starts_with("apply - ")
        || lower.starts_with("run `-")
        || lower.starts_with("apply |")
        || lower.starts_with("run `|")
}

fn looks_like_markdown_fragment(command: &str) -> bool {
    let trimmed = command.trim();
    trimmed.starts_with("- ")
        || trimmed.starts_with("* ")
        || trimmed.starts_with("+ ")
        || trimmed.starts_with("> ")
        || trimmed.starts_with("#")
        || trimmed.starts_with("| ")
        || trimmed.starts_with('|')
        || trimmed.ends_with('|')
        || trimmed.starts_with("[ ]")
        || trimmed.starts_with("[x]")
        || trimmed.starts_with("- [")
        || trimmed.contains(" | ")
}

fn starts_with_numbered_list_marker(text: &str) -> bool {
    Regex::new(r"^\s*\d{1,3}[\.)]\s+").unwrap().is_match(text)
}

fn looks_like_programming_syntax(command: &str) -> bool {
    let trimmed = command.trim();
    let lower = trimmed.to_ascii_lowercase();
    if matches!(
        lower.as_str(),
        "try {" | "try{" | "catch {" | "finally {" | "else {" | "do {"
    ) {
        return true;
    }
    if Regex::new(r"^(try|catch|finally|if|for|while|switch|match|else)\b.*\{\s*$")
        .unwrap()
        .is_match(trimmed)
    {
        return true;
    }
    if Regex::new(r"^(return|throw|break|continue)\b")
        .unwrap()
        .is_match(trimmed)
    {
        return true;
    }
    if Regex::new(r"^(let|const|var|fn|def|class|struct|enum|impl|use|import|from|public|private|protected|static)\b")
        .unwrap()
        .is_match(trimmed)
    {
        return true;
    }
    trimmed == "}" || trimmed == ");" || trimmed.ends_with(";") && !trimmed.contains(" && ")
}

fn looks_like_process_instruction(command: &str) -> bool {
    let trimmed = command.trim();
    let without_marker = Regex::new(r"^\s*\d{1,3}[\.)]\s+")
        .unwrap()
        .replace(trimmed, "");
    let first = without_marker.split_whitespace().next().unwrap_or("");
    let upper = first.to_ascii_uppercase();
    let process_verbs = [
        "STOP", "DIAGNOSE", "FIX", "GUARD", "VERIFY", "REVIEW", "DOCUMENT", "ASK", "CHECK",
        "CONFIRM", "ENSURE", "AVOID", "PREFER", "NEVER", "ALWAYS", "DO", "DON'T", "DONT",
    ];
    process_verbs.contains(&upper.as_str())
        && (first == upper || starts_with_numbered_list_marker(trimmed))
}

fn looks_like_prose_sentence(command: &str) -> bool {
    let trimmed = command.trim();
    if trimmed.ends_with('.') || trimmed.ends_with('!') || trimmed.ends_with('?') {
        return true;
    }
    let lower = trimmed.to_ascii_lowercase();
    let prose_fragments = [
        " do not ",
        " must ",
        " should ",
        " before ",
        " after ",
        " because ",
        " without ",
        " with evidence",
        " user approval",
        " rollback plan",
        " root cause",
        " recurrence",
    ];
    prose_fragments
        .iter()
        .any(|fragment| lower.contains(fragment))
        && !trimmed.contains(" --")
        && !trimmed.contains(" -")
}

fn is_plausible_executable_token(tool: &str) -> bool {
    let tool = tool.trim();
    if tool.starts_with('/') || tool.starts_with("./") || tool.starts_with("../") {
        return true;
    }
    if !Regex::new(r"^[a-z][a-z0-9_.+-]*$").unwrap().is_match(tool) {
        return false;
    }
    let rejected = [
        "a",
        "after",
        "always",
        "an",
        "and",
        "before",
        "catch",
        "caution",
        "do",
        "does",
        "don",
        "dont",
        "else",
        "finally",
        "fix",
        "for",
        "from",
        "guard",
        "if",
        "in",
        "let",
        "must",
        "never",
        "note",
        "output",
        "publishing",
        "required",
        "return",
        "should",
        "stop",
        "the",
        "this",
        "to",
        "try",
        "use",
        "using",
        "var",
        "warning",
        "when",
        "while",
        "with",
    ];
    !rejected.contains(&tool)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catches_obvious_fake_cli_operations() {
        for command in [
            "try {",
            "1. STOP adding features or making changes",
            "3. DIAGNOSE using the triage checklist",
            "5. GUARD against recurrence",
            "| fake | table |",
            "- It contains sensitive data",
        ] {
            assert!(
                !is_plausible_cli_command(command),
                "{command:?} should be implausible"
            );
        }
    }

    #[test]
    fn accepts_common_cli_invocations() {
        for command in [
            "cargo test -p skiller",
            "kubectl get pods -n default",
            "deployctl plan --dry-run",
            "python3 script.py --input data.json",
            "./tool status --json",
        ] {
            assert!(
                is_plausible_cli_command(command),
                "{command:?} should be plausible"
            );
        }
    }
}

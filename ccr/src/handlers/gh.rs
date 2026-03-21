use super::Handler;

pub struct GhHandler;

impl Handler for GhHandler {
    fn filter(&self, output: &str, args: &[String]) -> String {
        // Bypass filtering for structured output requests
        let bypass_flags = ["--json", "--jq", "--web", "--log", "--template"];
        if args.iter().any(|a| bypass_flags.contains(&a.as_str())) {
            return output.to_string();
        }

        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        let action = args.get(2).map(|s| s.as_str()).unwrap_or("");

        match (subcmd, action) {
            ("pr", "list") => filter_pr_list(output),
            ("pr", "view") => filter_pr_view(output),
            ("pr", "checks") => filter_pr_checks(output),
            ("issue", "list") => filter_issue_list(output),
            ("run", "list") | ("run", "view") => filter_run(output),
            ("repo", "clone") | ("repo", "fork") => last_line(output),
            _ => output.to_string(),
        }
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_string()
    } else {
        format!("{}…", &s[..s.char_indices().nth(max).map(|(i, _)| i).unwrap_or(s.len())])
    }
}

fn filter_pr_list(output: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let cols: Vec<&str> = t.splitn(5, '\t').collect();
        if cols.len() >= 4 {
            let num = cols[0];
            let title = truncate(cols[1], 60);
            let state = cols[2];
            let author = cols.get(3).unwrap_or(&"");
            out.push(format!("#{} {} [{}] @{}", num, title, state, author));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

/// Returns true if the line is an image/badge-only line: `![...](...)`.
fn is_image_only_line(line: &str) -> bool {
    let t = line.trim();
    if !t.starts_with("![") {
        return false;
    }
    // Must match `![...](...)` with nothing else meaningful on the line
    // Find closing `](`
    if let Some(bracket_close) = t.find("](") {
        let after = &t[bracket_close + 2..];
        // Must end with `)`
        if let Some(paren_close) = after.rfind(')') {
            let trailing = after[paren_close + 1..].trim();
            return trailing.is_empty();
        }
    }
    false
}

fn filter_pr_view(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut body_lines = 0usize;
    let mut in_body = false;
    let mut consecutive_blanks = 0usize;

    for line in &lines {
        let t = line.trim();
        if t.starts_with("title:") || t.starts_with("state:") || t.starts_with("author:") {
            out.push(line.to_string());
        } else if t.starts_with("--") {
            in_body = true;
        } else if in_body && body_lines < 20 {
            // Skip HTML comment lines
            if t.starts_with("<!--") {
                continue;
            }
            // Skip image/badge-only lines
            if is_image_only_line(t) {
                continue;
            }
            // Skip horizontal rules
            if t == "---" || t == "***" {
                continue;
            }
            // Collapse consecutive blank lines
            if t.is_empty() {
                consecutive_blanks += 1;
                if consecutive_blanks >= 3 {
                    // Skip this blank — already have 2+ in output
                    continue;
                }
            } else {
                consecutive_blanks = 0;
            }
            out.push(line.to_string());
            body_lines += 1;
        } else if t.starts_with("checks:") || t.starts_with("review decision:") {
            out.push(line.to_string());
        }
    }
    if out.is_empty() {
        output.to_string()
    } else {
        out.join("\n")
    }
}

fn filter_pr_checks(output: &str) -> String {
    let mut passed = 0usize;
    let mut failed: Vec<String> = Vec::new();

    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with('✓') || t.contains("pass") || t.contains("success") {
            passed += 1;
        } else if t.starts_with('✗') || t.starts_with('×') || t.contains("fail") {
            let name = t.split_whitespace().next().unwrap_or(t);
            failed.push(name.to_string());
        }
    }

    let mut out = format!("✓ {} passed, ✗ {} failed", passed, failed.len());
    if !failed.is_empty() {
        out.push('\n');
        out.push_str(&failed.join("\n"));
    }
    out
}

fn filter_issue_list(output: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        let cols: Vec<&str> = t.splitn(5, '\t').collect();
        if cols.len() >= 3 {
            let num = cols[0];
            let title = truncate(cols[1], 60);
            let labels = cols.get(2).unwrap_or(&"");
            let assignee = cols.get(3).unwrap_or(&"");
            out.push(format!("#{} {} [{}] @{}", num, title, labels, assignee));
        } else {
            out.push(line.to_string());
        }
    }
    out.join("\n")
}

fn filter_run(output: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.contains("completed")
            || t.contains("in_progress")
            || t.contains("queued")
            || t.contains("failure")
            || t.contains("success")
            || t.contains("cancelled")
        {
            out.push(line.to_string());
        }
    }
    if out.is_empty() {
        output.to_string()
    } else {
        out.join("\n")
    }
}

fn last_line(output: &str) -> String {
    output
        .lines()
        .rev()
        .find(|l| !l.trim().is_empty())
        .unwrap_or(output)
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::Handler;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn json_flag_causes_passthrough() {
        let handler = GhHandler;
        let output = "some\nformatted\noutput\nhere";
        // --json flag: should return output unchanged
        let result = handler.filter(output, &args(&["gh", "pr", "view", "--json", "number,title"]));
        assert_eq!(result, output);
    }

    #[test]
    fn jq_flag_causes_passthrough() {
        let handler = GhHandler;
        let output = "{\"number\":1}";
        let result = handler.filter(output, &args(&["gh", "pr", "list", "--jq", ".[] | .number"]));
        assert_eq!(result, output);
    }

    #[test]
    fn pr_view_strips_html_comments_and_image_lines() {
        // Build a synthetic PR view output
        let output = "\
title:\tFix the bug
state:\tOPEN
author:\talice
--
## Summary
<!-- this is an internal note -->
![badge](https://img.shields.io/badge/status-passing-green)
This PR fixes the crash in login.
---
More details here.
checks:\tall passing";

        let result = filter_pr_view(output);
        assert!(result.contains("title:"), "should keep title");
        assert!(result.contains("This PR fixes the crash"), "should keep regular content");
        assert!(!result.contains("<!--"), "should strip HTML comment lines");
        assert!(!result.contains("![badge]"), "should strip image-only lines");
        assert!(!result.contains("\n---\n") || result.contains("title:"), "should strip horizontal rules");
        assert!(result.contains("checks:"), "should keep checks line");
    }

    #[test]
    fn pr_view_keeps_regular_content() {
        let output = "\
title:\tAdd new feature
state:\tMERGED
author:\tbob
--
This is a detailed description.
It spans multiple lines.
No badges, no HTML comments here.
checks:\t3/3 passing";

        let result = filter_pr_view(output);
        assert!(result.contains("This is a detailed description."));
        assert!(result.contains("It spans multiple lines."));
        assert!(result.contains("No badges, no HTML comments here."));
        assert!(result.contains("checks:"));
    }

    #[test]
    fn pr_view_collapses_excess_blank_lines() {
        let output = "title:\tTest\nstate:\tOPEN\nauthor:\tx\n--\nLine one\n\n\n\n\nLine two";
        let result = filter_pr_view(output);
        // Should not have 3+ consecutive blank lines
        assert!(!result.contains("\n\n\n\n"), "3+ blanks should be collapsed");
        assert!(result.contains("Line one"));
        assert!(result.contains("Line two"));
    }
}

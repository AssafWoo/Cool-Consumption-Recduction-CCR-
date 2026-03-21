use super::util;
use super::Handler;

pub struct PipHandler;

impl Handler for PipHandler {
    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        // uv doesn't support -q the same way; leave uv args unchanged
        let is_uv = args.get(0).map(|s| s.as_str()).unwrap_or("") == "uv";
        if !is_uv && (subcmd == "install" || subcmd == "add") {
            if args.iter().any(|a| a == "-q" || a == "--quiet") {
                args.to_vec()
            } else {
                let mut out = args.to_vec();
                out.push("-q".to_string());
                out
            }
        } else {
            args.to_vec()
        }
    }

    fn filter(&self, output: &str, args: &[String]) -> String {
        let cmd = args.get(0).map(|s| s.as_str()).unwrap_or("");
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");

        match subcmd {
            "freeze" | "list" => return output.to_string(),
            "install" | "add" => {
                if cmd == "uv" {
                    return filter_uv_install(output);
                }
                return filter_pip_install(output);
            }
            _ => {}
        }

        // Default: keep only final non-empty line
        output
            .lines()
            .rev()
            .find(|l| !l.trim().is_empty())
            .unwrap_or(output)
            .to_string()
    }
}

fn filter_pip_install(output: &str) -> String {
    const PIP_SATISFIED_RULES: &[util::MatchOutputRule] = &[util::MatchOutputRule {
        success_pattern: r"(?i)Requirement already satisfied|already up-to-date|already installed",
        error_pattern: r"(?i)error|ERROR|Failed|failed",
        ok_message: "ok (already satisfied)",
    }];
    if let Some(msg) = util::check_match_output(output, PIP_SATISFIED_RULES) {
        return msg;
    }

    let mut warnings: Vec<String> = Vec::new();
    let mut installed = 0usize;

    for line in output.lines() {
        let t = line.trim();
        if t.starts_with("Successfully installed") {
            installed += t
                .trim_start_matches("Successfully installed")
                .split_whitespace()
                .count();
        } else if t.to_uppercase().starts_with("WARNING") || t.to_uppercase().starts_with("ERROR") {
            warnings.push(line.to_string());
        }
    }

    let mut out: Vec<String> = warnings;
    if installed > 0 {
        out.push(format!("[pip install complete — {} packages]", installed));
    } else {
        let summary: Vec<&str> = output
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.contains("already satisfied")
                    || t.contains("Requirement already")
                    || t.to_uppercase().starts_with("ERROR")
            })
            .take(5)
            .collect();
        if !summary.is_empty() {
            out.extend(summary.iter().map(|l| l.to_string()));
        } else {
            return output.to_string();
        }
    }
    out.join("\n")
}

fn filter_uv_install(output: &str) -> String {
    const UV_SATISFIED_RULES: &[util::MatchOutputRule] = &[util::MatchOutputRule {
        success_pattern: r"(?i)Audited \d+ packages in",
        error_pattern: r"(?i)error|failed",
        ok_message: "ok (up to date)",
    }];
    if let Some(msg) = util::check_match_output(output, UV_SATISFIED_RULES) {
        return msg;
    }

    // uv outputs: "Resolved N packages", "Prepared N packages", "Installed N packages", "Audited N packages"
    let mut warnings: Vec<String> = Vec::new();
    let mut installed = 0usize;
    let mut resolved = 0usize;

    for line in output.lines() {
        let t = line.trim();
        if t.starts_with("Installed ") && t.contains("package") {
            if let Some(n) = t
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<usize>().ok())
            {
                installed += n;
            }
        } else if t.starts_with("Resolved ") && t.contains("package") {
            if let Some(n) = t
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<usize>().ok())
            {
                resolved += n;
            }
        } else if t.starts_with("error") || t.starts_with("warning") || t.starts_with("  x ") {
            warnings.push(line.to_string());
        }
        // Drop: progress bars, "Downloading", "Building", "Audited"
    }

    let mut out: Vec<String> = warnings;
    if installed > 0 {
        out.push(format!(
            "[uv install complete — {} packages installed, {} resolved]",
            installed, resolved
        ));
    } else if resolved > 0 {
        out.push(format!("[uv: {} packages already satisfied]", resolved));
    } else {
        return output.to_string();
    }
    out.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> PipHandler {
        PipHandler
    }

    #[test]
    fn pip_already_satisfied_short_circuits() {
        let output =
            "Requirement already satisfied: requests in /usr/lib/python3/dist-packages (2.28.0)";
        let result = handler().filter(
            output,
            &[
                "pip".to_string(),
                "install".to_string(),
                "requests".to_string(),
            ],
        );
        assert_eq!(result, "ok (already satisfied)");
    }

    #[test]
    fn uv_audited_short_circuits() {
        let output = "Resolved 42 packages in 0.05s\nAudited 42 packages in 0.1s";
        let result = handler().filter(output, &["uv".to_string(), "install".to_string()]);
        assert_eq!(result, "ok (up to date)");
    }

    #[test]
    fn pip_actual_install_not_short_circuited() {
        let output = "Collecting requests\n  Downloading requests-2.31.0-py3-none-any.whl (62 kB)\nSuccessfully installed requests-2.31.0";
        let result = handler().filter(
            output,
            &[
                "pip".to_string(),
                "install".to_string(),
                "requests".to_string(),
            ],
        );
        assert_ne!(result, "ok (already satisfied)");
        assert!(result.contains("pip install complete") || result.contains("requests"));
    }
}

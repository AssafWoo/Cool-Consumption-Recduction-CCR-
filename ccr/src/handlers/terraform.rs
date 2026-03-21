use super::util;
use super::Handler;

pub struct TerraformHandler;

impl Handler for TerraformHandler {
    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        match subcmd {
            "plan" | "apply" | "destroy" => {
                if args.iter().any(|a| a == "-no-color" || a == "--no-color") {
                    args.to_vec()
                } else {
                    let mut out = args.to_vec();
                    out.push("-no-color".to_string());
                    out
                }
            }
            _ => args.to_vec(),
        }
    }

    fn filter(&self, output: &str, args: &[String]) -> String {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        match subcmd {
            "plan" => filter_plan(output),
            "apply" => filter_apply(output),
            "init" => filter_init(output),
            "validate" => filter_validate(output),
            _ => output.to_string(),
        }
    }
}

fn filter_plan(output: &str) -> String {
    const PLAN_NO_CHANGE_RULES: &[util::MatchOutputRule] = &[util::MatchOutputRule {
        success_pattern: r"No changes\. Your infrastructure matches the configuration",
        error_pattern: r"Error:",
        ok_message: "no changes detected",
    }];
    if let Some(msg) = util::check_match_output(output, PLAN_NO_CHANGE_RULES) {
        return msg;
    }

    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.starts_with('+')
            || t.starts_with('-')
            || t.starts_with('~')
            || t.starts_with("Plan:")
            || t.contains("No changes")
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

fn filter_apply(output: &str) -> String {
    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.contains(": Creating...")
            || t.contains(": Creation complete")
            || t.contains(": Destroying...")
            || t.contains(": Destruction complete")
            || t.contains(": Modifying...")
            || t.contains(": Modifications complete")
            || t.starts_with("Apply complete!")
            || t.contains("Error:")
            || t.starts_with("Error ")
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

fn filter_init(output: &str) -> String {
    let has_error = output.lines().any(|l| {
        let t = l.trim();
        t.starts_with("Error") || t.starts_with("error")
    });
    if has_error {
        let errors: Vec<&str> = output
            .lines()
            .filter(|l| {
                let t = l.trim();
                t.starts_with("Error") || t.starts_with("error") || t.contains("Error:")
            })
            .collect();
        return errors.join("\n");
    }
    "[terraform init complete]".to_string()
}

fn filter_validate(output: &str) -> String {
    const VALIDATE_OK_RULES: &[util::MatchOutputRule] = &[util::MatchOutputRule {
        success_pattern: r"(?i)The configuration is valid|Success!",
        error_pattern: r"(?i)error|Error",
        ok_message: "terraform validate: ok",
    }];
    if let Some(msg) = util::check_match_output(output, VALIDATE_OK_RULES) {
        return msg;
    }

    let mut out: Vec<String> = Vec::new();
    for line in output.lines() {
        let t = line.trim();
        if t.contains("Success")
            || t.contains("error")
            || t.contains("Error")
            || t.contains("warning")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> TerraformHandler {
        TerraformHandler
    }

    #[test]
    fn plan_no_changes_short_circuits() {
        let output = "Refreshing state...\nNo changes. Your infrastructure matches the configuration.\nTerraform has compared your real infrastructure against your configuration\nand found no differences, so no changes are needed.";
        let result = handler().filter(output, &["terraform".to_string(), "plan".to_string()]);
        assert_eq!(result, "no changes detected");
    }

    #[test]
    fn plan_with_error_not_short_circuited() {
        let output = "No changes. Your infrastructure matches the configuration.\nError: Invalid resource configuration";
        let result = handler().filter(output, &["terraform".to_string(), "plan".to_string()]);
        assert_ne!(result, "no changes detected");
    }

    #[test]
    fn validate_ok_short_circuits() {
        let output = "Success! The configuration is valid.\n";
        let result = handler().filter(output, &["terraform".to_string(), "validate".to_string()]);
        assert_eq!(result, "terraform validate: ok");
    }
}

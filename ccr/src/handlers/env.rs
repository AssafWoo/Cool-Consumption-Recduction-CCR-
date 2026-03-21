use super::Handler;

pub struct EnvHandler;

const SENSITIVE_PATTERNS: &[&str] = &[
    "KEY", "SECRET", "TOKEN", "PASSWORD", "PASS", "CREDENTIAL", "AUTH",
];

/// Path-related variable names (exact match).
const PATH_VARS: &[&str] = &[
    "PATH", "MANPATH", "PYTHONPATH", "GOPATH", "NODE_PATH", "LD_LIBRARY_PATH",
];

/// Prefixes that put a var into the Language/Runtime group.
const LANG_PREFIXES: &[&str] = &[
    "PYTHON", "GO", "RUBY", "JAVA", "NODE", "DENO", "BUN", "RUST", "CARGO",
];

/// Prefixes that put a var into the Tools group.
const TOOLS_PREFIXES: &[&str] = &[
    "EDITOR", "SHELL", "TERM", "GIT_", "DOCKER_", "KUBECONFIG", "HELM_",
];

/// Prefixes that put a var into the Cloud/Services group.
const CLOUD_PREFIXES: &[&str] = &[
    "AWS_", "GCP_", "GOOGLE_", "AZURE_", "DATABASE_", "REDIS_", "MONGO_",
];

/// Substrings in the key name that put a var into the Cloud/Services group.
const CLOUD_KEY_CONTAINS: &[&str] = &["URL", "ENDPOINT", "HOST", "PORT"];

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord, Clone, Copy)]
enum Category {
    Path,
    LangRuntime,
    CloudServices,
    Tools,
    Other,
}

fn is_sensitive(key: &str) -> bool {
    let k = key.to_uppercase();
    SENSITIVE_PATTERNS.iter().any(|pat| k.contains(pat))
}

fn categorize(key: &str) -> Category {
    let k = key.to_uppercase();

    // PATH group: exact match
    if PATH_VARS.iter().any(|&p| k == p) {
        return Category::Path;
    }

    // Language/Runtime: starts with
    if LANG_PREFIXES.iter().any(|&p| k.starts_with(p)) {
        return Category::LangRuntime;
    }

    // Cloud/Services: starts with or contains keyword
    if CLOUD_PREFIXES.iter().any(|&p| k.starts_with(p))
        || CLOUD_KEY_CONTAINS.iter().any(|&s| k.contains(s))
    {
        return Category::CloudServices;
    }

    // Tools: starts with
    if TOOLS_PREFIXES.iter().any(|&p| k.starts_with(p)) {
        return Category::Tools;
    }

    Category::Other
}

impl Handler for EnvHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        let vars: Vec<(String, String, Category)> = output
            .lines()
            .filter_map(|line| {
                let eq = line.find('=')?;
                let key = line[..eq].to_string();
                let val = line[eq + 1..].to_string();
                Some((key, val))
            })
            .map(|(k, v)| {
                let v_out = if is_sensitive(&k) {
                    "[redacted]".to_string()
                } else {
                    v
                };
                let cat = categorize(&k);
                (k, v_out, cat)
            })
            .collect();

        let mut path_vars: Vec<(&str, &str)> = Vec::new();
        let mut lang_vars: Vec<(&str, &str)> = Vec::new();
        let mut cloud_vars: Vec<(&str, &str)> = Vec::new();
        let mut tools_vars: Vec<(&str, &str)> = Vec::new();
        let mut other_vars: Vec<(&str, &str)> = Vec::new();

        for (k, v, cat) in &vars {
            match cat {
                Category::Path => path_vars.push((k, v)),
                Category::LangRuntime => lang_vars.push((k, v)),
                Category::CloudServices => cloud_vars.push((k, v)),
                Category::Tools => tools_vars.push((k, v)),
                Category::Other => other_vars.push((k, v)),
            }
        }

        let mut out: Vec<String> = Vec::new();

        fn emit_section(out: &mut Vec<String>, title: &str, entries: &[(&str, &str)]) {
            if entries.is_empty() {
                return;
            }
            out.push(format!("[{}]", title));
            for (k, v) in entries {
                out.push(format!("{}={}", k, v));
            }
            out.push(String::new());
        }

        emit_section(&mut out, "PATH", &path_vars);
        emit_section(&mut out, "Language/Runtime", &lang_vars);
        emit_section(&mut out, "Cloud/Services", &cloud_vars);
        emit_section(&mut out, "Tools", &tools_vars);

        // Other: cap at 10, show [+N more] if needed
        const MAX_OTHER: usize = 10;
        if !other_vars.is_empty() {
            let total_other = other_vars.len();
            let shown = &other_vars[..MAX_OTHER.min(total_other)];
            let header = if total_other > MAX_OTHER {
                format!("[Other — {} vars]", total_other)
            } else {
                "[Other]".to_string()
            };
            out.push(header);
            for (k, v) in shown {
                out.push(format!("{}={}", k, v));
            }
            if total_other > MAX_OTHER {
                out.push(format!("[+{} more]", total_other - MAX_OTHER));
            }
        }

        // Remove trailing blank line if present
        while out.last().map(|s| s.is_empty()).unwrap_or(false) {
            out.pop();
        }

        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::Handler;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    fn filter(input: &str) -> String {
        EnvHandler.filter(input, &args(&[]))
    }

    #[test]
    fn path_vars_categorized_correctly() {
        let input = "PATH=/usr/bin:/usr/local/bin\nGOPATH=/home/user/go\nEDITOR=vim";
        let result = filter(input);
        // PATH section should appear before Tools section
        let path_pos = result.find("[PATH]").expect("should have PATH section");
        let tools_pos = result.find("[Tools]").expect("should have Tools section");
        assert!(path_pos < tools_pos, "PATH section should come before Tools section");
        assert!(result.contains("PATH=/usr/bin:/usr/local/bin"));
        assert!(result.contains("GOPATH=/home/user/go"));
    }

    #[test]
    fn aws_vars_go_to_cloud_services() {
        let input = "AWS_REGION=us-east-1\nAWS_ACCESS_KEY_ID=AKIAIOSFODNN7EXAMPLE\nLANG=en_US.UTF-8";
        let result = filter(input);
        assert!(result.contains("[Cloud/Services]"), "should have Cloud/Services section");
        // AWS_REGION should be in Cloud/Services
        let cloud_pos = result.find("[Cloud/Services]").unwrap();
        let aws_region_pos = result.find("AWS_REGION=us-east-1").unwrap();
        assert!(aws_region_pos > cloud_pos, "AWS_REGION should appear after [Cloud/Services] header");
        // AWS_ACCESS_KEY_ID is sensitive — value should be redacted
        assert!(result.contains("AWS_ACCESS_KEY_ID=[redacted]"), "sensitive key should be redacted");
    }

    #[test]
    fn sensitive_keys_show_redacted() {
        let input = "MY_SECRET=supersecret\nDB_PASSWORD=hunter2\nNORMAL_VAR=hello";
        let result = filter(input);
        assert!(result.contains("MY_SECRET=[redacted]"), "SECRET pattern should redact");
        assert!(result.contains("DB_PASSWORD=[redacted]"), "PASSWORD pattern should redact");
        assert!(result.contains("NORMAL_VAR=hello"), "non-sensitive var should show value");
        assert!(!result.contains("supersecret"), "secret value must not appear");
        assert!(!result.contains("hunter2"), "password value must not appear");
    }

    #[test]
    fn other_category_capped_at_10() {
        // Generate 15 vars that don't match any special category
        let input: String = (1..=15)
            .map(|i| format!("MYVAR{}=value{}", i, i))
            .collect::<Vec<_>>()
            .join("\n");
        let result = filter(&input);
        assert!(result.contains("[Other — 15 vars]"), "should show total count in header");
        assert!(result.contains("[+5 more]"), "should show overflow marker");
        // Count actual var lines shown (lines containing "=value")
        let shown_count = result.lines().filter(|l| l.contains("=value")).count();
        assert_eq!(shown_count, 10, "should show exactly 10 other vars");
    }
}

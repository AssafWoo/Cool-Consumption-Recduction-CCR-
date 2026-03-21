use super::Handler;

pub struct TreeHandler;

impl Handler for TreeHandler {
    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        let has_ignore = args
            .iter()
            .any(|a| a == "-I" || a.starts_with("-I") || a == "--gitignore" || a == "-a");
        if has_ignore {
            return args.to_vec();
        }
        let mut out = args.to_vec();
        out.push("-I".to_string());
        out.push(
            "node_modules|.git|target|__pycache__|.next|dist|build|.cache|.venv|venv".to_string(),
        );
        out
    }

    fn filter(&self, output: &str, _args: &[String]) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.len() <= 30 {
            return output.to_string();
        }

        // Always keep the last summary line ("N directories, M files")
        let summary = lines
            .iter()
            .rev()
            .find(|l| l.contains("director") && l.contains("file"))
            .map(|l| l.to_string());

        let mut out: Vec<String> = lines.iter().take(25).map(|l| l.to_string()).collect();
        let remaining = lines.len() - 25;
        // Don't count summary line in remaining if present
        let extra = if summary.is_some() {
            remaining.saturating_sub(1)
        } else {
            remaining
        };
        out.push(format!("[... {} more entries]", extra));
        if let Some(s) = summary {
            out.push(s);
        }
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn handler() -> TreeHandler {
        TreeHandler
    }

    #[test]
    fn test_rewrite_args_injects_ignore_when_not_present() {
        let h = handler();
        let args: Vec<String> = vec!["tree".to_string(), ".".to_string()];
        let result = h.rewrite_args(&args);
        let i_pos = result.iter().position(|a| a == "-I");
        assert!(i_pos.is_some(), "expected -I to be injected, got: {:?}", result);
        let pattern = &result[i_pos.unwrap() + 1];
        assert!(pattern.contains("node_modules"), "got: {}", pattern);
        assert!(pattern.contains(".git"), "got: {}", pattern);
    }

    #[test]
    fn test_rewrite_args_does_not_inject_when_user_has_I() {
        let h = handler();
        let args: Vec<String> = vec![
            "tree".to_string(),
            "-I".to_string(),
            "vendor".to_string(),
        ];
        let result = h.rewrite_args(&args);
        // Should be identical to input
        assert_eq!(result, args);
    }

    #[test]
    fn test_rewrite_args_does_not_inject_when_user_has_gitignore() {
        let h = handler();
        let args: Vec<String> = vec!["tree".to_string(), "--gitignore".to_string()];
        let result = h.rewrite_args(&args);
        assert_eq!(result, args);
    }

    #[test]
    fn test_rewrite_args_does_not_inject_when_user_has_a_flag() {
        let h = handler();
        let args: Vec<String> = vec!["tree".to_string(), "-a".to_string()];
        let result = h.rewrite_args(&args);
        assert_eq!(result, args);
    }

    #[test]
    fn test_filter_short_output_unchanged() {
        let h = handler();
        let output = ".\n├── src\n│   └── main.rs\n└── Cargo.toml\n\n1 directory, 2 files\n";
        let result = h.filter(output, &[]);
        assert_eq!(result, output);
    }

    #[test]
    fn test_filter_long_output_truncated_with_summary() {
        let h = handler();
        // Build 40 lines + summary
        let mut lines: Vec<String> = (0..40).map(|i| format!("├── file{}.rs", i)).collect();
        lines.push("".to_string());
        lines.push("0 directories, 40 files".to_string());
        let output = lines.join("\n");
        let result = h.filter(&output, &[]);
        assert!(result.contains("[... "), "should have truncation marker, got: {}", result);
        assert!(result.contains("0 directories, 40 files"), "should keep summary, got: {}", result);
    }
}

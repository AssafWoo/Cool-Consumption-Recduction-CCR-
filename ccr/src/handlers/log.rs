use std::collections::HashMap;
use std::sync::OnceLock;

use super::Handler;

pub struct LogHandler;

// ── Regex statics ─────────────────────────────────────────────────────────────

fn re_timestamp() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(r"\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}").unwrap()
    })
}

fn re_uuid() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| {
        regex::Regex::new(
            r"[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}",
        )
        .unwrap()
    })
}

fn re_hex() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\b0x[0-9a-fA-F]{6,}\b").unwrap())
}

fn re_large_num() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"\b\d{6,}\b").unwrap())
}

fn re_abs_path() -> &'static regex::Regex {
    static RE: OnceLock<regex::Regex> = OnceLock::new();
    RE.get_or_init(|| regex::Regex::new(r"(?:/[\w.\-]+){3,}").unwrap())
}

// ── Normalization ─────────────────────────────────────────────────────────────

fn normalize_line(line: &str) -> String {
    let s = re_timestamp().replace_all(line, "<TS>");
    let s = re_uuid().replace_all(&s, "<UUID>");
    let s = re_hex().replace_all(&s, "<HEX>");
    let s = re_large_num().replace_all(&s, "<NUM>");
    let s = re_abs_path().replace_all(&s, "<PATH>");
    s.into_owned()
}

// ── Line classification helpers ───────────────────────────────────────────────

fn is_error_line(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("error") || l.contains("fatal") || l.contains("panic") || l.contains("exception")
}

fn is_warning_line(line: &str) -> bool {
    let l = line.to_lowercase();
    l.contains("warn")
}

// ── Handler ───────────────────────────────────────────────────────────────────

impl Handler for LogHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        // 1. Group lines: normalized_form → (count, first_original)
        let mut order: Vec<String> = Vec::new();
        let mut groups: HashMap<String, (usize, String)> = HashMap::new();

        for line in output.lines() {
            let norm = normalize_line(line);
            if let Some(entry) = groups.get_mut(&norm) {
                entry.0 += 1;
            } else {
                order.push(norm.clone());
                groups.insert(norm, (1, line.to_string()));
            }
        }

        // 2. Collect errors and warnings for the summary block
        let mut errors: Vec<(usize, String)> = Vec::new();
        let mut warnings: Vec<(usize, String)> = Vec::new();

        for norm in &order {
            let (count, _) = &groups[norm];
            if is_error_line(norm) {
                errors.push((*count, norm.clone()));
            } else if is_warning_line(norm) {
                warnings.push((*count, norm.clone()));
            }
        }

        // Sort by count descending
        errors.sort_by(|a, b| b.0.cmp(&a.0));
        warnings.sort_by(|a, b| b.0.cmp(&a.0));

        let has_summary = !errors.is_empty() || !warnings.is_empty();

        let mut out: Vec<String> = Vec::new();

        // 3. Prepend summary block if there are errors/warnings
        if has_summary {
            out.push("--- Log Summary ---".to_string());
            for (count, norm) in errors.iter().take(10) {
                out.push(format!("[ERRORx{}] {}", count, norm));
            }
            for (count, norm) in warnings.iter().take(5) {
                out.push(format!("[WARNx{}] {}", count, norm));
            }
            out.push(String::new()); // blank separator
        }

        // 4. Emit deduplicated lines (normalized form), cap at 200 total output lines
        for norm in &order {
            if out.len() >= 200 {
                break;
            }
            let (count, _original) = &groups[norm];
            if *count == 1 {
                out.push(norm.clone());
            } else {
                out.push(format!("{}  [×{}]", norm, count));
            }
        }

        out.join("\n")
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::Handler;

    fn h() -> LogHandler { LogHandler }
    fn args() -> Vec<String> { vec!["log".to_string()] }

    #[test]
    fn repeated_lines_collapse() {
        let input = "hello world\nhello world\nhello world\n";
        let result = h().filter(input, &args());
        assert!(result.contains("[×3]"), "expected [×3] in: {}", result);
        // Should only appear once as the deduplicated line
        assert_eq!(result.matches("hello world").count(), 1);
    }

    #[test]
    fn timestamps_normalized() {
        let input = "2024-01-15T10:30:00 starting up\n2024-01-15T10:30:01 ready\n";
        let result = h().filter(input, &args());
        assert!(!result.contains("2024-01-15"), "raw timestamp should be replaced");
        assert!(result.contains("<TS>"));
    }

    #[test]
    fn uuid_normalized() {
        let input = "request id=550e8400-e29b-41d4-a716-446655440000 received\n";
        let result = h().filter(input, &args());
        assert!(!result.contains("550e8400"), "UUID should be replaced");
        assert!(result.contains("<UUID>"));
    }

    #[test]
    fn error_summary_appears_when_errors_present() {
        let input = "INFO: started\nERROR: connection refused\nINFO: retrying\n";
        let result = h().filter(input, &args());
        assert!(result.contains("--- Log Summary ---"), "summary block missing");
        assert!(result.contains("[ERRORx"), "error entry missing in summary");
    }

    #[test]
    fn clean_log_has_no_summary_block() {
        let input = "INFO: service started\nINFO: listening on port 8080\nINFO: ready\n";
        let result = h().filter(input, &args());
        assert!(
            !result.contains("--- Log Summary ---"),
            "summary block should not appear for clean log"
        );
    }

    #[test]
    fn hex_normalized() {
        let input = "memory at 0xdeadbeef00 leaked\n";
        let result = h().filter(input, &args());
        assert!(result.contains("<HEX>"));
        assert!(!result.contains("0xdeadbeef00"));
    }

    #[test]
    fn large_numbers_normalized() {
        let input = "processed 1234567 records\n";
        let result = h().filter(input, &args());
        assert!(result.contains("<NUM>"));
    }
}

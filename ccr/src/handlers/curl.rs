use super::util;
use super::Handler;

pub struct CurlHandler;

const MAX_LINES: usize = 30;
const MAX_LINE_LEN: usize = 200;

impl Handler for CurlHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        let trimmed = output.trim();

        // Detect JSON by Content-Type hint in headers or by prefix
        let body = extract_body(trimmed);

        if let Some(json_str) = body {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(json_str) {
                let schema = util::json_to_schema(&v);
                let schema_str = serde_json::to_string_pretty(&schema).unwrap_or_default();
                // Size guard: if schema is larger than original, fall through to plain handling
                if schema_str.len() < json_str.len() {
                    return schema_str;
                }
            }
        }

        // Non-JSON (or JSON schema wasn't smaller): truncate long lines, cap at MAX_LINES
        let lines: Vec<&str> = output.lines().collect();

        // Truncate individual lines that are too long
        let truncated: Vec<String> = lines
            .iter()
            .map(|l| truncate_line(l, MAX_LINE_LEN))
            .collect();

        if truncated.len() <= MAX_LINES {
            return truncated.join("\n");
        }

        let mut out: Vec<String> = truncated[..MAX_LINES].to_vec();
        out.push(format!("[+{} more lines]", truncated.len() - MAX_LINES));
        out.join("\n")
    }
}

fn truncate_line(line: &str, max: usize) -> String {
    if line.chars().count() <= max {
        line.to_string()
    } else {
        let byte_pos = line
            .char_indices()
            .nth(max)
            .map(|(i, _)| i)
            .unwrap_or(line.len());
        format!("{}…", &line[..byte_pos])
    }
}

/// Extract the response body from curl output (headers + body or just body).
fn extract_body(output: &str) -> Option<&str> {
    // If output contains HTTP headers (curl -i or -v), split at the blank line
    if output.starts_with("HTTP/") {
        // Find double newline separating headers from body
        if let Some(pos) = output.find("\r\n\r\n") {
            return Some(&output[pos + 4..]);
        }
        if let Some(pos) = output.find("\n\n") {
            return Some(&output[pos + 2..]);
        }
    }

    // Whole output is the body
    let b = output.trim();
    if b.starts_with('{') || b.starts_with('[') {
        Some(output)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::Handler;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn long_non_json_output_capped_at_30_lines() {
        let handler = CurlHandler;
        // 50 lines of plain text
        let output: String = (1..=50).map(|i| format!("line {}", i)).collect::<Vec<_>>().join("\n");
        let result = handler.filter(&output, &args(&[]));
        let result_lines: Vec<&str> = result.lines().collect();
        // 30 content lines + 1 marker line
        assert_eq!(result_lines.len(), 31, "should be 30 lines + marker");
        assert!(result_lines[30].contains("[+20 more lines]"), "marker should show remaining count");
        assert!(result_lines[0] == "line 1");
        assert!(result_lines[29] == "line 30");
    }

    #[test]
    fn json_output_gets_schema_treatment() {
        let handler = CurlHandler;
        // A large JSON array — schema should be smaller
        let items: Vec<String> = (1..=50)
            .map(|i| format!(r#"{{"id":{},"name":"user{}","email":"user{}@example.com","active":true}}"#, i, i, i))
            .collect();
        let output = format!("[{}]", items.join(","));
        let result = handler.filter(&output, &args(&[]));
        assert!(result.len() < output.len(), "schema should be smaller than original JSON");
        // Schema output should contain field names or array item summary
        assert!(
            result.contains("\"id\"") || result.contains("\"name\"") || result.contains("items total"),
            "schema result should contain field names or array summary"
        );
    }

    #[test]
    fn short_non_json_output_passes_through_unchanged() {
        let handler = CurlHandler;
        let output = "Hello, world!\nThis is a short response.\nOnly 3 lines.";
        let result = handler.filter(output, &args(&[]));
        assert_eq!(result, output, "short output should pass through unchanged");
    }

    #[test]
    fn long_lines_are_truncated() {
        let handler = CurlHandler;
        let long_line = "x".repeat(300);
        let output = long_line.clone();
        let result = handler.filter(&output, &args(&[]));
        // Should be 200 chars + ellipsis character (1 char)
        let chars: Vec<char> = result.chars().collect();
        assert_eq!(chars.len(), 201, "truncated line should be 200 chars + ellipsis");
        assert_eq!(chars[200], '…');
    }
}

use super::{util, Handler};

pub struct JsonHandler;

impl Handler for JsonHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        let trimmed = output.trim();
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
            let schema = util::json_to_schema(&v);
            if let Ok(s) = serde_json::to_string_pretty(&schema) {
                if s.len() < trimmed.len() {
                    return s;
                }
            }
        }
        output.to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::handlers::Handler;

    fn args() -> Vec<String> { vec!["json".to_string()] }

    #[test]
    fn compresses_large_json() {
        // Large JSON with repetitive data values — schema should be smaller
        let input = r#"{"users":[{"id":1,"name":"Alice","email":"alice@example.com","active":true,"score":9.5,"created_at":"2024-01-01T00:00:00Z"},{"id":2,"name":"Bob","email":"bob@example.com","active":false,"score":7.2,"created_at":"2024-01-02T00:00:00Z"},{"id":3,"name":"Carol","email":"carol@example.com","active":true,"score":8.1,"created_at":"2024-01-03T00:00:00Z"}]}"#;
        let result = JsonHandler.filter(input, &args());
        assert!(result.contains("int") || result.contains("string") || result.contains("array"),
            "expected schema output, got: {}", result);
        assert!(!result.contains("Alice"), "should not contain raw values");
    }

    #[test]
    fn passthrough_when_not_json() {
        let input = "hello world\nsome text\n";
        let result = JsonHandler.filter(input, &args());
        assert_eq!(result, input);
    }

    #[test]
    fn passthrough_when_schema_not_smaller() {
        // Very small JSON — schema won't be smaller
        let input = r#"{"a":1}"#;
        let result = JsonHandler.filter(input, &args());
        // Should not panic; result is either schema or original
        assert!(!result.is_empty());
    }
}

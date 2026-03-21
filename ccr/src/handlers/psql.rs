use super::Handler;

pub struct PsqlHandler;

impl Handler for PsqlHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        let lines: Vec<&str> = output.lines().collect();
        if lines.is_empty() {
            return output.to_string();
        }

        // Keep psql ERROR lines always
        let has_error = lines
            .iter()
            .any(|l| l.trim().starts_with("ERROR:") || l.trim().starts_with("FATAL:"));
        if has_error {
            let errors: Vec<&str> = lines
                .iter()
                .filter(|l| {
                    let t = l.trim();
                    t.starts_with("ERROR:")
                        || t.starts_with("FATAL:")
                        || t.starts_with("DETAIL:")
                        || t.starts_with("HINT:")
                })
                .copied()
                .collect();
            return errors.join("\n");
        }

        // Strip +----+ / =====+===== border lines
        let data_lines: Vec<&str> = lines
            .iter()
            .filter(|l| {
                let t = l.trim();
                !t.is_empty() && !t.chars().all(|c| c == '+' || c == '-' || c == '=')
            })
            .copied()
            .collect();

        if data_lines.is_empty() {
            return output.to_string();
        }

        // Parse each line: strip outer `|` borders, split on ` | `, trim cells,
        // then rejoin with `  |  ` for a cleaner look.
        // Lines that look like "(N rows)" are kept verbatim.
        let parse_row = |line: &str| -> String {
            let t = line.trim();
            // Row-count lines like "(5 rows)" – keep as-is
            if t.starts_with('(') && t.contains("row") {
                return t.to_string();
            }
            // Strip leading/trailing `|`
            let s = if t.starts_with('|') { &t[1..] } else { t };
            let s = if s.ends_with('|') { &s[..s.len() - 1] } else { s };
            // Split on ` | ` (the standard psql column separator)
            let cells: Vec<&str> = s.split(" | ").map(|c| c.trim()).collect();
            cells.join("  |  ")
        };

        let cleaned: Vec<String> = data_lines.iter().map(|l| parse_row(l)).collect();

        // Separate the optional trailing "(N rows)" line so it is always
        // appended after any truncation marker.
        let (row_count_line, data_cleaned): (Option<String>, Vec<String>) = {
            if let Some(last) = cleaned.last() {
                if last.trim().starts_with('(') && last.contains("row") {
                    let rc = last.clone();
                    let data = cleaned[..cleaned.len() - 1].to_vec();
                    (Some(rc), data)
                } else {
                    (None, cleaned)
                }
            } else {
                (None, cleaned)
            }
        };

        let total = data_cleaned.len(); // includes header as first element
        const MAX_ROWS: usize = 20;

        let mut out: Vec<String> = Vec::new();

        if total <= MAX_ROWS + 1 {
            // +1 for header: fits within budget
            out.extend(data_cleaned);
        } else {
            out.push(data_cleaned[0].clone()); // header
            for row in data_cleaned.iter().skip(1).take(MAX_ROWS) {
                out.push(row.clone());
            }
            let remaining = total - 1 - MAX_ROWS;
            if remaining > 0 {
                out.push(format!("[+{} more rows]", remaining));
            }
        }

        if let Some(rc) = row_count_line {
            out.push(rc);
        }

        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler() -> PsqlHandler {
        PsqlHandler
    }

    // A standard psql table output
    const TABLE_OUTPUT: &str = "\
 id  | name    | age \n\
-----+---------+-----\n\
 1   | Alice   | 30  \n\
 2   | Bob     | 25  \n\
 3   | Charlie | 35  \n\
(3 rows)\n\
";

    #[test]
    fn table_strips_borders_and_formats_with_separators() {
        let h = make_handler();
        let args: Vec<String> = vec![];
        let result = h.filter(TABLE_OUTPUT, &args);
        let result_lines: Vec<&str> = result.lines().collect();

        // Header line should contain column names separated by "  |  "
        let header = result_lines[0];
        assert!(header.contains("id"), "header should contain id");
        assert!(header.contains("name"), "header should contain name");
        assert!(header.contains("age"), "header should contain age");
        // Separator style should be "  |  " not the raw psql ` | ` inside a border row
        assert!(header.contains("  |  "), "header should use clean separator");

        // No raw border lines (all dashes/plus)
        for line in &result_lines {
            assert!(
                !line.chars().all(|c| c == '+' || c == '-' || c == '='),
                "border line leaked into output: {line}"
            );
        }

        // Data rows present
        assert!(result.contains("Alice"));
        assert!(result.contains("Bob"));
        assert!(result.contains("Charlie"));
    }

    #[test]
    fn row_count_line_is_preserved() {
        let h = make_handler();
        let args: Vec<String> = vec![];
        let result = h.filter(TABLE_OUTPUT, &args);
        assert!(
            result.contains("(3 rows)"),
            "row count line should be preserved"
        );
        // It should be the last line
        let last = result.lines().last().unwrap();
        assert_eq!(last.trim(), "(3 rows)");
    }

    #[test]
    fn error_output_keeps_error_detail_hint_lines() {
        let input = "\
ERROR: column \"foo\" does not exist\n\
LINE 1: SELECT foo FROM bar;\n\
               ^\n\
DETAIL: There is no column named foo.\n\
HINT: Did you mean to use bar.id?\n\
";
        let h = make_handler();
        let args: Vec<String> = vec![];
        let result = h.filter(input, &args);

        assert!(result.contains("ERROR:"));
        assert!(result.contains("DETAIL:"));
        assert!(result.contains("HINT:"));
        // The raw SQL context lines should be dropped
        assert!(!result.contains("LINE 1:"), "LINE context should be dropped");
        assert!(
            !result.contains("SELECT foo"),
            "raw SQL line should be dropped"
        );
    }

    #[test]
    fn truncation_caps_at_max_rows_and_appends_tail() {
        // Build a table with 25 data rows (> MAX_ROWS=20)
        let mut lines = vec![
            " id  | value ".to_string(),
            "-----+-------".to_string(),
        ];
        for i in 1..=25 {
            lines.push(format!(" {:<4}| val{} ", i, i));
        }
        lines.push("(25 rows)".to_string());
        let input = lines.join("\n");

        let h = make_handler();
        let args: Vec<String> = vec![];
        let result = h.filter(&input, &args);
        let result_lines: Vec<&str> = result.lines().collect();

        // header + 20 data rows + "[+5 more rows]" + "(25 rows)"
        // = 23 lines
        assert_eq!(result_lines.len(), 23, "unexpected line count: {result_lines:?}");
        assert!(
            result_lines[result_lines.len() - 2].contains("[+5 more rows]"),
            "truncation marker missing"
        );
        assert_eq!(result_lines.last().unwrap().trim(), "(25 rows)");
    }

    #[test]
    fn empty_output_returns_empty() {
        let h = make_handler();
        let args: Vec<String> = vec![];
        let result = h.filter("", &args);
        assert_eq!(result, "");
    }
}

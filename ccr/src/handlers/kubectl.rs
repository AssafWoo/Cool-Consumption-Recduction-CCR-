use super::Handler;

pub struct KubectlHandler;

impl Handler for KubectlHandler {
    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        if subcmd == "logs" && !args.iter().any(|a| a.starts_with("--tail")) {
            let mut out = args.to_vec();
            out.push("--tail=200".to_string());
            return out;
        }
        args.to_vec()
    }

    fn filter(&self, output: &str, args: &[String]) -> String {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        match subcmd {
            "get" => filter_get(output),
            "logs" => filter_logs(output),
            "describe" => filter_describe(output),
            "apply" | "delete" | "rollout" => filter_changes(output),
            _ => output.to_string(),
        }
    }
}

const INTERESTING_COLUMNS: &[&str] = &[
    "NAME", "STATUS", "READY", "STATE", "PHASE", "TYPE", "CLUSTER-IP", "EXTERNAL-IP",
];

const MAX_GET_ROWS: usize = 30;

fn filter_get(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.is_empty() {
        return output.to_string();
    }

    // First non-empty line is the header
    let header_idx = match lines.iter().position(|l| !l.trim().is_empty()) {
        Some(i) => i,
        None => return output.to_string(),
    };

    let header = lines[header_idx];

    // Parse column positions from the header by finding where each word starts.
    // kubectl output uses fixed-width columns separated by two or more spaces.
    let col_starts: Vec<usize> = {
        let mut starts = Vec::new();
        let mut in_word = false;
        for (i, c) in header.char_indices() {
            if c != ' ' && !in_word {
                starts.push(i);
                in_word = true;
            } else if c == ' ' {
                in_word = false;
            }
        }
        starts
    };

    if col_starts.is_empty() {
        return output.to_string();
    }

    // Extract column names from header
    let col_names: Vec<String> = col_starts
        .iter()
        .enumerate()
        .map(|(i, &start)| {
            let end = if i + 1 < col_starts.len() {
                col_starts[i + 1]
            } else {
                header.len()
            };
            // trim trailing spaces from the slice
            let end = end.min(header.len());
            header[start..end].trim().to_uppercase()
        })
        .collect();

    // Determine which column indices to keep.
    // Always keep NAME (index 0).  Keep others that are "interesting".
    let keep_indices: Vec<usize> = {
        let interesting: Vec<usize> = col_names
            .iter()
            .enumerate()
            .filter(|(_, name)| INTERESTING_COLUMNS.contains(&name.as_str()))
            .map(|(i, _)| i)
            .collect();

        if interesting.is_empty() {
            // Fallback: keep first 3 columns
            (0..col_names.len().min(3)).collect()
        } else {
            interesting
        }
    };

    // Helper: extract a cell value for a given column index from a raw line
    let extract_cell = |line: &str, col_idx: usize| -> String {
        let start = col_starts[col_idx];
        if start >= line.len() {
            return String::new();
        }
        let end = if col_idx + 1 < col_starts.len() {
            col_starts[col_idx + 1].min(line.len())
        } else {
            line.len()
        };
        line[start..end].trim().to_string()
    };

    // Build output rows (header + data)
    let mut out: Vec<String> = Vec::new();

    // Header row
    let header_cells: Vec<String> = keep_indices
        .iter()
        .map(|&i| col_names[i].clone())
        .collect();
    out.push(header_cells.join("\t"));

    // Data rows (skip the header line itself)
    let data_lines: Vec<&str> = lines
        .iter()
        .skip(header_idx + 1)
        .filter(|l| !l.trim().is_empty())
        .copied()
        .collect();

    let total_data = data_lines.len();
    let capped = data_lines.iter().take(MAX_GET_ROWS);

    for line in capped {
        let cells: Vec<String> = keep_indices.iter().map(|&i| extract_cell(line, i)).collect();
        out.push(cells.join("\t"));
    }

    if total_data > MAX_GET_ROWS {
        out.push(format!("[+{} more]", total_data - MAX_GET_ROWS));
    }

    out.join("\n")
}

fn filter_logs(output: &str) -> String {
    let lines_in = output.lines().count();
    if lines_in == 0 {
        return output.to_string();
    }
    let budget = (lines_in / 3).max(20).min(200);
    ccr_core::summarizer::summarize(output, budget).output
}

fn filter_describe(output: &str) -> String {
    let keep_sections = ["Name:", "Status:", "Conditions:", "Events:"];
    let mut out: Vec<String> = Vec::new();
    let mut in_section = false;
    let mut annotation_count = 0usize;
    let mut in_annotations = false;

    for line in output.lines() {
        let t = line.trim();

        if t.starts_with("Annotations:") || t.starts_with("Labels:") {
            in_annotations = true;
            annotation_count = 0;
            out.push(line.to_string());
            continue;
        }

        if in_annotations {
            if line.starts_with(' ') || line.starts_with('\t') {
                annotation_count += 1;
                if annotation_count <= 5 {
                    out.push(line.to_string());
                } else if annotation_count == 6 {
                    out.push(format!("[{} annotations]", annotation_count));
                }
                continue;
            } else {
                in_annotations = false;
            }
        }

        let is_section = keep_sections.iter().any(|s| t.starts_with(s));
        if is_section {
            in_section = true;
        } else if !t.is_empty() && !line.starts_with(' ') && !line.starts_with('\t') {
            in_section = false;
        }

        if in_section || is_section {
            out.push(line.to_string());
        }
    }

    if out.is_empty() {
        output.to_string()
    } else {
        out.join("\n")
    }
}

fn filter_changes(output: &str) -> String {
    let out: Vec<&str> = output
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && (t.contains("created")
                    || t.contains("deleted")
                    || t.contains("configured")
                    || t.contains("unchanged")
                    || t.contains("error")
                    || t.contains("Error")
                    || t.starts_with("deployment.")
                    || t.starts_with("service.")
                    || t.starts_with("pod/"))
        })
        .collect();
    if out.is_empty() {
        output.to_string()
    } else {
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // A realistic `kubectl get pods` snippet
    const PODS_OUTPUT: &str = "\
NAME                          READY   STATUS    RESTARTS   AGE
my-app-6d4f8b9c7-xk2pq        1/1     Running   3          5d
another-pod-5b7c9d8f4-qr3mn   2/2     Running   0          2h
";

    #[test]
    fn filter_get_keeps_name_status_ready_drops_age_restarts() {
        let result = filter_get(PODS_OUTPUT);
        // Should contain NAME, READY, STATUS columns
        assert!(result.contains("NAME"), "missing NAME header");
        assert!(result.contains("STATUS"), "missing STATUS header");
        assert!(result.contains("READY"), "missing READY header");
        // AGE and RESTARTS should NOT appear as column headers
        let header_line = result.lines().next().unwrap();
        assert!(!header_line.contains("AGE"), "AGE should be dropped");
        assert!(!header_line.contains("RESTARTS"), "RESTARTS should be dropped");
        // Data should contain pod name
        assert!(result.contains("my-app-6d4f8b9c7-xk2pq"));
        assert!(result.contains("Running"));
    }

    #[test]
    fn filter_get_fallback_keeps_first_three_columns_when_no_interesting() {
        // Columns that are not in INTERESTING_COLUMNS
        let weird = "\
FOO   BAR   BAZ   QUUX
aaa   bbb   ccc   ddd
";
        let result = filter_get(weird);
        let header = result.lines().next().unwrap();
        // Should keep the first 3: FOO, BAR, BAZ
        assert!(header.contains("FOO"));
        assert!(header.contains("BAR"));
        assert!(header.contains("BAZ"));
        // QUUX should be dropped
        assert!(!header.contains("QUUX"), "QUUX should be dropped in fallback");
    }

    #[test]
    fn filter_get_caps_at_30_rows() {
        // Build a table with 35 data rows
        let mut lines = vec![
            "NAME                    STATUS    READY".to_string(),
        ];
        for i in 0..35 {
            lines.push(format!("pod-{:<20} Running   1/1", i));
        }
        let input = lines.join("\n");
        let result = filter_get(&input);
        let result_lines: Vec<&str> = result.lines().collect();
        // header + 30 data + 1 "[+N more]" line
        assert_eq!(result_lines.len(), 32, "expected header + 30 rows + tail line");
        assert!(result_lines.last().unwrap().contains("[+5 more]"));
    }

    #[test]
    fn filter_changes_keeps_configured_and_created_lines() {
        let input = "\
deployment.apps/my-app configured
some noise line with no keywords
service/my-svc created
another irrelevant line
pod/debug-pod-xyz unchanged
";
        let result = filter_changes(input);
        assert!(result.contains("configured"));
        assert!(result.contains("created"));
        assert!(result.contains("unchanged"));
        assert!(!result.contains("noise"), "noise lines should be dropped");
        assert!(!result.contains("irrelevant"), "irrelevant lines should be dropped");
    }

    #[test]
    fn filter_changes_returns_original_when_nothing_matches() {
        let input = "no relevant lines here\njust random text\n";
        let result = filter_changes(input);
        // passthrough preserves the original (including trailing newline)
        assert_eq!(result, input);
    }
}

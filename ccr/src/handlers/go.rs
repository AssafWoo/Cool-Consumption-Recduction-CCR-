use super::util;
use super::Handler;

pub struct GoHandler;

impl Handler for GoHandler {
    fn filter(&self, output: &str, args: &[String]) -> String {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        match subcmd {
            "build" | "install" | "vet" => filter_build(output),
            "test" => filter_test(output),
            "run" => filter_run(output),
            "mod" => filter_mod(output),
            _ => output.to_string(),
        }
    }
}

fn filter_build(output: &str) -> String {
    // Go build errors look like: "path/file.go:42:5: error message"
    let errors: Vec<&str> = output
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && (t.contains(": undefined")
                    || t.contains(": cannot")
                    || t.contains(": syntax error")
                    || t.contains(": declared and not used")
                    || t.contains(": imported and not used")
                    || t.contains(": too many")
                    || t.contains(": not enough")
                    || t.contains(": ambiguous")
                    || t.contains(": multiple")
                    || (t.contains(".go:") && t.contains(": ")))
        })
        .collect();

    if errors.is_empty() {
        if output.trim().is_empty() {
            return "[build OK]".to_string();
        }
        return output.to_string();
    }
    errors.join("\n")
}

fn filter_test(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    let mut out: Vec<String> = Vec::new();
    let mut in_failure = false;
    let mut failure_lines = 0usize;
    let mut pass_count = 0usize;
    let mut all_pass = true; // true until we see a FAIL marker

    for line in &lines {
        let t = line.trim();

        // Explicitly drop framework noise
        if t.starts_with("=== RUN")
            || t.starts_with("=== PAUSE")
            || t.starts_with("=== CONT")
            || t.starts_with("coverage:")
        {
            continue;
        }

        // "PASS" alone on a line — note it, emit summary later
        if t == "PASS" {
            // will be handled by the final summary; just skip the raw line
            continue;
        }

        // Count passing individual tests
        if t.starts_with("--- PASS:") {
            pass_count += 1;
            // Do not emit these lines
            if in_failure {
                // A PASS marker ends an active failure block
                in_failure = false;
            }
            continue;
        }

        // FAIL markers
        if t.starts_with("FAIL") || t.starts_with("--- FAIL:") {
            all_pass = false;
            out.push(line.to_string());
            in_failure = true;
            failure_lines = 0;
            continue;
        }

        // Panic lines
        if t.starts_with("panic:") || t.starts_with("goroutine ") {
            all_pass = false;
            out.push(line.to_string());
            continue;
        }

        if in_failure {
            if failure_lines < 10 {
                out.push(line.to_string());
                failure_lines += 1;
            } else if failure_lines == 10 {
                out.push("[... truncated ...]".to_string());
                failure_lines += 1;
            }
            // Blank line ends failure block (after we have a few lines of context)
            if t.is_empty() && failure_lines > 2 {
                in_failure = false;
            }
            continue;
        }

        // Summary: ok / FAIL with package + time
        if (t.starts_with("ok ") || t.starts_with("FAIL\t") || t.starts_with("FAIL "))
            && t.contains('\t')
        {
            out.push(line.to_string());
            continue;
        }

        // Error / hard-keep output
        if util::is_hard_keep(t) {
            out.push(line.to_string());
        }
    }

    // Append pass summary
    if all_pass && out.is_empty() {
        return "[all tests passed]".to_string();
    }
    if pass_count > 0 {
        out.push(format!("[{} tests passed]", pass_count));
    }

    if out.is_empty() {
        output.to_string()
    } else {
        out.join("\n")
    }
}

fn filter_run(output: &str) -> String {
    let lines: Vec<&str> = output.lines().collect();
    if lines.len() <= 50 {
        return output.to_string();
    }
    // Traceback / panic: keep from the panic line onward
    if let Some(pos) = output.find("goroutine 1 [running]:") {
        return output[pos..].to_string();
    }
    if let Some(pos) = output.find("panic:") {
        return output[pos..].to_string();
    }
    // Long output: BERT summarize
    let result = ccr_core::summarizer::summarize(output, 40);
    result.output
}

fn filter_mod(output: &str) -> String {
    // go mod tidy / download — keep warnings and errors only
    let important: Vec<&str> = output
        .lines()
        .filter(|l| {
            let t = l.trim();
            !t.is_empty()
                && (util::is_hard_keep(t)
                    || t.starts_with("go: ")
                    || t.contains("module")
                    || t.contains("version"))
        })
        .take(20)
        .collect();
    if important.is_empty() {
        "[go mod complete]".to_string()
    } else {
        important.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── filter_build ──────────────────────────────────────────────────────────

    #[test]
    fn build_ok_for_empty_output() {
        assert_eq!(filter_build(""), "[build OK]");
        assert_eq!(filter_build("   \n  "), "[build OK]");
    }

    #[test]
    fn build_extracts_go_error_lines() {
        let output = "# mypackage\n\
                      ./main.go:10:5: undefined: Foo\n\
                      ./main.go:20:3: cannot use x (type int) as type string\n\
                      some unrelated output line";
        let result = filter_build(output);
        assert!(result.contains("undefined: Foo"), "should keep undefined error");
        assert!(result.contains("cannot use x"), "should keep cannot error");
        assert!(!result.contains("unrelated"), "should drop noise");
    }

    #[test]
    fn build_keeps_ambiguous_and_multiple_errors() {
        let output = "# pkg\n./a.go:5:1: ambiguous import: found package foo\n./b.go:9:2: multiple-value in single-value context";
        let result = filter_build(output);
        assert!(result.contains("ambiguous import"), "should keep ambiguous");
        assert!(result.contains("multiple-value"), "should keep multiple");
    }

    #[test]
    fn build_passthrough_when_no_errors_but_has_output() {
        let output = "some non-error output that does not match patterns";
        let result = filter_build(output);
        assert_eq!(result, output);
    }

    // ── filter_test ───────────────────────────────────────────────────────────

    #[test]
    fn test_strips_run_and_pass_lines_keeps_fail() {
        let output = "=== RUN   TestFoo\n\
                      --- PASS: TestFoo (0.00s)\n\
                      === RUN   TestBar\n\
                      --- FAIL: TestBar (0.01s)\n\
                          bar_test.go:42: expected 1 got 2\n\
                      FAIL\tmy/pkg\t0.123s";
        let result = filter_test(output);
        assert!(!result.contains("=== RUN"), "should strip RUN markers");
        assert!(!result.contains("--- PASS:"), "should strip PASS lines");
        assert!(result.contains("--- FAIL: TestBar"), "should keep FAIL line");
        assert!(result.contains("expected 1 got 2"), "should keep failure detail");
        assert!(result.contains("FAIL\tmy/pkg"), "should keep package summary");
    }

    #[test]
    fn test_emits_pass_count_summary() {
        let output = "=== RUN   TestAlpha\n\
                      --- PASS: TestAlpha (0.00s)\n\
                      === RUN   TestBeta\n\
                      --- PASS: TestBeta (0.00s)\n\
                      === RUN   TestGamma\n\
                      --- PASS: TestGamma (0.00s)\n\
                      PASS\n\
                      ok  \tmy/pkg\t0.010s";
        let result = filter_test(output);
        assert!(
            result.contains("[3 tests passed]"),
            "should emit pass count: got {:?}",
            result
        );
    }

    #[test]
    fn test_pure_pass_output_emits_all_tests_passed() {
        let output = "=== RUN   TestOne\n\
                      --- PASS: TestOne (0.00s)\n\
                      PASS";
        let result = filter_test(output);
        // With one passing test and no failures the summary should fire
        assert!(
            result.contains("[all tests passed]") || result.contains("[1 tests passed]"),
            "should indicate all passed: got {:?}",
            result
        );
    }

    #[test]
    fn test_strips_coverage_lines() {
        let output = "=== RUN   TestFoo\n\
                      --- PASS: TestFoo (0.00s)\n\
                      coverage: 82.5% of statements\n\
                      ok  \tmy/pkg\t0.050s";
        let result = filter_test(output);
        assert!(!result.contains("coverage:"), "should strip coverage lines");
    }

    #[test]
    fn test_strips_pause_and_cont_markers() {
        let output = "=== RUN   TestParallel\n\
                      === PAUSE TestParallel\n\
                      === CONT  TestParallel\n\
                      --- PASS: TestParallel (0.00s)\n\
                      PASS";
        let result = filter_test(output);
        assert!(!result.contains("=== PAUSE"), "should strip PAUSE");
        assert!(!result.contains("=== CONT"), "should strip CONT");
    }

    #[test]
    fn test_failure_detail_truncated_after_10_lines() {
        let detail: String = (0..20).map(|i| format!("detail line {}\n", i)).collect();
        let output = format!(
            "--- FAIL: TestBig (0.00s)\n{}\nFAIL\tpkg\t0.1s",
            detail
        );
        let result = filter_test(&output);
        assert!(
            result.contains("[... truncated ...]"),
            "should truncate long failure blocks"
        );
    }

    // ── filter_mod ────────────────────────────────────────────────────────────

    #[test]
    fn mod_complete_for_empty_output() {
        assert_eq!(filter_mod(""), "[go mod complete]");
    }

    #[test]
    fn mod_keeps_go_prefix_lines() {
        let output = "go: downloading github.com/foo/bar v1.2.3\nsome noise\n";
        let result = filter_mod(output);
        assert!(result.contains("go: downloading"), "should keep go: lines");
        assert!(!result.contains("some noise"), "should drop noise");
    }
}

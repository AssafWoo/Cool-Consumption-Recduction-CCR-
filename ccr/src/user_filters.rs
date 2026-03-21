use serde::Deserialize;
use std::collections::HashMap;
use crate::handlers::Handler;

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserFiltersFile {
    #[serde(default)]
    pub commands: HashMap<String, UserCommandFilter>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct UserCommandFilter {
    #[serde(default)]
    pub strip_lines_matching: Vec<String>,
    #[serde(default)]
    pub keep_lines_matching: Vec<String>,
    pub match_output: Option<UserMatchOutput>,
    pub on_empty: Option<String>,
    pub max_lines: Option<usize>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct UserMatchOutput {
    pub pattern: String,
    pub message: String,
    pub unless_pattern: Option<String>,
}

/// Load user filter definitions from global config and project-local config.
/// Project-local overrides global for the same command keys.
pub fn load_user_filters() -> UserFiltersFile {
    let mut merged = UserFiltersFile::default();

    // 1. Try global config: ~/.config/ccr/filters.toml
    if let Some(config_dir) = dirs::config_dir() {
        let global_path = config_dir.join("ccr").join("filters.toml");
        if let Ok(contents) = std::fs::read_to_string(&global_path) {
            if let Ok(parsed) = toml::from_str::<UserFiltersFile>(&contents) {
                for (k, v) in parsed.commands {
                    merged.commands.insert(k, v);
                }
            }
        }
    }

    // 2. Try project-local: .ccr/filters.toml (from cwd)
    if let Ok(cwd) = std::env::current_dir() {
        let local_path = cwd.join(".ccr").join("filters.toml");
        if let Ok(contents) = std::fs::read_to_string(&local_path) {
            if let Ok(parsed) = toml::from_str::<UserFiltersFile>(&contents) {
                for (k, v) in parsed.commands {
                    // Project-local overrides global
                    merged.commands.insert(k, v);
                }
            }
        }
    }

    merged
}

pub struct UserFilterHandler {
    pub filter_def: UserCommandFilter,
}

impl Handler for UserFilterHandler {
    fn filter(&self, output: &str, _args: &[String]) -> String {
        let def = &self.filter_def;

        // 1. Check match_output short-circuit
        if let Some(mo) = &def.match_output {
            let pattern_matches = output.contains(&mo.pattern);
            let unless_blocks = mo
                .unless_pattern
                .as_ref()
                .map(|p| output.contains(p.as_str()))
                .unwrap_or(false);
            if pattern_matches && !unless_blocks {
                return mo.message.clone();
            }
        }

        // 2. Apply strip_lines_matching
        let mut lines: Vec<&str> = output.lines().collect();
        if !def.strip_lines_matching.is_empty() {
            lines.retain(|line| {
                !def.strip_lines_matching
                    .iter()
                    .any(|pat| line.contains(pat.as_str()))
            });
        }

        // 3. Apply keep_lines_matching
        if !def.keep_lines_matching.is_empty() {
            lines.retain(|line| {
                def.keep_lines_matching
                    .iter()
                    .any(|pat| line.contains(pat.as_str()))
            });
        }

        // 4. Apply max_lines cap
        if let Some(max) = def.max_lines {
            lines.truncate(max);
        }

        // 5. If result is empty, return on_empty
        if lines.is_empty() {
            return def.on_empty.clone().unwrap_or_default();
        }

        // 6. Return filtered output
        lines.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_handler(def: UserCommandFilter) -> UserFilterHandler {
        UserFilterHandler { filter_def: def }
    }

    #[test]
    fn test_strip_lines_matching_removes_correct_lines() {
        let def = UserCommandFilter {
            strip_lines_matching: vec!["WARNING".to_string(), "DEBUG".to_string()],
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "INFO: ok\nWARNING: something\nDEBUG: trace\nINFO: done";
        let result = handler.filter(output, &[]);
        assert!(result.contains("INFO: ok"));
        assert!(result.contains("INFO: done"));
        assert!(!result.contains("WARNING"));
        assert!(!result.contains("DEBUG"));
    }

    #[test]
    fn test_keep_lines_matching_keeps_only_matching() {
        let def = UserCommandFilter {
            keep_lines_matching: vec!["ERROR".to_string()],
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "INFO: ok\nERROR: something bad\nINFO: done\nERROR: another";
        let result = handler.filter(output, &[]);
        let lines: Vec<&str> = result.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines.iter().all(|l| l.contains("ERROR")));
    }

    #[test]
    fn test_match_output_fires_when_pattern_matches() {
        let def = UserCommandFilter {
            match_output: Some(UserMatchOutput {
                pattern: "Build succeeded".to_string(),
                message: "✓ build ok".to_string(),
                unless_pattern: None,
            }),
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "Compiling...\nBuild succeeded\nFinished in 2s";
        let result = handler.filter(output, &[]);
        assert_eq!(result, "✓ build ok");
    }

    #[test]
    fn test_match_output_blocked_by_unless_pattern() {
        let def = UserCommandFilter {
            match_output: Some(UserMatchOutput {
                pattern: "Build succeeded".to_string(),
                message: "✓ build ok".to_string(),
                unless_pattern: Some("with warnings".to_string()),
            }),
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "Build succeeded\nFinished with warnings";
        let result = handler.filter(output, &[]);
        // unless_pattern matches → short-circuit should NOT fire
        assert_ne!(result, "✓ build ok");
        assert!(result.contains("Build succeeded"));
    }

    #[test]
    fn test_on_empty_returned_when_all_filtered() {
        let def = UserCommandFilter {
            strip_lines_matching: vec!["noise".to_string()],
            on_empty: Some("(no output)".to_string()),
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "noise\nnoise again\nmore noise";
        let result = handler.filter(output, &[]);
        assert_eq!(result, "(no output)");
    }

    #[test]
    fn test_on_empty_default_when_not_set() {
        let def = UserCommandFilter {
            strip_lines_matching: vec!["everything".to_string()],
            on_empty: None,
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "everything goes away";
        let result = handler.filter(output, &[]);
        assert_eq!(result, "");
    }

    #[test]
    fn test_max_lines_caps_output() {
        let def = UserCommandFilter {
            max_lines: Some(3),
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "line1\nline2\nline3\nline4\nline5";
        let result = handler.filter(output, &[]);
        assert_eq!(result.lines().count(), 3);
        assert!(result.contains("line1"));
        assert!(!result.contains("line4"));
    }

    #[test]
    fn test_strip_then_keep_combined() {
        let def = UserCommandFilter {
            strip_lines_matching: vec!["noise".to_string()],
            keep_lines_matching: vec!["important".to_string()],
            ..Default::default()
        };
        let handler = make_handler(def);
        let output = "important: yes\nnoise: skip\nimportant noise: tricky\nother: skip";
        let result = handler.filter(output, &[]);
        // "important noise: tricky" is first stripped (contains "noise"), then keep_lines checks survivors
        // After strip: ["important: yes", "other: skip"]
        // After keep:  ["important: yes"]
        assert!(result.contains("important: yes"));
        assert!(!result.contains("noise"));
        assert!(!result.contains("other"));
    }
}

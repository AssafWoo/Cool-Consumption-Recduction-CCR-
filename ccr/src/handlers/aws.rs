use super::util;
use super::Handler;

pub struct AwsHandler;

// Subcommands that return structured JSON output and accept --output json.
// S3 transfer commands, configure, and help do not.
const JSON_SUBCMDS: &[&str] = &[
    "ec2", "ecs", "eks", "lambda", "s3api", "iam", "rds", "elb", "elbv2",
    "cloudformation", "cloudwatch", "sns", "sqs", "sts", "ssm", "secretsmanager",
    "route53", "logs", "dynamodb", "kinesis", "glue", "emr", "athena",
];

const MAX_RESOURCES: usize = 25;

impl Handler for AwsHandler {
    fn rewrite_args(&self, args: &[String]) -> Vec<String> {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        let should_inject = JSON_SUBCMDS.contains(&subcmd)
            && !args.iter().any(|a| a == "--output");
        if should_inject {
            let mut out = args.to_vec();
            out.push("--output".to_string());
            out.push("json".to_string());
            return out;
        }
        args.to_vec()
    }

    fn filter(&self, output: &str, args: &[String]) -> String {
        let subcmd = args.get(1).map(|s| s.as_str()).unwrap_or("");
        let action = args.get(2).map(|s| s.as_str()).unwrap_or("");

        // Always preserve errors
        if output.trim_start().starts_with("An error")
            || (output.contains("Error") && output.contains("Code"))
        {
            return output.to_string();
        }

        if subcmd == "s3" && action == "ls" {
            return filter_s3_ls(output);
        }

        // JSON output: try resource extraction first, then fall back to schema
        let trimmed = output.trim();
        if trimmed.starts_with('{') || trimmed.starts_with('[') {
            if let Ok(v) = serde_json::from_str::<serde_json::Value>(trimmed) {
                // Try specific AWS resource extraction first
                if let Some(extracted) = extract_aws_resources(subcmd, action, &v) {
                    if extracted.len() < trimmed.len() {
                        return extracted;
                    }
                }
                // Fall back to schema extraction
                let schema = util::json_to_schema(&v);
                let schema_str = serde_json::to_string_pretty(&schema).unwrap_or_default();
                if schema_str.len() < trimmed.len() {
                    return schema_str;
                }
            }
        }

        output.to_string()
    }
}

/// Extract key identifying fields from common AWS list/describe responses.
/// Returns `Some(compact_string)` if extraction succeeded and produced useful output,
/// or `None` to fall through to the next handler.
fn extract_aws_resources(subcmd: &str, action: &str, v: &serde_json::Value) -> Option<String> {
    match (subcmd, action) {
        ("ec2", "describe-instances") => extract_ec2_instances(v),
        ("ec2", "describe-security-groups") => extract_ec2_security_groups(v),
        ("iam", "list-users") => extract_iam_list(v, "Users", &["UserName", "UserId", "CreateDate"]),
        ("iam", "list-roles") => extract_iam_list(v, "Roles", &["RoleName", "RoleId"]),
        ("lambda", "list-functions") => extract_lambda_functions(v),
        ("ecs", "list-clusters") => extract_ecs_arn_list(v, "clusterArns"),
        ("ecs", "list-services") => extract_ecs_arn_list(v, "serviceArns"),
        ("ecs", "list-tasks") => extract_ecs_arn_list(v, "taskArns"),
        ("s3api", "list-buckets") => extract_s3api_buckets(v),
        _ => extract_generic_list(v),
    }
}

fn extract_ec2_instances(v: &serde_json::Value) -> Option<String> {
    let reservations = v.get("Reservations")?.as_array()?;
    let mut lines: Vec<String> = Vec::new();
    let mut total = 0usize;

    for reservation in reservations {
        let instances = reservation.get("Instances")?.as_array()?;
        for inst in instances {
            total += 1;
            if lines.len() >= MAX_RESOURCES {
                continue;
            }
            let id = str_field(inst, "InstanceId");
            let state = inst
                .get("State")
                .and_then(|s| s.get("Name"))
                .and_then(|n| n.as_str())
                .unwrap_or("-");
            let ip = inst
                .get("PublicIpAddress")
                .and_then(|v| v.as_str())
                .unwrap_or("-");
            let itype = str_field(inst, "InstanceType");
            lines.push(format!("{}\t{}\t{}\t{}", id, state, ip, itype));
        }
    }

    emit_lines(lines, total, Some("InstanceId\tState\tPublicIp\tType"))
}

fn extract_ec2_security_groups(v: &serde_json::Value) -> Option<String> {
    let groups = v.get("SecurityGroups")?.as_array()?;
    let total = groups.len();
    let lines: Vec<String> = groups
        .iter()
        .take(MAX_RESOURCES)
        .map(|g| {
            let gid = str_field(g, "GroupId");
            let name = str_field(g, "GroupName");
            let desc = str_field(g, "Description");
            format!("{}\t{}\t{}", gid, name, desc)
        })
        .collect();
    emit_lines(lines, total, Some("GroupId\tGroupName\tDescription"))
}

fn extract_iam_list(v: &serde_json::Value, key: &str, fields: &[&str]) -> Option<String> {
    let items = v.get(key)?.as_array()?;
    let total = items.len();
    let lines: Vec<String> = items
        .iter()
        .take(MAX_RESOURCES)
        .map(|item| {
            fields
                .iter()
                .map(|f| str_field(item, f))
                .collect::<Vec<_>>()
                .join("\t")
        })
        .collect();
    let header = fields.join("\t");
    emit_lines(lines, total, Some(&header))
}

fn extract_lambda_functions(v: &serde_json::Value) -> Option<String> {
    let funcs = v.get("Functions")?.as_array()?;
    let total = funcs.len();
    let lines: Vec<String> = funcs
        .iter()
        .take(MAX_RESOURCES)
        .map(|f| {
            let name = str_field(f, "FunctionName");
            let runtime = str_field(f, "Runtime");
            let modified = str_field(f, "LastModified");
            format!("{}\t{}\t{}", name, runtime, modified)
        })
        .collect();
    emit_lines(lines, total, Some("FunctionName\tRuntime\tLastModified"))
}

fn extract_ecs_arn_list(v: &serde_json::Value, key: &str) -> Option<String> {
    let arns = v.get(key)?.as_array()?;
    let total = arns.len();
    let lines: Vec<String> = arns
        .iter()
        .take(MAX_RESOURCES)
        .filter_map(|a| a.as_str())
        .map(|arn| {
            // Extract the last path component from the ARN
            arn.rsplit('/').next().unwrap_or(arn).to_string()
        })
        .collect();
    emit_lines(lines, total, None)
}

fn extract_s3api_buckets(v: &serde_json::Value) -> Option<String> {
    let buckets = v.get("Buckets")?.as_array()?;
    let total = buckets.len();
    let lines: Vec<String> = buckets
        .iter()
        .take(MAX_RESOURCES)
        .map(|b| {
            let name = str_field(b, "Name");
            let created = str_field(b, "CreationDate");
            format!("{}\t{}", name, created)
        })
        .collect();
    emit_lines(lines, total, Some("Name\tCreationDate"))
}

/// Generic fallback: for a top-level array, extract the first 5 fields of each item.
/// For a top-level object containing a single array value, unwrap it first.
fn extract_generic_list(v: &serde_json::Value) -> Option<String> {
    let arr = match v {
        serde_json::Value::Array(a) => a,
        serde_json::Value::Object(map) => {
            // If there is exactly one key holding an array, use that
            if map.len() == 1 {
                let only = map.values().next()?;
                only.as_array()?
            } else {
                return None;
            }
        }
        _ => return None,
    };

    if arr.is_empty() {
        return None;
    }

    // Only handle arrays of objects
    if !arr[0].is_object() {
        return None;
    }

    let total = arr.len();
    let lines: Vec<String> = arr
        .iter()
        .take(MAX_RESOURCES)
        .filter_map(|item| {
            let obj = item.as_object()?;
            let fields: Vec<String> = obj
                .iter()
                .take(5)
                .map(|(k, val)| {
                    let v_str = match val {
                        serde_json::Value::String(s) => s.clone(),
                        serde_json::Value::Number(n) => n.to_string(),
                        serde_json::Value::Bool(b) => b.to_string(),
                        serde_json::Value::Null => "-".to_string(),
                        _ => "[object]".to_string(),
                    };
                    format!("{}={}", k, v_str)
                })
                .collect();
            Some(fields.join("\t"))
        })
        .collect();

    emit_lines(lines, total, None)
}

/// Format lines with an optional header, appending a `[+N more]` trailer when capped.
fn emit_lines(lines: Vec<String>, total: usize, header: Option<&str>) -> Option<String> {
    if lines.is_empty() {
        return None;
    }
    let mut out: Vec<String> = Vec::new();
    if let Some(h) = header {
        out.push(h.to_string());
    }
    out.extend(lines);
    if total > MAX_RESOURCES {
        out.push(format!("[+{} more]", total - MAX_RESOURCES));
    }
    Some(out.join("\n"))
}

/// Get a string field from a JSON object, returning "-" if absent or not a string.
fn str_field<'a>(obj: &'a serde_json::Value, key: &str) -> String {
    obj.get(key)
        .and_then(|v| v.as_str())
        .unwrap_or("-")
        .to_string()
}

fn filter_s3_ls(output: &str) -> String {
    let mut prefixes: std::collections::HashMap<String, (usize, u64)> =
        std::collections::HashMap::new();
    let mut loose_count = 0usize;
    let mut loose_size = 0u64;

    for line in output.lines() {
        let t = line.trim();
        if t.is_empty() {
            continue;
        }
        if t.starts_with("PRE ") {
            let prefix = t[4..].trim().to_string();
            prefixes.entry(prefix).or_insert((0, 0));
            continue;
        }
        let parts: Vec<&str> = t.split_whitespace().collect();
        if parts.len() >= 4 {
            if let Ok(size) = parts[2].parse::<u64>() {
                loose_count += 1;
                loose_size += size;
            }
        }
    }

    let mut out: Vec<String> = Vec::new();
    if loose_count > 0 {
        out.push(format!("{} objects, {} bytes", loose_count, loose_size));
    }
    for (prefix, (count, size)) in &prefixes {
        if *count > 0 {
            out.push(format!("{}: {} objects, {} bytes", prefix, count, size));
        } else {
            out.push(format!("{}/", prefix));
        }
    }
    if out.is_empty() {
        output.to_string()
    } else {
        out.sort();
        out.join("\n")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    // ── extract_aws_resources ─────────────────────────────────────────────────

    #[test]
    fn ec2_describe_instances_extracts_instance_id() {
        let json = serde_json::json!({
            "Reservations": [{
                "Instances": [{
                    "InstanceId": "i-0abc123",
                    "State": { "Name": "running" },
                    "PublicIpAddress": "1.2.3.4",
                    "InstanceType": "t3.micro"
                }]
            }]
        });
        let result = extract_aws_resources("ec2", "describe-instances", &json).unwrap();
        assert!(result.contains("i-0abc123"), "should contain InstanceId");
        assert!(result.contains("running"), "should contain state");
        assert!(result.contains("1.2.3.4"), "should contain IP");
        assert!(result.contains("t3.micro"), "should contain type");
    }

    #[test]
    fn lambda_list_functions_extracts_name_and_runtime() {
        let json = serde_json::json!({
            "Functions": [
                {
                    "FunctionName": "my-func",
                    "Runtime": "python3.11",
                    "LastModified": "2024-01-01T00:00:00.000+0000"
                },
                {
                    "FunctionName": "other-func",
                    "Runtime": "nodejs18.x",
                    "LastModified": "2024-02-01T00:00:00.000+0000"
                }
            ]
        });
        let result = extract_aws_resources("lambda", "list-functions", &json).unwrap();
        assert!(result.contains("my-func"), "should contain FunctionName");
        assert!(result.contains("python3.11"), "should contain Runtime");
        assert!(result.contains("other-func"), "should contain second function");
    }

    #[test]
    fn s3api_list_buckets_extracts_bucket_names() {
        let json = serde_json::json!({
            "Buckets": [
                { "Name": "my-bucket", "CreationDate": "2023-06-01T12:00:00+00:00" },
                { "Name": "other-bucket", "CreationDate": "2023-07-15T08:30:00+00:00" }
            ],
            "Owner": { "ID": "abc123" }
        });
        let result = extract_aws_resources("s3api", "list-buckets", &json).unwrap();
        assert!(result.contains("my-bucket"), "should contain first bucket name");
        assert!(result.contains("other-bucket"), "should contain second bucket name");
        assert!(result.contains("2023-06-01"), "should contain creation date");
    }

    #[test]
    fn cap_at_25_resources_emits_trailer() {
        let instances: Vec<serde_json::Value> = (0..30)
            .map(|i| {
                serde_json::json!({
                    "InstanceId": format!("i-{:010}", i),
                    "State": { "Name": "running" },
                    "PublicIpAddress": "0.0.0.0",
                    "InstanceType": "t3.micro"
                })
            })
            .collect();
        let json = serde_json::json!({ "Reservations": [{ "Instances": instances }] });
        let result = extract_aws_resources("ec2", "describe-instances", &json).unwrap();
        assert!(result.contains("[+5 more]"), "should show overflow trailer");
    }

    #[test]
    fn ecs_list_clusters_extracts_arn_last_component() {
        let json = serde_json::json!({
            "clusterArns": [
                "arn:aws:ecs:us-east-1:123456789012:cluster/prod-cluster",
                "arn:aws:ecs:us-east-1:123456789012:cluster/staging-cluster"
            ]
        });
        let result = extract_aws_resources("ecs", "list-clusters", &json).unwrap();
        assert!(result.contains("prod-cluster"), "should contain cluster name");
        assert!(result.contains("staging-cluster"), "should contain second cluster");
        assert!(!result.contains("arn:aws:ecs"), "should strip ARN prefix");
    }

    // ── filter (error passthrough) ────────────────────────────────────────────

    #[test]
    fn error_output_passes_through_unchanged() {
        let handler = AwsHandler;
        let error_output =
            "An error occurred (InvalidClientTokenId) when calling the ListBuckets operation: \
             The security token included in the request is invalid.\n\
             Error Code: InvalidClientTokenId";
        let result = handler.filter(error_output, &args(&["aws", "s3api", "list-buckets"]));
        assert_eq!(result, error_output);
    }

    #[test]
    fn error_with_code_passes_through_unchanged() {
        let handler = AwsHandler;
        let error_output = "Error\nCode: AccessDenied\nMessage: Access denied";
        let result = handler.filter(error_output, &args(&["aws", "ec2", "describe-instances"]));
        assert_eq!(result, error_output);
    }

    // ── rewrite_args ──────────────────────────────────────────────────────────

    #[test]
    fn rewrite_args_injects_output_json_for_ec2() {
        let handler = AwsHandler;
        let a = args(&["aws", "ec2", "describe-instances"]);
        let out = handler.rewrite_args(&a);
        assert!(out.contains(&"--output".to_string()));
        assert!(out.contains(&"json".to_string()));
    }

    #[test]
    fn rewrite_args_skips_injection_when_output_already_set() {
        let handler = AwsHandler;
        let a = args(&["aws", "ec2", "describe-instances", "--output", "table"]);
        let out = handler.rewrite_args(&a);
        // Should not duplicate --output
        let count = out.iter().filter(|x| x.as_str() == "--output").count();
        assert_eq!(count, 1);
    }

    #[test]
    fn rewrite_args_skips_injection_for_s3_cp() {
        let handler = AwsHandler;
        let a = args(&["aws", "s3", "cp", "file.txt", "s3://bucket/"]);
        let out = handler.rewrite_args(&a);
        assert!(!out.contains(&"--output".to_string()));
    }

    // ── filter_s3_ls (unchanged behaviour) ───────────────────────────────────

    #[test]
    fn s3_ls_summarises_objects_and_prefixes() {
        let output = "2024-01-01 00:00:00       1024 file1.txt\n\
                      2024-01-02 00:00:00       2048 file2.txt\n\
                      PRE images/\n\
                      PRE logs/";
        let result = filter_s3_ls(output);
        assert!(result.contains("2 objects"), "should count loose objects");
        assert!(result.contains("3072 bytes"), "should sum sizes");
        assert!(result.contains("images/"), "should list prefix");
        assert!(result.contains("logs/"), "should list prefix");
    }
}

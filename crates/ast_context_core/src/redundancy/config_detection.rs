use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 88: Environment variable usage
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_env_var_usage(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let patterns = [
        ("std::env::var", "std::env::var"),
        ("env::var", "env::var"),
        ("os.environ", "os.environ"),
        ("os.getenv", "os.getenv"),
        ("process.env.", "process.env"),
        ("process.env[", "process.env"),
        ("os.Getenv", "os.Getenv"),
        ("System.getenv", "System.getenv"),
        ("getenv(", "getenv()"),
        ("dotenv", "dotenv"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        for (pattern, label) in &patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::EnvVarUsage {
                        function_name: func.name.clone(),
                        env_pattern: label.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` reads environment variables via `{}` — behavior depends on deployment configuration.",
                        func.name, label
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 89: Hardcoded endpoint
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_hardcoded_endpoints(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        // Look for hardcoded URLs (http:// or https://) excluding common safe ones
        let mut found = Vec::new();
        for line in src.lines() {
            let trimmed = line.trim();
            // Skip comments
            if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("/*") || trimmed.starts_with('*') {
                continue;
            }
            // Match http(s) URLs
            if let Some(start) = trimmed.find("http://").or_else(|| trimmed.find("https://")) {
                let url_area = &trimmed[start..];
                // Extract until quote, space, or end
                let end = url_area.find(|c: char| c == '"' || c == '\'' || c == '`' || c == ' ' || c == ')').unwrap_or(url_area.len());
                let url = &url_area[..end];
                // Skip common non-hardcoded patterns (docs, schemas, etc.)
                if url.contains("example.com") || url.contains("schema.org")
                    || url.contains("w3.org") || url.contains("xml") || url.contains("xmlns")
                    || url.contains("json-schema") || url.contains("swagger")
                    || url.contains("//TODO") || url.contains("//FIXME")
                    || url.contains("jsonrpc") || url.contains("modelcontextprotocol")
                    || url.contains("spdx.org")
                {
                    continue;
                }
                if !found.contains(&url.to_string()) {
                    found.push(url.to_string());
                }
            }
            // Match IP addresses (basic pattern)
            if trimmed.contains("127.0.0.1") || trimmed.contains("0.0.0.0") {
                // Common localhost, skip
            } else {
                // Look for IP:port patterns like "192.168.1.1:8080"
                for word in trimmed.split(|c: char| c == '"' || c == '\'' || c == '`' || c == ' ') {
                    if word.len() >= 7 {
                        let parts: Vec<&str> = word.split('.').collect();
                        if parts.len() >= 4 && parts[..3].iter().all(|p| p.parse::<u8>().is_ok()) {
                            if !found.contains(&word.to_string()) {
                                found.push(word.to_string());
                            }
                        }
                    }
                }
            }
        }

        for endpoint in found {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::HardcodedEndpoint {
                    function_name: func.name.clone(),
                    endpoint: endpoint.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}` contains hardcoded endpoint `{}` — should be a configuration value.",
                    func.name, endpoint
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 90: Feature flag
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_feature_flags(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let patterns = [
        ("#[cfg(feature", "Rust cfg(feature)"),
        ("#[cfg(target", "Rust cfg(target)"),
        ("#ifdef", "C/C++ #ifdef"),
        ("#if defined", "C/C++ #if defined"),
        ("feature_enabled", "feature_enabled()"),
        ("is_feature_on", "is_feature_on()"),
        ("feature_flag", "feature_flag"),
        ("FeatureFlag", "FeatureFlag"),
        ("FEATURE_", "FEATURE_ constant"),
        ("toggles.", "feature toggle"),
        ("LaunchDarkly", "LaunchDarkly"),
        ("unleash", "Unleash"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        for (pattern, label) in &patterns {
            if src.contains(pattern) {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::FeatureFlag {
                        name: func.name.clone(),
                        location: label.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` uses {} — behavior varies by feature/platform configuration.",
                        func.name, label
                    ),
                });
                break;
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 91: Config file usage
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_config_file_usage(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let config_patterns = [
        (".env", "dotenv file"),
        ("ctx.config.yaml", "YAML config"),
        ("ctx.config.yml", "YAML config"),
        ("ctx.config.json", "JSON config"),
        ("ctx.config.toml", "TOML config"),
        ("settings.yaml", "YAML settings"),
        ("settings.yml", "YAML settings"),
        ("settings.json", "JSON settings"),
        ("settings.toml", "TOML settings"),
        ("application.properties", "Java properties"),
        ("application.yml", "Spring YAML"),
        ("appsettings.json", ".NET appsettings"),
        ("pyproject.toml", "pyproject.toml"),
        ("Cargo.toml", "Cargo.toml"),
        ("package.json", "package.json"),
        ("tsconfig.json", "tsconfig"),
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let src = match &func.source {
            Some(s) => s.as_str(),
            None => continue,
        };

        for (pattern, label) in &config_patterns {
            if src.contains(pattern) {
                // Skip if it's just in a comment
                let in_comment = src.lines().any(|line| {
                    let t = line.trim();
                    line.contains(pattern) && (t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*'))
                }) && !src.lines().any(|line| {
                    let t = line.trim();
                    line.contains(pattern) && !(t.starts_with("//") || t.starts_with('#') || t.starts_with("/*") || t.starts_with('*'))
                });

                if in_comment {
                    continue;
                }

                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::ConfigFileUsage {
                        function_name: func.name.clone(),
                        config_pattern: label.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` references {} — this function depends on external configuration.",
                        func.name, label
                    ),
                });
                break;
            }
        }
    }
}

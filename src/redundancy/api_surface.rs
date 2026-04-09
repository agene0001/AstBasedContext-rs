use crate::types::node::GraphNode;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 82: Unstable public API
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unstable_public_api(
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

        let is_public = func.visibility.as_deref() == Some("public");
        if !is_public {
            continue;
        }

        let caller_count = ctx.caller_indices(idx).len();
        let param_count = func.args.len();

        // Exclude "self" from param count for methods
        let effective_params = if func.args.first().map(|a| a == "self" || a == "&self" || a == "&mut self").unwrap_or(false) {
            param_count.saturating_sub(1)
        } else {
            param_count
        };

        if caller_count >= 5 && effective_params >= 4 {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::UnstablePublicApi {
                    function_name: func.name.clone(),
                    caller_count,
                    param_count: effective_params,
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "Public API `{}` has {} callers and {} params — changing its signature has high impact.",
                    func.name, caller_count, effective_params
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 83: Undocumented public API
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_undocumented_public_api(
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

        let is_public = func.visibility.as_deref() == Some("public");
        if !is_public {
            continue;
        }

        // Skip trivial ctx.functions
        let line_count = func.source.as_ref().map(|s| s.lines().count()).unwrap_or(0);
        if line_count < 5 {
            continue;
        }

        // Skip test ctx.functions
        if func.name.starts_with("test") || func.name.starts_with("test_") {
            continue;
        }

        let has_docs = func.docstring.as_ref().map(|d| !d.trim().is_empty()).unwrap_or(false);
        if has_docs {
            continue;
        }

        // Only report if function has callers (actually used API)
        let caller_count = ctx.caller_indices(idx).len();
        if caller_count == 0 {
            continue;
        }

        findings.push(Finding {
            tier: Tier::Medium,
            kind: FindingKind::UndocumentedPublicApi {
                function_name: func.name.clone(),
                file_name: func.path.file_name().map(|f| f.to_string_lossy().to_string()).unwrap_or_default(),
            },
            node_indices: vec![idx.index()],
            description: format!(
                "Public function `{}` has {} callers but no documentation.",
                func.name, caller_count
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 84: Leaky abstraction
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_leaky_abstraction(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Collect names of internal/private types
    let stdlib_types: std::collections::HashSet<&str> = [
        "String", "str", "Vec", "Option", "Result", "HashMap", "HashSet",
        "Box", "Arc", "Rc", "Mutex", "RwLock", "Cell", "RefCell",
        "bool", "i8", "i16", "i32", "i64", "i128", "u8", "u16", "u32", "u64", "u128",
        "f32", "f64", "usize", "isize", "char",
        "int", "float", "list", "dict", "set", "tuple", "bytes", "None", "void",
        "number", "string", "boolean", "any", "object", "Array", "Map", "Set",
        "Promise", "Future", "Iterator", "Iterable",
        "error", "byte", "rune", "interface",
    ].iter().copied().collect();

    let mut internal_types: std::collections::HashSet<String> = std::collections::HashSet::new();

    for idx in ctx.graph.graph.node_indices() {
        let node = &ctx.graph.graph[idx];
        match node {
            GraphNode::Class(c) => {
                if c.name.starts_with('_') {
                    internal_types.insert(c.name.clone());
                }
            }
            GraphNode::Struct(s) => {
                // Structs without pub visibility are internal in Rust
                if s.name.starts_with('_') || s.name.starts_with("__") {
                    internal_types.insert(s.name.clone());
                }
            }
            _ => {}
        }
    }

    if internal_types.is_empty() {
        return;
    }

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        if func.is_dependency {
            continue;
        }

        let is_public = func.visibility.as_deref() == Some("public");
        if !is_public {
            continue;
        }

        let mut leaked = Vec::new();

        // Check arg types
        for t in func.arg_types.iter().flatten() {
            for internal in &internal_types {
                if !stdlib_types.contains(internal.as_str()) && t.contains(internal.as_str())
                    && !leaked.contains(internal)
                {
                    leaked.push(internal.clone());
                }
            }
        }

        // Check return type
        if let Some(ret) = &func.return_type {
            for internal in &internal_types {
                if !stdlib_types.contains(internal.as_str()) && ret.contains(internal.as_str())
                    && !leaked.contains(internal)
                {
                    leaked.push(internal.clone());
                }
            }
        }

        if !leaked.is_empty() {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::LeakyAbstraction {
                    function_name: func.name.clone(),
                    internal_types_exposed: leaked.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "Public `{}` leaks internal types [{}] — couples callers to implementation.",
                    func.name, leaked.join(", ")
                ),
            });
        }
    }
}

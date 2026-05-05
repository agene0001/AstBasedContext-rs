use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 11: Suggest facade — external modules calling many internals of one module
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_facade(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Group ctx.functions by their containing file (directory = "module")
    let mut file_functions: HashMap<String, Vec<NodeIndex>> = HashMap::new();
    for &(idx, node) in &ctx.functions {
        if let GraphNode::Function(f) = node {
            // Use the parent directory as the "module"
            if let Some(parent) = std::path::Path::new(&f.path).parent() {
                let module = parent.to_string_lossy().to_string();
                file_functions.entry(module).or_default().push(idx);
            }
        }
    }

    // For each module, count how many of its ctx.functions are called from outside
    for (module, func_indices) in &file_functions {
        if func_indices.len() < 4 {
            continue; // too small to need a facade
        }

        let func_set: HashSet<NodeIndex> = func_indices.iter().copied().collect();
        let mut external_callers: HashSet<NodeIndex> = HashSet::new();
        let mut internal_functions_called: HashSet<NodeIndex> = HashSet::new();

        for &func_idx in func_indices {
            for (caller_idx, caller_node) in ctx.get_callers_of(func_idx) {
                if let GraphNode::Function(cf) = caller_node {
                    let caller_module = std::path::Path::new(&cf.path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    // Caller is external if it's in a different module
                    if &caller_module != module && !func_set.contains(&caller_idx) {
                        external_callers.insert(caller_idx);
                        internal_functions_called.insert(func_idx);
                    }
                }
            }
        }

        let internal_called = internal_functions_called.len();
        let external_count = external_callers.len();

        // Need at least 4 internal ctx.functions called by at least 3 external callers
        if internal_called >= 4 && external_count >= 3 {
            let module_name = std::path::Path::new(module)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| module.clone());

            let tier = if internal_called >= 6 && external_count >= 5 {
                Tier::High
            } else {
                Tier::Medium
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::SuggestFacade {
                    module_name: module_name.clone(),
                    internal_functions_called: internal_called,
                    external_caller_count: external_count,
                },
                node_indices: internal_functions_called.iter().map(|i| i.index()).collect(),
                description: format!(
                    "`{}`: {} internal functions called by {} external callers — add a facade.",
                    module_name, internal_called, external_count,
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 12: Suggest factory — scattered constructor calls to sibling classes
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_factory(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find classes that share a base class (via INHERITS edges)
    let mut base_to_children: HashMap<NodeIndex, Vec<(NodeIndex, String)>> = HashMap::new();

    for &(idx, node) in &ctx.classes {
        if let GraphNode::Class(c) = node {
            let edges = ctx.graph.outgoing_edges(idx);
            for (parent_idx, kind) in &edges {
                if matches!(kind, EdgeKind::Inherits) {
                    base_to_children
                        .entry(*parent_idx)
                        .or_default()
                        .push((idx, c.name.clone()));
                }
            }
        }
    }

    // For each base with 3+ children, check if constructors are called from scattered locations
    for (base_idx, children) in &base_to_children {
        if children.len() < 3 {
            continue;
        }

        let child_indices: HashSet<NodeIndex> = children.iter().map(|(idx, _)| *idx).collect();
        let mut call_sites: HashSet<String> = HashSet::new(); // unique caller file paths

        for &(child_idx, _) in children {
            // Check callers of the class node itself (e.g. Python `MyClass()` constructor calls)
            for (caller_idx, caller_node) in ctx.get_callers_of(child_idx) {
                if let GraphNode::Function(cf) = caller_node {
                    let caller_parent = ctx.parent_of(caller_idx);
                    if let Some(cp) = caller_parent {
                        if !child_indices.contains(&cp) {
                            call_sites.insert(cf.path.to_string_lossy().to_string());
                        }
                    }
                }
            }

            // Look for callers of the child's constructor methods
            let child_methods = ctx.get_children(child_idx);
            for (method_idx, method_node) in child_methods {
                if let GraphNode::Function(f) = method_node {
                    if f.name == "new" || f.name == "__init__" || f.name == "constructor" || f.name == "create" {
                        for (caller_idx, caller_node) in ctx.get_callers_of(method_idx) {
                            if let GraphNode::Function(cf) = caller_node {
                                // Don't count calls from sibling classes themselves
                                let caller_parent = ctx.parent_of(caller_idx);
                                if let Some(cp) = caller_parent {
                                    if !child_indices.contains(&cp) {
                                        call_sites.insert(cf.path.to_string_lossy().to_string());
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }

        if call_sites.len() >= 2 {
            let base_name = ctx.graph.get_node(*base_idx)
                .map(|n| n.name().to_string())
                .unwrap_or_else(|| "?".to_string());
            let sibling_names: Vec<String> = children.iter().map(|(_, n)| n.clone()).collect();

            let tier = if children.len() >= 4 && call_sites.len() >= 3 {
                Tier::High
            } else {
                Tier::Medium
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::SuggestFactory {
                    base_name: base_name.clone(),
                    sibling_names: sibling_names.clone(),
                    call_site_count: call_sites.len(),
                },
                node_indices: children.iter().map(|(idx, _)| idx.index()).collect(),
                description: format!(
                    "{} inherit `{}`, constructed in {} locations — add factory method on `{}` to centralize.",
                    sibling_names.join(", "), base_name, call_sites.len(), base_name,
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 13: Suggest builder — functions/constructors with too many parameters
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_builder(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Only flag constructors or ctx.functions with 6+ params
        let param_count = func.args.len();
        if param_count < 6 {
            continue;
        }

        let is_constructor = func.name == "new"
            || func.name == "__init__"
            || func.name == "constructor"
            || func.name == "create"
            || func.name == "init"
            || func.name == "build"
            || func.name == "make";

        let tier = if is_constructor && param_count >= 8 {
            Tier::High
        } else {
            Tier::Medium
        };

        let context = if is_constructor { "Constructor" } else { "Function" };

        findings.push(Finding {
            tier,
            kind: FindingKind::SuggestBuilder {
                function_name: func.name.clone(),
                param_count,
            },
            node_indices: vec![idx.index()],
            description: format!(
                "{} `{}`: {} params — use builder pattern for optional defaults.",
                context, func.name, param_count,
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 14: Suggest strategy — trait/interface with multiple implementors
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_strategy(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find traits/interfaces/abstract classes with 3+ implementors
    for idx in ctx.graph.graph.node_indices() {
        let trait_name = match &ctx.graph.graph[idx] {
            GraphNode::Trait(t) => t.name.clone(),
            GraphNode::Interface(i) => i.name.clone(),
            GraphNode::Class(c) => c.name.clone(),
            _ => continue,
        };

        let implementors = ctx.graph.get_implementors(idx);
        if implementors.len() < 3 {
            continue;
        }

        let impl_names: Vec<String> = implementors
            .iter()
            .map(|(_, node)| node.name().to_string())
            .collect();

        // Check if callers branch to select which implementor to use.
        // We look for ctx.functions that call methods on multiple implementors.
        let impl_indices: HashSet<NodeIndex> = implementors.iter().map(|(i, _)| *i).collect();

        // Collect all methods across all implementors
        let mut impl_method_indices: HashSet<NodeIndex> = HashSet::new();
        for &(impl_idx, _) in &implementors {
            for (child_idx, child_node) in ctx.get_children(impl_idx) {
                if matches!(child_node, GraphNode::Function(_)) {
                    impl_method_indices.insert(child_idx);
                }
            }
        }

        // Find callers that call methods on 2+ different implementors
        let mut caller_impl_map: HashMap<NodeIndex, HashSet<NodeIndex>> = HashMap::new();
        for &method_idx in &impl_method_indices {
            for (caller_idx, _) in ctx.get_callers_of(method_idx) {
                // Which implementor does this method belong to?
                let parent = ctx.parent_of(method_idx);
                if let Some(p) = parent {
                    if impl_indices.contains(&p) {
                        caller_impl_map.entry(caller_idx).or_default().insert(p);
                    }
                }
            }
        }

        let branching_callers = caller_impl_map
            .values()
            .filter(|impls| impls.len() >= 2)
            .count();

        // Even without branching callers, 3+ implementors is suggestive
        let tier = if branching_callers >= 2 || implementors.len() >= 4 {
            Tier::Medium
        } else {
            Tier::Low
        };

        // Only emit if there's real signal
        if branching_callers >= 1 || implementors.len() >= 3 {
            findings.push(Finding {
                tier,
                kind: FindingKind::SuggestStrategy {
                    trait_name: trait_name.clone(),
                    implementor_names: impl_names.clone(),
                },
                node_indices: std::iter::once(idx.index())
                    .chain(implementors.iter().map(|(i, _)| i.index()))
                    .collect(),
                description: format!(
                    "`{}`: {} implementors ({}){}. Runtime selection → Strategy pattern.",
                    trait_name,
                    implementors.len(),
                    impl_names.join(", "),
                    if branching_callers > 0 {
                        format!(", {} callers branch across them", branching_callers)
                    } else {
                        String::new()
                    },
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 15: Suggest template method — base class with consistently overridden methods
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_template_method(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find classes with 2+ subclasses (via INHERITS incoming edges)
    for &(idx, node) in &ctx.classes {
        let base_name = match node {
            GraphNode::Class(c) => c.name.clone(),
            _ => continue,
        };

        let subclasses = ctx.graph.get_implementors(idx);
        if subclasses.len() < 2 {
            continue;
        }

        // Get the base class's method names
        let base_methods: HashSet<String> = ctx.graph
            .get_children(idx)
            .into_iter()
            .filter_map(|(_, node)| {
                if let GraphNode::Function(f) = node {
                    if f.name != "__init__" && f.name != "new" && f.name != "constructor" {
                        return Some(f.name.clone());
                    }
                }
                None
            })
            .collect();

        if base_methods.is_empty() {
            continue;
        }

        // For each subclass, check which base methods they override
        let mut override_counts: HashMap<String, usize> = HashMap::new();
        for (sub_idx, _) in &subclasses {
            let sub_methods: HashSet<String> = ctx.graph
                .get_children(*sub_idx)
                .into_iter()
                .filter_map(|(_, node)| {
                    if let GraphNode::Function(f) = node {
                        Some(f.name.clone())
                    } else {
                        None
                    }
                })
                .collect();

            for method in &base_methods {
                if sub_methods.contains(method) {
                    *override_counts.entry(method.clone()).or_default() += 1;
                }
            }
        }

        // "Hook methods" are those overridden by ALL subclasses
        let hook_methods: Vec<String> = override_counts
            .iter()
            .filter(|(_, count)| **count == subclasses.len())
            .map(|(name, _)| name.clone())
            .collect();

        if hook_methods.len() >= 2 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::SuggestTemplateMethod {
                    base_name: base_name.clone(),
                    hook_methods: hook_methods.clone(),
                    subclass_count: subclasses.len(),
                },
                node_indices: std::iter::once(idx.index())
                    .chain(subclasses.iter().map(|(i, _)| i.index()))
                    .collect(),
                description: format!(
                    "`{}`: {} subclasses all override [{}] — Template Method pattern.",
                    base_name,
                    subclasses.len(),
                    hook_methods.join(", "),
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 16: Suggest observer — high fan-in from unrelated modules
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_observer(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let callers = ctx.get_callers_of(idx);
        if callers.len() < 6 {
            continue;
        }

        // Count distinct directories the callers come from
        let caller_modules: HashSet<String> = callers
            .iter()
            .filter_map(|(_, caller_node)| {
                if let GraphNode::Function(cf) = caller_node {
                    std::path::Path::new(&cf.path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                } else {
                    None
                }
            })
            .collect();

        let own_module = std::path::Path::new(&func.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        // Only count external modules
        let external_modules: usize = caller_modules.iter().filter(|m| **m != own_module).count();

        if external_modules >= 4 {
            let tier = if external_modules >= 6 && callers.len() >= 10 {
                Tier::Medium
            } else {
                Tier::Low
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::SuggestObserver {
                    function_name: func.name.clone(),
                    caller_module_count: external_modules,
                    total_callers: callers.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} callers from {} modules — if reacting to same event, use Observer pattern.",
                    func.name, callers.len(), external_modules,
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 17: Suggest decorator — wrapper with before/after logic (noisy)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_decorator(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let line_count = src.lines().count();
        // Decorators are small wrappers but larger than passthroughs
        if !(5..=25).contains(&line_count) {
            continue;
        }

        // Must call exactly one function (like a passthrough, but with extra logic)
        let callees = ctx.get_callees_of(idx);
        if callees.len() != 1 {
            continue;
        }

        let (_, callee_node) = &callees[0];
        let callee_name = callee_node.name();

        // Check if there's code both BEFORE and AFTER the call
        let lines: Vec<&str> = src.lines().collect();
        let call_line_idx = lines.iter().position(|l| l.contains(callee_name));
        if let Some(call_pos) = call_line_idx {
            let code_before = lines[..call_pos]
                .iter()
                .any(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with("//") && !t.starts_with('#') && !t.starts_with("fn ") && !t.starts_with("def ") && !t.starts_with("func ") && !t.starts_with("function ") && !t.contains('{')
                });
            let code_after = lines[call_pos + 1..]
                .iter()
                .any(|l| {
                    let t = l.trim();
                    !t.is_empty() && !t.starts_with("//") && !t.starts_with('#') && t != "}"
                });

            if code_before && code_after {
                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::SuggestDecorator {
                        wrapper_name: func.name.clone(),
                        wrapped_name: callee_name.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` wraps `{}` with before/after logic ({}L) — repeating? use Decorator/middleware.",
                        func.name, callee_name, line_count,
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 18: Suggest mediator — module with high fan-in AND fan-out (noisy)
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn suggest_mediator(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Group ctx.functions by directory (module)
    let mut module_functions: HashMap<String, Vec<NodeIndex>> = HashMap::new();
    for &(idx, node) in &ctx.functions {
        if let GraphNode::Function(f) = node {
            if let Some(parent) = std::path::Path::new(&f.path).parent() {
                let module = parent.to_string_lossy().to_string();
                module_functions.entry(module).or_default().push(idx);
            }
        }
    }

    for (module, func_indices) in &module_functions {
        if func_indices.len() < 3 {
            continue;
        }

        let func_set: HashSet<NodeIndex> = func_indices.iter().copied().collect();

        // Fan-in: how many external modules call ctx.functions in this module
        let mut incoming_modules: HashSet<String> = HashSet::new();
        // Fan-out: how many external modules does this module call
        let mut outgoing_modules: HashSet<String> = HashSet::new();

        for &func_idx in func_indices {
            // Incoming
            for (caller_idx, caller_node) in ctx.get_callers_of(func_idx) {
                if !func_set.contains(&caller_idx) {
                    if let GraphNode::Function(cf) = caller_node {
                        if let Some(p) = std::path::Path::new(&cf.path).parent() {
                            let m = p.to_string_lossy().to_string();
                            if &m != module {
                                incoming_modules.insert(m);
                            }
                        }
                    }
                }
            }

            // Outgoing
            for (callee_idx, callee_node) in ctx.get_callees_of(func_idx) {
                if !func_set.contains(&callee_idx) {
                    if let GraphNode::Function(cf) = callee_node {
                        if let Some(p) = std::path::Path::new(&cf.path).parent() {
                            let m = p.to_string_lossy().to_string();
                            if &m != module {
                                outgoing_modules.insert(m);
                            }
                        }
                    }
                }
            }
        }

        let fan_in = incoming_modules.len();
        let fan_out = outgoing_modules.len();

        // High fan-in AND fan-out = coordination hub
        if fan_in >= 4 && fan_out >= 4 {
            let module_name = std::path::Path::new(module)
                .file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| module.clone());

            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::SuggestMediator {
                    module_name: module_name.clone(),
                    fan_in,
                    fan_out,
                },
                node_indices: func_indices.iter().map(|i| i.index()).collect(),
                description: format!(
                    "`{}`: fan-in={}, fan-out={} — coordination hub, consider Mediator pattern.",
                    module_name, fan_in, fan_out,
                ),
            });
        }
    }
}

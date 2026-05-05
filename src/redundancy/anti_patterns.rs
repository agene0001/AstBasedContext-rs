use crate::types::EdgeKind;
use crate::types::node::GraphNode;
use petgraph::graph::NodeIndex;
use std::collections::HashMap;
use std::collections::HashSet;
use super::context::AnalysisContext;
use super::helpers::p_min_len;
use super::types::{Tier, FindingKind, Finding};

// ─────────────────────────────────────────────────────────────────────────────
// Check 19: God class/module — too many methods or functions in one place
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_god_class(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for idx in ctx.graph.graph.node_indices() {
        let (name, node_type) = match &ctx.graph.graph[idx] {
            GraphNode::Class(c) => (c.name.clone(), "Class"),
            GraphNode::File(f) => {
                let name = std::path::Path::new(&f.path)
                    .file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_else(|| f.path.to_string_lossy().to_string());
                (name, "File")
            }
            _ => continue,
        };

        let children = ctx.get_children(idx);
        let method_count = children
            .iter()
            .filter(|(_, node)| matches!(node, GraphNode::Function(_)))
            .count();

        let threshold = match node_type {
            "Class" => 20,
            "File" => 30,
            _ => 30,
        };

        if method_count < threshold {
            continue;
        }

        let tier = if method_count >= threshold * 2 {
            Tier::High
        } else {
            Tier::Medium
        };

        findings.push(Finding {
            tier,
            kind: FindingKind::GodClass {
                name: name.clone(),
                method_count,
                node_type: node_type.to_string(),
            },
            node_indices: vec![idx.index()],
            description: format!(
                "{} `{}`: {} functions (threshold {}) — split into focused modules.",
                node_type, name, method_count, threshold,
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 20: Circular dependencies — cycles in the module import graph
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_circular_dependencies(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Build a simplified directed ctx.graph of file→file dependencies via IMPORTS
    // We map file NodeIndex → a compact usize for the cycle detection ctx.graph
    let file_indices: Vec<NodeIndex> = ctx.files.iter().map(|&(idx, _)| idx).collect();

    if file_indices.len() < 2 {
        return;
    }

    let idx_to_pos: HashMap<NodeIndex, usize> = file_indices
        .iter()
        .enumerate()
        .map(|(pos, &idx)| (idx, pos))
        .collect();

    // Build an adjacency-based digraph for cycle detection
    let mut dep_graph = petgraph::graph::DiGraph::<usize, ()>::new();
    let positions: Vec<petgraph::graph::NodeIndex> = (0..file_indices.len())
        .map(|i| dep_graph.add_node(i))
        .collect();

    for (pos, &file_idx) in file_indices.iter().enumerate() {
        // Use CALLS edges between ctx.functions to infer file-level dependencies
        for &child_idx in ctx.children_indices(file_idx) {
            if !matches!(&ctx.graph.graph[child_idx], GraphNode::Function(_)) {
                continue;
            }
            for &callee_idx in ctx.callee_indices(child_idx) {
                // Find which file contains the callee using precomputed parent_map
                let callee_file = ctx.parent_of(callee_idx)
                    .and_then(|parent| {
                        // Could be file→function or file→class→function
                        if matches!(&ctx.graph.graph[parent], GraphNode::File(_)) {
                            Some(parent)
                        } else {
                            // One more level up
                            ctx.parent_of(parent).filter(|&gp|
                                matches!(&ctx.graph.graph[gp], GraphNode::File(_))
                            )
                        }
                    });

                if let Some(callee_file_idx) = callee_file {
                    if callee_file_idx != file_idx {
                        if let Some(&target_pos) = idx_to_pos.get(&callee_file_idx) {
                            dep_graph.update_edge(positions[pos], positions[target_pos], ());
                        }
                    }
                }
            }
        }
    }

    // Find strongly connected components (SCCs) with size > 1 = cycles
    let sccs = petgraph::algo::tarjan_scc(&dep_graph);

    let mut seen_cycles: HashSet<Vec<usize>> = HashSet::new();
    for scc in &sccs {
        if scc.len() < 2 {
            continue;
        }

        // Map back to file names
        let mut cycle_positions: Vec<usize> = scc
            .iter()
            .map(|&n| dep_graph[n])
            .collect();
        cycle_positions.sort();

        if seen_cycles.contains(&cycle_positions) {
            continue;
        }
        seen_cycles.insert(cycle_positions.clone());

        let cycle_names: Vec<String> = cycle_positions
            .iter()
            .map(|&pos| {
                let file_idx = file_indices[pos];
                match &ctx.graph.graph[file_idx] {
                    GraphNode::File(f) => std::path::Path::new(&f.path)
                        .file_name()
                        .map(|n| n.to_string_lossy().to_string())
                        .unwrap_or_else(|| f.path.to_string_lossy().to_string()),
                    _ => "?".to_string(),
                }
            })
            .collect();

        let cycle_node_indices: Vec<usize> = cycle_positions
            .iter()
            .map(|&pos| file_indices[pos].index())
            .collect();

        let tier = if scc.len() <= 3 { Tier::High } else { Tier::Medium };

        findings.push(Finding {
            tier,
            kind: FindingKind::CircularDependency {
                cycle: cycle_names.clone(),
            },
            node_indices: cycle_node_indices,
            description: format!(
                "Circular dep: {} files [{}] — extract shared logic to break cycle.",
                scc.len(),
                cycle_names.join(" → "),
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 21: Feature envy — function uses another class more than its own
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_feature_envy(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Find which class/struct this function belongs to (via precomputed parent_map)
        let own_class = ctx.parent_of(idx)
            .filter(|&parent| {
                matches!(&ctx.graph.graph[parent], GraphNode::Class(_) | GraphNode::Struct(_))
            });

        let own_class_idx = match own_class {
            Some(c) => c,
            None => continue, // free function, skip
        };

        let own_class_name = ctx.graph.graph[own_class_idx].name().to_string();

        // Get all siblings (methods in the same class)
        let own_methods: HashSet<NodeIndex> = ctx.graph
            .get_children(own_class_idx)
            .into_iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .map(|(i, _)| i)
            .collect();

        // Count calls to own class methods vs calls to other class methods
        let callees = ctx.get_callees_of(idx);
        let mut own_calls = 0usize;
        let mut external_class_calls: HashMap<NodeIndex, usize> = HashMap::new();

        for (callee_idx, _) in &callees {
            if own_methods.contains(callee_idx) {
                own_calls += 1;
            } else {
                // Find which class the callee belongs to
                let callee_class = ctx.parent_of(*callee_idx)
                    .filter(|&parent| {
                        matches!(&ctx.graph.graph[parent], GraphNode::Class(_) | GraphNode::Struct(_))
                    });

                if let Some(cc) = callee_class {
                    if cc != own_class_idx {
                        *external_class_calls.entry(cc).or_default() += 1;
                    }
                }
            }
        }

        // Find the most-envied class
        if let Some((&envied_idx, &envied_calls)) = external_class_calls
            .iter()
            .max_by_key(|(_, count)| *count)
        {
            // Only flag if envied_calls > own_calls AND envied_calls >= 3
            if envied_calls > own_calls && envied_calls >= 3 {
                let envied_name = ctx.graph.graph[envied_idx].name().to_string();

                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::FeatureEnvy {
                        function_name: func.name.clone(),
                        own_class: own_class_name.clone(),
                        envied_class: envied_name.clone(),
                        own_calls,
                        envied_calls,
                    },
                    node_indices: vec![idx.index(), envied_idx.index()],
                    description: format!(
                        "`{}` in `{}`: {} calls on `{}` vs {} on own class — may belong in `{}`.",
                        func.name, own_class_name, envied_calls, envied_name,
                        own_calls, envied_name,
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 22: Shotgun surgery — changing a function affects many modules
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_shotgun_surgery(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let callers = ctx.get_callers_of(idx);
        if callers.len() < 4 {
            continue;
        }

        // Count how many distinct directories (modules) the callers span
        let own_module = std::path::Path::new(&func.path)
            .parent()
            .map(|p| p.to_string_lossy().to_string())
            .unwrap_or_default();

        let caller_modules: HashSet<String> = callers
            .iter()
            .filter_map(|(_, caller_node)| {
                if let GraphNode::Function(cf) = caller_node {
                    let module = std::path::Path::new(&cf.path)
                        .parent()
                        .map(|p| p.to_string_lossy().to_string())
                        .unwrap_or_default();
                    if module != own_module {
                        Some(module)
                    } else {
                        None
                    }
                } else {
                    None
                }
            })
            .collect();

        let affected = caller_modules.len();

        // Shotgun surgery: if changing this function's signature/behavior
        // would require updating callers in 5+ different modules
        if affected >= 5 {
            let tier = if affected >= 8 && callers.len() >= 15 {
                Tier::Medium
            } else {
                Tier::Low
            };

            findings.push(Finding {
                tier,
                kind: FindingKind::ShotgunSurgery {
                    function_name: func.name.clone(),
                    affected_modules: affected,
                    total_callers: callers.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} callers across {} modules — stabilize interface or add abstraction layer.",
                    func.name, callers.len(), affected,
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 29: Dead code
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_dead_code(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Gather all file-level (non-method, non-nested) ctx.functions that have zero callers.
    // Exclude main/entry points, test ctx.functions, and very small ctx.functions.
    let entry_names: HashSet<&str> = [
        "main", "run", "setup", "teardown", "init", "configure", "app", "create_app",
    ]
    .iter()
    .copied()
    .collect();

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Skip entry points and test/setup ctx.functions
        let lower = func.name.to_lowercase();
        if entry_names.contains(func.name.as_str())
            || lower.starts_with("test")
            || lower.starts_with("__")
            || func.decorators.iter().any(|d| d.contains("test") || d.contains("fixture") || d.contains("setup"))
        {
            continue;
        }

        // Skip methods (they may be called via dynamic dispatch)
        if func.class_context.is_some() {
            continue;
        }

        // Skip trivial ctx.functions (< 3 lines)
        if let Some(src) = &func.source {
            if src.lines().count() < 3 {
                continue;
            }
        }

        if ctx.caller_indices(idx).is_empty() {
            findings.push(Finding {
                tier: Tier::Critical,
                kind: FindingKind::DeadCode {
                    name: func.name.clone(),
                    file_path: func.path.display().to_string(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}` in {} is never called — dead code.",
                    func.name,
                    func.path.file_name().unwrap_or_default().to_string_lossy()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 30: Long parameter list
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_long_parameter_list(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const THRESHOLD: usize = 6;
    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Skip `self` param
        let effective_count = func.args.iter().filter(|a| *a != "self" && *a != "&self" && *a != "&mut self").count();
        if effective_count >= THRESHOLD {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::LongParameterList {
                    function_name: func.name.clone(),
                    param_count: effective_count,
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} params — group into a struct or use a builder.",
                    func.name, effective_count
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 31: Data clumps
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_data_clumps(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Build a map from sorted param-set (of size 3+) to ctx.functions that have all of them
    let min_clump = 3;
    let min_functions = 3;

    // Gather (function_name, param_set) for ctx.functions with 3+ params
    let func_params: Vec<(&str, HashSet<&str>)> = ctx.functions
        .iter()
        .filter_map(|(_, node)| {
            let func = match node {
                GraphNode::Function(f) => f,
                _ => return None,
            };
            let params: HashSet<&str> = func.args.iter()
                .map(|a| a.as_str())
                .filter(|a| *a != "self" && *a != "&self" && *a != "&mut self")
                .collect();
            if params.len() >= min_clump {
                Some((func.name.as_str(), params))
            } else {
                None
            }
        })
        .collect();

    if func_params.len() < min_functions {
        return;
    }

    // For each pair-wise combination, find shared params >= min_clump
    let mut clumps: HashMap<Vec<String>, Vec<String>> = HashMap::new();

    for i in 0..func_params.len() {
        for j in (i + 1)..func_params.len() {
            let shared: Vec<String> = func_params[i].1
                .intersection(&func_params[j].1)
                .map(|s| s.to_string())
                .collect();
            if shared.len() >= min_clump {
                let mut key = shared.clone();
                key.sort();
                clumps
                    .entry(key)
                    .or_default()
                    .push(func_params[i].0.to_string());
                let mut key2 = shared.clone();
                key2.sort();
                clumps
                    .entry(key2)
                    .or_default()
                    .push(func_params[j].0.to_string());
            }
        }
    }

    // Deduplicate function names per clump and report those with 3+ ctx.functions
    for (params, func_names) in &clumps {
        let mut unique: Vec<String> = func_names.clone();
        unique.sort();
        unique.dedup();
        if unique.len() >= min_functions {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::DataClump {
                    function_names: unique.clone(),
                    clumped_params: params.clone(),
                },
                node_indices: vec![],
                description: format!(
                    "Params [{}] co-occur in {} functions ({}) — group into a struct.",
                    params.join(", "),
                    unique.len(),
                    unique.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 32: Middle man
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_middle_man(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // For each class, check if 80%+ of its methods are passthroughs to a single other class.
    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let class_name = class_node.name();

        // Get methods of this class
        let methods: Vec<(NodeIndex, &GraphNode)> = ctx.get_children(*class_idx)
            .into_iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .collect();

        if methods.len() < 3 {
            continue;
        }

        // For each method, check if it calls exactly one function from another class
        let mut delegate_targets: HashMap<String, usize> = HashMap::new();
        let mut delegate_count = 0usize;

        for (m_idx, _m_node) in &methods {
            let callees = ctx.get_callees_of(*m_idx);
            if callees.len() == 1 {
                if let GraphNode::Function(callee_f) = callees[0].1 {
                    if let Some(ref callee_class) = callee_f.class_context {
                        if callee_class != class_name {
                            *delegate_targets.entry(callee_class.clone()).or_insert(0) += 1;
                            delegate_count += 1;
                        }
                    }
                }
            }
        }

        let total = methods.len();
        let ratio = delegate_count as f64 / total as f64;

        if ratio >= 0.8 {
            // Find the most-delegated-to class
            if let Some((target, _)) = delegate_targets.iter().max_by_key(|(_, c)| *c) {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::MiddleMan {
                        class_name: class_name.to_string(),
                        delegated_class: target.clone(),
                        delegation_ratio: ratio,
                        total_methods: total,
                    },
                    node_indices: vec![class_idx.index()],
                    description: format!(
                        "`{}` delegates {:.0}% of {} methods to `{}` — remove the middleman.",
                        class_name, ratio * 100.0, total, target
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 33: Lazy class
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_lazy_class(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (class_idx, class_node) in &classes {
        let class_data = match class_node {
            GraphNode::Class(d) => d,
            _ => continue,
        };

        let methods: Vec<_> = ctx.get_children(*class_idx)
            .into_iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .collect();

        // Only flag if no bases (not a subclass override point) and very few methods
        if methods.len() <= 2 && class_data.bases.is_empty() {
            // Check if methods are trivial (< 5 lines each)
            let all_trivial = methods.iter().all(|(_, n)| {
                n.source_snippet().map(|s| s.lines().count() <= 5).unwrap_or(true)
            });

            if all_trivial {
                findings.push(Finding {
                    tier: Tier::Medium,
                    kind: FindingKind::LazyClass {
                        class_name: class_data.name.clone(),
                        method_count: methods.len(),
                    },
                    node_indices: vec![class_idx.index()],
                    description: format!(
                        "`{}`: {} trivial method(s) — inline into caller or merge with related class.",
                        class_data.name, methods.len()
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 34: Refused bequest
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_refused_bequest(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    use petgraph::visit::EdgeRef;

    // Find classes that inherit but don't call/override any parent methods
    let classes: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if let GraphNode::Class(d) = node {
                if !d.bases.is_empty() {
                    return Some((idx, node));
                }
            }
            None
        })
        .collect();

    for (child_idx, child_node) in &classes {
        let child_data = match child_node {
            GraphNode::Class(d) => d,
            _ => continue,
        };

        // Find parent class via INHERITS edge
        let parent_names: Vec<String> = ctx.graph.graph
            .edges_directed(*child_idx, petgraph::Direction::Outgoing)
            .filter(|e| matches!(e.weight(), EdgeKind::Inherits))
            .map(|e| ctx.graph.graph[e.target()].name().to_string())
            .collect();

        if parent_names.is_empty() {
            continue;
        }

        // Get child's methods
        let child_methods: Vec<(NodeIndex, &GraphNode)> = ctx.get_children(*child_idx)
            .into_iter()
            .filter(|(_, n)| matches!(n, GraphNode::Function(_)))
            .collect();

        // Check if any child method calls a parent method
        let mut calls_parent = false;
        for (m_idx, _) in &child_methods {
            let callees = ctx.get_callees_of(*m_idx);
            for (_, callee_node) in &callees {
                if let GraphNode::Function(cf) = callee_node {
                    if let Some(ref cc) = cf.class_context {
                        if parent_names.contains(cc) {
                            calls_parent = true;
                            break;
                        }
                    }
                }
            }
            if calls_parent { break; }
        }

        // Also check if child method names overlap with parent (suggesting override)
        let child_method_names: HashSet<&str> = child_methods.iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n { Some(f.name.as_str()) } else { None }
            })
            .collect();

        // If child has methods but never calls parent and shares no method names,
        // it's likely a refused bequest
        if !calls_parent && !child_methods.is_empty() {
            for parent_name in &parent_names {
                // Find parent's methods
                let parent_indices = ctx.graph.find_by_name("Class", parent_name);
                for &pidx in parent_indices {
                    let parent_methods: HashSet<&str> = ctx.get_children(pidx)
                        .into_iter()
                        .filter_map(|(_, n)| {
                            if let GraphNode::Function(f) = n { Some(f.name.as_str()) } else { None }
                        })
                        .collect();

                    let overrides = child_method_names.intersection(&parent_methods).count();
                    if overrides == 0 && !parent_methods.is_empty() {
                        findings.push(Finding {
                            tier: Tier::Medium,
                            kind: FindingKind::RefusedBequest {
                                child_name: child_data.name.clone(),
                                parent_name: parent_name.clone(),
                            },
                            node_indices: vec![child_idx.index()],
                            description: format!(
                                "`{}` inherits `{}` but overrides/calls none of its methods — prefer composition.",
                                child_data.name, parent_name
                            ),
                        });
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 35: Speculative generality
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_speculative_generality(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find interfaces/traits/abstract-base-classes with exactly one implementor
    let abstractions: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::Trait(_) | GraphNode::Interface(_) | GraphNode::Class(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (abs_idx, abs_node) in &abstractions {
        let implementors = ctx.graph.get_implementors(*abs_idx);
        if implementors.len() == 1 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::SpeculativeGenerality {
                    interface_name: abs_node.name().to_string(),
                    sole_implementor: implementors[0].1.name().to_string(),
                },
                node_indices: vec![abs_idx.index(), implementors[0].0.index()],
                description: format!(
                    "`{}` has one implementor (`{}`) — abstraction may be premature.",
                    abs_node.name(), implementors[0].1.name()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 36: Inappropriate intimacy
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_inappropriate_intimacy(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    use petgraph::visit::EdgeRef;

    // Build class-to-class call counts
    let mut class_calls: HashMap<(String, String), usize> = HashMap::new();

    for &(idx, node) in &ctx.functions {
        let caller_class = match node {
            GraphNode::Function(f) => match &f.class_context {
                Some(c) => c.clone(),
                None => continue,
            },
            _ => continue,
        };

        for edge in ctx.graph.graph.edges_directed(idx, petgraph::Direction::Outgoing) {
            if !matches!(edge.weight(), EdgeKind::Calls { .. }) {
                continue;
            }
            let target = &ctx.graph.graph[edge.target()];
            if let GraphNode::Function(tf) = target {
                if let Some(ref tc) = tf.class_context {
                    if *tc != caller_class {
                        *class_calls.entry((caller_class.clone(), tc.clone())).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    // Find bidirectional high coupling
    let threshold = 5;
    let mut reported: HashSet<(String, String)> = HashSet::new();

    for ((a, b), a_to_b) in &class_calls {
        if *a_to_b < threshold {
            continue;
        }
        let b_to_a = class_calls.get(&(b.clone(), a.clone())).copied().unwrap_or(0);
        if b_to_a < threshold {
            continue;
        }

        let key = if a < b { (a.clone(), b.clone()) } else { (b.clone(), a.clone()) };
        if !reported.insert(key) {
            continue;
        }

        findings.push(Finding {
            tier: Tier::Low,
            kind: FindingKind::InappropriateIntimacy {
                class_a: a.clone(),
                class_b: b.clone(),
                a_to_b_calls: *a_to_b,
                b_to_a_calls: b_to_a,
            },
            node_indices: vec![],
            description: format!(
                "`{}` ↔ `{}`: {} A→B, {} B→A calls — extract shared logic or introduce mediator.",
                a, b, a_to_b, b_to_a
            ),
        });
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 37: Deep nesting / high complexity
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_deep_nesting(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const COMPLEXITY_THRESHOLD: u32 = 20;

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        if func.cyclomatic_complexity >= COMPLEXITY_THRESHOLD {
            let line_count = func.source.as_ref().map(|s| s.lines().count()).unwrap_or(0);
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DeepNesting {
                    function_name: func.name.clone(),
                    complexity: func.cyclomatic_complexity,
                    line_count,
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: cc={}, {}L — extract helpers or use early returns.",
                    func.name, func.cyclomatic_complexity, line_count
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 46: Divergent change
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_divergent_change(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const THRESHOLD: usize = 6;

    let files: Vec<(NodeIndex, &GraphNode)> = ctx.graph
        .graph
        .node_indices()
        .filter_map(|idx| {
            let node = &ctx.graph.graph[idx];
            if matches!(node, GraphNode::File(_)) {
                Some((idx, node))
            } else {
                None
            }
        })
        .collect();

    for (file_idx, file_node) in &files {
        let mut caller_dirs: HashSet<String> = HashSet::new();

        for &child_idx in ctx.children_indices(*file_idx) {
            for (_, caller_node) in ctx.get_callers_of(child_idx) {
                if let GraphNode::Function(cf) = caller_node {
                    if let Some(parent) = cf.path.parent() {
                        caller_dirs.insert(parent.display().to_string());
                    }
                }
            }
        }

        // Remove own directory
        if let GraphNode::File(fd) = file_node {
            if let Some(own_dir) = fd.path.parent() {
                caller_dirs.remove(&own_dir.display().to_string());
            }
        }

        if caller_dirs.len() >= THRESHOLD {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::DivergentChange {
                    file_name: file_node.name().to_string(),
                    caller_module_count: caller_dirs.len(),
                },
                node_indices: vec![file_idx.index()],
                description: format!(
                    "`{}` called from {} modules — too many reasons to change, split by responsibility.",
                    file_node.name(), caller_dirs.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 47: Parallel inheritance
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_parallel_inheritance(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Build parent → children map
    let mut parent_children: HashMap<String, Vec<String>> = HashMap::new();

    for &(_idx, node) in &ctx.classes {
        let _cd = if let GraphNode::Class(cd) = node { cd } else { continue };
        if let GraphNode::Class(cd) = node {
            for base in &cd.bases {
                parent_children
                    .entry(base.clone())
                    .or_default()
                    .push(cd.name.clone());
            }
        }
    }

    let hierarchies: Vec<(&String, &Vec<String>)> = parent_children
        .iter()
        .filter(|(_, children)| children.len() >= 2)
        .collect();

    let mut reported: HashSet<(String, String)> = HashSet::new();

    for i in 0..hierarchies.len() {
        for j in (i + 1)..hierarchies.len() {
            let (parent_a, children_a) = hierarchies[i];
            let (parent_b, children_b) = hierarchies[j];

            let mut paired = 0usize;
            for ca in children_a {
                for cb in children_b {
                    let common_prefix = ca.chars().zip(cb.chars())
                        .take_while(|(a, b)| a == b)
                        .count();
                    if common_prefix >= 3 {
                        paired += 1;
                        break;
                    }
                }
            }

            if paired >= 2 {
                let key = if parent_a < parent_b {
                    (parent_a.clone(), parent_b.clone())
                } else {
                    (parent_b.clone(), parent_a.clone())
                };
                if !reported.insert(key) {
                    continue;
                }

                findings.push(Finding {
                    tier: Tier::Low,
                    kind: FindingKind::ParallelInheritance {
                        hierarchy_a: parent_a.clone(),
                        hierarchy_b: parent_b.clone(),
                        paired_count: paired,
                    },
                    node_indices: vec![],
                    description: format!(
                        "Hierarchies `{}` and `{}`: {} paired subclasses — merge with composition or generics.",
                        parent_a, parent_b, paired
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 48: Primitive obsession
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_primitive_obsession(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    let primitives: HashSet<&str> = [
        "str", "string", "String", "int", "i32", "i64", "u32", "u64",
        "float", "f32", "f64", "bool", "boolean", "number", "double",
        "long", "short", "byte", "char", "usize", "isize",
    ].iter().copied().collect();

    const MIN_PRIMITIVE_PARAMS: usize = 3;

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        if func.arg_types.is_empty() {
            continue;
        }

        let primitive_params: Vec<String> = func.args.iter()
            .zip(func.arg_types.iter())
            .filter(|(name, _)| *name != "self" && *name != "&self" && *name != "&mut self")
            .filter_map(|(name, type_opt)| {
                if let Some(type_ann) = type_opt {
                    let clean = type_ann.trim_start_matches('&').trim_start_matches("mut ").trim();
                    if primitives.contains(clean) {
                        return Some(format!("{}: {}", name, type_ann));
                    }
                }
                None
            })
            .collect();

        let non_self_count = func.args.iter()
            .filter(|a| *a != "self" && *a != "&self" && *a != "&mut self")
            .count();

        if primitive_params.len() >= MIN_PRIMITIVE_PARAMS && primitive_params.len() == non_self_count {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::PrimitiveObsession {
                    function_name: func.name.clone(),
                    primitive_params: primitive_params.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} primitive-only params [{}] — introduce domain types.",
                    func.name, primitive_params.len(),
                    primitive_params.join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 49: Large class
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_large_class(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const LINE_THRESHOLD: usize = 500;

    for &(idx, node) in &ctx.classes {
        let (name, source, node_type) = match node {
            GraphNode::Class(d) => (&d.name, &d.source, "class"),
            _ => continue,
        };

        if let Some(src) = source {
            let line_count = src.lines().count();
            if line_count >= LINE_THRESHOLD {
                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::LargeClass {
                        name: name.clone(),
                        line_count,
                        node_type: node_type.to_string(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}`: {}L — split into focused classes.",
                        name, line_count
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 50: Unstable dependency
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_unstable_dependency(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    const HIGH_FAN_IN: usize = 15;

    let mut file_caller_count: HashMap<String, usize> = HashMap::new();

    for &(idx, node) in &ctx.functions {
        let _f = if let GraphNode::Function(f) = node { f } else { continue };
        if let GraphNode::Function(f) = node {
            let callers = ctx.get_callers_of(idx);
            let caller_files: HashSet<String> = callers.iter()
                .filter_map(|(_, cn)| {
                    if let GraphNode::Function(cf) = cn {
                        Some(cf.path.display().to_string())
                    } else {
                        None
                    }
                })
                .collect();
            let file_path = f.path.display().to_string();
            for cf in caller_files {
                if cf != file_path {
                    *file_caller_count.entry(file_path.clone()).or_insert(0) += 1;
                }
            }
        }
    }

    use petgraph::visit::EdgeRef;
    let mut reported: HashSet<(String, String)> = HashSet::new();

    // Build file stem → file_caller_count lookup for module resolution
    let file_stem_to_path: HashMap<String, (String, String)> = file_caller_count
        .iter()
        .filter_map(|(path, &_count)| {
            let p = std::path::Path::new(path);
            let stem = p.file_stem()?.to_string_lossy().to_string();
            let name = p.file_name()?.to_string_lossy().to_string();
            Some((stem, (path.clone(), name)))
        })
        .collect();

    for &(idx, node) in &ctx.files {
        if let GraphNode::File(fd) = node {
            let own_path = fd.path.display().to_string();

            for edge in ctx.graph.graph.edges_directed(idx, petgraph::Direction::Outgoing) {
                if !matches!(edge.weight(), EdgeKind::Imports { .. }) {
                    continue;
                }
                let target = &ctx.graph.graph[edge.target()];

                // Imports point to Module nodes; resolve module name to file path
                let module_name = target.name();
                let module_stem = module_name.split('.').next_back().unwrap_or(module_name);

                if let Some((dep_path, dep_name)) = file_stem_to_path.get(module_stem) {
                    if *dep_path == own_path {
                        continue;
                    }
                    let fan_in = file_caller_count.get(dep_path).copied().unwrap_or(0);

                    if fan_in >= HIGH_FAN_IN {
                        let key = (own_path.clone(), dep_path.clone());
                        if !reported.insert(key) {
                            continue;
                        }
                        findings.push(Finding {
                            tier: Tier::Low,
                            kind: FindingKind::UnstableDependency {
                                dependent_name: fd.name.clone(),
                                dependency_name: dep_name.clone(),
                                dependency_caller_count: fan_in,
                            },
                            node_indices: vec![idx.index(), edge.target().index()],
                            description: format!(
                                "`{}` → `{}` ({} callers) — high-ripple dependency.",
                                fd.name, dep_name, fan_in
                            ),
                        });
                    }
                }
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 64: Anemic domain model
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_anemic_domain_model(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Class where 80%+ methods are getters/setters (get_X/set_X, getX/setX, @property)
    let getter_setter_patterns: &[&str] = &[
        "get_", "set_", "get", "set", "is_",
    ];

    for &(idx, node) in &ctx.classes {
        if !matches!(node, GraphNode::Class(_)) {
            continue;
        }

        let methods: Vec<String> = ctx.get_children(idx)
            .into_iter()
            .filter_map(|(_, n)| {
                if let GraphNode::Function(f) = n {
                    // Skip __init__, __str__ etc
                    if !f.name.starts_with("__") { Some(f.name.clone()) } else { None }
                } else {
                    None
                }
            })
            .collect();

        if methods.len() < 4 {
            continue;
        }

        let gs_count = methods.iter().filter(|m| {
            let lower = m.to_lowercase();
            getter_setter_patterns.iter().any(|p| lower.starts_with(p))
                && lower.len() > p_min_len(&lower, getter_setter_patterns)
        }).count();

        let ratio = gs_count as f64 / methods.len() as f64;
        if ratio >= 0.8 && gs_count >= 4 {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::AnemicDomainModel {
                    class_name: node.name().to_string(),
                    getter_setter_count: gs_count,
                    total_methods: methods.len(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {}/{} getters/setters — add domain behavior instead of exposing raw data.",
                    node.name(), gs_count, methods.len()
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 65: Magic numbers/strings
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_magic_numbers(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Look for numeric literals in function source that aren't 0, 1, -1, 2
    let allowed_numbers: HashSet<&str> = [
        "0", "1", "2", "-1", "0.0", "1.0", "0.5", "100", "true", "false",
        "None", "null", "nil", "undefined", "\"\"", "''",
    ].iter().copied().collect();

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Simple heuristic: find numeric literals that look like magic numbers
        let mut literals = Vec::new();
        for line in src.lines() {
            let trimmed = line.trim();
            // Skip comments, imports, string literals
            if trimmed.starts_with("//") || trimmed.starts_with('#') || trimmed.starts_with("import") {
                continue;
            }
            // Find standalone numbers (not in variable names)
            for word in trimmed.split(|c: char| !c.is_alphanumeric() && c != '.' && c != '-') {
                let clean = word.trim();
                if clean.is_empty() {
                    continue;
                }
                // Check if it's a number
                if clean.parse::<f64>().is_ok() && !allowed_numbers.contains(clean) {
                    // Skip constant definitions and duplicates
                    if !trimmed.contains("const ") && !trimmed.contains("CONST")
                        && !literals.contains(&clean.to_string())
                    {
                        literals.push(clean.to_string());
                    }
                }
            }
        }

        if literals.len() >= 3 {
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::MagicNumber {
                    function_name: func.name.clone(),
                    literals: literals.iter().take(5).cloned().collect(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} magic numbers [{}] — extract as named constants.",
                    func.name, literals.len(),
                    literals.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 66: Mutable global state
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_mutable_global_state(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Find module-level variables that are mutable (no class context, no function context)
    for &(idx, node) in &ctx.variables {
        let _v = if let GraphNode::Variable(v) = node { v } else { continue };
        if let GraphNode::Variable(v) = node {
            // Skip if inside a class or function
            if v.context.is_some() || v.class_context.is_some() {
                continue;
            }

            // Check naming: uppercase = constant, lowercase = likely mutable
            let is_constant = v.name.chars().all(|c| c.is_uppercase() || c == '_');
            if is_constant {
                continue;
            }

            // Check for mutable patterns in value
            let is_mutable_collection = v.value.as_ref().map(|val| {
                val.contains("[]") || val.contains("{}") || val.contains("dict(")
                    || val.contains("list(") || val.contains("set(")
                    || val.contains("Vec::new") || val.contains("HashMap::new")
            }).unwrap_or(false);

            if is_mutable_collection {
                let file_name = v.path.file_name()
                    .map(|n| n.to_string_lossy().to_string())
                    .unwrap_or_default();

                findings.push(Finding {
                    tier: Tier::High,
                    kind: FindingKind::MutableGlobalState {
                        variable_name: v.name.clone(),
                        file_name: file_name.clone(),
                    },
                    node_indices: vec![idx.index()],
                    description: format!(
                        "`{}` in `{}`: module-level mutable state — encapsulate in a class or inject as dependency.",
                        v.name, file_name
                    ),
                });
            }
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 67: Empty catch
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_empty_catch(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Detect empty catch/except blocks in function source
    let empty_catch_patterns: &[&str] = &[
        "except:\n        pass",
        "except Exception:\n        pass",
        "catch {\n    }",
        "catch {\n}",
        "catch (Exception e) {\n    }",
        "catch (Exception e) {\n}",
        "catch (_) {}",
        "catch (_) { }",
    ];

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        let lower = src.to_lowercase();
        let has_empty_catch = empty_catch_patterns.iter().any(|p| lower.contains(&p.to_lowercase()))
            || (lower.contains("except") && lower.contains("pass") && !lower.contains("log"))
            || (lower.contains("catch") && lower.contains("{ }"));

        if has_empty_catch {
            findings.push(Finding {
                tier: Tier::High,
                kind: FindingKind::EmptyCatch {
                    function_name: func.name.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: empty catch/except — errors silently swallowed.",
                    func.name
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 68: Callback hell
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_callback_hell(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Detect deeply nested closures/callbacks by counting indentation levels
    const NESTING_THRESHOLD: usize = 4;

    for &(idx, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };

        // Count nested callback/closure patterns
        let mut max_nesting = 0usize;
        let mut current_nesting = 0usize;

        for line in src.lines() {
            let trimmed = line.trim();
            // Count nested function/closure definitions
            if trimmed.contains("function(") || trimmed.contains("function (")
                || trimmed.contains("=>") || trimmed.contains("lambda ")
                || trimmed.contains("|{") || trimmed.contains("| {")
                || (trimmed.contains("def ") && current_nesting > 0)
            {
                current_nesting += 1;
                max_nesting = max_nesting.max(current_nesting);
            }
            // Track closing braces roughly
            if trimmed == "}" || trimmed == "});" || trimmed == "})" {
                current_nesting = current_nesting.saturating_sub(1);
            }
        }

        if max_nesting >= NESTING_THRESHOLD {
            findings.push(Finding {
                tier: Tier::Medium,
                kind: FindingKind::CallbackHell {
                    function_name: func.name.clone(),
                    nesting_depth: max_nesting,
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} nested callback levels — use async/await or extract named functions.",
                    func.name, max_nesting
                ),
            });
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Check 69: API inconsistency
// ─────────────────────────────────────────────────────────────────────────────

pub(super) fn detect_api_inconsistency(
    ctx: &AnalysisContext,
    findings: &mut Vec<Finding>,
) {
    // Group ctx.functions by common prefix (e.g., create_user, create_order, create_product)
    // and check if they have inconsistent parameter patterns
    let mut prefix_groups: HashMap<String, Vec<(&str, &[String])>> = HashMap::new();

    for &(_, node) in &ctx.functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };

        // Extract prefix (e.g., "create" from "create_user")
        let prefix = if let Some(pos) = func.name.find('_') {
            &func.name[..pos]
        } else if func.name.len() > 3 {
            // camelCase: extract verb prefix
            let end = func.name.char_indices()
                .skip(1)
                .find(|(_, c)| c.is_uppercase())
                .map(|(i, _)| i)
                .unwrap_or(func.name.len());
            &func.name[..end]
        } else {
            continue;
        };

        if prefix.len() >= 3 {
            prefix_groups
                .entry(prefix.to_string())
                .or_default()
                .push((&func.name, &func.args));
        }
    }

    let mut reported: HashSet<String> = HashSet::new();

    for (prefix, group) in &prefix_groups {
        if group.len() < 3 {
            continue;
        }

        // Check if param counts vary significantly
        let counts: Vec<usize> = group.iter().map(|(_, args)| args.len()).collect();
        let min_count = counts.iter().min().copied().unwrap_or(0);
        let max_count = counts.iter().max().copied().unwrap_or(0);

        if max_count - min_count >= 3 {
            if !reported.insert(prefix.clone()) {
                continue;
            }

            let names: Vec<String> = group.iter().map(|(n, _)| n.to_string()).collect();
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::ApiInconsistency {
                    function_names: names.iter().take(5).cloned().collect(),
                    shared_prefix: prefix.clone(),
                },
                node_indices: vec![],
                description: format!(
                    "`{}` prefix: {}-{} params across {} functions — standardize signatures.",
                    prefix, min_count, max_count, group.len()
                ),
            });
        }
    }
}

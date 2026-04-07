use petgraph::graph::NodeIndex;

use super::code_graph::CodeGraph;
use crate::types::node::GraphNode;
use crate::types::EdgeKind;

type TokenizedEntry<'a> = ((NodeIndex, &'a GraphNode, &'a str), Vec<&'a str>);

impl CodeGraph {
    /// Get all functions called by the function at `idx`.
    pub fn get_callees_of(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode)> {
        self.outgoing_edges(idx)
            .into_iter()
            .filter(|(_, kind)| matches!(kind, EdgeKind::Calls { .. }))
            .filter_map(|(target, _)| {
                self.get_node(target).map(|n| (target, n))
            })
            .collect()
    }

    /// Get all functions that call the function at `idx`.
    pub fn get_callers_of(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode)> {
        self.incoming_edges(idx)
            .into_iter()
            .filter(|(_, kind)| matches!(kind, EdgeKind::Calls { .. }))
            .filter_map(|(source, _)| {
                self.get_node(source).map(|n| (source, n))
            })
            .collect()
    }

    /// Get the inheritance chain for a class (parent classes).
    pub fn get_inheritance_chain(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode)> {
        let mut chain = Vec::new();
        let mut current = idx;
        let mut visited = std::collections::HashSet::new();
        visited.insert(current);

        loop {
            let parents: Vec<_> = self
                .outgoing_edges(current)
                .into_iter()
                .filter(|(_, kind)| matches!(kind, EdgeKind::Inherits))
                .collect();

            if let Some((parent_idx, _)) = parents.first() {
                if visited.contains(parent_idx) {
                    break; // Avoid infinite loops on circular inheritance
                }
                visited.insert(*parent_idx);
                if let Some(node) = self.get_node(*parent_idx) {
                    chain.push((*parent_idx, node));
                }
                current = *parent_idx;
            } else {
                break;
            }
        }
        chain
    }

    /// Get all nodes contained by `idx` (direct children via CONTAINS).
    pub fn get_children(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode)> {
        self.outgoing_edges(idx)
            .into_iter()
            .filter(|(_, kind)| matches!(kind, EdgeKind::Contains))
            .filter_map(|(target, _)| {
                self.get_node(target).map(|n| (target, n))
            })
            .collect()
    }

    /// Find functions that are never called by any other function (dead code candidates).
    pub fn find_dead_code(&self) -> Vec<(NodeIndex, &GraphNode)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if !matches!(node, GraphNode::Function(_)) {
                    return None;
                }
                // Check if anyone calls this function
                let callers = self.get_callers_of(idx);
                if callers.is_empty() {
                    Some((idx, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get the N most complex functions, sorted descending by cyclomatic complexity.
    pub fn most_complex_functions(&self, limit: usize) -> Vec<(NodeIndex, &GraphNode, u32)> {
        let mut funcs: Vec<_> = self
            .graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if let GraphNode::Function(f) = node {
                    Some((idx, node, f.cyclomatic_complexity))
                } else {
                    None
                }
            })
            .collect();
        funcs.sort_by(|a, b| b.2.cmp(&a.2));
        funcs.truncate(limit);
        funcs
    }

    /// Get all modules/imports that a file depends on.
    pub fn get_file_imports(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode, &EdgeKind)> {
        self.outgoing_edges(idx)
            .into_iter()
            .filter(|(_, kind)| matches!(kind, EdgeKind::Imports { .. }))
            .filter_map(|(target, kind)| {
                self.get_node(target).map(|n| (target, n, kind))
            })
            .collect()
    }

    /// Build a full call chain from a function (BFS traversal of CALLS edges).
    pub fn get_call_chain(&self, idx: NodeIndex, max_depth: usize) -> Vec<(NodeIndex, &GraphNode, usize)> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        visited.insert(idx);
        queue.push_back((idx, 0usize));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for (callee_idx, callee_node) in self.get_callees_of(current) {
                if visited.insert(callee_idx) {
                    result.push((callee_idx, callee_node, depth + 1));
                    queue.push_back((callee_idx, depth + 1));
                }
            }
        }
        result
    }

    /// BFS from `idx` following CALLS edges in reverse (callers of callers).
    /// Returns all transitive callers with their BFS depth.
    pub fn get_transitive_callers(&self, idx: NodeIndex, max_depth: usize) -> Vec<(NodeIndex, &GraphNode, usize)> {
        let mut result = Vec::new();
        let mut visited = std::collections::HashSet::new();
        let mut queue = std::collections::VecDeque::new();

        visited.insert(idx);
        queue.push_back((idx, 0usize));

        while let Some((current, depth)) = queue.pop_front() {
            if depth >= max_depth {
                continue;
            }
            for (caller_idx, caller_node) in self.get_callers_of(current) {
                if visited.insert(caller_idx) {
                    result.push((caller_idx, caller_node, depth + 1));
                    queue.push_back((caller_idx, depth + 1));
                }
            }
        }
        result
    }

    /// Get all classes/interfaces that implement or inherit from the node at `idx`.
    pub fn get_implementors(&self, idx: NodeIndex) -> Vec<(NodeIndex, &GraphNode)> {
        self.incoming_edges(idx)
            .into_iter()
            .filter(|(_, kind)| matches!(kind, EdgeKind::Inherits | EdgeKind::Implements))
            .filter_map(|(source, _)| {
                self.get_node(source).map(|n| (source, n))
            })
            .collect()
    }

    /// Find groups of potentially similar/redundant nodes.
    ///
    /// Requires the graph to have been built with `--annotate` so nodes have
    /// source snippets. Groups nodes of the same type that share significant
    /// structural similarity based on:
    /// - Line count within 30% of each other
    /// - Token overlap (identifier tokens extracted from source)
    ///
    /// Returns groups of 2+ similar nodes, sorted by group size descending.
    pub fn find_similar_nodes(
        &self,
        label_filter: Option<&str>,
        min_lines: usize,
    ) -> Vec<Vec<(NodeIndex, &GraphNode)>> {
        use std::collections::HashMap;

        // Collect annotated nodes with their source
        let candidates: Vec<(NodeIndex, &GraphNode, &str)> = self
            .graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if let Some(filter) = label_filter {
                    if node.label() != filter {
                        return None;
                    }
                }
                let src = node.source_snippet()?;
                if src.lines().count() < min_lines {
                    return None;
                }
                Some((idx, node, src))
            })
            .collect();

        if candidates.is_empty() {
            return Vec::new();
        }

        // Group by label first (only compare within same type)
        let mut by_label: HashMap<&str, Vec<(NodeIndex, &GraphNode, &str)>> = HashMap::new();
        for &(idx, node, src) in &candidates {
            by_label.entry(node.label()).or_default().push((idx, node, src));
        }

        let mut groups: Vec<Vec<(NodeIndex, &GraphNode)>> = Vec::new();

        for nodes in by_label.values() {
            if nodes.len() < 2 {
                continue;
            }

            // Extract tokens for each node
            let tokenized: Vec<TokenizedEntry<'_>> = nodes
                .iter()
                .map(|entry| {
                    let tokens = extract_tokens(entry.2);
                    (*entry, tokens)
                })
                .collect();

            // Compare all pairs
            let mut used = vec![false; tokenized.len()];
            for i in 0..tokenized.len() {
                if used[i] {
                    continue;
                }
                let mut group = vec![(tokenized[i].0 .0, tokenized[i].0 .1)];

                for j in (i + 1)..tokenized.len() {
                    if used[j] {
                        continue;
                    }
                    let sim = token_similarity(&tokenized[i].1, &tokenized[j].1);
                    let line_ratio = line_count_ratio(tokenized[i].0 .2, tokenized[j].0 .2);

                    // Similar if >40% token overlap AND within 50% line count
                    if sim > 0.4 && line_ratio > 0.5 {
                        group.push((tokenized[j].0 .0, tokenized[j].0 .1));
                        used[j] = true;
                    }
                }

                if group.len() >= 2 {
                    used[i] = true;
                    groups.push(group);
                }
            }
        }

        groups.sort_by_key(|b| std::cmp::Reverse(b.len()));
        groups
    }
}

/// Extract identifier-like tokens from source for similarity comparison.
fn extract_tokens(source: &str) -> Vec<&str> {
    source
        .split(|c: char| !c.is_alphanumeric() && c != '_')
        .filter(|t| t.len() > 2) // skip very short tokens
        .collect()
}

/// Jaccard similarity of two token sets.
fn token_similarity(a: &[&str], b: &[&str]) -> f64 {
    use std::collections::HashSet;
    let set_a: HashSet<&str> = a.iter().copied().collect();
    let set_b: HashSet<&str> = b.iter().copied().collect();
    let intersection = set_a.intersection(&set_b).count();
    let union = set_a.union(&set_b).count();
    if union == 0 {
        return 0.0;
    }
    intersection as f64 / union as f64
}

/// Ratio of line counts (smaller/larger), so 1.0 = same length.
fn line_count_ratio(a: &str, b: &str) -> f64 {
    let la = a.lines().count().max(1) as f64;
    let lb = b.lines().count().max(1) as f64;
    la.min(lb) / la.max(lb)
}

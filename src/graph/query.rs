use petgraph::graph::NodeIndex;

use super::code_graph::CodeGraph;
use super::structural;
use crate::types::node::GraphNode;
use crate::types::EdgeKind;

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
    ///
    /// Filters out functions that legitimately have no in-graph CALLS edge but are
    /// not dead: program entry points, test functions (invoked by the test runner),
    /// and trait/interface methods (reached via dynamic dispatch).
    ///
    /// Note: visibility is intentionally *not* used to filter here. Dynamic
    /// languages (Python, JS) mark essentially every function `public`, so
    /// excluding public functions would discard nearly all real dead code in
    /// those codebases.
    pub fn find_dead_code(&self) -> Vec<(NodeIndex, &GraphNode)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                let GraphNode::Function(f) = node else {
                    return None;
                };
                // Entry points are invoked by the runtime, not by call edges.
                if f.name == "main" || f.name == "_start" {
                    return None;
                }
                // Test functions are invoked by the test runner.
                if f.name.starts_with("test_")
                    || f.decorators.iter().any(|d| d.contains("test"))
                {
                    return None;
                }
                // Trait/interface methods are dispatched dynamically, so a missing
                // CALLS edge doesn't imply they're unused.
                if self.is_interface_method(idx) {
                    return None;
                }
                if self.get_callers_of(idx).is_empty() {
                    Some((idx, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// True if the function at `idx` is contained directly by a Trait or Interface
    /// node (i.e. it is dispatched dynamically rather than called directly).
    fn is_interface_method(&self, idx: NodeIndex) -> bool {
        self.incoming_edges(idx).into_iter().any(|(src, kind)| {
            matches!(kind, EdgeKind::Contains)
                && matches!(
                    self.get_node(src),
                    Some(GraphNode::Trait(_)) | Some(GraphNode::Interface(_))
                )
        })
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

    /// Pre-compute and cache a structural AST fingerprint for every annotated
    /// node, so similarity queries don't have to re-parse snippets each call.
    ///
    /// Called once after an annotated build. Each source-bearing node's snippet
    /// is parsed with tree-sitter and reduced to a sorted production histogram
    /// (see the [`structural`](crate::graph::structural) module). One parser is
    /// reused per language. Nodes without a snippet or grammar are skipped.
    pub fn compute_structural_fingerprints(&mut self) {
        use std::collections::HashMap;
        use crate::types::Language;

        let mut parsers: HashMap<Language, Option<tree_sitter::Parser>> = HashMap::new();
        // Collect first (immutable borrow of the graph), then insert.
        let mut computed: Vec<(usize, Vec<(u64, u32)>)> = Vec::new();
        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            let Some(src) = node.source_snippet() else {
                continue;
            };
            let Some(lang) = node_language(node) else {
                continue;
            };
            let Some(parser) = parsers
                .entry(lang)
                .or_insert_with(|| structural::parser_for(lang))
                .as_mut()
            else {
                continue;
            };
            if let Some(pairs) = structural::fingerprint_sorted(parser, src) {
                computed.push((idx.index(), pairs));
            }
        }
        for (i, pairs) in computed {
            self.fingerprints.insert(i, pairs);
        }
    }

    /// Find groups of potentially similar/redundant nodes.
    ///
    /// Requires the graph to have been built with `--annotate` so nodes have
    /// source snippets. Comparison is primarily **structural**: each snippet is
    /// re-parsed with tree-sitter and compared by AST shape (see the
    /// [`structural`](crate::graph::structural) module), so functions that are
    /// structurally identical but use different identifier names are still
    /// grouped. When a snippet can't be parsed (unsupported language or
    /// fragmentary source) it falls back to lexical token overlap.
    ///
    /// Two nodes of the same type are grouped when they share:
    /// - structural cosine similarity ≥ 0.75 (or, on the lexical fallback path,
    ///   token-overlap Jaccard > 0.4), AND
    /// - line count within ~40% of each other (a size sanity guard).
    ///
    /// Returns groups of 2+ similar nodes. The output is deterministic: groups
    /// are ordered by size descending, ties broken by the first node's index.
    pub fn find_similar_nodes(
        &self,
        label_filter: Option<&str>,
        min_lines: usize,
    ) -> Vec<Vec<(NodeIndex, &GraphNode)>> {
        use std::collections::HashMap;
        use crate::types::Language;

        // Collect annotated nodes with their source.
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

        // One structural fingerprint per candidate. For annotated graphs these
        // were computed once at index time (see `compute_structural_fingerprints`)
        // and are just looked up here. For older graphs without the side-table we
        // fall back to re-parsing the snippet on the fly, reusing one parser per
        // language. `None` means structural comparison is unavailable for that
        // node (no grammar / parse failure) → lexical fallback is used.
        let mut parsers: HashMap<Language, Option<tree_sitter::Parser>> = HashMap::new();
        let fingerprints: Vec<Option<structural::Fingerprint>> = candidates
            .iter()
            .map(|(idx, node, src)| {
                if let Some(pairs) = self.fingerprints.get(&idx.index()) {
                    return Some(structural::pairs_to_map(pairs));
                }
                let lang = node_language(node)?;
                let parser = parsers
                    .entry(lang)
                    .or_insert_with(|| structural::parser_for(lang))
                    .as_mut()?;
                structural::fingerprint_with(parser, src)
            })
            .collect();

        // Group candidate indices by label (only compare within the same type).
        let mut by_label: HashMap<&str, Vec<usize>> = HashMap::new();
        for (i, (_, node, _)) in candidates.iter().enumerate() {
            by_label.entry(node.label()).or_default().push(i);
        }

        let mut groups: Vec<Vec<(NodeIndex, &GraphNode)>> = Vec::new();

        for indices in by_label.values() {
            if indices.len() < 2 {
                continue;
            }

            // IDF weights over this label group's fingerprints, so that
            // ubiquitous grammar productions don't inflate similarity between
            // otherwise-unrelated nodes of the same type.
            let corpus: Vec<&structural::Fingerprint> = indices
                .iter()
                .filter_map(|&i| fingerprints[i].as_ref())
                .collect();
            let idf = structural::idf_weights(&corpus);

            // Greedy clustering: anchor on the first unused node, attach all
            // later nodes similar to it.
            let mut used = vec![false; indices.len()];
            for a in 0..indices.len() {
                if used[a] {
                    continue;
                }
                let ia = indices[a];
                let mut group = vec![(candidates[ia].0, candidates[ia].1)];

                for b in (a + 1)..indices.len() {
                    if used[b] {
                        continue;
                    }
                    let ib = indices[b];
                    if snippets_similar(&candidates, &fingerprints, &idf, ia, ib) {
                        group.push((candidates[ib].0, candidates[ib].1));
                        used[b] = true;
                    }
                }

                if group.len() >= 2 {
                    used[a] = true;
                    groups.push(group);
                }
            }
        }

        // Deterministic order: largest groups first, ties broken by the first
        // node's index so repeated runs produce byte-identical output.
        groups.sort_by_key(|g| {
            (
                std::cmp::Reverse(g.len()),
                g.first().map(|(i, _)| i.index()).unwrap_or(0),
            )
        });
        groups
    }
}

/// Minimum structural cosine similarity for two snippets to be grouped.
const STRUCTURAL_SIMILARITY_THRESHOLD: f64 = 0.75;

/// Resolve a node's language from its file path extension.
fn node_language(node: &GraphNode) -> Option<crate::types::Language> {
    let (path, _, _) = node.location();
    let ext = std::path::Path::new(&path).extension()?.to_str()?;
    crate::types::Language::from_extension(ext)
}

/// Decide whether candidates `i` and `j` are similar enough to group.
///
/// Uses structural (AST-shape) cosine similarity when both fingerprints are
/// available, otherwise falls back to lexical token overlap. Both paths apply a
/// line-count sanity guard so wildly different-sized snippets aren't grouped.
fn snippets_similar(
    candidates: &[(NodeIndex, &GraphNode, &str)],
    fingerprints: &[Option<structural::Fingerprint>],
    idf: &std::collections::HashMap<u64, f64>,
    i: usize,
    j: usize,
) -> bool {
    let src_i = candidates[i].2;
    let src_j = candidates[j].2;

    match (&fingerprints[i], &fingerprints[j]) {
        (Some(fp_i), Some(fp_j)) => {
            line_count_ratio(src_i, src_j) > 0.4
                && structural::weighted_cosine(fp_i, fp_j, idf) >= STRUCTURAL_SIMILARITY_THRESHOLD
        }
        _ => {
            // Lexical fallback (one or both snippets couldn't be parsed).
            let toks_i = extract_tokens(src_i);
            let toks_j = extract_tokens(src_j);
            token_similarity(&toks_i, &toks_j) > 0.4 && line_count_ratio(src_i, src_j) > 0.5
        }
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

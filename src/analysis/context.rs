use std::cell::OnceCell;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

use petgraph::graph::NodeIndex;

use crate::graph::structural;
use crate::graph::CodeGraph;
use crate::types::node::GraphNode;
use crate::types::{EdgeKind, Language};
use super::semantic::SemanticProvider;
use super::types::AnalysisConfig;

#[allow(dead_code)]
pub(crate) struct AnalysisContext<'a> {
    pub graph: &'a CodeGraph,
    pub config: &'a AnalysisConfig,

    /// Semantic resolver (types, references, receiver mutability). A
    /// [`NullProvider`](super::semantic::NullProvider) on the default path;
    /// checks consult it and fall back to heuristics when it answers `None`.
    pub semantic: &'a dyn SemanticProvider,

    // ── Node collections by type ───────────────────────────────────────
    pub functions: Vec<(NodeIndex, &'a GraphNode)>,
    pub classes: Vec<(NodeIndex, &'a GraphNode)>,
    pub files: Vec<(NodeIndex, &'a GraphNode)>,
    pub structs: Vec<(NodeIndex, &'a GraphNode)>,
    pub traits: Vec<(NodeIndex, &'a GraphNode)>,
    pub interfaces: Vec<(NodeIndex, &'a GraphNode)>,
    pub enums: Vec<(NodeIndex, &'a GraphNode)>,
    pub modules: Vec<(NodeIndex, &'a GraphNode)>,
    pub variables: Vec<(NodeIndex, &'a GraphNode)>,

    // ── Call graph (precomputed) ───────────────────────────────────────
    pub callers_map: HashMap<NodeIndex, Vec<NodeIndex>>,
    pub callees_map: HashMap<NodeIndex, Vec<NodeIndex>>,

    // ── Containment ────────────────────────────────────────────────────
    pub children_map: HashMap<NodeIndex, Vec<NodeIndex>>,
    pub parent_map: HashMap<NodeIndex, NodeIndex>,

    // ── Inheritance / implements ────────────────────────────────────────
    pub subclasses: HashMap<NodeIndex, Vec<NodeIndex>>,
    pub implementors: HashMap<NodeIndex, Vec<NodeIndex>>,

    // ── Tests ──────────────────────────────────────────────────────────
    pub has_test: HashSet<NodeIndex>,

    // ── Structural fingerprints (for false-positive reduction) ─────────
    /// AST-shape fingerprint per function node (only for annotated graphs).
    pub fn_fingerprints: HashMap<NodeIndex, structural::Fingerprint>,
    /// IDF weights over the function-fingerprint corpus, so ubiquitous grammar
    /// productions don't inflate structural similarity.
    pub fn_idf: HashMap<u64, f64>,

    /// Per-function source with string-literal and comment bytes blanked to
    /// spaces (newlines preserved), so keyword/substring pattern checks don't
    /// match text that only appears inside a string or comment. Built lazily on
    /// first [`masked_source`](Self::masked_source) call — filtered/scoped
    /// analyses that never run a pattern check pay nothing.
    masked_sources: OnceCell<HashMap<NodeIndex, String>>,

    /// When `config.scope_paths` is set, the set of nodes whose file path
    /// matches the scope. `None` means "no scope — everything is in scope".
    /// Lets the heavy O(n²) checks skip pair work that can't touch the scope.
    scope: Option<HashSet<NodeIndex>>,

    /// Absolute indexed root (the `Repository` node's canonicalized path). Node
    /// paths are stored relative to it; semantic (LSP) checks resolve them to
    /// absolute through [`resolve_path`](Self::resolve_path) so queries don't
    /// depend on the process working directory.
    root: Option<PathBuf>,
}

#[allow(dead_code)]
impl<'a> AnalysisContext<'a> {
    /// Build the precomputed context from a code graph. Single pass through nodes + edges.
    pub(super) fn build(
        graph: &'a CodeGraph,
        config: &'a AnalysisConfig,
        semantic: &'a dyn SemanticProvider,
    ) -> Self {
        use petgraph::visit::EdgeRef;

        let mut functions = Vec::new();
        let mut classes = Vec::new();
        let mut files = Vec::new();
        let mut structs = Vec::new();
        let mut traits = Vec::new();
        let mut interfaces = Vec::new();
        let mut enums = Vec::new();
        let mut modules = Vec::new();
        let mut variables = Vec::new();

        // Single pass: collect nodes by type
        for idx in graph.graph.node_indices() {
            let node = &graph.graph[idx];
            match node {
                GraphNode::Function(_) => functions.push((idx, node)),
                GraphNode::Class(_) => classes.push((idx, node)),
                GraphNode::File(_) => files.push((idx, node)),
                GraphNode::Struct(_) => structs.push((idx, node)),
                GraphNode::Trait(_) => traits.push((idx, node)),
                GraphNode::Interface(_) => interfaces.push((idx, node)),
                GraphNode::Enum(_) => enums.push((idx, node)),
                GraphNode::Module(_) => modules.push((idx, node)),
                GraphNode::Variable(_) => variables.push((idx, node)),
                _ => {}
            }
        }

        // Single pass: build edge maps
        let mut callers_map: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        let mut callees_map: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        let mut children_map: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        let mut parent_map: HashMap<NodeIndex, NodeIndex> = HashMap::new();
        let mut subclasses: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        let mut implementors: HashMap<NodeIndex, Vec<NodeIndex>> = HashMap::new();
        let mut has_test: HashSet<NodeIndex> = HashSet::new();

        for edge in graph.graph.edge_references() {
            let src = edge.source();
            let tgt = edge.target();
            match edge.weight() {
                EdgeKind::Calls { .. } => {
                    callees_map.entry(src).or_default().push(tgt);
                    callers_map.entry(tgt).or_default().push(src);
                }
                EdgeKind::Contains => {
                    children_map.entry(src).or_default().push(tgt);
                    parent_map.insert(tgt, src);
                }
                EdgeKind::Inherits => {
                    subclasses.entry(tgt).or_default().push(src);
                }
                EdgeKind::Implements => {
                    implementors.entry(tgt).or_default().push(src);
                }
                EdgeKind::Tests => {
                    has_test.insert(tgt);
                }
                _ => {}
            }
        }

        // Load structural fingerprints for functions (annotated graphs only) and
        // compute IDF weights over them. Used to confirm lexical similarity
        // findings against actual AST shape, cutting false positives.
        let mut fn_fingerprints: HashMap<NodeIndex, structural::Fingerprint> = HashMap::new();
        for &(idx, _) in &functions {
            if let Some(pairs) = graph.fingerprint_pairs(idx) {
                fn_fingerprints.insert(idx, structural::pairs_to_map(pairs));
            }
        }
        let corpus: Vec<&structural::Fingerprint> = fn_fingerprints.values().collect();
        let fn_idf = structural::idf_weights(&corpus);

        // The graph records the canonicalized absolute root; node paths are
        // relative to it.
        let root = graph.root.clone();

        // Precompute the in-scope node set when a path scope is requested. A node
        // is in scope if its file path contains any of the scope substrings.
        let scope = config.scope_paths.as_ref().map(|paths| {
            graph
                .graph
                .node_indices()
                .filter(|&idx| {
                    let p = graph.graph[idx].location().0;
                    paths.iter().any(|s| p.contains(s.as_str()))
                })
                .collect::<HashSet<NodeIndex>>()
        });

        Self {
            graph,
            config,
            semantic,
            functions,
            classes,
            files,
            structs,
            traits,
            interfaces,
            enums,
            modules,
            variables,
            callers_map,
            callees_map,
            children_map,
            parent_map,
            subclasses,
            implementors,
            has_test,
            fn_fingerprints,
            fn_idf,
            masked_sources: OnceCell::new(),
            scope,
            root,
        }
    }

    /// Resolve a (possibly root-relative) node path to an absolute path, so
    /// language-server queries and file reads don't depend on the process CWD.
    /// Returns the input unchanged when it's already absolute or the root is
    /// unknown.
    pub fn resolve_path(&self, path: &Path) -> PathBuf {
        if path.is_absolute() {
            return path.to_path_buf();
        }
        match &self.root {
            Some(r) => r.join(path),
            None => path.to_path_buf(),
        }
    }

    /// Whether a path scope is active for this analysis.
    pub fn has_scope(&self) -> bool {
        self.scope.is_some()
    }

    /// Whether `idx` is within the active scope. Always `true` when no scope is
    /// set, so unscoped analyses treat every node as in scope.
    pub fn in_scope(&self, idx: NodeIndex) -> bool {
        self.scope.as_ref().is_none_or(|s| s.contains(&idx))
    }

    /// A function's source with string-literal and comment content masked out
    /// (replaced by spaces, newlines kept so line/column offsets are unchanged).
    ///
    /// Pattern checks that scan source text for keywords (`time.sleep(`,
    /// `requests.get(`, …) should read this instead of `func.source` so a match
    /// inside a string literal or comment doesn't produce a false positive. The
    /// whole map is built (one tree-sitter parse per function) on the first call
    /// and cached for the lifetime of the context. Returns `None` only when the
    /// node has no annotated source (same as reading `func.source`).
    ///
    /// Do NOT use this for checks that legitimately inspect literal content
    /// (e.g. hardcoded-endpoint / env-var detection) — they need the raw source.
    pub fn masked_source(&self, idx: NodeIndex) -> Option<&str> {
        let functions = &self.functions;
        self.masked_sources
            .get_or_init(|| build_masked_map(functions))
            .get(&idx)
            .map(|s| s.as_str())
    }

    /// IDF-weighted structural (AST-shape) cosine similarity between two function
    /// nodes, in `[0.0, 1.0]`. Returns `None` when either node has no stored
    /// fingerprint (e.g. a graph built before fingerprints existed), so callers
    /// can fall back to their lexical decision rather than dropping a finding.
    pub fn structural_cosine(&self, a: NodeIndex, b: NodeIndex) -> Option<f64> {
        let fa = self.fn_fingerprints.get(&a)?;
        let fb = self.fn_fingerprints.get(&b)?;
        Some(structural::weighted_cosine(fa, fb, &self.fn_idf))
    }

    // ── Convenience accessors ──────────────────────────────────────────

    /// Get callers as (NodeIndex, &GraphNode) pairs (mirrors CodeGraph::get_callers_of).
    pub fn get_callers_of(&self, idx: NodeIndex) -> Vec<(NodeIndex, &'a GraphNode)> {
        self.callers_map
            .get(&idx)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&i| self.graph.get_node(i).map(|n| (i, n)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get callees as (NodeIndex, &GraphNode) pairs (mirrors CodeGraph::get_callees_of).
    pub fn get_callees_of(&self, idx: NodeIndex) -> Vec<(NodeIndex, &'a GraphNode)> {
        self.callees_map
            .get(&idx)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&i| self.graph.get_node(i).map(|n| (i, n)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get children as (NodeIndex, &GraphNode) pairs (mirrors CodeGraph::get_children).
    pub fn get_children(&self, idx: NodeIndex) -> Vec<(NodeIndex, &'a GraphNode)> {
        self.children_map
            .get(&idx)
            .map(|indices| {
                indices
                    .iter()
                    .filter_map(|&i| self.graph.get_node(i).map(|n| (i, n)))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get caller indices as a slice (zero allocation).
    pub fn caller_indices(&self, idx: NodeIndex) -> &[NodeIndex] {
        self.callers_map.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get callee indices as a slice (zero allocation).
    pub fn callee_indices(&self, idx: NodeIndex) -> &[NodeIndex] {
        self.callees_map.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Get children indices as a slice (zero allocation).
    pub fn children_indices(&self, idx: NodeIndex) -> &[NodeIndex] {
        self.children_map.get(&idx).map(|v| v.as_slice()).unwrap_or(&[])
    }

    /// Number of callers for a function.
    pub fn caller_count(&self, idx: NodeIndex) -> usize {
        self.callers_map.get(&idx).map(|v| v.len()).unwrap_or(0)
    }

    /// Number of callees for a function.
    pub fn callee_count(&self, idx: NodeIndex) -> usize {
        self.callees_map.get(&idx).map(|v| v.len()).unwrap_or(0)
    }

    /// Parent node (via CONTAINS edge).
    pub fn parent_of(&self, idx: NodeIndex) -> Option<NodeIndex> {
        self.parent_map.get(&idx).copied()
    }

    /// Whether this function has test coverage (Tests edge or caller from test).
    pub fn has_test_coverage(&self, idx: NodeIndex) -> bool {
        self.has_test.contains(&idx)
    }
}

/// Build the masked-source map for every function node that has annotated
/// source. One tree-sitter parser is created per language and reused across all
/// that language's functions.
fn build_masked_map(functions: &[(NodeIndex, &GraphNode)]) -> HashMap<NodeIndex, String> {
    let mut parsers: HashMap<Language, Option<tree_sitter::Parser>> = HashMap::new();
    let mut out = HashMap::new();
    for &(idx, node) in functions {
        let func = match node {
            GraphNode::Function(f) => f,
            _ => continue,
        };
        let src = match &func.source {
            Some(s) => s,
            None => continue,
        };
        let lang = func.language;
        let parser = parsers
            .entry(lang)
            .or_insert_with(|| structural::parser_for(lang));
        let masked = match parser {
            Some(p) => mask_strings_and_comments(p, src),
            // No grammar available — leave the source unchanged.
            None => src.clone(),
        };
        out.insert(idx, masked);
    }
    out
}

/// Replace the bytes of every string-literal and comment node in `source` with
/// spaces (newlines preserved), so the result has identical length and line
/// structure but no keyword text hiding inside strings/comments. Returns the
/// source unchanged if it can't be parsed.
fn mask_strings_and_comments(parser: &mut tree_sitter::Parser, source: &str) -> String {
    let tree = match parser.parse(source.as_bytes(), None) {
        Some(t) => t,
        None => return source.to_string(),
    };
    let mut bytes = source.as_bytes().to_vec();
    let mut stack = vec![tree.root_node()];
    while let Some(n) = stack.pop() {
        if is_masked_kind(n.kind()) {
            // Blank the whole node span; don't recurse (an interpolation's
            // embedded code is rare and masking it errs toward precision).
            for b in &mut bytes[n.byte_range()] {
                if *b != b'\n' {
                    *b = b' ';
                }
            }
            continue;
        }
        let mut c = n.walk();
        for child in n.children(&mut c) {
            stack.push(child);
        }
    }
    // Spaces and preserved newlines are all single-byte, so the result is still
    // valid UTF-8; fall back to the original on the (impossible) error path.
    String::from_utf8(bytes).unwrap_or_else(|_| source.to_string())
}

/// Whether a tree-sitter node kind denotes a string literal, comment, char
/// literal, or heredoc across the supported grammars (kinds vary by language:
/// `string`, `string_literal`, `interpreted_string_literal`, `line_comment`,
/// `block_comment`, `character_literal`, `heredoc_body`, …).
fn is_masked_kind(kind: &str) -> bool {
    kind.contains("string")
        || kind.contains("comment")
        || kind.contains("char_literal")
        || kind.contains("character")
        || kind.contains("heredoc")
}

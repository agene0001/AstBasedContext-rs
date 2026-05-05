use std::collections::{HashMap, HashSet};

use petgraph::graph::NodeIndex;

use crate::graph::CodeGraph;
use crate::types::node::GraphNode;
use crate::types::EdgeKind;
use super::types::AnalysisConfig;

#[allow(dead_code)]
pub(crate) struct AnalysisContext<'a> {
    pub graph: &'a CodeGraph,
    pub config: &'a AnalysisConfig,

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

}

#[allow(dead_code)]
impl<'a> AnalysisContext<'a> {
    /// Build the precomputed context from a code graph. Single pass through nodes + edges.
    pub(super) fn build(graph: &'a CodeGraph, config: &'a AnalysisConfig) -> Self {
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

        Self {
            graph,
            config,
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
        }
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

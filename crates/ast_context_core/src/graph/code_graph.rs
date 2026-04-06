use std::collections::HashMap;
use std::path::PathBuf;

use petgraph::graph::{DiGraph, NodeIndex};
use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

use crate::types::node::GraphNode;
use crate::types::EdgeKind;

/// An in-memory directed code graph backed by petgraph.
#[derive(Debug, Serialize, Deserialize)]
pub struct CodeGraph {
    pub graph: DiGraph<GraphNode, EdgeKind>,
    /// Maps file/dir/repo paths → node indices for O(1) lookup.
    #[serde(skip)]
    path_index: HashMap<PathBuf, Vec<NodeIndex>>,
    /// Maps "label\0name" → node indices for fast name lookups.
    /// Uses a single string key to allow zero-allocation lookups via `find_by_name`.
    #[serde(skip)]
    name_index: HashMap<String, Vec<NodeIndex>>,
}

impl CodeGraph {
    pub fn new() -> Self {
        Self {
            graph: DiGraph::new(),
            path_index: HashMap::new(),
            name_index: HashMap::new(),
        }
    }

    /// Add a node and return its index.
    pub fn add_node(&mut self, node: GraphNode) -> NodeIndex {
        let label = node.label().to_string();
        let name = node.name().to_string();

        // Extract path for indexing
        let path = match &node {
            GraphNode::Repository(d) => Some(d.path.clone()),
            GraphNode::Directory(d) => Some(d.path.clone()),
            GraphNode::File(d) => Some(d.path.clone()),
            GraphNode::Function(d) => Some(d.path.clone()),
            GraphNode::Class(d) => Some(d.path.clone()),
            GraphNode::Variable(d) => Some(d.path.clone()),
            GraphNode::Module(_) => None,
            GraphNode::Trait(d) => Some(d.path.clone()),
            GraphNode::Interface(d) => Some(d.path.clone()),
            GraphNode::Struct(d) => Some(d.path.clone()),
            GraphNode::Enum(d) => Some(d.path.clone()),
            GraphNode::Macro(d) => Some(d.path.clone()),
        };

        let idx = self.graph.add_node(node);

        if let Some(p) = path {
            self.path_index.entry(p).or_default().push(idx);
        }
        let key = format!("{}\0{}", label, name);
        self.name_index.entry(key).or_default().push(idx);

        idx
    }

    /// Add a directed edge.
    pub fn add_edge(&mut self, from: NodeIndex, to: NodeIndex, kind: EdgeKind) {
        self.graph.add_edge(from, to, kind);
    }

    /// Find nodes by label and name.
    pub fn find_by_name(&self, label: &str, name: &str) -> &[NodeIndex] {
        let key = format!("{}\0{}", label, name);
        self.name_index
            .get(&key)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Find all nodes associated with a path.
    pub fn find_by_path(&self, path: &PathBuf) -> &[NodeIndex] {
        self.path_index
            .get(path)
            .map(|v| v.as_slice())
            .unwrap_or(&[])
    }

    /// Find functions by name.
    pub fn find_functions(&self, name: &str) -> Vec<NodeIndex> {
        self.find_by_name("Function", name).to_vec()
    }

    /// Find classes by name.
    pub fn find_classes(&self, name: &str) -> Vec<NodeIndex> {
        self.find_by_name("Class", name).to_vec()
    }

    /// Get the GraphNode at an index.
    pub fn get_node(&self, idx: NodeIndex) -> Option<&GraphNode> {
        self.graph.node_weight(idx)
    }

    /// Count nodes by label.
    pub fn node_count(&self) -> usize {
        self.graph.node_count()
    }

    /// Count edges.
    pub fn edge_count(&self) -> usize {
        self.graph.edge_count()
    }

    /// Return all edges from a node.
    pub fn outgoing_edges(
        &self,
        idx: NodeIndex,
    ) -> Vec<(NodeIndex, &EdgeKind)> {
        self.graph
            .edges(idx)
            .map(|e| (e.target(), e.weight()))
            .collect()
    }

    /// Return all edges pointing to a node.
    pub fn incoming_edges(
        &self,
        idx: NodeIndex,
    ) -> Vec<(NodeIndex, &EdgeKind)> {
        use petgraph::Direction;
        self.graph
            .edges_directed(idx, Direction::Incoming)
            .map(|e| (e.source(), e.weight()))
            .collect()
    }
}

impl CodeGraph {
    /// Rebuild the in-memory indexes after deserialization.
    pub fn rebuild_indexes(&mut self) {
        self.path_index.clear();
        self.name_index.clear();

        for idx in self.graph.node_indices() {
            let node = &self.graph[idx];
            let label = node.label().to_string();
            let name = node.name().to_string();

            let path = match node {
                GraphNode::Repository(d) => Some(d.path.clone()),
                GraphNode::Directory(d) => Some(d.path.clone()),
                GraphNode::File(d) => Some(d.path.clone()),
                GraphNode::Function(d) => Some(d.path.clone()),
                GraphNode::Class(d) => Some(d.path.clone()),
                GraphNode::Variable(d) => Some(d.path.clone()),
                GraphNode::Module(_) => None,
                GraphNode::Trait(d) => Some(d.path.clone()),
                GraphNode::Interface(d) => Some(d.path.clone()),
                GraphNode::Struct(d) => Some(d.path.clone()),
                GraphNode::Enum(d) => Some(d.path.clone()),
                GraphNode::Macro(d) => Some(d.path.clone()),
            };

            if let Some(p) = path {
                self.path_index.entry(p).or_default().push(idx);
            }
            let key = format!("{}\0{}", label, name);
            self.name_index.entry(key).or_default().push(idx);
        }
    }

    /// Save the graph to a JSON file.
    pub fn save(&self, path: &std::path::Path) -> crate::error::Result<()> {
        let file = std::fs::File::create(path).map_err(|e| crate::error::Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        serde_json::to_writer(std::io::BufWriter::new(file), self)
            .map_err(|e| crate::error::Error::Graph(format!("JSON write error: {e}")))?;
        Ok(())
    }

    /// Load a graph from a JSON file.
    pub fn load(path: &std::path::Path) -> crate::error::Result<Self> {
        let file = std::fs::File::open(path).map_err(|e| crate::error::Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut graph: CodeGraph = serde_json::from_reader(std::io::BufReader::new(file))
            .map_err(|e| crate::error::Error::Graph(format!("JSON read error: {e}")))?;
        graph.rebuild_indexes();
        Ok(graph)
    }

    /// Search all nodes whose name contains the query (case-insensitive).
    pub fn search_by_name(&self, query: &str) -> Vec<(NodeIndex, &GraphNode)> {
        let q = query.to_lowercase();
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if node.name().to_lowercase().contains(&q) {
                    Some((idx, node))
                } else {
                    None
                }
            })
            .collect()
    }

    /// Get all node indices with a specific label.
    pub fn nodes_by_label(&self, label: &str) -> Vec<(NodeIndex, &GraphNode)> {
        self.graph
            .node_indices()
            .filter_map(|idx| {
                let node = &self.graph[idx];
                if node.label() == label {
                    Some((idx, node))
                } else {
                    None
                }
            })
            .collect()
    }
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

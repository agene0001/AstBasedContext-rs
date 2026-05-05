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

    /// Current graph file format version. Increment when the serialized schema changes.
    pub const FORMAT_VERSION: u32 = 1;

    /// Save the graph to a JSON file with a version envelope and config fingerprint.
    pub fn save(&self, path: &std::path::Path) -> crate::error::Result<()> {
        self.save_with_config(path, false, &[])
    }

    /// Save the graph including the config options it was built with.
    /// The config fingerprint is used on load to detect stale caches.
    pub fn save_with_config(
        &self,
        path: &std::path::Path,
        annotate: bool,
        exclude: &[String],
    ) -> crate::error::Result<()> {
        let file = std::fs::File::create(path).map_err(|e| crate::error::Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let mut sorted_exclude = exclude.to_vec();
        sorted_exclude.sort();
        let envelope = serde_json::json!({
            "version": Self::FORMAT_VERSION,
            "config": {
                "annotate": annotate,
                "exclude": sorted_exclude,
            },
            "graph": self,
        });
        serde_json::to_writer(std::io::BufWriter::new(file), &envelope)
            .map_err(|e| crate::error::Error::Graph(format!("JSON write error: {e}")))?;
        Ok(())
    }

    /// Load a graph from a JSON file, checking the version envelope.
    pub fn load(path: &std::path::Path) -> crate::error::Result<Self> {
        Self::load_with_config(path, None, None)
    }

    /// Load a graph, optionally validating the config fingerprint.
    ///
    /// If `expected_annotate` or `expected_exclude` are `Some`, the cache is
    /// rejected (returns `Err`) if the stored config does not match.
    pub fn load_with_config(
        path: &std::path::Path,
        expected_annotate: Option<bool>,
        expected_exclude: Option<&[String]>,
    ) -> crate::error::Result<Self> {
        let file = std::fs::File::open(path).map_err(|e| crate::error::Error::Io {
            path: path.to_path_buf(),
            source: e,
        })?;
        let raw: serde_json::Value =
            serde_json::from_reader(std::io::BufReader::new(file))
                .map_err(|e| crate::error::Error::Graph(format!("JSON read error: {e}")))?;

        // Check version field — graphs saved before versioning had no "version" key,
        // so treat missing version as 0 (incompatible).
        let file_version = raw.get("version").and_then(|v| v.as_u64()).unwrap_or(0) as u32;
        if file_version != Self::FORMAT_VERSION {
            return Err(crate::error::Error::Graph(format!(
                "Graph file version mismatch: file is v{file_version}, tool expects v{}. \
                 Re-index your project to regenerate the graph.",
                Self::FORMAT_VERSION
            )));
        }

        // Validate config fingerprint if requested.
        if let Some(cfg) = raw.get("config") {
            if let Some(want_annotate) = expected_annotate {
                let cached_annotate = cfg.get("annotate").and_then(|v| v.as_bool()).unwrap_or(false);
                if cached_annotate != want_annotate {
                    return Err(crate::error::Error::Graph(format!(
                        "Cache config mismatch: cache has annotate={cached_annotate}, \
                         requested annotate={want_annotate}. Re-indexing."
                    )));
                }
            }
            if let Some(want_exclude) = expected_exclude {
                let cached_exclude: Vec<String> = cfg
                    .get("exclude")
                    .and_then(|v| v.as_array())
                    .map(|arr| arr.iter().filter_map(|v| v.as_str().map(|s| s.to_string())).collect())
                    .unwrap_or_default();
                let mut want_sorted = want_exclude.to_vec();
                want_sorted.sort();
                if cached_exclude != want_sorted {
                    return Err(crate::error::Error::Graph(
                        "Cache config mismatch: exclude patterns changed. Re-indexing.".into(),
                    ));
                }
            }
        } else if expected_annotate == Some(true) {
            // Old cache with no config field, but caller wants annotations.
            return Err(crate::error::Error::Graph(
                "Cache has no config fingerprint and annotate=true was requested. Re-indexing.".into(),
            ));
        }

        let graph_value = raw.get("graph").ok_or_else(|| {
            crate::error::Error::Graph("Graph file is missing 'graph' field".into())
        })?;

        let mut graph: CodeGraph = serde_json::from_value(graph_value.clone())
            .map_err(|e| crate::error::Error::Graph(format!("JSON decode error: {e}")))?;
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

    /// Check if the graph was built with source annotations.
    pub fn has_annotations(&self) -> bool {
        self.graph.node_indices().any(|idx| {
            self.graph[idx].source_snippet().is_some()
        })
    }
}

impl Default for CodeGraph {
    fn default() -> Self {
        Self::new()
    }
}

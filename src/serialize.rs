use std::io::{BufWriter, Write};
use std::path::Path;

use crate::error::{Error, Result};
use crate::graph::CodeGraph;

use petgraph::visit::EdgeRef;
use serde::{Deserialize, Serialize};

/// A serializable node entry for JSONL export.
#[derive(Serialize, Deserialize)]
struct NodeEntry {
    id: usize,
    label: String,
    name: String,
    data: serde_json::Value,
}

/// A serializable edge entry for JSONL export.
#[derive(Serialize, Deserialize)]
struct EdgeEntry {
    source: usize,
    target: usize,
    label: String,
    data: serde_json::Value,
}

/// Export the graph as two JSONL files: nodes.jsonl and edges.jsonl.
pub fn export_jsonl(graph: &CodeGraph, output_dir: &Path) -> Result<()> {
    std::fs::create_dir_all(output_dir).map_err(|e| Error::Io {
        path: output_dir.to_path_buf(),
        source: e,
    })?;

    // Export nodes
    let nodes_path = output_dir.join("nodes.jsonl");
    let nodes_file = std::fs::File::create(&nodes_path).map_err(|e| Error::Io {
        path: nodes_path.clone(),
        source: e,
    })?;
    let mut nodes_writer = BufWriter::new(nodes_file);

    for idx in graph.graph.node_indices() {
        let node = &graph.graph[idx];
        let entry = NodeEntry {
            id: idx.index(),
            label: node.label().to_string(),
            name: node.name().to_string(),
            data: serde_json::to_value(node).unwrap_or_default(),
        };
        serde_json::to_writer(&mut nodes_writer, &entry)
            .map_err(|e| Error::Graph(format!("JSON write error: {e}")))?;
        nodes_writer
            .write_all(b"\n")
            .map_err(|e| Error::Io {
                path: nodes_path.clone(),
                source: e,
            })?;
    }

    // Export edges
    let edges_path = output_dir.join("edges.jsonl");
    let edges_file = std::fs::File::create(&edges_path).map_err(|e| Error::Io {
        path: edges_path.clone(),
        source: e,
    })?;
    let mut edges_writer = BufWriter::new(edges_file);

    for edge in graph.graph.edge_references() {
        let entry = EdgeEntry {
            source: edge.source().index(),
            target: edge.target().index(),
            label: edge.weight().label().to_string(),
            data: serde_json::to_value(edge.weight()).unwrap_or_default(),
        };
        serde_json::to_writer(&mut edges_writer, &entry)
            .map_err(|e| Error::Graph(format!("JSON write error: {e}")))?;
        edges_writer
            .write_all(b"\n")
            .map_err(|e| Error::Io {
                path: edges_path.clone(),
                source: e,
            })?;
    }

    Ok(())
}

/// Export the full graph as a single JSON file.
pub fn export_json(graph: &CodeGraph, output_path: &Path) -> Result<()> {
    #[derive(Serialize)]
    struct FullGraph {
        nodes: Vec<NodeEntry>,
        edges: Vec<EdgeEntry>,
    }

    let nodes: Vec<NodeEntry> = graph
        .graph
        .node_indices()
        .map(|idx| {
            let node = &graph.graph[idx];
            NodeEntry {
                id: idx.index(),
                label: node.label().to_string(),
                name: node.name().to_string(),
                data: serde_json::to_value(node).unwrap_or_default(),
            }
        })
        .collect();

    let edges: Vec<EdgeEntry> = graph
        .graph
        .edge_references()
        .map(|edge| EdgeEntry {
            source: edge.source().index(),
            target: edge.target().index(),
            label: edge.weight().label().to_string(),
            data: serde_json::to_value(edge.weight()).unwrap_or_default(),
        })
        .collect();

    let full = FullGraph { nodes, edges };
    let file = std::fs::File::create(output_path).map_err(|e| Error::Io {
        path: output_path.to_path_buf(),
        source: e,
    })?;
    serde_json::to_writer_pretty(file, &full)
        .map_err(|e| Error::Graph(format!("JSON write error: {e}")))?;

    Ok(())
}

/// Print graph statistics to stdout.
pub fn print_stats(graph: &CodeGraph) {
    let mut counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for idx in graph.graph.node_indices() {
        let label = graph.graph[idx].label();
        *counts.entry(label).or_default() += 1;
    }

    let mut edge_counts: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
    for edge in graph.graph.edge_references() {
        let label = edge.weight().label();
        *edge_counts.entry(label).or_default() += 1;
    }

    println!("Graph Statistics:");
    println!("  Total nodes: {}", graph.node_count());
    println!("  Total edges: {}", graph.edge_count());
    println!();
    println!("  Nodes by type:");
    let mut sorted: Vec<_> = counts.into_iter().collect();
    sorted.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (label, count) in &sorted {
        println!("    {label}: {count}");
    }
    println!();
    println!("  Edges by type:");
    let mut sorted: Vec<_> = edge_counts.into_iter().collect();
    sorted.sort_by_key(|(_, c)| std::cmp::Reverse(*c));
    for (label, count) in &sorted {
        println!("    {label}: {count}");
    }
}

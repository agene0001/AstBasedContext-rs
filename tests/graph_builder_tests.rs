use std::path::PathBuf;

use ast_context::graph::GraphBuilder;
use ast_context::types::EdgeKind;
use ast_context::types::node::GraphNode;

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures/python")
}

#[test]
fn test_build_graph_from_fixtures() {
    let path = fixtures_path();
    if !path.exists() {
        panic!("Fixtures not found at {}", path.display());
    }

    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    // Should have nodes
    assert!(graph.node_count() > 0, "graph should have nodes");
    assert!(graph.edge_count() > 0, "graph should have edges");

    // Check for specific functions
    let hello_fns = graph.find_functions("hello");
    assert!(!hello_fns.is_empty(), "should find 'hello' function");

    let add_fns = graph.find_functions("add");
    assert!(!add_fns.is_empty(), "should find 'add' function");

    // Check for classes
    let greeter = graph.find_classes("Greeter");
    assert!(!greeter.is_empty(), "should find 'Greeter' class");

    let formal = graph.find_classes("FormalGreeter");
    assert!(!formal.is_empty(), "should find 'FormalGreeter' class");

    // Check repository node exists
    let repos: Vec<_> = graph
        .graph
        .node_indices()
        .filter(|&idx| matches!(graph.get_node(idx), Some(GraphNode::Repository(_))))
        .collect();
    assert_eq!(repos.len(), 1);

    // Check file nodes
    let files: Vec<_> = graph
        .graph
        .node_indices()
        .filter(|&idx| matches!(graph.get_node(idx), Some(GraphNode::File(_))))
        .collect();
    assert!(files.len() >= 2, "should have at least 2 file nodes");
}

#[test]
fn test_contains_edges() {
    let path = fixtures_path();
    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    // Find a file node and check it has CONTAINS edges to its functions
    let files: Vec<_> = graph
        .graph
        .node_indices()
        .filter(|&idx| {
            if let Some(GraphNode::File(f)) = graph.get_node(idx) {
                f.name == "simple.py"
            } else {
                false
            }
        })
        .collect();
    assert_eq!(files.len(), 1);

    let children = graph.get_children(files[0]);
    let child_names: Vec<&str> = children.iter().map(|(_, n)| n.name()).collect();
    assert!(child_names.contains(&"hello"), "simple.py should contain 'hello'");
    assert!(child_names.contains(&"add"), "simple.py should contain 'add'");
    assert!(child_names.contains(&"Greeter"), "simple.py should contain 'Greeter'");
}

#[test]
fn test_inheritance_edges() {
    let path = fixtures_path();
    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    let formal = graph.find_classes("FormalGreeter");
    assert!(!formal.is_empty());

    let chain = graph.get_inheritance_chain(formal[0]);
    let parent_names: Vec<&str> = chain.iter().map(|(_, n)| n.name()).collect();
    assert!(
        parent_names.contains(&"Greeter"),
        "FormalGreeter should inherit from Greeter"
    );
}

#[test]
fn test_import_edges() {
    let path = fixtures_path();
    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    // calls_and_imports.py imports from simple
    let files: Vec<_> = graph
        .graph
        .node_indices()
        .filter(|&idx| {
            if let Some(GraphNode::File(f)) = graph.get_node(idx) {
                f.name == "calls_and_imports.py"
            } else {
                false
            }
        })
        .collect();
    assert_eq!(files.len(), 1);

    let edges = graph.outgoing_edges(files[0]);
    let import_edges: Vec<_> = edges
        .iter()
        .filter(|(_, kind)| matches!(kind, EdgeKind::Imports { .. }))
        .collect();
    assert!(
        !import_edges.is_empty(),
        "calls_and_imports.py should have IMPORTS edges"
    );
}

#[test]
fn test_module_nodes() {
    let path = fixtures_path();
    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    let modules: Vec<_> = graph
        .graph
        .node_indices()
        .filter(|&idx| matches!(graph.get_node(idx), Some(GraphNode::Module(_))))
        .collect();
    assert!(!modules.is_empty(), "should have Module nodes for imports");
}

#[test]
fn test_calls_edges() {
    let path = fixtures_path();
    let graph = GraphBuilder::build(&path).expect("graph build should succeed");

    // The 'main' function in calls_and_imports.py calls 'hello' and 'add'
    let main_fns = graph.find_functions("main");
    assert!(!main_fns.is_empty(), "should find 'main' function");

    let callees = graph.get_callees_of(main_fns[0]);
    let callee_names: Vec<&str> = callees.iter().map(|(_, n)| n.name()).collect();

    // main() calls hello(), add(), Greeter(), and print()
    assert!(
        callee_names.contains(&"hello") || callee_names.contains(&"add"),
        "main should call hello or add, got: {:?}",
        callee_names
    );
}

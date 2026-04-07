use std::path::PathBuf;

fn main() {
    // Dart grammar (vendored — crates.io version targets an incompatible tree-sitter version).
    // The grammars/dart/ directory includes:
    //   parser.c, scanner.c   — generated grammar C files
    //   tree_sitter/parser.h  — bundled tree-sitter C header (same as other grammar crates do)
    let dart_dir = PathBuf::from("grammars/dart");

    cc::Build::new()
        .include(&dart_dir)
        .file(dart_dir.join("parser.c"))
        .warnings(false)
        .compile("tree-sitter-dart-parser");

    cc::Build::new()
        .include(&dart_dir)
        .file(dart_dir.join("scanner.c"))
        .warnings(false)
        .compile("tree-sitter-dart-scanner");

    println!("cargo:rerun-if-changed=grammars/dart/parser.c");
    println!("cargo:rerun-if-changed=grammars/dart/scanner.c");
    println!("cargo:rerun-if-changed=grammars/dart/tree_sitter/parser.h");
}

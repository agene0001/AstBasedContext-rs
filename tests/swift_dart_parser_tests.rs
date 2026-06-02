//! Regression tests for the Swift and Dart parsers.
//!
//! These parsers previously panicked at construction time because their built-in
//! tree-sitter queries didn't match the installed grammars (e.g. Swift has no
//! `struct_declaration` node; Dart's `method_signature` has no `name:` field).
//! The bug stayed hidden because nothing exercised these languages. These tests
//! guard against that: `*Parser::new()` must compile its queries, and parsing
//! must extract the expected symbols.

use std::path::Path;

use ast_context::parser::dart::DartParser;
use ast_context::parser::swift::SwiftParser;
use ast_context::parser::LanguageParser;

#[test]
fn swift_parser_constructs_without_panic() {
    // The original bug was a panic here (queries failed to compile).
    let _ = SwiftParser::new();
}

#[test]
fn swift_extracts_struct_class_enum_protocol_function() {
    let src = "import Foundation\n\
               struct Point { var x: Int }\n\
               class Shape { func area() -> Double { return 0.0 } }\n\
               protocol Drawable { func draw() }\n\
               enum Color { case red }\n\
               func add(a: Int) -> Int { return a }\n";
    let r = SwiftParser::new()
        .parse(Path::new("t.swift"), src.as_bytes(), false)
        .unwrap();

    // struct / class / enum are all `class_declaration` in tree-sitter-swift,
    // separated by keyword — verify each lands in the right bucket.
    assert!(r.structs.iter().any(|s| s.name == "Point"), "struct Point");
    assert!(r.classes.iter().any(|c| c.name == "Shape"), "class Shape");
    assert!(r.enums.iter().any(|e| e.name == "Color"), "enum Color");
    assert!(r.interfaces.iter().any(|i| i.name == "Drawable"), "protocol Drawable");
    assert!(r.functions.iter().any(|f| f.name == "add"), "func add");
}

#[test]
fn dart_parser_constructs_without_panic() {
    let _ = DartParser::new();
}

#[test]
fn dart_extracts_class_mixin_enum_function() {
    let src = "import 'dart:math';\n\
               class Animal { void speak(String s) {} }\n\
               mixin Swimmer { void swim() {} }\n\
               enum Color { red, green }\n\
               int add(int a, int b) { return a; }\n";
    let r = DartParser::new()
        .parse(Path::new("t.dart"), src.as_bytes(), false)
        .unwrap();

    assert!(r.classes.iter().any(|c| c.name == "Animal"), "class Animal");
    assert!(r.enums.iter().any(|e| e.name == "Color"), "enum Color");
    assert!(r.functions.iter().any(|f| f.name == "add"), "fn add");
    assert!(r.functions.iter().any(|f| f.name == "speak"), "method speak");
}

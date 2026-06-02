pub mod common;
pub mod python;
pub mod rust_lang;
pub mod typescript;
pub mod javascript;
pub mod go;
pub mod java;
pub mod c_lang;
pub mod cpp;
pub mod csharp;
pub mod ruby;
pub mod php;
pub mod swift;
pub mod dart;

use std::path::Path;

use crate::error::Result;
use crate::types::{FileParseResult, Language};

/// Trait implemented by each language-specific parser.
pub trait LanguageParser: Send + Sync {
    fn language(&self) -> Language;

    /// Parse source bytes from the given path and return a structured result.
    fn parse(&self, path: &Path, source: &[u8], is_dependency: bool) -> Result<FileParseResult>;
}

/// Return the raw tree-sitter grammar for a language.
///
/// Used by structural-similarity analysis to re-parse annotated source snippets
/// into a syntax tree at query time (the graph stores snippets, not trees).
pub fn ts_language(lang: Language) -> Option<tree_sitter::Language> {
    let l: tree_sitter::Language = match lang {
        Language::Python => tree_sitter_python::LANGUAGE.into(),
        Language::Rust => tree_sitter_rust::LANGUAGE.into(),
        Language::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Language::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Language::Go => tree_sitter_go::LANGUAGE.into(),
        Language::Java => tree_sitter_java::LANGUAGE.into(),
        Language::C => tree_sitter_c::LANGUAGE.into(),
        Language::Cpp => tree_sitter_cpp::LANGUAGE.into(),
        Language::CSharp => tree_sitter_c_sharp::LANGUAGE.into(),
        Language::Ruby => tree_sitter_ruby::LANGUAGE.into(),
        Language::Php => tree_sitter_php::LANGUAGE_PHP.into(),
        Language::Swift => tree_sitter_swift::LANGUAGE.into(),
        Language::Dart => dart::LANGUAGE.into(),
    };
    Some(l)
}

/// Return the appropriate parser for a file extension (without the dot).
pub fn parser_for_extension(ext: &str) -> Option<Box<dyn LanguageParser>> {
    match Language::from_extension(ext)? {
        Language::Python => Some(Box::new(python::PythonParser::new())),
        Language::Rust => Some(Box::new(rust_lang::RustParser::new())),
        Language::TypeScript => Some(Box::new(typescript::TypeScriptParser::new())),
        Language::JavaScript => Some(Box::new(javascript::JavaScriptParser::new())),
        Language::Go => Some(Box::new(go::GoParser::new())),
        Language::Java => Some(Box::new(java::JavaParser::new())),
        Language::C => Some(Box::new(c_lang::CParser::new())),
        Language::Cpp => Some(Box::new(cpp::CppParser::new())),
        Language::CSharp => Some(Box::new(csharp::CSharpParser::new())),
        Language::Ruby => Some(Box::new(ruby::RubyParser::new())),
        Language::Php => Some(Box::new(php::PhpParser::new())),
        Language::Swift => Some(Box::new(swift::SwiftParser::new())),
        Language::Dart => Some(Box::new(dart::DartParser::new())),
    }
}

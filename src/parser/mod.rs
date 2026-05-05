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

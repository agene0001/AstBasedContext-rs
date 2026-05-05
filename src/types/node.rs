use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::language::Language;

/// Span within a source file (1-based lines, 0-based columns).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub struct SourceSpan {
    pub start_line: u32,
    pub end_line: u32,
    pub start_col: u32,
    pub end_col: u32,
}

/// A typed field declaration (struct/class field or parameter).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FieldDecl {
    pub name: String,
    pub type_annotation: Option<String>,
    pub visibility: Option<String>,
    pub is_static: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RepositoryData {
    pub name: String,
    pub path: PathBuf,
    pub is_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileData {
    pub name: String,
    pub path: PathBuf,
    pub relative_path: String,
    pub language: Language,
    pub is_dependency: bool,
    /// Number of public functions/methods in this file.
    #[serde(default)]
    pub public_count: usize,
    /// Number of private/internal functions/methods in this file.
    #[serde(default)]
    pub private_count: usize,
    /// Number of comment lines in this file.
    #[serde(default)]
    pub comment_line_count: usize,
    /// Total lines of code in this file.
    #[serde(default)]
    pub total_lines: usize,
    /// Whether this file is a test file.
    #[serde(default)]
    pub is_test_file: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectoryData {
    pub name: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionData {
    pub name: String,
    pub path: PathBuf,
    pub span: SourceSpan,
    pub args: Vec<String>,
    /// Type annotations for each arg (parallel to `args`).
    #[serde(default)]
    pub arg_types: Vec<Option<String>>,
    /// Return type annotation.
    #[serde(default)]
    pub return_type: Option<String>,
    /// Visibility: "public", "private", "protected", or None for language default.
    #[serde(default)]
    pub visibility: Option<String>,
    /// Whether this is a static/class method (not bound to an instance).
    #[serde(default)]
    pub is_static: bool,
    /// Whether this is an abstract/virtual method.
    #[serde(default)]
    pub is_abstract: bool,
    pub cyclomatic_complexity: u32,
    pub decorators: Vec<String>,
    /// Enclosing function or class name.
    pub context: Option<String>,
    /// Type of the enclosing context node (e.g. "function_definition", "class_definition").
    pub context_type: Option<String>,
    /// Enclosing class name, if this function is a method.
    pub class_context: Option<String>,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
    pub docstring: Option<String>,
    /// Whether this function is async.
    #[serde(default)]
    pub is_async: bool,
    /// TODO/FIXME/HACK comments found in or near this function.
    #[serde(default)]
    pub todo_comments: Vec<String>,
    /// Exception/error types this function explicitly raises/throws.
    #[serde(default)]
    pub raises: Vec<String>,
    /// Whether this function contains try/catch/except error handling.
    #[serde(default)]
    pub has_error_handling: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClassData {
    pub name: String,
    pub path: PathBuf,
    pub span: SourceSpan,
    pub bases: Vec<String>,
    pub decorators: Vec<String>,
    /// Typed field declarations (instance + static fields).
    #[serde(default)]
    pub fields: Vec<FieldDecl>,
    pub context: Option<String>,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
    pub docstring: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImportData {
    pub name: String,
    pub full_import_name: Option<String>,
    pub line_number: u32,
    pub alias: Option<String>,
    pub language: Language,
    pub is_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VariableData {
    pub name: String,
    pub path: PathBuf,
    pub line_number: u32,
    pub value: Option<String>,
    pub type_annotation: Option<String>,
    pub context: Option<String>,
    pub class_context: Option<String>,
    pub language: Language,
    pub is_dependency: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunctionCallData {
    pub name: String,
    pub full_name: String,
    pub line_number: u32,
    pub args: Vec<String>,
    pub inferred_obj_type: Option<String>,
    /// (context_name, context_type, context_line)
    pub context: Option<(String, String, u32)>,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModuleData {
    pub name: String,
    pub full_import_name: Option<String>,
    pub language: Language,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitData {
    pub name: String,
    pub path: std::path::PathBuf,
    pub span: SourceSpan,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InterfaceData {
    pub name: String,
    pub path: std::path::PathBuf,
    pub span: SourceSpan,
    pub bases: Vec<String>,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StructData {
    pub name: String,
    pub path: std::path::PathBuf,
    pub span: SourceSpan,
    /// Typed field declarations.
    #[serde(default)]
    pub fields: Vec<FieldDecl>,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnumData {
    pub name: String,
    pub path: std::path::PathBuf,
    pub span: SourceSpan,
    pub variants: Vec<String>,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MacroData {
    pub name: String,
    pub path: std::path::PathBuf,
    pub line_number: u32,
    pub language: Language,
    pub is_dependency: bool,
    pub source: Option<String>,
}

/// Sum type for all graph nodes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum GraphNode {
    Repository(RepositoryData),
    Directory(DirectoryData),
    File(FileData),
    Function(FunctionData),
    Class(ClassData),
    Variable(VariableData),
    Module(ModuleData),
    Trait(TraitData),
    Interface(InterfaceData),
    Struct(StructData),
    Enum(EnumData),
    Macro(MacroData),
}

impl GraphNode {
    /// Returns a human-readable label for this node type.
    pub fn label(&self) -> &'static str {
        match self {
            GraphNode::Repository(_) => "Repository",
            GraphNode::Directory(_) => "Directory",
            GraphNode::File(_) => "File",
            GraphNode::Function(_) => "Function",
            GraphNode::Class(_) => "Class",
            GraphNode::Variable(_) => "Variable",
            GraphNode::Module(_) => "Module",
            GraphNode::Trait(_) => "Trait",
            GraphNode::Interface(_) => "Interface",
            GraphNode::Struct(_) => "Struct",
            GraphNode::Enum(_) => "Enum",
            GraphNode::Macro(_) => "Macro",
        }
    }

    /// Short label for compact output (MCP, CLI). Saves tokens in LLM context.
    pub fn short_label(&self) -> &'static str {
        match self {
            GraphNode::Repository(_) => "repo",
            GraphNode::Directory(_) => "dir",
            GraphNode::File(_) => "file",
            GraphNode::Function(_) => "fn",
            GraphNode::Class(_) => "cls",
            GraphNode::Variable(_) => "var",
            GraphNode::Module(_) => "mod",
            GraphNode::Trait(_) => "trait",
            GraphNode::Interface(_) => "iface",
            GraphNode::Struct(_) => "st",
            GraphNode::Enum(_) => "enum",
            GraphNode::Macro(_) => "macro",
        }
    }

    /// Returns the source snippet for this node, if annotated.
    pub fn source_snippet(&self) -> Option<&str> {
        match self {
            GraphNode::Function(d) => d.source.as_deref(),
            GraphNode::Class(d) => d.source.as_deref(),
            GraphNode::Trait(d) => d.source.as_deref(),
            GraphNode::Interface(d) => d.source.as_deref(),
            GraphNode::Struct(d) => d.source.as_deref(),
            GraphNode::Enum(d) => d.source.as_deref(),
            GraphNode::Macro(d) => d.source.as_deref(),
            _ => None,
        }
    }

    /// Returns the name of this node.
    pub fn location(&self) -> (String, usize, usize) {
        match self {
            GraphNode::Function(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Class(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Trait(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Interface(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Struct(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Enum(d) => (d.path.to_string_lossy().into_owned(), d.span.start_line as usize, d.span.end_line as usize),
            GraphNode::Macro(d) => (d.path.to_string_lossy().into_owned(), d.line_number as usize, d.line_number as usize),
            GraphNode::Variable(d) => (d.path.to_string_lossy().into_owned(), d.line_number as usize, d.line_number as usize),
            GraphNode::File(d) => (d.path.to_string_lossy().into_owned(), 0, 0),
            _ => (String::new(), 0, 0),
        }
    }

    pub fn name(&self) -> &str {
        match self {
            GraphNode::Repository(d) => &d.name,
            GraphNode::Directory(d) => &d.name,
            GraphNode::File(d) => &d.name,
            GraphNode::Function(d) => &d.name,
            GraphNode::Class(d) => &d.name,
            GraphNode::Variable(d) => &d.name,
            GraphNode::Module(d) => &d.name,
            GraphNode::Trait(d) => &d.name,
            GraphNode::Interface(d) => &d.name,
            GraphNode::Struct(d) => &d.name,
            GraphNode::Enum(d) => &d.name,
            GraphNode::Macro(d) => &d.name,
        }
    }
}

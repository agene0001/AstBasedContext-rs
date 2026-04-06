use serde::{Deserialize, Serialize};
use std::path::PathBuf;

use super::language::Language;
use super::node::{
    ClassData, EnumData, FunctionCallData, FunctionData, ImportData, InterfaceData, MacroData,
    StructData, TraitData, VariableData,
};

/// Language-agnostic output from parsing a single file.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FileParseResult {
    pub path: PathBuf,
    pub language: Language,
    pub functions: Vec<FunctionData>,
    pub classes: Vec<ClassData>,
    pub variables: Vec<VariableData>,
    pub imports: Vec<ImportData>,
    pub function_calls: Vec<FunctionCallData>,
    pub is_dependency: bool,
    // Language-specific collections
    pub traits: Vec<TraitData>,
    pub interfaces: Vec<InterfaceData>,
    pub structs: Vec<StructData>,
    pub enums: Vec<EnumData>,
    pub macros: Vec<MacroData>,
    /// Total lines of code in the file.
    #[serde(default)]
    pub total_lines: usize,
    /// Number of comment lines in the file.
    #[serde(default)]
    pub comment_line_count: usize,
    /// Whether this file is a test file.
    #[serde(default)]
    pub is_test_file: bool,
}

impl FileParseResult {
    /// Create a result with only the common fields populated.
    pub fn new(path: PathBuf, language: Language, is_dependency: bool) -> Self {
        Self {
            path,
            language,
            functions: Vec::new(),
            classes: Vec::new(),
            variables: Vec::new(),
            imports: Vec::new(),
            function_calls: Vec::new(),
            is_dependency,
            traits: Vec::new(),
            interfaces: Vec::new(),
            structs: Vec::new(),
            enums: Vec::new(),
            macros: Vec::new(),
            total_lines: 0,
            comment_line_count: 0,
            is_test_file: false,
        }
    }
}

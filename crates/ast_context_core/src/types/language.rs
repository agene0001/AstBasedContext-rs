use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    Python,
    Rust,
    TypeScript,
    JavaScript,
    Go,
    Java,
    C,
    Cpp,
}

impl Language {
    /// Returns the Language for a given file extension (without the dot).
    pub fn from_extension(ext: &str) -> Option<Self> {
        match ext {
            "py" | "pyw" => Some(Language::Python),
            "rs" => Some(Language::Rust),
            "ts" | "tsx" => Some(Language::TypeScript),
            "js" | "jsx" | "mjs" | "cjs" => Some(Language::JavaScript),
            "go" => Some(Language::Go),
            "java" => Some(Language::Java),
            "c" => Some(Language::C),
            "cpp" | "cc" | "cxx" | "hpp" | "hh" | "h" => Some(Language::Cpp),
            _ => None,
        }
    }

    /// Returns the canonical name of this language.
    pub fn name(&self) -> &'static str {
        match self {
            Language::Python => "python",
            Language::Rust => "rust",
            Language::TypeScript => "typescript",
            Language::JavaScript => "javascript",
            Language::Go => "go",
            Language::Java => "java",
            Language::C => "c",
            Language::Cpp => "cpp",
        }
    }

    /// Returns all file extensions associated with this language (without the dot).
    pub fn extensions(&self) -> &'static [&'static str] {
        match self {
            Language::Python => &["py", "pyw"],
            Language::Rust => &["rs"],
            Language::TypeScript => &["ts", "tsx"],
            Language::JavaScript => &["js", "jsx", "mjs", "cjs"],
            Language::Go => &["go"],
            Language::Java => &["java"],
            Language::C => &["c"],
            Language::Cpp => &["cpp", "cc", "cxx", "hpp", "hh", "h"],
        }
    }
}

impl std::fmt::Display for Language {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.name())
    }
}

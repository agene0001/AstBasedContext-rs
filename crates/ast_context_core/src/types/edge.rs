use serde::{Deserialize, Serialize};

/// Edge types in the code graph.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EdgeKind {
    /// Containment: Repository→Directory, Directory→File, File→Function, Class→Method, etc.
    Contains,

    /// Function/method call.
    Calls {
        line_number: u32,
        args: Vec<String>,
        full_call_name: String,
    },

    /// Class inheritance.
    Inherits,

    /// Interface/trait implementation (Phase 2).
    Implements,

    /// File imports a module.
    Imports {
        line_number: u32,
        alias: Option<String>,
        imported_name: Option<String>,
    },

    /// Function has a parameter.
    HasParameter,

    /// Test function tests a target function (test_foo → foo).
    Tests,
}

impl EdgeKind {
    pub fn label(&self) -> &'static str {
        match self {
            EdgeKind::Contains => "CONTAINS",
            EdgeKind::Calls { .. } => "CALLS",
            EdgeKind::Inherits => "INHERITS",
            EdgeKind::Implements => "IMPLEMENTS",
            EdgeKind::Imports { .. } => "IMPORTS",
            EdgeKind::HasParameter => "HAS_PARAMETER",
            EdgeKind::Tests => "TESTS",
        }
    }
}

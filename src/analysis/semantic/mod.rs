//! Semantic resolution seam.
//!
//! Tree-sitter gives us syntax, not semantics: it can see `x.foo()` but not the
//! type of `x`, which `foo` resolves to, or whether `foo` takes `&self` or
//! `&mut self`. Checks that need those facts today fall back to syntactic
//! proxies (hardcoded method lists, reference-guard heuristics) — good-faith
//! guesses, not certainties.
//!
//! This module is the seam where a real resolver (a language server such as
//! rust-analyzer) can plug in to turn those guesses into facts. The analysis
//! engine only ever talks to the [`SemanticProvider`] trait, so it stays
//! language-agnostic and degrades gracefully: when the provider can't answer
//! (`None`), the check keeps its existing heuristic at its existing (lower) tier.
//!
//! Phase 1 ships only the seam and [`NullProvider`] (answers nothing). No check
//! consults it yet, so behavior is unchanged. Later phases add an LSP-backed
//! provider behind an opt-in flag.

#[cfg(feature = "lsp")]
mod lsp;
#[cfg(feature = "lsp")]
pub use lsp::LspProvider;

/// A position in a source file (1-based line, 0-based UTF-8 column), as a
/// language server would address it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Location {
    pub file: String,
    pub line: usize,
    pub col: usize,
}

/// Mutability of a binding or a method receiver (`&self` vs `&mut self`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mutability {
    Shared,
    Mutable,
}

/// A resolved type, as reported by the provider. `name` is the display form
/// (e.g. `std::string::String`); the flags are filled in when the provider can
/// determine them cheaply, and left `None` otherwise.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypeInfo {
    pub name: String,
    /// Whether the type implements `Copy` (a `.clone()` on it is redundant).
    pub is_copy: Option<bool>,
    /// Size in bytes, when known.
    pub size: Option<usize>,
}

/// Resolved call edges around a position — the accurate call graph that
/// tree-sitter can only approximate (it can't resolve method/trait dispatch).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CallEdges {
    pub incoming: Vec<Location>,
    pub outgoing: Vec<Location>,
}

/// The semantic facts the analysis engine may ask for. Every method defaults to
/// "I don't know" (`None`), so a minimal provider is an empty impl and every
/// check has a defined fallback. Real providers override only what they can
/// answer with certainty.
pub trait SemanticProvider {
    /// Is this provider able to answer at all? (e.g. the server is warmed up and
    /// the project builds). Checks can use this to skip provider-only work
    /// entirely when there's nothing to gain.
    fn is_available(&self) -> bool {
        false
    }

    /// Resolved type of the expression at `loc`.
    fn type_of(&self, _loc: &Location) -> Option<TypeInfo> {
        None
    }

    /// Whether `ty` implements `Copy`. Separated from [`TypeInfo::is_copy`] so a
    /// provider can answer it on demand without resolving a full type first.
    fn is_copy(&self, _ty: &TypeInfo) -> Option<bool> {
        None
    }

    /// All references/usages of the symbol declared or used at `loc` — true
    /// cross-file/cross-crate liveness, including trait objects, fn-pointers and
    /// macro expansions that the AST call graph misses.
    fn references(&self, _loc: &Location) -> Option<Vec<Location>> {
        None
    }

    /// Receiver mutability of the method call at `loc` (`&self` vs `&mut self`),
    /// read from the method's signature rather than a hardcoded name list.
    fn receiver_mutability(&self, _loc: &Location) -> Option<Mutability> {
        None
    }

    /// Resolved incoming/outgoing call edges at `loc`.
    fn call_edges(&self, _loc: &Location) -> Option<CallEdges> {
        None
    }
}

/// The default provider: knows nothing. With it installed, every check falls
/// back to its syntactic heuristic, i.e. today's behavior. This is what
/// [`super::analyze`] uses; the deeper, opt-in path supplies a real provider via
/// [`super::analyze_with`].
pub struct NullProvider;

impl SemanticProvider for NullProvider {}

#[cfg(test)]
mod tests {
    use super::*;

    fn loc() -> Location {
        Location { file: "x.rs".into(), line: 1, col: 0 }
    }

    fn ty() -> TypeInfo {
        TypeInfo { name: "String".into(), is_copy: None, size: None }
    }

    #[test]
    fn null_provider_answers_nothing() {
        // The default path must stay heuristic-only: every query is `None` and
        // the provider reports itself unavailable, so checks fall back.
        let p = NullProvider;
        assert!(!p.is_available());
        assert!(p.type_of(&loc()).is_none());
        assert!(p.is_copy(&ty()).is_none());
        assert!(p.references(&loc()).is_none());
        assert!(p.receiver_mutability(&loc()).is_none());
        assert!(p.call_edges(&loc()).is_none());
    }

    #[test]
    fn provider_is_object_safe() {
        // The engine holds `&dyn SemanticProvider`; this must compile.
        let p = NullProvider;
        let dynamic: &dyn SemanticProvider = &p;
        assert!(!dynamic.is_available());
    }
}

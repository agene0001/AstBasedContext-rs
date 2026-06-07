//! Intra-procedural use-def analysis.
//!
//! Re-parses a single function body and records the facts that turn "I saw a
//! `.clone()` inside a loop" *guesses* into grounded questions like "is this
//! cloned value loop-invariant?". For each function it collects:
//!   - loop spans,
//!   - variable declarations (`let` bindings + parameters),
//!   - mutations (assignment, `&mut`, and common mutating method calls),
//!   - `<ident>.clone()` sites.
//!
//! This is the foundation for "good faith" optimization checks. It is purely
//! syntactic/positional — no type information (that's the LSP's job). Rust only
//! for now; other languages return `None` and their checks fall back to the old
//! heuristics.

use std::collections::HashMap;

use tree_sitter::{Node, Parser};

use crate::types::Language;

/// Methods that mutate their receiver. Used to mark a variable as changed inside
/// a loop without type info. Incomplete by nature (exotic mutators are missed —
/// that gap is what the LSP integration will close); kept conservative so we err
/// toward *not* over-claiming invariance.
const MUTATING_METHODS: &[&str] = &[
    "push", "push_str", "pop", "insert", "remove", "clear", "extend", "append",
    "truncate", "retain", "drain", "sort", "sort_by", "sort_unstable", "dedup",
    "swap", "fill", "resize", "reserve", "set", "replace", "get_or_insert",
    "get_mut", "as_mut", "entry",
];

#[derive(Debug, Default)]
pub(super) struct UseDef {
    /// Byte spans of loop expressions (their whole extent, body included).
    loops: Vec<(usize, usize)>,
    /// Variable name → byte offsets where it is declared (`let` / parameter).
    decls: HashMap<String, Vec<usize>>,
    /// Variable name → byte offsets where it is mutated (assign / `&mut` / mut method).
    mutated: HashMap<String, Vec<usize>>,
    /// `(receiver_ident, byte_offset)` for each `<ident>.clone()` call.
    pub(super) clones: Vec<(String, usize)>,
}

impl UseDef {
    /// Analyze one function body. Returns `None` for unsupported languages.
    pub(super) fn analyze(source: &str, lang: Language) -> Option<UseDef> {
        if lang != Language::Rust {
            return None;
        }
        let ts = crate::parser::ts_language(lang)?;
        let mut parser = Parser::new();
        parser.set_language(&ts).ok()?;
        let tree = parser.parse(source, None)?;
        let mut ud = UseDef::default();
        walk(tree.root_node(), source.as_bytes(), &mut ud);
        Some(ud)
    }

    /// The smallest loop span containing `pos`, if any.
    fn enclosing_loop(&self, pos: usize) -> Option<(usize, usize)> {
        self.loops
            .iter()
            .filter(|&&(s, e)| s <= pos && pos < e)
            .min_by_key(|&&(s, e)| e - s)
            .copied()
    }

    /// True iff `var`, cloned at `pos` inside a loop, is **loop-invariant**: it has
    /// a known declaration, none of its declarations are inside the enclosing loop,
    /// and it is not mutated inside that loop. Such a clone produces the same value
    /// every iteration and can be hoisted out — a real, provable finding.
    pub(super) fn is_invariant_clone(&self, var: &str, pos: usize) -> bool {
        let Some((ls, le)) = self.enclosing_loop(pos) else {
            return false; // not in a loop
        };
        let Some(decls) = self.decls.get(var) else {
            return false; // origin unknown (field/global) — don't claim
        };
        if decls.iter().any(|&d| d >= ls && d < le) {
            return false; // (re)declared per-iteration
        }
        let mutated_in_loop = self
            .mutated
            .get(var)
            .is_some_and(|m| m.iter().any(|&p| p >= ls && p < le));
        !mutated_in_loop
    }
}

fn ident_text<'a>(node: Node, src: &'a [u8]) -> Option<&'a str> {
    node.utf8_text(src).ok()
}

fn walk(node: Node, src: &[u8], ud: &mut UseDef) {
    match node.kind() {
        "for_expression" | "while_expression" | "loop_expression" => {
            ud.loops.push((node.start_byte(), node.end_byte()));
        }
        "let_declaration" | "parameter" => {
            if let Some(pat) = node.child_by_field_name("pattern") {
                if pat.kind() == "identifier" {
                    if let Some(name) = ident_text(pat, src) {
                        ud.decls.entry(name.to_string()).or_default().push(node.start_byte());
                    }
                }
            }
        }
        "assignment_expression" | "compound_assignment_expr" => {
            if let Some(lhs) = node.child_by_field_name("left") {
                if lhs.kind() == "identifier" {
                    if let Some(name) = ident_text(lhs, src) {
                        ud.mutated.entry(name.to_string()).or_default().push(node.start_byte());
                    }
                }
            }
        }
        "reference_expression" => {
            // `&mut x` mutates x.
            let is_mut = node
                .children(&mut node.walk())
                .any(|c| c.kind() == "mutable_specifier");
            if is_mut {
                if let Some(val) = node.child_by_field_name("value") {
                    if val.kind() == "identifier" {
                        if let Some(name) = ident_text(val, src) {
                            ud.mutated.entry(name.to_string()).or_default().push(node.start_byte());
                        }
                    }
                }
            }
        }
        "call_expression" => {
            if let Some(func) = node.child_by_field_name("function") {
                if func.kind() == "field_expression" {
                    let method = func.child_by_field_name("field").and_then(|f| ident_text(f, src));
                    let receiver = func.child_by_field_name("value");
                    if let (Some(method), Some(recv)) = (method, receiver) {
                        if recv.kind() == "identifier" {
                            if let Some(rname) = ident_text(recv, src) {
                                if method == "clone" {
                                    ud.clones.push((rname.to_string(), node.start_byte()));
                                } else if MUTATING_METHODS.contains(&method) {
                                    ud.mutated.entry(rname.to_string()).or_default().push(node.start_byte());
                                }
                            }
                        }
                    }
                }
            }
        }
        _ => {}
    }

    let mut cursor = node.walk();
    for child in node.children(&mut cursor) {
        walk(child, src, ud);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn invariant_clones(src: &str) -> Vec<String> {
        let ud = UseDef::analyze(src, Language::Rust).unwrap();
        ud.clones
            .iter()
            .filter(|(v, p)| ud.is_invariant_clone(v, *p))
            .map(|(v, _)| v.clone())
            .collect()
    }

    #[test]
    fn param_clone_in_loop_is_invariant() {
        let src = "fn f(cfg: Config, n: usize) { for i in 0..n { let c = cfg.clone(); use_it(c); } }";
        assert_eq!(invariant_clones(src), vec!["cfg"]);
    }

    #[test]
    fn loop_variable_clone_is_not_invariant() {
        // `x` is the loop variable — not tracked as a decl, changes each iteration.
        let src = "fn f(items: Vec<T>) { for x in items { let c = x.clone(); } }";
        assert!(invariant_clones(src).is_empty());
    }

    #[test]
    fn clone_of_inner_decl_is_not_invariant() {
        let src = "fn f(n: usize) { for i in 0..n { let tmp = make(); let c = tmp.clone(); } }";
        assert!(invariant_clones(src).is_empty());
    }

    #[test]
    fn clone_of_reassigned_var_is_not_invariant() {
        let src = "fn f(mut s: String, n: usize) { for i in 0..n { s = next(); let c = s.clone(); } }";
        assert!(invariant_clones(src).is_empty());
    }

    #[test]
    fn clone_of_mutated_var_is_not_invariant() {
        let src = "fn f(mut s: String, n: usize) { for i in 0..n { s.push('x'); let c = s.clone(); } }";
        assert!(invariant_clones(src).is_empty());
    }

    #[test]
    fn read_only_method_does_not_block_invariance() {
        // `cfg.len()` reads but doesn't mutate, so cfg.clone() is still invariant.
        let src = "fn f(cfg: Config, n: usize) { for i in 0..n { let _ = cfg.len(); let c = cfg.clone(); } }";
        assert_eq!(invariant_clones(src), vec!["cfg"]);
    }

    #[test]
    fn clone_outside_any_loop_is_not_flagged() {
        let src = "fn f(cfg: Config) { let c = cfg.clone(); }";
        assert!(invariant_clones(src).is_empty());
    }
}

//! Structural (AST-shape) similarity for annotated source snippets.
//!
//! Unlike lexical token overlap, this re-parses each node's source snippet with
//! tree-sitter and compares the *shape* of the syntax tree. Identifier and
//! literal text is ignored — only grammar node kinds matter — so two functions
//! that are structurally identical but use completely different variable names
//! still score as similar.
//!
//! The fingerprint is a multiset of "production" tokens: for every internal AST
//! node we emit one token combining the node's kind with the ordered kinds of
//! its named children (e.g. `if_statement(binary_expression,block)`), hashed to
//! a `u64`. Comparing the multisets with cosine similarity yields a value in
//! `[0.0, 1.0]` where 1.0 means the two trees use the exact same productions in
//! the same proportions.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};

use tree_sitter::{Node, Parser};

use crate::types::Language;

/// A structural fingerprint: histogram of hashed production tokens → counts.
pub type Fingerprint = HashMap<u64, u32>;

/// Create a tree-sitter parser configured for `lang`, or `None` if unavailable.
pub fn parser_for(lang: Language) -> Option<Parser> {
    let ts_lang = crate::parser::ts_language(lang)?;
    let mut parser = Parser::new();
    parser.set_language(&ts_lang).ok()?;
    Some(parser)
}

/// Build a structural fingerprint from a source snippet using a pre-configured
/// parser. Returns `None` if the snippet fails to parse or yields no structure.
pub fn fingerprint_with(parser: &mut Parser, source: &str) -> Option<Fingerprint> {
    let tree = parser.parse(source.as_bytes(), None)?;
    let mut fp = Fingerprint::new();
    collect(tree.root_node(), &mut fp);
    (!fp.is_empty()).then_some(fp)
}

/// Like [`fingerprint_with`], but returns a `(token, count)` list sorted by
/// token. This is the form persisted on the graph: a stable order keeps the
/// serialized graph deterministic and round-trips back via [`pairs_to_map`].
pub fn fingerprint_sorted(parser: &mut Parser, source: &str) -> Option<Vec<(u64, u32)>> {
    let fp = fingerprint_with(parser, source)?;
    let mut pairs: Vec<(u64, u32)> = fp.into_iter().collect();
    pairs.sort_unstable_by_key(|&(token, _)| token);
    Some(pairs)
}

/// Rebuild a working fingerprint map from stored `(token, count)` pairs.
pub fn pairs_to_map(pairs: &[(u64, u32)]) -> Fingerprint {
    pairs.iter().copied().collect()
}

/// Recursively accumulate production tokens for `node` and its descendants.
fn collect(node: Node, fp: &mut Fingerprint) {
    let named_child_count = node.named_child_count();
    if named_child_count > 0 {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        node.kind().hash(&mut hasher);
        for i in 0..named_child_count {
            if let Some(child) = node.named_child(i as u32) {
                child.kind().hash(&mut hasher);
            }
        }
        *fp.entry(hasher.finish()).or_insert(0) += 1;
    }
    for i in 0..named_child_count {
        if let Some(child) = node.named_child(i as u32) {
            collect(child, fp);
        }
    }
}

/// Plain (unweighted) cosine similarity of two fingerprints, in `[0.0, 1.0]`.
///
/// Rarely the right metric on its own: every program in a language shares a core
/// of ubiquitous productions (`block`, `identifier`, …), which inflates the
/// baseline similarity between unrelated code. Prefer [`weighted_cosine`], which
/// discounts those common productions. This is kept as a fallback for when no
/// corpus is available to compute weights from.
pub fn cosine(a: &Fingerprint, b: &Fingerprint) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut dot = 0u64;
    for (token, &count) in small {
        if let Some(&other) = large.get(token) {
            dot += count as u64 * other as u64;
        }
    }
    if dot == 0 {
        return 0.0;
    }
    let norm = |m: &Fingerprint| -> f64 {
        m.values().map(|&v| (v as u64 * v as u64) as f64).sum::<f64>().sqrt()
    };
    dot as f64 / (norm(a) * norm(b))
}

/// Inverse-document-frequency weight per production token over a corpus of
/// fingerprints.
///
/// A token present in every fingerprint gets weight ~0 (it carries no signal),
/// while a token shared by only a few gets a large weight. Used to stop common
/// grammar productions from making every same-language function look similar.
pub fn idf_weights(corpus: &[&Fingerprint]) -> HashMap<u64, f64> {
    let n = corpus.len() as f64;
    let mut df: HashMap<u64, u32> = HashMap::new();
    for fp in corpus {
        for token in fp.keys() {
            *df.entry(*token).or_insert(0) += 1;
        }
    }
    df.into_iter()
        // Smoothed idf: df == n → 0.0; rarer tokens → larger weights.
        .map(|(token, d)| (token, ((n + 1.0) / (d as f64 + 1.0)).ln()))
        .collect()
}

/// IDF-weighted cosine similarity of two fingerprints, in `[0.0, 1.0]`.
///
/// Each production's contribution is scaled by its `idf` weight, so distinctive
/// shared structure dominates and ubiquitous productions are ignored. Falls back
/// to plain [`cosine`] in the degenerate case where every shared token has zero
/// weight (e.g. two trivial functions whose every production is ubiquitous).
pub fn weighted_cosine(a: &Fingerprint, b: &Fingerprint, idf: &HashMap<u64, f64>) -> f64 {
    if a.is_empty() || b.is_empty() {
        return 0.0;
    }
    let w = |t: &u64| idf.get(t).copied().unwrap_or(0.0);
    let norm = |m: &Fingerprint| -> f64 {
        m.iter()
            .map(|(t, &c)| {
                let v = c as f64 * w(t);
                v * v
            })
            .sum::<f64>()
            .sqrt()
    };
    let (na, nb) = (norm(a), norm(b));
    if na == 0.0 || nb == 0.0 {
        // All structure is ubiquitous; weighting carries no signal here.
        return cosine(a, b);
    }
    let (small, large) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    let mut dot = 0.0;
    for (t, &ca) in small {
        if let Some(&cb) = large.get(t) {
            let wt = w(t);
            dot += (ca as f64 * wt) * (cb as f64 * wt);
        }
    }
    dot / (na * nb)
}

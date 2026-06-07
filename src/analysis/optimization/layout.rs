//! Struct layout / padding analysis.
//!
//! Unlike the heuristic optimization checks (which guess at hot paths and runtime
//! behavior the AST can't see), this is a *calculation*: from the field types we
//! compute the exact current size vs the size after an optimal field reorder, and
//! report the recoverable padding. No false positives — the padding either exists
//! or it doesn't.
//!
//! Language note: only C / C++ / Go are analysed — they keep declared field order,
//! so the source order IS the layout and a reorder is a safe win. Rust (and
//! Swift / C#) are excluded: default layout is already compiler-reordered to
//! optimal, and the one case the programmer fixes order — `#[repr(C)]` — is an
//! intentional FFI/ABI contract where reordering would break C interop.

use crate::types::Language;
use crate::types::node::GraphNode;
use super::super::context::AnalysisContext;
use super::super::types::{Finding, FindingKind, Tier};

/// (size, alignment) in bytes on a 64-bit target, for types we can size with
/// certainty. Returns `None` for anything ambiguous (references, generics,
/// user types) so the whole struct is skipped rather than mis-sized.
fn type_layout(lang: Language, ty: &str) -> Option<(usize, usize)> {
    let t = ty.trim();
    match lang {
        Language::Rust => rust_layout(t),
        Language::C | Language::Cpp => c_layout(t),
        Language::Go => go_layout(t),
        _ => None,
    }
}

fn rust_layout(t: &str) -> Option<(usize, usize)> {
    Some(match t {
        "u8" | "i8" | "bool" => (1, 1),
        "u16" | "i16" => (2, 2),
        "u32" | "i32" | "f32" | "char" => (4, 4),
        "u64" | "i64" | "f64" | "usize" | "isize" => (8, 8),
        "u128" | "i128" => (16, 16),
        "String" => (24, 8),
        _ if t.starts_with("Vec<") || t.starts_with("VecDeque<") => (24, 8),
        _ => return None,
    })
}

fn c_layout(t: &str) -> Option<(usize, usize)> {
    if t.ends_with('*') {
        return Some((8, 8)); // pointer
    }
    Some(match t {
        "char" | "signed char" | "unsigned char" | "bool" | "_Bool"
        | "int8_t" | "uint8_t" => (1, 1),
        "short" | "short int" | "unsigned short" | "int16_t" | "uint16_t" => (2, 2),
        "int" | "unsigned" | "unsigned int" | "float" | "int32_t" | "uint32_t" => (4, 4),
        "long" | "unsigned long" | "long long" | "unsigned long long" | "double"
        | "size_t" | "ssize_t" | "int64_t" | "uint64_t" | "intptr_t" | "uintptr_t" => (8, 8),
        _ => return None,
    })
}

fn go_layout(t: &str) -> Option<(usize, usize)> {
    if let Some(rest) = t.strip_prefix('*') {
        let _ = rest;
        return Some((8, 8)); // pointer
    }
    if t.starts_with("[]") {
        return Some((24, 8)); // slice header (ptr,len,cap)
    }
    Some(match t {
        "bool" | "int8" | "uint8" | "byte" => (1, 1),
        "int16" | "uint16" => (2, 2),
        "int32" | "uint32" | "float32" | "rune" => (4, 4),
        "int64" | "uint64" | "float64" | "int" | "uint" | "uintptr" | "complex64" => (8, 8),
        "string" | "complex128" => (16, 8),
        _ => return None,
    })
}

fn round_up(n: usize, align: usize) -> usize {
    n.div_ceil(align) * align
}

/// Size of a struct whose fields are laid out in the given order.
fn layout_size(fields: &[(usize, usize)]) -> usize {
    let mut offset = 0usize;
    let mut max_align = 1usize;
    for &(size, align) in fields {
        max_align = max_align.max(align);
        offset = round_up(offset, align) + size;
    }
    round_up(offset, max_align)
}

pub(crate) fn detect_struct_layout(ctx: &AnalysisContext, findings: &mut Vec<Finding>) {
    for &(idx, node) in &ctx.structs {
        let s = match node {
            GraphNode::Struct(s) => s,
            _ => continue,
        };

        // Language gate — only where the declared field order IS the layout and a
        // reorder is a safe, real win. C / C++ / Go keep field order and devs tune
        // it for packing. Rust (and Swift/C#) are excluded on purpose: default
        // layout is already compiler-reordered to optimal, and the one case the
        // programmer controls order — `#[repr(C)]` — is an intentional FFI/ABI
        // contract where reordering would *break* C interop.
        if !matches!(s.language, Language::C | Language::Cpp | Language::Go) {
            continue;
        }

        if s.fields.len() < 2 {
            continue;
        }

        // Size every field; if any is unsizeable, skip the struct (no guessing).
        let mut fields: Vec<(String, usize, usize)> = Vec::with_capacity(s.fields.len());
        let mut ok = true;
        for f in &s.fields {
            match f.type_annotation.as_deref().and_then(|t| type_layout(s.language, t)) {
                Some((size, align)) => fields.push((f.name.clone(), size, align)),
                None => {
                    ok = false;
                    break;
                }
            }
        }
        if !ok || fields.len() < 2 {
            continue;
        }

        let current = layout_size(&fields.iter().map(|&(_, sz, al)| (sz, al)).collect::<Vec<_>>());

        // Optimal order: largest alignment first, then largest size — the standard
        // padding-minimizing order (and what Rust's default repr does).
        let mut opt = fields.clone();
        opt.sort_by(|a, b| b.2.cmp(&a.2).then(b.1.cmp(&a.1)));
        let optimal = layout_size(&opt.iter().map(|&(_, sz, al)| (sz, al)).collect::<Vec<_>>());

        // Only flag a meaningful, recoverable win (≥ one 32-bit word).
        if current > optimal && current - optimal >= 4 {
            let order: Vec<String> = opt.iter().map(|(n, _, _)| n.clone()).collect();
            findings.push(Finding {
                tier: Tier::Low,
                kind: FindingKind::StructLayout {
                    struct_name: s.name.clone(),
                    current_size: current,
                    optimal_size: optimal,
                    suggested_order: order.clone(),
                },
                node_indices: vec![idx.index()],
                description: format!(
                    "`{}`: {} bytes → {} bytes (save {}) by reordering fields: {}",
                    s.name,
                    current,
                    optimal,
                    current - optimal,
                    order.join(", "),
                ),
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn c_padding_recoverable() {
        // char, long, char, double  →  reorder long, double, char, char
        let decl = [(1, 1), (8, 8), (1, 1), (8, 8)];
        assert_eq!(layout_size(&decl), 32);
        let mut opt = decl;
        opt.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        assert_eq!(layout_size(&opt), 24); // 8 bytes saved
    }

    #[test]
    fn already_optimal_has_no_savings() {
        let decl = [(8, 8), (8, 8), (1, 1), (1, 1)];
        assert_eq!(layout_size(&decl), 24);
        let mut opt = decl;
        opt.sort_by(|a, b| b.1.cmp(&a.1).then(b.0.cmp(&a.0)));
        assert_eq!(layout_size(&opt), 24);
    }

    #[test]
    fn type_tables() {
        assert_eq!(type_layout(Language::C, "double"), Some((8, 8)));
        assert_eq!(type_layout(Language::C, "char *"), Some((8, 8)));
        assert_eq!(type_layout(Language::Go, "string"), Some((16, 8)));
        assert_eq!(type_layout(Language::Go, "[]byte"), Some((24, 8)));
        assert_eq!(type_layout(Language::Rust, "u64"), Some((8, 8)));
        // unsizeable → None (struct gets skipped)
        assert_eq!(type_layout(Language::C, "struct Foo"), None);
    }
}

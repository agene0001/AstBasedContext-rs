// optimization_patterns.rs — Test patterns for optimization checks (103-109)
use std::collections::HashMap;

// ── CIL (clone-in-loop) — .clone() inside a loop ────────────────────────
pub fn process_items_with_clone(items: &[String], prefix: &str) -> Vec<String> {
    let mut results = Vec::new();
    for item in items {
        let key = prefix.to_string();
        results.push(format!("{}: {}", key, item));
    }
    results
}

// ── RCI (redundant-collect-iterate) — .collect().iter() chain ────────────
pub fn double_collect(data: &[i32]) -> Vec<i32> {
    let filtered: Vec<i32> = data.iter().filter(|x| **x > 0).cloned().collect::<Vec<_>>();
    filtered.iter().map(|x| x * 2).collect()
}

// ── RML (repeated-map-lookup) — same key 3+ times ───────────────────────
pub fn repeated_config_access(config: &HashMap<String, String>) -> String {
    let host = &config["server"];
    let port = &config["server"];
    let name = &config["server"];
    format!("{}:{}:{}", host, port, name)
}

// ── VNP (vec-no-presize) — Vec::new() + push in loop ────────────────────
pub fn collect_names(items: &[(String, i32)]) -> Vec<String> {
    let mut names = Vec::new();
    for (name, _score) in items {
        names.push(name.clone());
    }
    names
}

// ── STF (sort-then-find) — sort then .iter().find() ─────────────────────
pub fn find_sorted(mut data: Vec<i32>, target: i32) -> Option<i32> {
    data.sort();
    data.iter().find(|&&x| x == target).copied()
}

// ── URB (unbounded-recursion) — recursive with no depth param ────────────
pub fn walk_tree(node: &str) -> Vec<String> {
    let mut result = vec![node.to_string()];
    // Simulate children
    if node.len() > 1 {
        result.extend(walk_tree(&node[1..]));
    }
    result
}

// ── VEC (vectorize / SIMD candidate) ─────────────────────────────────────
pub fn dot_product(a: &[f64], b: &[f64]) -> f64 {
    let mut sum = 0.0;
    for i in 0..a.len() {
        sum += a[i] * b[i];
    }
    sum
}

pub fn scale_array(data: &mut [f64], factor: f64) {
    for i in 0..data.len() {
        data[i] *= factor;
        data[i] += 1.0;
    }
}

pub fn euclidean_distance(a: &[f64], b: &[f64]) -> f64 {
    let mut sum = 0.0;
    for i in 0..a.len() {
        let diff = a[i] - b[i];
        sum += diff * diff;
    }
    sum.sqrt()
}

// ── UCH (unnecessary-chain) — .filter().next() instead of .find() ───────
pub fn find_first_positive(data: &[i32]) -> Option<&i32> {
    data.iter().filter(|x| **x > 0).next()
}

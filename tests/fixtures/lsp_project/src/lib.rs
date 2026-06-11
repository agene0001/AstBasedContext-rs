pub mod other;

// Visibility: `pub` but referenced only inside this file → over-exposed.
pub fn file_local_helper(x: i32) -> i32 {
    x + 1
}

// Visibility: `pub` and referenced from other.rs → justified, must NOT flag.
pub fn cross_file_helper(x: i32) -> i32 {
    x * 2
}

pub fn uses_local() -> i32 {
    file_local_helper(41)
}

// &mut never mutated: `v` is only read (`.len()` is &self) → should flag.
pub fn count_items(v: &mut Vec<i32>) -> usize {
    v.len()
}

// &mut genuinely mutated via `.push()` (&mut self) → must NOT flag.
pub fn add_item(v: &mut Vec<i32>, x: i32) {
    v.push(x);
}

// &mut genuinely mutated via assignment to a deref → must NOT flag.
pub fn reset_value(p: &mut i32) {
    *p = 0;
}

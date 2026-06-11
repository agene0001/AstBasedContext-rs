//! `OrphanConfig` is a plain data type referenced by nothing — dead type.
pub fn real_entry() -> i32 {
    helper() + 1
}

fn helper() -> i32 {
    40 + 2
}

pub struct OrphanConfig {
    pub timeout: u32,
    pub retries: u32,
}

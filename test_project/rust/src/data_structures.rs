use std::collections::HashMap;

// Check 92: Vec used as set
pub fn collect_unique_tags(items: &[Item]) -> Vec<String> {
    let mut tags = Vec::new();
    for item in items {
        for tag in &item.tags {
            if !tags.contains(tag) {
                tags.push(tag.clone());
            }
        }
    }
    tags
}

// Check 94: Linear search in loop
pub fn find_overlapping_ids(list_a: &[Entry], list_b: &[Entry]) -> Vec<u64> {
    let mut results = Vec::new();
    for a in list_a {
        for b in list_b {
            if a.id == b.id {
                results.push(a.id);
            }
        }
    }
    results
}

// Check 95: String concatenation in loop
pub fn build_log_output(entries: &[LogEntry]) -> String {
    let mut output = String::new();
    for entry in entries {
        output += &format!("[{}] {}: {}\n", entry.level, entry.timestamp, entry.message);
    }
    output
}

// Check 98: HashMap with sequential keys
pub fn index_by_position(items: &[Item]) -> HashMap<usize, &Item> {
    let mut map = HashMap::new();
    for (i, item) in items.iter().enumerate() {
        map.insert(i, item);
    }
    map
}

// Check 99: Excessive collect-iterate (Rust-specific)
pub fn get_active_names(users: &[User]) -> Vec<String> {
    let active: Vec<&User> = users.iter().filter(|u| u.active).collect();
    let names: Vec<String> = active.iter().map(|u| u.name.clone()).collect();
    names
}

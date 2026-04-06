use std::collections::HashMap;

// Check 1: Passthrough wrapper
pub fn get_user(id: u64) -> Option<User> {
    fetch_user_from_db(id)
}

pub fn fetch_user_from_db(id: u64) -> Option<User> {
    let db = Database::connect();
    let row = db.query("SELECT * FROM users WHERE id = ?", &[&id]);
    match row {
        Ok(r) => Some(User::from_row(r)),
        Err(_) => None,
    }
}

// Check 2: Near-duplicates
pub fn serialize_user(user: &User) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("name".to_string(), user.name.clone());
    map.insert("email".to_string(), user.email.clone());
    map.insert("role".to_string(), user.role.clone());
    map.insert("active".to_string(), user.active.to_string());
    map.insert("created".to_string(), user.created_at.clone());
    map
}

pub fn serialize_customer(customer: &Customer) -> HashMap<String, String> {
    let mut map = HashMap::new();
    map.insert("name".to_string(), customer.name.clone());
    map.insert("email".to_string(), customer.email.clone());
    map.insert("role".to_string(), customer.role.clone());
    map.insert("active".to_string(), customer.active.to_string());
    map.insert("created".to_string(), customer.created_at.clone());
    map
}

// Check 4: Merge candidates — shared core, different branches
pub fn process_csv_export(records: &[Record]) -> String {
    let mut output = String::new();
    for record in records {
        if record.value.is_empty() {
            continue;
        }
        let cleaned = record.value.trim().to_lowercase();
        let formatted = format!("{},{},{}", record.id, cleaned, record.timestamp);
        output.push_str(&formatted);
        output.push('\n');
    }
    output
}

pub fn process_tsv_export(records: &[Record]) -> String {
    let mut output = String::new();
    for record in records {
        if record.value.is_empty() {
            continue;
        }
        let cleaned = record.value.trim().to_lowercase();
        let formatted = format!("{}\t{}\t{}", record.id, cleaned, record.timestamp);
        output.push_str(&formatted);
        output.push('\n');
    }
    output
}

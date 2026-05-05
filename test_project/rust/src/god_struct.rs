// Check 6: Overlapping structs
pub struct ShippingAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub recipient: String,
}

pub struct BillingAddress {
    pub street: String,
    pub city: String,
    pub state: String,
    pub zip: String,
    pub country: String,
    pub cardholder: String,
}

// Check 10: High LCOM — struct methods touch disjoint fields
pub struct AppManager {
    pub users: Vec<String>,
    pub orders: Vec<String>,
    pub logs: Vec<String>,
    pub cache: std::collections::HashMap<String, String>,
    pub config: std::collections::HashMap<String, String>,
}

impl AppManager {
    pub fn add_user(&mut self, name: String) {
        self.users.push(name);
    }

    pub fn remove_user(&mut self, name: &str) {
        self.users.retain(|u| u != name);
    }

    pub fn find_user(&self, name: &str) -> Option<&String> {
        self.users.iter().find(|u| u.as_str() == name)
    }

    pub fn add_order(&mut self, order: String) {
        self.orders.push(order);
    }

    pub fn pending_orders(&self) -> Vec<&String> {
        self.orders.iter().filter(|o| o.starts_with("pending")).collect()
    }

    pub fn log_info(&mut self, msg: String) {
        self.logs.push(format!("[INFO] {}", msg));
    }

    pub fn log_error(&mut self, msg: String) {
        self.logs.push(format!("[ERROR] {}", msg));
    }

    pub fn get_errors(&self) -> Vec<&String> {
        self.logs.iter().filter(|l| l.starts_with("[ERROR]")).collect()
    }

    pub fn cache_set(&mut self, key: String, value: String) {
        self.cache.insert(key, value);
    }

    pub fn cache_get(&self, key: &str) -> Option<&String> {
        self.cache.get(key)
    }

    pub fn set_config(&mut self, key: String, value: String) {
        self.config.insert(key, value);
    }

    pub fn get_config(&self, key: &str) -> Option<&String> {
        self.config.get(key)
    }
}

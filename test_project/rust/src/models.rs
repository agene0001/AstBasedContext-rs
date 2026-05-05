// models.rs — Base trait with 3+ implementors for PI, FTY, STR, SEL, HTE patterns.

pub trait Model {
    fn validate(&self) -> bool;
    fn serialize(&self) -> String;
}

pub struct UserModel {
    pub name: String,
    pub email: String,
}

impl Model for UserModel {
    fn validate(&self) -> bool { !self.name.is_empty() }
    fn serialize(&self) -> String { format!("user:{}", self.name) }
}

pub struct OrderModel {
    pub id: u64,
    pub total: f64,
}

impl Model for OrderModel {
    fn validate(&self) -> bool { self.total > 0.0 }
    fn serialize(&self) -> String { format!("order:{}", self.id) }
}

pub struct ProductModel {
    pub sku: String,
    pub price: f64,
}

impl Model for ProductModel {
    fn validate(&self) -> bool { !self.sku.is_empty() }
    fn serialize(&self) -> String { format!("product:{}", self.sku) }
}

// Constructor functions called from multiple files → FTY
impl UserModel {
    pub fn new(name: &str, email: &str) -> Self {
        Self { name: name.to_string(), email: email.to_string() }
    }
}

impl OrderModel {
    pub fn new(id: u64, total: f64) -> Self {
        Self { id, total }
    }
}

impl ProductModel {
    pub fn new(sku: &str, price: f64) -> Self {
        Self { sku: sku.to_string(), price }
    }
}

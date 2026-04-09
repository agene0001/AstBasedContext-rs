// type_patterns.rs — test fixtures targeting AstBasedContext-rs redundancy detectors
//
// Detectors targeted:
//   LPT  — detect_large_product_type   (type_system.rs)     : struct with ≥10 fields
//   SPG  — detect_speculative_generality (anti_patterns.rs)  : trait with exactly 1 implementor
//   SEL  — detect_suggest_sealed_type  (type_system.rs)     : trait with ≥2 implementors, all in same file
//   EO   — find_overlapping_enums      (struct_enum.rs)     : two enums sharing ≥50% variant names
//   SST  — detect_tagged_union         (type_system.rs)     : struct with tag field + branching method
//   FB   — detect_fluent_builder       (pattern_detection.rs): class with ≥3 Self-returning methods
//          NOTE: detect_fluent_builder iterates ctx.classes (GraphNode::Class only). The Rust parser
//          emits GraphNode::Struct for `struct` items, so FB cannot fire on Rust code as-is.
//          The QueryBuilder below is the intended Rust pattern; a language that maps to Class nodes
//          would trigger the detector with this structure.
//   SED  — suggest_enum_dispatch       (type_suggestions.rs): fn with *_mode/*_type/*_flag/*_kind
//          param + cc≥3 + branching on that param in source

// ─────────────────────────────────────────────────────────────────────────────
// LPT: Large product type — struct with 10 fields triggers detect_large_product_type
// ─────────────────────────────────────────────────────────────────────────────

pub struct AppConfig {
    pub host: String,
    pub port: u16,
    pub database_url: String,
    pub max_connections: u32,
    pub timeout_seconds: u64,
    pub retry_count: u32,
    pub log_level: String,
    pub api_key: String,
    pub enable_tls: bool,
    pub cert_path: String,
}

// ─────────────────────────────────────────────────────────────────────────────
// SPG: Speculative generality — trait with exactly 1 implementor
// ─────────────────────────────────────────────────────────────────────────────

pub trait Serializer {
    fn serialize(&self) -> String;
}

pub struct JsonSerializer;

impl Serializer for JsonSerializer {
    fn serialize(&self) -> String {
        String::new()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SEL: Suggest sealed type — trait with ≥2 implementors, all in this same file
// ─────────────────────────────────────────────────────────────────────────────

pub trait Shape {
    fn area(&self) -> f64;
}

pub struct Circle {
    pub r: f64,
}

pub struct Square {
    pub s: f64,
}

impl Shape for Circle {
    fn area(&self) -> f64 {
        3.14159 * self.r * self.r
    }
}

impl Shape for Square {
    fn area(&self) -> f64 {
        self.s * self.s
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// EO: Overlapping enums — two enums sharing ≥50% of variant names
// shared variants: Ok, NotFound, ServerError  (3 shared out of 5 union) = 60% ≥ 50%
// ─────────────────────────────────────────────────────────────────────────────

pub enum HttpStatus {
    Ok,
    NotFound,
    ServerError,
    BadRequest,
}

pub enum ApiStatus {
    Ok,
    NotFound,
    ServerError,
    Timeout,
}

// ─────────────────────────────────────────────────────────────────────────────
// SST: Suggest sum type / tagged union — struct with `kind` tag field (String),
//      and a method whose source contains a tag access pattern + branch keyword.
//      detect_tagged_union checks:
//        1. struct has a field in tag_field_names (kind ✓)
//        2. field type is primitive (String contains "str" ✓)
//        3. a child method source contains a tag_switch_pattern (.kind ✓)
//           AND a branch keyword (if  ✓)
// ─────────────────────────────────────────────────────────────────────────────

pub struct Event {
    pub kind: String,
    pub data: String,
}

impl Event {
    pub fn process(&self) -> String {
        if self.kind == "click" {
            "clicked".to_string()
        } else if self.kind == "hover" {
            "hovered".to_string()
        } else if self.kind == "focus" {
            "focused".to_string()
        } else {
            "unknown".to_string()
        }
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// FB: Fluent builder — ≥3 methods returning Self
//     NOTE: detect_fluent_builder only iterates ctx.classes (GraphNode::Class).
//     The Rust parser produces GraphNode::Struct for `struct` items, so this
//     detector will NOT fire on Rust files. Included for completeness and to
//     serve as the correct Rust idiom if the detector is extended to Structs.
// ─────────────────────────────────────────────────────────────────────────────

pub struct QueryBuilder {
    query: String,
    table: String,
    condition: String,
}

impl QueryBuilder {
    pub fn new() -> Self {
        QueryBuilder {
            query: String::new(),
            table: String::new(),
            condition: String::new(),
        }
    }

    pub fn select(mut self, cols: &str) -> Self {
        self.query.push_str(cols);
        self
    }

    pub fn from(mut self, table: &str) -> Self {
        self.table.push_str(table);
        self
    }

    pub fn where_clause(mut self, cond: &str) -> Self {
        self.condition.push_str(cond);
        self
    }

    pub fn limit(mut self, n: usize) -> Self {
        self.query.push_str(&format!(" LIMIT {n}"));
        self
    }

    pub fn build(self) -> String {
        format!(
            "SELECT {} FROM {} WHERE {}",
            self.query, self.table, self.condition
        )
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SED: Suggest enum dispatch — fn with `output_mode` param (ends_with("_mode"))
//      that branches on it, and cyclomatic complexity ≥ 3.
//      suggest_enum_dispatch checks:
//        1. param name matches a flag heuristic (output_mode ends_with "_mode" ✓)
//        2. cyclomatic_complexity >= 3  (the if/else chain gives cc=4 ✓)
//        3. source contains "if output_mode" (matches "if " + param ✓)
// ─────────────────────────────────────────────────────────────────────────────

pub fn format_output(data: &str, output_mode: &str) -> String {
    if output_mode == "json" {
        format!("{{\"data\": \"{}\"}}", data)
    } else if output_mode == "csv" {
        format!("data,{}", data)
    } else if output_mode == "xml" {
        format!("<data>{}</data>", data)
    } else {
        data.to_string()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// SN: Suggest newtype — struct with exactly 1 field and no methods.
// ─────────────────────────────────────────────────────────────────────────────

pub struct UserId {
    pub value: u64,
}

pub struct EmailAddress {
    pub value: String,
}

// ═══════════════════════════════════════════════════════════════════════════
// Test cases for passthrough wrapper false positive suppression
// ═══════════════════════════════════════════════════════════════════════════

// ── FP #2: Default::default() calling new() ────────────────────────────────
// The Default trait REQUIRES `fn default() -> Self` to exist. This is the
// idiomatic Rust pattern and should never be flagged as a passthrough.

pub struct Cache {
    pub entries: Vec<String>,
    pub max_size: usize,
}

impl Cache {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
            max_size: 1000,
        }
    }
}

impl Default for Cache {
    fn default() -> Self {
        Self::new()
    }
}

pub struct DataLoader {
    pub batch_size: usize,
    pub retries: u32,
}

impl DataLoader {
    pub fn new() -> Self {
        Self {
            batch_size: 100,
            retries: 3,
        }
    }
}

impl Default for DataLoader {
    fn default() -> Self {
        Self::new()
    }
}

// ── FP #3: Constructors that build structs AND call another function ────────
// These create and return a struct. They also happen to call a logging/init
// function. That's not a passthrough — it's a constructor with a side effect.

pub struct CombineProcessor {
    pub name: String,
    pub span_id: u64,
}

fn log_op_start(name: &str) -> u64 {
    // pretend this returns a tracing span id
    name.len() as u64
}

impl CombineProcessor {
    pub fn new(name: &str) -> Self {
        let span_id = log_op_start(name);
        Self {
            name: name.to_string(),
            span_id,
        }
    }
}

pub struct TeamStatsProcessor {
    pub name: String,
    pub span_id: u64,
}

impl TeamStatsProcessor {
    pub fn new(name: &str) -> Self {
        let span_id = log_op_start(name);
        Self {
            name: name.to_string(),
            span_id,
        }
    }
}

// ── FP #4: Accessor/getter methods accessing self fields ────────────────────
// These are instance methods providing an API over internal state, not
// passthroughs that forward parameters.

pub struct SessionManager {
    pub sessions: Vec<String>,
}

pub struct CacheDir {
    pub path: String,
}

fn dir_size(path: &str) -> u64 {
    path.len() as u64
}

impl CacheDir {
    pub fn size_bytes(&self) -> u64 {
        dir_size(&self.path)
    }
}

impl SessionManager {
    pub fn session_count(&self) -> usize {
        self.sessions.len()
    }
}

// ── FP #5: Named constructors with ..Default::default() ────────────────────
// These use struct update syntax for convenience but override specific fields.
// They're intentional API surface, not passthroughs.

pub struct PacingConfig {
    pub enabled: bool,
    pub rate_limit: u32,
    pub burst_size: u32,
    pub window_ms: u64,
}

impl Default for PacingConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            rate_limit: 100,
            burst_size: 10,
            window_ms: 1000,
        }
    }
}

impl PacingConfig {
    pub fn disabled() -> Self {
        Self {
            enabled: false,
            ..Default::default()
        }
    }

    pub fn aggressive() -> Self {
        Self {
            rate_limit: 1000,
            burst_size: 100,
            ..Default::default()
        }
    }
}

// ── TRUE POSITIVE: An actual passthrough that SHOULD be flagged ─────────────
// This function literally just delegates to another with the same params.

pub fn validate_input(data: &str) -> bool {
    check_format(data)
}

fn check_format(data: &str) -> bool {
    !data.is_empty() && data.len() < 1000
}

// ── TRUE POSITIVE: Another genuine passthrough ─────────────────────────────

pub fn save_record(id: u64, payload: &str) -> bool {
    persist_to_db(id, payload)
}

fn persist_to_db(id: u64, payload: &str) -> bool {
    id > 0 && !payload.is_empty()
}

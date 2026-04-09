// ═══════════════════════════════════════════════════════════════════════════
// Test cases for Rust-specific false positive suppression
// ═══════════════════════════════════════════════════════════════════════════

// ── Trait definitions ───────────────────────────────────────────────────────

pub trait DataSource {
    fn sport_id(&self) -> u32;
    fn db_manager(&self) -> &str;
    fn has_position_tracking(&self) -> bool;
}

pub trait SportScrapingStrategy {
    fn get_team_abbr(&self, team: &str) -> String;
    fn get_espn_slug(&self) -> &str;
    fn uses_cross_year_season(&self) -> bool;
}

// ── Structs ─────────────────────────────────────────────────────────────────

pub struct MLB;
pub struct NBA;
pub struct NFL;
pub struct NHL;

// ── FP #1: Trait impl methods — same trait on different types ───────────────
// These MUST NOT be flagged as near-duplicates. Rust requires each impl to
// have its own method body; there's no way to share them.

impl DataSource for MLB {
    fn sport_id(&self) -> u32 { 1 }
    fn db_manager(&self) -> &str { "sports_db" }
    fn has_position_tracking(&self) -> bool { true }
}

impl DataSource for NBA {
    fn sport_id(&self) -> u32 { 2 }
    fn db_manager(&self) -> &str { "sports_db" }
    fn has_position_tracking(&self) -> bool { true }
}

impl DataSource for NFL {
    fn sport_id(&self) -> u32 { 3 }
    fn db_manager(&self) -> &str { "sports_db" }
    fn has_position_tracking(&self) -> bool { true }
}

impl DataSource for NHL {
    fn sport_id(&self) -> u32 { 4 }
    fn db_manager(&self) -> &str { "sports_db" }
    fn has_position_tracking(&self) -> bool { true }
}

// ── FP #1 continued: Display trait impls ────────────────────────────────────

impl std::fmt::Display for MLB {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MLB")
    }
}

impl std::fmt::Display for NBA {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NBA")
    }
}

impl std::fmt::Display for NFL {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "NFL")
    }
}

// ── FP #6: Same-name factory constructors on different types ────────────────
// These return Self, so they can't be shared across types.

pub struct ESPNApiClient {
    pub sport: String,
}

pub struct ESPNGameScraper {
    pub sport: String,
}

impl ESPNApiClient {
    pub fn football() -> Self {
        Self { sport: "football".to_string() }
    }

    pub fn basketball() -> Self {
        Self { sport: "basketball".to_string() }
    }

    pub fn baseball() -> Self {
        Self { sport: "baseball".to_string() }
    }

    pub fn hockey() -> Self {
        Self { sport: "hockey".to_string() }
    }
}

impl ESPNGameScraper {
    pub fn football() -> Self {
        Self { sport: "football".to_string() }
    }

    pub fn basketball() -> Self {
        Self { sport: "basketball".to_string() }
    }

    pub fn baseball() -> Self {
        Self { sport: "baseball".to_string() }
    }

    pub fn hockey() -> Self {
        Self { sport: "hockey".to_string() }
    }
}

// ── TRUE POSITIVE: Free functions that ARE genuine near-duplicates ──────────
// These should STILL be flagged — they're not trait impls, not on different
// types, and could be refactored into a shared function.

pub fn format_user_record(name: &str, email: &str, role: &str) -> String {
    let cleaned_name = name.trim().to_lowercase();
    let cleaned_email = email.trim().to_lowercase();
    let cleaned_role = role.trim().to_lowercase();
    format!("{},{},{}", cleaned_name, cleaned_email, cleaned_role)
}

pub fn format_customer_record(name: &str, email: &str, role: &str) -> String {
    let cleaned_name = name.trim().to_lowercase();
    let cleaned_email = email.trim().to_lowercase();
    let cleaned_role = role.trim().to_lowercase();
    format!("{},{},{}", cleaned_name, cleaned_email, cleaned_role)
}

use std::collections::HashMap;

// ── Magic numbers (Check 65) ─────────────────────────────────────────────────
// MagicNumber fires when a function contains 3+ numeric literals that are not
// in the allowed set {0,1,2,-1,100,...}.

pub fn calculate_retry_delay(attempt: u32) -> u64 {
    // 500, 1500, 3600, 86400 are all magic numbers
    if attempt == 0 {
        500
    } else if attempt < 5 {
        1500
    } else if attempt < 10 {
        3600
    } else {
        86400
    }
}

pub fn compute_buffer_size(items: usize) -> usize {
    // 4096, 8192, 65536 are magic numbers
    if items < 4096 {
        8192
    } else {
        65536
    }
}

// ── Long parameter list (Check 30) ───────────────────────────────────────────
// LongParameterList fires at 6+ non-self params.

pub fn create_report(
    title: &str,
    author: &str,
    department: &str,
    year: u32,
    quarter: u32,
    include_charts: bool,
    include_appendix: bool,
) -> String {
    format!("{} by {} ({}/{} Q{})", title, author, department, year, quarter)
}

// ── Boolean blindness (Check 60) ─────────────────────────────────────────────
// BooleanBlindness fires when 2+ params start with is_/has_/should_/etc.

pub fn render_cell(
    value: &str,
    is_header: bool,
    is_selected: bool,
    is_editable: bool,
) -> String {
    format!("<td>{}</td>", value)
}

pub fn send_message(
    recipient: &str,
    body: &str,
    is_urgent: bool,
    should_archive: bool,
) -> bool {
    !recipient.is_empty() && !body.is_empty()
}

// ── Tech debt comments (Check 102) ───────────────────────────────────────────
// TechDebtComment fires on // TODO, // FIXME, // HACK, // XXX inside a fn.

pub fn parse_legacy_format(input: &[u8]) -> Vec<u8> {
    // TODO: replace this with the new binary protocol parser
    // FIXME: panics on inputs longer than 64KB
    // HACK: prepend a null byte because downstream expects it
    let mut result = vec![0u8];
    result.extend_from_slice(input);
    result
}

pub fn normalize_path(path: &str) -> String {
    // FIXME: does not handle UNC paths on Windows
    path.replace('\\', "/")
}

// ── Vec used as map (Check 93) ───────────────────────────────────────────────
// VecUsedAsMap fires on `.iter().find(|(` pattern.

pub fn find_header(headers: &[(String, String)], name: &str) -> Option<String> {
    headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name))
        .map(|(_, v)| v.clone())
}

pub fn lookup_env_override(overrides: &[(String, String)], key: &str) -> Option<&str> {
    overrides.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str())
}

// ── HashMap with sequential integer keys (Check 98) ──────────────────────────
// HashMapWithSequentialKeys fires when .insert(0,..), .insert(1,..), etc.
// appear on the same receiver across 3+ sequential keys.

pub fn build_status_codes() -> HashMap<usize, &'static str> {
    let mut map = HashMap::new();
    map.insert(0, "pending");
    map.insert(1, "running");
    map.insert(2, "success");
    map.insert(3, "failed");
    map
}

pub fn make_weekday_map() -> HashMap<usize, &'static str> {
    let mut days = HashMap::new();
    days.insert(0, "Monday");
    days.insert(1, "Tuesday");
    days.insert(2, "Wednesday");
    days.insert(3, "Thursday");
    days.insert(4, "Friday");
    days
}

// ── Data clumps (Check 31) ───────────────────────────────────────────────────
// DataClump fires when the same 3 param names appear together in 3+ functions.
// Clump: (recipient, subject, body)

pub fn send_email(recipient: &str, subject: &str, body: &str) -> bool {
    !recipient.is_empty() && !subject.is_empty() && !body.is_empty()
}

pub fn validate_email(recipient: &str, subject: &str, body: &str) -> bool {
    recipient.contains('@') && !subject.is_empty() && !body.is_empty()
}

pub fn log_email(recipient: &str, subject: &str, body: &str) {
    println!("To: {recipient} | Subject: {subject} | Len: {}", body.len());
}

pub fn queue_email(recipient: &str, subject: &str, body: &str, delay_secs: u64) {
    println!("Queued to {recipient} in {delay_secs}s: {subject}");
}

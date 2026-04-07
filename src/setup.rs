//! `ast-context setup` — auto-configure the MCP server for supported editors/AI tools.
//!
//! Supported targets:
//!   - Claude Desktop  (macOS & Windows)
//!   - Claude Code     (via `claude mcp add` CLI)
//!   - Zed             (~/.config/zed/settings.json)
//!   - Cursor          (~/.cursor/mcp.json)

use std::path::{Path, PathBuf};

// ── Public entry point ──────────────────────────────────────────────────────

pub fn run(mcp_path: Option<PathBuf>, dry_run: bool) {
    if dry_run {
        println!("Dry run — no files will be modified.\n");
    }

    let mcp_bin = match resolve_mcp_binary(mcp_path) {
        Some(p) => p,
        None => {
            eprintln!(
                "Error: could not find ast_context binary.\n\
                 Make sure it is installed:\n\
                 \n\
                 \tcargo install ast_context\n\
                 \n\
                 Or specify its path with --mcp-path <path>."
            );
            std::process::exit(1);
        }
    };

    println!("Using binary: {} mcp\n", mcp_bin.display());

    let mut configured = 0usize;
    let mut skipped = 0usize;

    for target in all_targets() {
        match target.detect() {
            DetectResult::NotInstalled => {
                println!("  [ ] {} — not detected, skipping", target.name());
            }
            DetectResult::AlreadyConfigured => {
                println!("  [✓] {} — already configured", target.name());
                skipped += 1;
            }
            DetectResult::ConfigFound(config_path) => {
                if dry_run {
                    println!(
                        "  [~] {} — would configure: {}",
                        target.name(),
                        config_path.display()
                    );
                    configured += 1;
                } else {
                    match target.configure(&config_path, &mcp_bin) {
                        Ok(()) => {
                            println!(
                                "  [✓] {} — configured: {}",
                                target.name(),
                                config_path.display()
                            );
                            configured += 1;
                        }
                        Err(e) => {
                            eprintln!("  [!] {} — failed: {e}", target.name());
                        }
                    }
                }
            }
            DetectResult::UseCommand => {
                if dry_run {
                    println!(
                        "  [~] {} — would run: {}",
                        target.name(),
                        target.configure_command(&mcp_bin)
                    );
                    configured += 1;
                } else {
                    match target.run_configure_command(&mcp_bin) {
                        Ok(()) => {
                            println!("  [✓] {} — configured", target.name());
                            configured += 1;
                        }
                        Err(e) => {
                            eprintln!("  [!] {} — failed: {e}", target.name());
                        }
                    }
                }
            }
        }
    }

    println!();
    if dry_run {
        println!(
            "Would configure {} target(s), {} already set up.",
            configured, skipped
        );
    } else if configured == 0 && skipped == 0 {
        println!(
            "No supported editors detected.\n\
             Supported: Claude Desktop, Claude Code, Zed, Cursor.\n\
             You can configure manually — see: https://github.com/agene0001/AstBasedContext-rs#mcp-server"
        );
    } else {
        println!(
            "Done. Configured {} target(s), {} already set up.",
            configured, skipped
        );
        if configured > 0 {
            println!("Restart your editor(s) for the changes to take effect.");
        }
    }
}

// ── Binary resolution ───────────────────────────────────────────────────────

fn resolve_mcp_binary(override_path: Option<PathBuf>) -> Option<PathBuf> {
    if let Some(p) = override_path {
        if p.exists() {
            return Some(p);
        }
        eprintln!(
            "Warning: specified --mcp-path does not exist: {}",
            p.display()
        );
        return None;
    }

    // 1. ~/.cargo/bin/ast_context[.exe]
    if let Some(home) = dirs::home_dir() {
        let bin = home.join(".cargo").join("bin").join(ast_context_exe_name());
        if bin.exists() {
            return Some(bin);
        }
    }

    // 2. Same directory as this binary (i.e. the current running binary itself)
    if let Ok(current_exe) = std::env::current_exe() {
        return Some(current_exe);
    }

    // 3. PATH lookup
    which_ast_context()
}

fn ast_context_exe_name() -> &'static str {
    if cfg!(windows) {
        "ast_context.exe"
    } else {
        "ast_context"
    }
}

fn which_ast_context() -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var).find_map(|dir| {
            let candidate = dir.join(ast_context_exe_name());
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

// ── Target abstraction ──────────────────────────────────────────────────────

enum DetectResult {
    NotInstalled,
    AlreadyConfigured,
    ConfigFound(PathBuf),
    UseCommand,
}

trait McpTarget {
    fn name(&self) -> &'static str;
    fn detect(&self) -> DetectResult;
    fn configure(&self, _config_path: &Path, _mcp_bin: &Path) -> Result<(), String> {
        Err("this target does not use file-based configuration".into())
    }
    fn configure_command(&self, _mcp_bin: &Path) -> String {
        String::new()
    }
    fn run_configure_command(&self, _mcp_bin: &Path) -> Result<(), String> {
        Err("not supported".into())
    }
}

fn all_targets() -> Vec<Box<dyn McpTarget>> {
    vec![
        Box::new(ClaudeDesktop),
        Box::new(ClaudeCode),
        Box::new(Zed),
        Box::new(Cursor),
        Box::new(Windsurf),
        Box::new(VsCode),
        Box::new(JetBrains),
    ]
}

// ── Claude Desktop ──────────────────────────────────────────────────────────

struct ClaudeDesktop;

impl McpTarget for ClaudeDesktop {
    fn name(&self) -> &'static str {
        "Claude Desktop"
    }

    fn detect(&self) -> DetectResult {
        let Some(config_path) = claude_desktop_config_path() else {
            return DetectResult::NotInstalled;
        };
        if !config_path.parent().is_some_and(|p| p.exists()) {
            return DetectResult::NotInstalled;
        }
        if config_path.exists()
            && json_has_ast_context(&config_path, &["mcpServers", "ast-context"])
        {
            return DetectResult::AlreadyConfigured;
        }
        DetectResult::ConfigFound(config_path)
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        let mut root = read_or_empty_object(config_path)?;
        let root_obj = root
            .as_object_mut()
            .ok_or("config root is not a JSON object")?;
        let servers = root_obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = servers
            .as_object_mut()
            .ok_or("mcpServers is not an object")?;
        map.insert(
            "ast-context".into(),
            serde_json::json!({
                "command": mcp_bin.to_string_lossy(),
                "args": ["mcp"]
            }),
        );
        write_json(config_path, &root)
    }
}

fn claude_desktop_config_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("Claude")
                .join("claude_desktop_config.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("Claude").join("claude_desktop_config.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        // Linux: Claude Desktop is not officially supported, but check anyway.
        dirs::config_dir().map(|c| c.join("Claude").join("claude_desktop_config.json"))
    }
}

// ── Claude Code ─────────────────────────────────────────────────────────────

struct ClaudeCode;

impl McpTarget for ClaudeCode {
    fn name(&self) -> &'static str {
        "Claude Code"
    }

    fn detect(&self) -> DetectResult {
        // Claude Code is present if the `claude` CLI is in PATH.
        if which("claude").is_none() {
            return DetectResult::NotInstalled;
        }
        // Check if already configured by looking at `claude mcp list` output.
        // If the command fails or output doesn't contain ast-context, configure it.
        let already = std::process::Command::new("claude")
            .args(["mcp", "list"])
            .output()
            .ok()
            .map(|o| String::from_utf8_lossy(&o.stdout).contains("ast-context"))
            .unwrap_or(false);
        if already {
            DetectResult::AlreadyConfigured
        } else {
            DetectResult::UseCommand
        }
    }

    fn configure_command(&self, mcp_bin: &Path) -> String {
        format!("claude mcp add ast-context {} mcp", mcp_bin.display())
    }

    fn run_configure_command(&self, mcp_bin: &Path) -> Result<(), String> {
        let status = std::process::Command::new("claude")
            .args([
                "mcp",
                "add",
                "ast-context",
                &mcp_bin.to_string_lossy(),
                "mcp",
            ])
            .status()
            .map_err(|e| format!("failed to run `claude mcp add`: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("`claude mcp add` exited with status {status}"))
        }
    }
}

// ── Zed ─────────────────────────────────────────────────────────────────────

struct Zed;

impl McpTarget for Zed {
    fn name(&self) -> &'static str {
        "Zed"
    }

    fn detect(&self) -> DetectResult {
        let Some(config_path) = zed_settings_path() else {
            return DetectResult::NotInstalled;
        };
        // Zed is "installed" if its config dir exists or the binary is in PATH.
        let config_dir_exists = config_path.parent().is_some_and(|p| p.exists());
        if !config_dir_exists && which("zed").is_none() {
            return DetectResult::NotInstalled;
        }
        if config_path.exists()
            && json_has_ast_context(&config_path, &["context_servers", "ast-context"])
        {
            return DetectResult::AlreadyConfigured;
        }
        DetectResult::ConfigFound(config_path)
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        let mut root = read_or_empty_object(config_path)?;
        let root_obj = root
            .as_object_mut()
            .ok_or("config root is not a JSON object")?;
        let servers = root_obj
            .entry("context_servers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = servers
            .as_object_mut()
            .ok_or("context_servers is not an object")?;
        map.insert(
            "ast-context".into(),
            serde_json::json!({
                "command": {
                    "path": mcp_bin.to_string_lossy(),
                    "args": ["mcp"]
                }
            }),
        );
        write_json(config_path, &root)
    }
}

fn zed_settings_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| h.join(".config").join("zed").join("settings.json"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        dirs::config_dir().map(|c| c.join("zed").join("settings.json"))
    }
}

// ── Cursor ───────────────────────────────────────────────────────────────────

struct Cursor;

impl McpTarget for Cursor {
    fn name(&self) -> &'static str {
        "Cursor"
    }

    fn detect(&self) -> DetectResult {
        let Some(config_path) = cursor_mcp_path() else {
            return DetectResult::NotInstalled;
        };
        if !config_path.parent().is_some_and(|p| p.exists()) {
            return DetectResult::NotInstalled;
        }
        if config_path.exists()
            && json_has_ast_context(&config_path, &["mcpServers", "ast-context"])
        {
            return DetectResult::AlreadyConfigured;
        }
        DetectResult::ConfigFound(config_path)
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        let mut root = read_or_empty_object(config_path)?;
        let root_obj = root
            .as_object_mut()
            .ok_or("config root is not a JSON object")?;
        let servers = root_obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = servers
            .as_object_mut()
            .ok_or("mcpServers is not an object")?;
        map.insert(
            "ast-context".into(),
            serde_json::json!({
                "command": mcp_bin.to_string_lossy(),
                "args": ["mcp"]
            }),
        );
        write_json(config_path, &root)
    }
}

fn cursor_mcp_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".cursor").join("mcp.json"))
}

// ── Windsurf ─────────────────────────────────────────────────────────────────

struct Windsurf;

impl McpTarget for Windsurf {
    fn name(&self) -> &'static str {
        "Windsurf"
    }

    fn detect(&self) -> DetectResult {
        let Some(config_path) = windsurf_mcp_path() else {
            return DetectResult::NotInstalled;
        };
        if !config_path.parent().is_some_and(|p| p.exists()) && which("windsurf").is_none() {
            return DetectResult::NotInstalled;
        }
        if config_path.exists()
            && json_has_ast_context(&config_path, &["mcpServers", "ast-context"])
        {
            return DetectResult::AlreadyConfigured;
        }
        DetectResult::ConfigFound(config_path)
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        let mut root = read_or_empty_object(config_path)?;
        let root_obj = root
            .as_object_mut()
            .ok_or("config root is not a JSON object")?;
        let servers = root_obj
            .entry("mcpServers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = servers
            .as_object_mut()
            .ok_or("mcpServers is not an object")?;
        map.insert(
            "ast-context".into(),
            serde_json::json!({
                "command": mcp_bin.to_string_lossy(),
                "args": ["mcp"]
            }),
        );
        write_json(config_path, &root)
    }
}

fn windsurf_mcp_path() -> Option<PathBuf> {
    dirs::home_dir().map(|h| h.join(".codeium").join("windsurf").join("mcp_config.json"))
}

// ── VS Code (GitHub Copilot) ──────────────────────────────────────────────────

struct VsCode;

impl McpTarget for VsCode {
    fn name(&self) -> &'static str {
        "VS Code (GitHub Copilot)"
    }

    fn detect(&self) -> DetectResult {
        let Some(config_path) = vscode_mcp_path() else {
            return DetectResult::NotInstalled;
        };
        if which("code").is_none() && !config_path.parent().is_some_and(|p| p.exists()) {
            return DetectResult::NotInstalled;
        }
        if config_path.exists() && json_has_ast_context(&config_path, &["servers", "ast-context"]) {
            return DetectResult::AlreadyConfigured;
        }
        DetectResult::ConfigFound(config_path)
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        let mut root = read_or_empty_object(config_path)?;
        let root_obj = root
            .as_object_mut()
            .ok_or("config root is not a JSON object")?;
        // VS Code mcp.json uses "inputs" + "servers" (not "mcpServers")
        root_obj
            .entry("inputs")
            .or_insert_with(|| serde_json::json!([]));
        let servers = root_obj
            .entry("servers")
            .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
        let map = servers.as_object_mut().ok_or("servers is not an object")?;
        map.insert(
            "ast-context".into(),
            serde_json::json!({
                "type": "stdio",
                "command": mcp_bin.to_string_lossy(),
                "args": ["mcp"]
            }),
        );
        write_json(config_path, &root)
    }
}

fn vscode_mcp_path() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("Code")
                .join("User")
                .join("mcp.json")
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("Code").join("User").join("mcp.json"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|c| c.join("Code").join("User").join("mcp.json"))
    }
}

// ── JetBrains (IntelliJ, PyCharm, GoLand, WebStorm, …) ──────────────────────

struct JetBrains;

impl McpTarget for JetBrains {
    fn name(&self) -> &'static str {
        "JetBrains IDEs"
    }

    fn detect(&self) -> DetectResult {
        let dirs = jetbrains_config_dirs();
        if dirs.is_empty() {
            return DetectResult::NotInstalled;
        }
        // Already configured if ANY JetBrains IDE has the entry.
        let already = dirs.iter().any(|dir| {
            let mcp = dir.join("mcp.json");
            mcp.exists() && json_has_ast_context(&mcp, &["mcpServers", "ast-context"])
        });
        if already {
            return DetectResult::AlreadyConfigured;
        }
        // Use the first detected dir as the target.
        DetectResult::ConfigFound(dirs[0].join("mcp.json"))
    }

    fn configure(&self, config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
        // Configure all detected JetBrains IDEs.
        let dirs = jetbrains_config_dirs();
        let targets: Vec<PathBuf> = if dirs.is_empty() {
            vec![config_path.to_path_buf()]
        } else {
            dirs.iter().map(|d| d.join("mcp.json")).collect()
        };

        let mut errors = Vec::new();
        for path in &targets {
            if let Err(e) = configure_jetbrains_mcp(path, mcp_bin) {
                errors.push(format!("{}: {e}", path.display()));
            }
        }
        if errors.is_empty() {
            Ok(())
        } else {
            Err(errors.join("; "))
        }
    }
}

fn configure_jetbrains_mcp(config_path: &Path, mcp_bin: &Path) -> Result<(), String> {
    let mut root = read_or_empty_object(config_path)?;
    let root_obj = root
        .as_object_mut()
        .ok_or("config root is not a JSON object")?;
    let servers = root_obj
        .entry("mcpServers")
        .or_insert_with(|| serde_json::Value::Object(serde_json::Map::new()));
    let map = servers
        .as_object_mut()
        .ok_or("mcpServers is not an object")?;
    map.insert(
        "ast-context".into(),
        serde_json::json!({
            "command": mcp_bin.to_string_lossy(),
            "args": ["mcp"]
        }),
    );
    write_json(config_path, &root)
}

/// Find all JetBrains per-IDE config directories on the current platform.
/// JetBrains stores config under a versioned path like `IntelliJIdea2025.1/`.
fn jetbrains_config_dirs() -> Vec<PathBuf> {
    let base = jetbrains_base_dir();
    let Some(base) = base else { return Vec::new() };
    if !base.exists() {
        return Vec::new();
    }
    let Ok(entries) = std::fs::read_dir(&base) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_dir() {
                Some(path)
            } else {
                None
            }
        })
        .collect();
    dirs.sort();
    dirs
}

fn jetbrains_base_dir() -> Option<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        dirs::home_dir().map(|h| {
            h.join("Library")
                .join("Application Support")
                .join("JetBrains")
        })
    }
    #[cfg(target_os = "windows")]
    {
        dirs::data_dir().map(|d| d.join("JetBrains"))
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        dirs::config_dir().map(|c| c.join("JetBrains"))
    }
}

// ── JSON helpers ─────────────────────────────────────────────────────────────

fn read_or_empty_object(path: &Path) -> Result<serde_json::Value, String> {
    if !path.exists() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    let content = std::fs::read_to_string(path)
        .map_err(|e| format!("failed to read {}: {e}", path.display()))?;

    let stripped = strip_json_comments(&content);
    let no_commas = strip_trailing_commas(&stripped);
    let trimmed = no_commas.trim();

    if trimmed.is_empty() {
        return Ok(serde_json::Value::Object(serde_json::Map::new()));
    }
    serde_json::from_str(trimmed).map_err(|e| format!("failed to parse {}: {e}", path.display()))
}

fn strip_trailing_commas(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();
    let mut in_string = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
        } else {
            if c == '"' {
                in_string = true;
                out.push(c);
            } else if c == '}' || c == ']' {
                let mut pop_count = 0;
                let mut found_comma = false;
                for ch in out.chars().rev() {
                    if ch.is_whitespace() {
                        pop_count += ch.len_utf8();
                    } else if ch == ',' {
                        found_comma = true;
                        pop_count += ch.len_utf8();
                        break;
                    } else {
                        break;
                    }
                }
                if found_comma {
                    let new_len = out.len() - pop_count;
                    out.truncate(new_len);
                }
                out.push(c);
            } else {
                out.push(c);
            }
        }
    }
    out
}

fn strip_json_comments(json: &str) -> String {
    let mut out = String::with_capacity(json.len());
    let mut chars = json.chars().peekable();
    let mut in_string = false;
    let mut in_line_comment = false;
    let mut in_block_comment = false;

    while let Some(c) = chars.next() {
        if in_string {
            out.push(c);
            if c == '\\' {
                if let Some(next) = chars.next() {
                    out.push(next);
                }
            } else if c == '"' {
                in_string = false;
            }
        } else if in_line_comment {
            if c == '\n' {
                in_line_comment = false;
                out.push(c);
            }
        } else if in_block_comment {
            if c == '*' && chars.peek() == Some(&'/') {
                chars.next();
                in_block_comment = false;
            }
        } else {
            if c == '"' {
                in_string = true;
                out.push(c);
            } else if c == '/' && chars.peek() == Some(&'/') {
                chars.next();
                in_line_comment = true;
            } else if c == '/' && chars.peek() == Some(&'*') {
                chars.next();
                in_block_comment = true;
            } else {
                out.push(c);
            }
        }
    }
    out
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .map_err(|e| format!("failed to create directory {}: {e}", parent.display()))?;
    }
    let content = serde_json::to_string_pretty(value)
        .map_err(|e| format!("failed to serialise JSON: {e}"))?;
    std::fs::write(path, content).map_err(|e| format!("failed to write {}: {e}", path.display()))
}

/// Return true if the JSON at `path` already has a non-null value at the given key path.
fn json_has_ast_context(path: &Path, key_path: &[&str]) -> bool {
    let Ok(content) = std::fs::read_to_string(path) else {
        return false;
    };
    let Ok(value) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let mut current = &value;
    for key in key_path {
        match current.get(key) {
            Some(v) => current = v,
            None => return false,
        }
    }
    !current.is_null()
}

fn which(binary: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{binary}.exe")
    } else {
        binary.to_string()
    };
    std::env::var_os("PATH").and_then(|path_var| {
        std::env::split_paths(&path_var).find_map(|dir| {
            let candidate = dir.join(&exe);
            if candidate.exists() {
                Some(candidate)
            } else {
                None
            }
        })
    })
}

//! A minimal, hand-rolled LSP client and the [`LspProvider`] that adapts it to
//! [`SemanticProvider`]. No new dependencies — JSON-RPC over the child's
//! stdio, framed exactly like the existing MCP server, using `serde_json`.
//!
//! Scope is deliberately small: spawn a language server (rust-analyzer), run the
//! `initialize` handshake, wait for indexing to finish, and answer
//! `textDocument/references`. That's the one query the dead-code check needs to
//! replace its textual reference-guard heuristic with ground truth. Everything
//! else on the trait stays `None` (falls back to heuristics) until a later phase
//! needs it.
//!
//! Operational reality this client accepts:
//! - The project must build for rust-analyzer to resolve anything; if the server
//!   can't start we return `None` from [`LspProvider::start`] and the engine
//!   runs pure-AST.
//! - Indexing takes seconds; we block on the `rustAnalyzer/Indexing` progress
//!   `end` before answering, so we never query a cold server (a silent
//!   false-negative trap).

use std::collections::HashSet;
use std::io::{self, BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde_json::{json, Value};

use super::{Location, Mutability, SemanticProvider, TypeInfo};

/// How long to wait for the server to finish its initial indexing before giving
/// up and proceeding best-effort.
const READY_TIMEOUT: Duration = Duration::from_secs(90);

/// A synchronous JSON-RPC/LSP client driving one language-server subprocess.
struct LspClient {
    child: Child,
    stdin: ChildStdin,
    reader: BufReader<ChildStdout>,
    next_id: i64,
    /// Document URIs already sent via `textDocument/didOpen`.
    opened: HashSet<String>,
}

impl LspClient {
    /// Spawn `program` rooted at `root` and run the `initialize` handshake.
    fn start(program: &str, root: &Path) -> io::Result<Self> {
        let stderr = if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
            Stdio::inherit()
        } else {
            Stdio::null()
        };
        let mut child = Command::new(program)
            .current_dir(root)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(stderr)
            .spawn()?;

        let stdin = child.stdin.take().ok_or_else(|| broken("no stdin"))?;
        let stdout = child.stdout.take().ok_or_else(|| broken("no stdout"))?;

        let mut client = LspClient {
            child,
            stdin,
            reader: BufReader::new(stdout),
            next_id: 1,
            opened: HashSet::new(),
        };
        client.initialize(root)?;
        Ok(client)
    }

    // ── transport ──────────────────────────────────────────────────────────

    fn send(&mut self, msg: &Value) -> io::Result<()> {
        let body = serde_json::to_string(msg)?;
        write!(self.stdin, "Content-Length: {}\r\n\r\n{}", body.len(), body)?;
        self.stdin.flush()
    }

    /// Read one framed LSP message (headers + JSON body).
    fn read_message(&mut self) -> io::Result<Value> {
        let mut content_length: Option<usize> = None;
        loop {
            let mut line = String::new();
            if self.reader.read_line(&mut line)? == 0 {
                return Err(broken("server closed the connection"));
            }
            let trimmed = line.trim_end_matches(['\r', '\n']);
            if trimmed.is_empty() {
                break; // end of headers
            }
            if let Some(rest) = trimmed
                .strip_prefix("Content-Length:")
                .or_else(|| trimmed.strip_prefix("content-length:"))
            {
                content_length = rest.trim().parse().ok();
            }
        }
        let len = content_length.ok_or_else(|| broken("missing Content-Length"))?;
        let mut buf = vec![0u8; len];
        self.reader.read_exact(&mut buf)?;
        Ok(serde_json::from_slice(&buf)?)
    }

    /// Reply with an empty result to a server→client request (e.g.
    /// `window/workDoneProgress/create`, `client/registerCapability`). Ignoring
    /// these stalls rust-analyzer.
    fn respond_empty(&mut self, id: i64) -> io::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": null}))
    }

    /// Send a request and pump the stream until its response arrives, servicing
    /// any interleaved server requests and ignoring notifications.
    fn request(&mut self, method: &str, params: Value) -> io::Result<Value> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;

        loop {
            let msg = self.read_message()?;
            match (msg.get("id").and_then(Value::as_i64), msg.get("method")) {
                // server→client request: must answer to keep the server moving
                (Some(req_id), Some(_)) => self.respond_empty(req_id)?,
                // the response we're waiting for
                (Some(resp_id), None) if resp_id == id => {
                    if let Some(err) = msg.get("error") {
                        return Err(broken(&format!("lsp {method} error: {err}")));
                    }
                    return Ok(msg.get("result").cloned().unwrap_or(Value::Null));
                }
                // a response to a different id, or a notification: ignore
                _ => {}
            }
        }
    }

    fn notify(&mut self, method: &str, params: Value) -> io::Result<()> {
        self.send(&json!({"jsonrpc": "2.0", "method": method, "params": params}))
    }

    // ── lifecycle ────────────────────────────────────────────────────────────

    fn initialize(&mut self, root: &Path) -> io::Result<()> {
        let root_uri = path_to_uri(root);
        let params = json!({
            "processId": std::process::id(),
            "rootUri": root_uri,
            // Modern rust-analyzer discovers the Cargo workspace via
            // workspaceFolders; rootUri alone leaves it with no project loaded
            // (every query then returns null).
            "workspaceFolders": [{ "uri": root_uri, "name": "root" }],
            "capabilities": {
                "workspace": { "workspaceFolders": true, "configuration": true },
                "textDocument": {
                    "references": { "dynamicRegistration": false }
                },
                "window": { "workDoneProgress": true },
                // rust-analyzer's canonical readiness signal: it sends
                // `experimental/serverStatus` with `quiescent: true` once the
                // project is fully loaded and indexed.
                "experimental": { "serverStatusNotification": true }
            }
        });
        self.request("initialize", params)?;
        self.notify("initialized", json!({}))?;
        self.wait_ready()?;
        Ok(())
    }

    /// Block until rust-analyzer reports the project is loaded and quiescent, so
    /// we never query a cold server (which silently returns `null`). Primary
    /// signal is the `experimental/serverStatus` notification; a completed
    /// indexing `$/progress` is a fallback for servers that don't send it.
    /// Best-effort: proceeds after [`READY_TIMEOUT`].
    fn wait_ready(&mut self) -> io::Result<()> {
        let deadline = Instant::now() + READY_TIMEOUT;
        while Instant::now() < deadline {
            let msg = self.read_message()?;
            // Service server requests so loading isn't blocked on us.
            if let (Some(id), Some(_)) = (msg.get("id").and_then(Value::as_i64), msg.get("method")) {
                self.respond_empty(id)?;
                continue;
            }
            if msg.get("method").and_then(Value::as_str) == Some("experimental/serverStatus")
                && msg.pointer("/params/quiescent").and_then(Value::as_bool) == Some(true)
            {
                if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
                    eprintln!(
                        "lsp serverStatus quiescent (health={:?})",
                        msg.pointer("/params/health")
                    );
                }
                return Ok(());
            }
        }
        Ok(())
    }

    // ── queries ──────────────────────────────────────────────────────────────

    /// `textDocument/didOpen` (once per document) so the server has an explicit
    /// overlay of the on-disk text we're addressing positions in.
    fn ensure_open(&mut self, path: &Path, uri: &str) -> io::Result<()> {
        if self.opened.contains(uri) {
            return Ok(());
        }
        let text = std::fs::read_to_string(path).unwrap_or_default();
        self.notify(
            "textDocument/didOpen",
            json!({
                "textDocument": {
                    "uri": uri,
                    "languageId": "rust",
                    "version": 1,
                    "text": text
                }
            }),
        )?;
        self.opened.insert(uri.to_string());
        Ok(())
    }

    /// Usages of the symbol at `loc` (declaration excluded). `loc.line` is
    /// 1-based, `loc.col` 0-based; LSP wants both 0-based.
    fn references(&mut self, loc: &Location) -> io::Result<Vec<Location>> {
        let path = PathBuf::from(&loc.file);
        let uri = path_to_uri(&path);
        self.ensure_open(&path, &uri)?;

        let params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": loc.line.saturating_sub(1), "character": loc.col },
            "context": { "includeDeclaration": false }
        });
        let result = self.request("textDocument/references", params)?;

        let mut out = Vec::new();
        if let Some(arr) = result.as_array() {
            for item in arr {
                let file = item
                    .get("uri")
                    .and_then(Value::as_str)
                    .map(uri_to_path)
                    .unwrap_or_default();
                let line = item
                    .pointer("/range/start/line")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                let col = item
                    .pointer("/range/start/character")
                    .and_then(Value::as_u64)
                    .unwrap_or(0) as usize;
                out.push(Location { file, line: line + 1, col });
            }
        }
        Ok(out)
    }

    /// Receiver mutability of the method call whose name is at `loc`, read from
    /// the signature rust-analyzer shows on hover. `&mut self` ⇒ `Mutable`; a
    /// `&self` / `self` / `mut self` receiver ⇒ `Shared`; no resolvable
    /// signature ⇒ `None`.
    fn receiver_mutability(&mut self, loc: &Location) -> io::Result<Option<Mutability>> {
        let text = self.hover_markdown(loc)?;
        // `&mut self` is checked first: a signature like `fn f(&self, x: &mut T)`
        // contains `&mut` but not the substring `&mut self`, so it reads Shared.
        Ok(if text.contains("&mut self") {
            Some(Mutability::Mutable)
        } else if text.contains("self") {
            Some(Mutability::Shared)
        } else {
            None
        })
    }

    /// Resolved type of the expression at `loc`, parsed from the type
    /// rust-analyzer shows on hover (e.g. ```` ```rust\nlet x: i32\n``` ````).
    fn type_of(&mut self, loc: &Location) -> io::Result<Option<TypeInfo>> {
        let text = self.hover_markdown(loc)?;
        if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
            eprintln!("lsp hover raw: {text:?}");
        }
        Ok(parse_hover_type(&text).map(|name| TypeInfo { name, is_copy: None, size: None }))
    }

    /// Raw markdown of `textDocument/hover` at `loc`.
    fn hover_markdown(&mut self, loc: &Location) -> io::Result<String> {
        let path = PathBuf::from(&loc.file);
        let uri = path_to_uri(&path);
        self.ensure_open(&path, &uri)?;

        let result = self.request(
            "textDocument/hover",
            json!({
                "textDocument": { "uri": uri },
                "position": { "line": loc.line.saturating_sub(1), "character": loc.col }
            }),
        )?;
        Ok(result
            .pointer("/contents/value")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string())
    }

    fn shutdown(&mut self) {
        let _ = self.request("shutdown", Value::Null);
        let _ = self.notify("exit", Value::Null);
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for LspClient {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// An [`SemanticProvider`] backed by a live language server. Construct it once
/// per workspace and keep it warm; [`start`](LspProvider::start) returns `None`
/// when the server can't be launched, so callers degrade to AST cleanly.
pub struct LspProvider {
    client: Mutex<LspClient>,
}

impl LspProvider {
    /// Start rust-analyzer for `root`. Returns `None` if the binary isn't on
    /// `PATH` or the handshake fails — never panics, never blocks forever.
    ///
    /// The server binary defaults to `rust-analyzer` but can be overridden with
    /// `AST_CONTEXT_RUST_ANALYZER`, which is useful when the `rust-analyzer` on
    /// `PATH` is a rustup proxy with no component installed for the active
    /// toolchain.
    pub fn start(root: &Path) -> Option<Self> {
        let program = std::env::var("AST_CONTEXT_RUST_ANALYZER")
            .unwrap_or_else(|_| "rust-analyzer".to_string());
        Self::start_with(&program, root)
    }

    /// Start an arbitrary server program (used by tests).
    pub fn start_with(program: &str, root: &Path) -> Option<Self> {
        match LspClient::start(program, root) {
            Ok(client) => Some(LspProvider { client: Mutex::new(client) }),
            Err(e) => {
                if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
                    eprintln!("LspProvider::start failed: {e}");
                }
                None
            }
        }
    }
}

impl SemanticProvider for LspProvider {
    fn is_available(&self) -> bool {
        true
    }

    fn references(&self, loc: &Location) -> Option<Vec<Location>> {
        let mut client = self.client.lock().ok()?;
        match client.references(loc) {
            Ok(refs) => Some(refs),
            Err(e) => {
                if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
                    eprintln!("lsp references failed: {e}");
                }
                None
            }
        }
    }

    fn receiver_mutability(&self, loc: &Location) -> Option<Mutability> {
        let mut client = self.client.lock().ok()?;
        match client.receiver_mutability(loc) {
            Ok(m) => m,
            Err(e) => {
                if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
                    eprintln!("lsp receiver_mutability failed: {e}");
                }
                None
            }
        }
    }

    fn type_of(&self, loc: &Location) -> Option<TypeInfo> {
        let mut client = self.client.lock().ok()?;
        match client.type_of(loc) {
            Ok(t) => t,
            Err(e) => {
                if std::env::var_os("AST_CONTEXT_LSP_DEBUG").is_some() {
                    eprintln!("lsp type_of failed: {e}");
                }
                None
            }
        }
    }

    /// Stock LSP has no "does `T: Copy`?" query, so this answers only for the
    /// known-`Copy` primitive *value* types — `Some(true)` for those, `None`
    /// (unknown) otherwise. Deliberately conservative: a user `#[derive(Copy)]`
    /// type reads as unknown rather than a guess, and references are excluded so
    /// the consumer never mistakes a deref-clone for a redundant one.
    fn is_copy(&self, ty: &TypeInfo) -> Option<bool> {
        if COPY_PRIMITIVES.contains(&ty.name.trim()) {
            Some(true)
        } else {
            None
        }
    }
}

/// `Copy` primitive value types. References are intentionally absent:
/// `(&x).clone()` auto-derefs and clones the pointee, so it isn't redundant.
const COPY_PRIMITIVES: &[&str] = &[
    "i8", "i16", "i32", "i64", "i128", "isize", "u8", "u16", "u32", "u64", "u128", "usize", "f32",
    "f64", "bool", "char", "()",
];

/// Extract the type from rust-analyzer hover text. Works for both the fenced
/// markdown form (```` ```rust\nlet x: i32\n``` ````) and the plaintext form
/// rust-analyzer emits by default, which looks like:
///
/// ```text
/// crate::path::SourceSpan
///
/// pub start_line: u32
///
/// size = 4, align = 0x4, …
/// ```
///
/// The binding is the first non-comment line carrying a `name: Type`
/// annotation; we skip the leading path line (`::`, no `: `) and trailing
/// `size = …` notes, then take the text after the binding colon.
fn parse_hover_type(md: &str) -> Option<String> {
    let line = md
        .lines()
        .map(|l| l.trim().trim_start_matches("```rust").trim_start_matches("```").trim())
        .find(|l| !l.is_empty() && !l.starts_with("//") && l.contains(": "))?;
    let after = &line[line.find(": ")? + 2..];
    let ty = after
        .split(" = ")
        .next()
        .unwrap_or(after)
        .trim()
        .trim_end_matches([';', ',']);
    (!ty.is_empty()).then(|| ty.to_string())
}

// ── helpers ──────────────────────────────────────────────────────────────────

fn broken(msg: &str) -> io::Error {
    io::Error::other(msg.to_string())
}

/// `file://` URI for an absolute path, percent-encoding everything outside the
/// URI unreserved set (path separators kept). Critical when the path contains a
/// space or other special byte — an unencoded URI won't match the server's
/// internal file id, and queries silently return nothing.
fn path_to_uri(path: &Path) -> String {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut out = String::from("file://");
    for &b in abs.to_string_lossy().as_bytes() {
        match b {
            b'/' | b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

fn uri_to_path(uri: &str) -> String {
    let s = uri.strip_prefix("file://").unwrap_or(uri);
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let Ok(b) = u8::from_str_radix(&s[i + 1..i + 3], 16) {
                out.push(b);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spawns the real rust-analyzer on *this* crate and asserts it resolves
    // references for a known, definitely-used symbol. Ignored by default: it's
    // slow (indexing) and requires rust-analyzer on PATH. Run with:
    //   cargo test --features lsp -- --ignored lsp_references_live
    #[test]
    #[ignore]
    fn lsp_references_live() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        // Allow overriding the server binary; the rustup `rust-analyzer` proxy is
        // broken on machines without the component installed for the default
        // toolchain.
        let program = std::env::var("AST_CONTEXT_LSP_RA").unwrap_or_else(|_| "rust-analyzer".into());
        let provider = LspProvider::start_with(&program, &root).expect("rust-analyzer should start");
        assert!(provider.is_available());

        // `SourceSpan` is declared in src/types/node.rs and used widely.
        let file = root.join("src/types/node.rs");
        let text = std::fs::read_to_string(&file).unwrap();
        let (line0, col) = text
            .lines()
            .enumerate()
            .find_map(|(i, l)| l.find("struct SourceSpan").map(|c| (i, c + "struct ".len())))
            .expect("SourceSpan declaration present");

        let loc = Location { file: file.to_string_lossy().into_owned(), line: line0 + 1, col };
        let refs = provider.references(&loc).expect("references answered");
        assert!(!refs.is_empty(), "SourceSpan is used; expected references");
    }

    // Hovers on a known `&mut self` method call (`Vec::push`) and a `&self` one
    // (`<[_]>::contains`) and checks the resolved receiver mutability. Same
    // gating as the references test. Run with:
    //   cargo test --features lsp -- --ignored receiver_mutability_live
    #[test]
    #[ignore]
    fn receiver_mutability_live() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let program = std::env::var("AST_CONTEXT_LSP_RA").unwrap_or_else(|_| "rust-analyzer".into());
        let provider = LspProvider::start_with(&program, &root).expect("rust-analyzer should start");

        let file = root.join("src/analysis/optimization/dataflow.rs");
        let text = std::fs::read_to_string(&file).unwrap();

        // Locate the method name immediately after a `.<needle>` call site.
        let method_loc = |needle: &str| -> Location {
            let (line0, col) = text
                .lines()
                .enumerate()
                .find_map(|(i, l)| l.find(needle).map(|c| (i, c + 1))) // +1: past the '.'
                .unwrap_or_else(|| panic!("{needle} present"));
            Location { file: file.to_string_lossy().into_owned(), line: line0 + 1, col }
        };

        assert_eq!(
            provider.receiver_mutability(&method_loc(".push(")),
            Some(Mutability::Mutable),
            "Vec::push takes &mut self"
        );
        assert_eq!(
            provider.receiver_mutability(&method_loc(".contains(")),
            Some(Mutability::Shared),
            "slice::contains takes &self"
        );
    }

    #[test]
    fn parse_hover_type_handles_plain_and_fenced() {
        // rust-analyzer's default plaintext hover (path line + binding + size note).
        assert_eq!(
            parse_hover_type("crate::types::node::SourceSpan\n\npub start_line: u32\n\nsize = 4, align = 0x4"),
            Some("u32".to_string())
        );
        // Fenced markdown form.
        assert_eq!(parse_hover_type("```rust\nlet x: i32\n```"), Some("i32".to_string()));
        // Generics keep their inner commas/colons-free content.
        assert_eq!(
            parse_hover_type("let m: HashMap<String, Vec<u8>>"),
            Some("HashMap<String, Vec<u8>>".to_string())
        );
        // A shared reference type is preserved (the caller decides it isn't Copy).
        assert_eq!(parse_hover_type("let r: &String"), Some("&String".to_string()));
        // `= value` suffix and trailing punctuation are trimmed.
        assert_eq!(parse_hover_type("let x: i32 = 5"), Some("i32".to_string()));
        // A bare path with no binding annotation yields nothing.
        assert_eq!(parse_hover_type("crate::types::node::SourceSpan"), None);
    }

    // Resolves the type of a `u32` field (Copy) and a `String` field (not), and
    // checks is_copy. Run with:
    //   cargo test --features lsp -- --ignored type_of_live
    #[test]
    #[ignore]
    fn type_of_live() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let program = std::env::var("AST_CONTEXT_LSP_RA").unwrap_or_else(|_| "rust-analyzer".into());
        let provider = LspProvider::start_with(&program, &root).expect("rust-analyzer should start");

        let file = root.join("src/types/node.rs");
        let text = std::fs::read_to_string(&file).unwrap();
        let at = |needle: &str| -> Location {
            let (line0, col) = text
                .lines()
                .enumerate()
                .find_map(|(i, l)| l.find(needle).map(|c| (i, c)))
                .unwrap_or_else(|| panic!("{needle} present"));
            Location { file: file.to_string_lossy().into_owned(), line: line0 + 1, col }
        };

        let u32_ty = provider.type_of(&at("start_line: u32")).expect("u32 field type resolved");
        eprintln!("type_of(start_line) = {u32_ty:?}");
        assert_eq!(u32_ty.name, "u32");
        assert_eq!(provider.is_copy(&u32_ty), Some(true));

        let str_ty = provider.type_of(&at("name: String")).expect("String field type resolved");
        eprintln!("type_of(name) = {str_ty:?}");
        assert_eq!(str_ty.name, "String");
        assert_eq!(provider.is_copy(&str_ty), None, "String is not a known Copy primitive");
    }
}

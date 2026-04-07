use std::path::{Path, PathBuf};

use ignore::WalkBuilder;
use ignore::overrides::OverrideBuilder;

use crate::parser;
use crate::types::Language;

/// Default binary/media patterns to always skip.
const DEFAULT_IGNORE_EXTENSIONS: &[&str] = &[
    "png", "jpg", "jpeg", "gif", "svg", "mp4", "mp3", "zip", "tar", "gz",
    "bz2", "xz", "ico", "woff", "woff2", "ttf", "eot", "pdf", "exe",
    "dll", "so", "dylib", "o", "a", "class", "jar", "pyc", "pyo",
];

/// Name of the project-level ignore file (like .gitignore but for ast-context).
pub const IGNORE_FILENAME: &str = ".astcontextignore";

/// Name of the user-local ignore file — not committed to git, for personal exclusions.
pub const IGNORE_FILENAME_LOCAL: &str = ".astcontextignore.local";

/// Patterns that identify test files by path segment or name convention.
const TEST_PATH_SEGMENTS: &[&str] = &[
    "/tests/", "/test/", "/spec/", "/specs/", "/__tests__/", "/e2e/",
];
const TEST_FILE_PREFIXES: &[&str] = &["test_", "spec_"];
const TEST_FILE_SUFFIXES: &[&str] = &[
    "_test.rs", "_test.go", "_spec.rb", ".test.ts", ".test.js",
    ".spec.ts", ".spec.js", "_test.py", "_spec.py",
];

/// Walk `root` respecting .gitignore and return all files we can parse.
pub fn walk_source_files(root: &Path) -> Vec<PathBuf> {
    walk_source_files_with_excludes(root, &[])
}

/// Walk `root` with additional exclude patterns (glob syntax, e.g. "vendor/**", "*.generated.go").
///
/// Exclude patterns use gitignore glob syntax. In addition to any patterns passed here,
/// the walker also reads `.astcontextignore` and `.astcontextignore.local` files from
/// the directory tree.
pub fn walk_source_files_with_excludes(root: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
    walk_source_files_full(root, exclude_patterns, false)
}

/// Walk with full options including `skip_tests`.
///
/// When `skip_tests` is true, files identified as test files are excluded.
/// This produces a smaller, faster graph focused on production code.
pub fn walk_source_files_full(
    root: &Path,
    exclude_patterns: &[String],
    skip_tests: bool,
) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    // Read .astcontextignore and .astcontextignore.local from the directory tree.
    // The .local variant is for per-user exclusions that shouldn't be committed.
    builder.add_custom_ignore_filename(IGNORE_FILENAME);
    builder.add_custom_ignore_filename(IGNORE_FILENAME_LOCAL);

    // Apply CLI exclude patterns as overrides
    if !exclude_patterns.is_empty() {
        let mut overrides = OverrideBuilder::new(root);
        for pattern in exclude_patterns {
            // Negate the pattern so it acts as an exclusion
            let negated = format!("!{}", pattern);
            if let Err(e) = overrides.add(&negated) {
                log::warn!("Invalid exclude pattern '{}': {}", pattern, e);
            }
        }
        if let Ok(built) = overrides.build() {
            builder.overrides(built);
        }
    }

    let walker = builder.build();

    for entry in walker.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let ext = match path.extension().and_then(|e| e.to_str()) {
            Some(e) => e,
            None => continue,
        };

        // Skip known binary extensions
        if DEFAULT_IGNORE_EXTENSIONS.contains(&ext) {
            continue;
        }

        // Only include files we have a parser for
        if Language::from_extension(ext).is_none() {
            continue;
        }

        // Optionally skip test files
        if skip_tests && is_test_file(path) {
            continue;
        }

        files.push(path.to_path_buf());
    }

    files.sort();
    files
}

/// Heuristic: returns true if the path looks like a test file.
pub fn is_test_file(path: &Path) -> bool {
    let path_str = path.to_string_lossy();
    let path_str = path_str.replace('\\', "/");

    // Check path segments (e.g. /tests/, /spec/)
    if TEST_PATH_SEGMENTS.iter().any(|seg| path_str.contains(seg)) {
        return true;
    }

    if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
        // Check filename prefixes (test_, spec_)
        if TEST_FILE_PREFIXES.iter().any(|p| name.starts_with(p)) {
            return true;
        }
        // Check filename suffixes (_test.rs, .test.ts, etc.)
        if TEST_FILE_SUFFIXES.iter().any(|s| name.ends_with(s)) {
            return true;
        }
    }

    false
}

/// Return the parser for a given file path, based on its extension.
pub fn parser_for_path(path: &Path) -> Option<Box<dyn parser::LanguageParser>> {
    let ext = path.extension()?.to_str()?;
    parser::parser_for_extension(ext)
}

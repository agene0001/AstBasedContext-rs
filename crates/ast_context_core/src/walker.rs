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

/// Walk `root` respecting .gitignore and return all files we can parse.
pub fn walk_source_files(root: &Path) -> Vec<PathBuf> {
    walk_source_files_with_excludes(root, &[])
}

/// Walk `root` with additional exclude patterns (glob syntax, e.g. "vendor/**", "*.generated.go").
///
/// Exclude patterns use gitignore glob syntax. In addition to any patterns passed here,
/// the walker also reads `.astcontextignore` files from the directory tree.
pub fn walk_source_files_with_excludes(root: &Path, exclude_patterns: &[String]) -> Vec<PathBuf> {
    let mut files = Vec::new();

    let mut builder = WalkBuilder::new(root);
    builder
        .hidden(true) // skip hidden files/dirs
        .git_ignore(true)
        .git_global(true)
        .git_exclude(true);

    // Read .astcontextignore files from the directory tree
    builder.add_custom_ignore_filename(IGNORE_FILENAME);

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
        if Language::from_extension(ext).is_some() {
            files.push(path.to_path_buf());
        }
    }

    files.sort();
    files
}

/// Return the parser for a given file path, based on its extension.
pub fn parser_for_path(path: &Path) -> Option<Box<dyn parser::LanguageParser>> {
    let ext = path.extension()?.to_str()?;
    parser::parser_for_extension(ext)
}

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

use log::info;
use notify::{Config, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{Error, Result};
use crate::graph::GraphBuilder;
use crate::types::Language;

/// Events emitted by the file watcher.
#[derive(Debug, Clone)]
pub enum WatchEvent {
    /// Files were changed, the graph has been rebuilt.
    GraphRebuilt {
        changed_files: Vec<PathBuf>,
        node_count: usize,
        edge_count: usize,
    },
    /// An error occurred during rebuild.
    Error(String),
}

/// A file watcher that monitors a directory and rebuilds the graph on changes.
pub struct FileWatcher {
    root_path: PathBuf,
    /// Channel to receive watch events.
    pub events: mpsc::Receiver<WatchEvent>,
    /// Handle to stop the watcher.
    stop_tx: Option<mpsc::Sender<()>>,
    /// Handle to the watcher thread.
    thread: Option<std::thread::JoinHandle<()>>,
}

impl FileWatcher {
    /// Start watching a directory. Returns a FileWatcher that emits events on graph changes.
    ///
    /// `debounce_ms` controls how long to wait after the last file change before rebuilding
    /// (default: 2000ms). `exclude_patterns` uses gitignore glob syntax for exclusion.
    pub fn start(
        root_path: &Path,
        debounce_ms: Option<u64>,
    ) -> Result<Self> {
        Self::start_with_excludes(root_path, debounce_ms, &[])
    }

    /// Start watching with exclude patterns.
    pub fn start_with_excludes(
        root_path: &Path,
        debounce_ms: Option<u64>,
        exclude_patterns: &[String],
    ) -> Result<Self> {
        let root_path = root_path.canonicalize().map_err(|e| Error::Io {
            path: root_path.to_path_buf(),
            source: e,
        })?;

        let debounce = Duration::from_millis(debounce_ms.unwrap_or(2000));
        let (event_tx, event_rx) = mpsc::channel();
        let (stop_tx, stop_rx) = mpsc::channel();
        let (notify_tx, notify_rx) = mpsc::channel();

        let mut watcher = RecommendedWatcher::new(
            move |res: std::result::Result<notify::Event, notify::Error>| {
                if let Ok(event) = res {
                    let _ = notify_tx.send(event);
                }
            },
            Config::default(),
        )
        .map_err(|e| Error::Graph(format!("Failed to create file watcher: {e}")))?;

        watcher
            .watch(&root_path, RecursiveMode::Recursive)
            .map_err(|e| Error::Graph(format!("Failed to watch directory: {e}")))?;

        let root = root_path.clone();
        let excludes: Vec<String> = exclude_patterns.to_vec();
        let thread = std::thread::spawn(move || {
            // Keep watcher alive in this thread
            let _watcher = watcher;
            let mut pending_changes: HashSet<PathBuf> = HashSet::new();
            let mut last_event_time = std::time::Instant::now();

            loop {
                // Check for stop signal
                if stop_rx.try_recv().is_ok() {
                    info!("File watcher stopping");
                    break;
                }

                // Drain all pending notify events
                while let Ok(event) = notify_rx.try_recv() {
                    for path in event.paths {
                        if path.is_file() {
                            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                                if Language::from_extension(ext).is_some() {
                                    pending_changes.insert(path);
                                    last_event_time = std::time::Instant::now();
                                }
                            }
                        }
                    }
                }

                // If we have pending changes and enough time has passed, rebuild
                if !pending_changes.is_empty() && last_event_time.elapsed() >= debounce {
                    let changed: Vec<PathBuf> = pending_changes.drain().collect();
                    info!("Rebuilding graph due to {} changed files", changed.len());

                    match GraphBuilder::build_full(&root, false, &excludes, None) {
                        Ok(graph) => {
                            let _ = event_tx.send(WatchEvent::GraphRebuilt {
                                changed_files: changed,
                                node_count: graph.node_count(),
                                edge_count: graph.edge_count(),
                            });
                        }
                        Err(e) => {
                            let _ = event_tx.send(WatchEvent::Error(format!(
                                "Graph rebuild failed: {e}"
                            )));
                        }
                    }
                }

                std::thread::sleep(Duration::from_millis(200));
            }
        });

        Ok(Self {
            root_path,
            events: event_rx,
            stop_tx: Some(stop_tx),
            thread: Some(thread),
        })
    }

    /// Get the path being watched.
    pub fn root_path(&self) -> &Path {
        &self.root_path
    }

    /// Stop the watcher.
    pub fn stop(&mut self) {
        if let Some(tx) = self.stop_tx.take() {
            let _ = tx.send(());
        }
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Drop for FileWatcher {
    fn drop(&mut self) {
        self.stop();
    }
}

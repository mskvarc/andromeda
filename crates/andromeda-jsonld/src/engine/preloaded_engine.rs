//! Engines for unit tests that resolve exactly the documents a test preloads.

use crate::engine::processor::JsonLdEngine;
use serde_json::Value;
use std::{
    collections::HashMap,
    env,
    fs,
    process,
    sync::atomic::{AtomicU64, Ordering},
};
use url::Url;

/// A process-wide counter making every temporary context file name unique,
/// so parallel tests never write the same file.
static PRELOADED_CONTEXT_FILE_SEQUENCE: AtomicU64 = AtomicU64::new(0);

/// An engine that resolves exactly `documents`, preloaded from temporary
/// files it reads at build time, and never the network.
pub(crate) fn engine_preloading(documents: &[(String, Value)]) -> JsonLdEngine {
    let mut preloads = HashMap::new();
    let mut paths = Vec::with_capacity(documents.len());
    for (url, document) in documents {
        let sequence = PRELOADED_CONTEXT_FILE_SEQUENCE.fetch_add(1, Ordering::Relaxed);
        let path = env::temp_dir().join(format!("andromeda-preloaded-context-{}-{sequence}.jsonld", process::id()));
        fs::write(&path, document.to_string()).expect("the temporary directory is writable");
        preloads.insert(Url::parse(url).expect("the fixture names a valid URL"), path.clone());
        paths.push(path);
    }
    let engine = JsonLdEngine::builder()
        .allow_remote(false)
        .preload_contexts(preloads)
        .build()
        .expect("engine builds");
    for path in paths {
        fs::remove_file(&path).expect("the temporary context is removable");
    }
    engine
}

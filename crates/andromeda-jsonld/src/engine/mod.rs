//! Expanding and compacting NGSI-LD documents.
//!
//! [`processor::JsonLdEngine`] is the whole of the public surface;
//! [`builder::JsonLdEngineBuilder`] constructs it,
//! [`expansion_context::ExpansionContext`] is the processed context a typed
//! expansion reads, and [`context_compactor::ContextCompactor`] compacts the
//! documents of one operation against the context it processed once. Every
//! document the engine itself expands goes to the JSON-LD processor, which
//! resolves whatever contexts it names.

pub mod builder;
pub mod compaction_context;
pub mod context_compactor;
pub mod context_url;
pub mod expanded_input;
pub mod expansion_context;
pub mod options;
/// Engines for unit tests that resolve exactly the documents a test preloads.
#[cfg(test)]
pub(crate) mod preloaded_engine;
pub(crate) mod processed_context_cache;
pub mod processor;
pub mod term_lookup;
pub(crate) mod user_context_scan;

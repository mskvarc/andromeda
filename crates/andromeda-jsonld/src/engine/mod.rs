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
pub mod context_document;
pub(crate) mod context_syntax;
pub mod context_url;
pub mod expanded_text;
pub mod expansion_context;
pub(crate) mod member_order;
pub mod options;
pub mod prefixed_name;
/// Engines for unit tests that resolve exactly the documents a test preloads.
#[cfg(test)]
#[expect(
    clippy::expect_used,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
pub(crate) mod preloaded_engine;
pub(crate) mod processed_context_cache;
pub(crate) mod processing_warning;
pub mod processor;
pub mod processor_text;
pub mod term_lookup;
pub(crate) mod user_context_scan;

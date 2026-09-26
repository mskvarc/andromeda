//! Expanding and compacting NGSI-LD documents.
//!
//! [`processor::JsonLdEngine`] is the whole of the public surface;
//! [`builder::JsonLdEngineBuilder`] constructs it, and
//! [`context_compactor::ContextCompactor`] compacts the documents of one
//! operation against the context it processed once. Everything else here is one
//! of two things it does behind that surface.
//!
//! The first is the ordinary path: hand the document to the JSON-LD
//! processor, which resolves whatever contexts it names. The second is the
//! fastpath — [`batch_context`] resolves one batch's `@context` once,
//! [`term_map`] turns it into a term table, and [`fast_entity`] and
//! [`fast_attribute`] walk an entity's DOM and emit the expanded form
//! directly. The fastpath answers `None` for anything it cannot prove
//! equivalent, and the equivalence is pinned by tests that run both paths
//! over the same document.

pub mod batch_context;
pub mod builder;
pub mod compaction_context;
pub mod compaction_context_cache;
pub mod context_compactor;
pub mod context_url;
pub mod core_term_semantics;
pub mod fast_attribute;
pub mod fast_entity;
pub mod options;
pub mod processor;
pub mod term_lookup;
pub mod term_map;

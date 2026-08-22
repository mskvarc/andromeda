//! Compacting expanded NGSI-LD IRIs back to terms, without naming the compactor.
//!
//! A stored filter carries expanded IRIs — a Subscription's `q` and `csf`
//! (clause 5.2.12) are expanded through the request `@context` when the
//! resource is written (clause 5.5.7) — and has to be given back spelled in
//! the terms of the `@context` the reader supplied. That needs a JSON-LD
//! processor, which sits above this crate, so the need is stated here as a
//! port and the JSON-LD layer supplies the implementation, exactly as
//! [`crate::term_expander::TermExpander`] does for the opposite direction.

use andromeda_ngsi_ld_lexicon::attribute_name::AttributeName;
use std::future::Future;

/// Compacts expanded NGSI-LD IRIs against an already-selected `@context`.
///
/// Implementations carry the context. Attribute names and entity-type names
/// are separate methods because JSON-LD compaction of a type differs from
/// that of a property term, mirroring the two expansion methods.
///
/// Both methods are infallible by design: clause 4.5.1 permits a rendering to
/// carry a full IRI when no term abbreviates it, so a name that does not
/// compact is answered as it was given rather than turned into a failure the
/// caller could only resolve the same way.
pub trait TermCompactor: Sync {
    /// Compact an attribute IRI to the term the context spells it with.
    fn compact_term(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send;

    /// Compact an entity-type IRI to the term the context spells it with.
    fn compact_type(&self, name: &AttributeName) -> impl Future<Output = AttributeName> + Send;
}

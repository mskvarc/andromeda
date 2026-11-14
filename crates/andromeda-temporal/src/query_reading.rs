//! How the text of one temporal request is read.
//!
//! A temporal query's terms expand under the request's JSON-LD context
//! (clause 6.3.5), and its `q` target languages are admitted and select
//! languageMap keys under the level the request negotiated with the sources it
//! reaches, and the Linked Entity hops of its `q` are admitted for the
//! operation the request is (clause 4.5.23). Every phase of the query reads the
//! request these ways, so the three travel together.

use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_linked_retrieval::admission::LinkedOperation;
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;

/// How one temporal request's text is read.
#[derive(Debug, Clone, Copy)]
pub struct QueryReading<'a> {
    /// The JSON-LD context the request's terms expand under.
    pub context: &'a ContextSpec,
    /// What the request's `q` target languages may be and how they select
    /// languageMap keys.
    pub language_targeting: LanguageTargeting,
    /// The operation the request is, under the broker's temporal Linked
    /// Entity mode, which admits or refuses the hops of its `q`.
    pub operation: LinkedOperation,
}

//! How the text of one temporal request is read.
//!
//! A temporal query's terms expand under the request's JSON-LD context
//! (clause 6.3.5), and its `q` target languages are admitted and select
//! languageMap keys under the level the request negotiated with the sources it
//! reaches. Every phase of
//! the query reads the request both ways, so the two travel together.

use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_query::filter::language_targeting::LanguageTargeting;

/// How one temporal request's text is read.
#[derive(Debug, Clone, Copy)]
pub struct QueryReading<'a> {
    /// The JSON-LD context the request's terms expand under.
    pub context: &'a ContextSpec,
    /// What the request's `q` target languages may be and how they select
    /// languageMap keys.
    pub language_targeting: LanguageTargeting,
}

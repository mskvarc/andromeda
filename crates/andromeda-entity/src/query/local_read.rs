//! The query a read of the entities this broker holds runs (clause 5.7.2.4).
//!
//! Under split entities the local query of clause 5.7.2.4 matches the
//! identifier, id pattern and type alone and reads each local part whole: the
//! filters and the Attribute list decide on the aggregated Entities. Each read
//! states which of the two queries it runs.

use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_query::{candidate_filters::CandidateFilters, filter::language_targeting::LanguageTargeting};
use andromeda_ngsi_ld_request::query::entity::admitted::AdmittedEntityQuery;

/// The query one local read of an entity query runs.
#[derive(Debug, Clone, Copy)]
pub struct LocalRead<'q> {
    /// The query, as admitted.
    pub query: &'q AdmittedEntityQuery,
    /// The JSON-LD context the query's terms expand under.
    pub context: &'q ContextSpec,
    /// What the query's target languages could be and how they select
    /// languageMap keys.
    pub language_targeting: LanguageTargeting,
    /// Whether the read holds the entities to the filters split entities leave
    /// to the aggregated Entity.
    pub filters: CandidateFilters,
}

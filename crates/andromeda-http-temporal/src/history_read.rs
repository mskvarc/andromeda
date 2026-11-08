//! What one read of temporal history is bound to.
//!
//! Every phase of a temporal read (the clause 4.5.23 join, the Entity Map
//! the read is recorded in, the payload it answers with) addresses the same
//! five things: the service the history is read through, the tenant-scoped
//! target the read stays inside, the query that bounds it, the JSON-LD
//! context its terms expand under, and how its target languages select
//! languageMap keys. No phase uses one of them without the others, so they
//! travel as one value rather than as five parameters.

use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_query::{filter::language_targeting::LanguageTargeting, projection::attrs_selection::AttrsAdmission};
use andromeda_ngsi_ld_request::query::temporal::TemporalQueryParameters;
use andromeda_temporal::service::TemporalService;

/// One read of temporal history, and what it is bound to.
pub(crate) struct TemporalHistoryRead<'a> {
    /// The service every part of the history is read through.
    pub(crate) service: &'a TemporalService,
    /// The tenant-scoped target the read stays inside.
    pub(crate) target: &'a ContextTarget,
    /// The query that bounds the read: its temporal window, its filters and
    /// the clause 4.5.23 join it asks for.
    pub(crate) params: &'a TemporalQueryParameters,
    /// The JSON-LD context the read's terms expand under (clause 6.3.5).
    pub(crate) context: &'a ContextSpec,
    /// What the query's target languages may be and how they select
    /// languageMap keys.
    pub(crate) language_targeting: LanguageTargeting,
    /// Which Entity members the read's `attrs` admits, set by the operation the read answers.
    pub(crate) attrs_admission: AttrsAdmission,
}

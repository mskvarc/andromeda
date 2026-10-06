//! The query a client sent, as the typed parameters of the operation it is.
//!
//! Clause 4.3.6.1 narrows a forwarded request against what a registration
//! covers, and the three operations that forward -- Retrieve Entity (clause
//! 5.7.1), Query Entities (5.7.2) and the temporal pair (5.7.3, 5.7.4) -- reach
//! that narrowing with three different parameter sets. This is what they have
//! in common: the members the narrowing reads before it knows which operation
//! it is looking at.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{scope::query::ScopeQuery, type_selection::grammar::TypeExprRaw};
use andromeda_ngsi_ld_request::{
    entity::GetEntityParams,
    query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters},
};

/// The query a client sent, as the typed parameters of the operation it is.
#[derive(Debug, Clone, Copy)]
pub enum ClientQuery<'a> {
    /// A Query Entities request (clause 5.7.2).
    EntityQuery(&'a EntityQueryParameters),
    /// A Retrieve Entity request (clause 5.7.1), carrying the identifier of the
    /// target entity so a per-block narrowing can tell which registration
    /// blocks cover it (clause 4.3.6.3).
    EntityRetrieve {
        /// The identifier of the entity being retrieved.
        id: &'a Identifier,
        /// The retrieval parameters as the client wrote them.
        params: &'a GetEntityParams,
    },
    /// A temporal query or retrieval (clauses 5.7.3 and 5.7.4).
    Temporal(&'a TemporalQueryParameters),
}

impl ClientQuery<'_> {
    /// The clause 4.17 type selector the client wrote, if the operation has one.
    #[must_use]
    pub const fn entity_type(&self) -> Option<&TypeExprRaw> {
        match self {
            ClientQuery::EntityQuery(params) => params.entity_type.as_ref(),
            ClientQuery::Temporal(params) => params.entity_type.as_ref(),
            ClientQuery::EntityRetrieve { params, .. } => params.entity_type.as_ref(),
        }
    }

    /// The entity identifiers the client selected, if the operation has any.
    #[must_use]
    pub fn entity_ids(&self) -> Option<&[Identifier]> {
        match self {
            ClientQuery::EntityQuery(params) => params.id.as_deref(),
            ClientQuery::Temporal(params) => params.id.as_deref(),
            ClientQuery::EntityRetrieve { .. } => None,
        }
    }

    /// The Temporal Property the client's temporal operation runs on, which
    /// orders the instances of an entity's history; the operations that are
    /// not temporal have none, which reads as the default.
    #[must_use]
    pub fn timeproperty(&self) -> TimeProp {
        match self {
            ClientQuery::Temporal(params) => params.timeproperty_or_default(),
            ClientQuery::EntityQuery(_) | ClientQuery::EntityRetrieve { .. } => TimeProp::Observed,
        }
    }

    /// The scope query used to preselect registrations by their declared
    /// coverage footprint.
    #[must_use]
    pub const fn scope_q(&self) -> Option<&ScopeQuery> {
        match self {
            ClientQuery::EntityQuery(params) => params.scope_q.as_ref(),
            ClientQuery::Temporal(params) => params.scope_q.as_ref(),
            ClientQuery::EntityRetrieve { .. } => None,
        }
    }
}

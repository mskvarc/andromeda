//! The query a client sent, as the typed parameters of the operation it is.
//!
//! Clause 4.3.6.1 narrows a forwarded request against what a registration
//! covers, and the three operations that forward (Retrieve Entity, clause
//! 5.7.1; Query Entities, 5.7.2; and the temporal pair, 5.7.3 and 5.7.4) reach
//! that narrowing with three different parameter sets. This is what they have
//! in common: the members the narrowing reads before it knows which operation
//! it is looking at.

use andromeda_ngsi_ld_lexicon::{identifier::Identifier, time_prop::TimeProp};
use andromeda_ngsi_ld_query::{filter::expression::Expr, scope::query::ScopeQuery};
use andromeda_ngsi_ld_request::{
    entity::GetEntityParams,
    query::{entity::parameters::EntityQueryParameters, entity_selector::EntitySelectorParams, temporal::TemporalQueryParameters},
};
use std::borrow::Cow;

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
    /// A Query Temporal Evolution of Entities request (clause 5.7.4) a client
    /// sent. Its Linked Entities are walked by this broker once every holder
    /// has answered, so no source is asked to walk them.
    TemporalQuery(&'a TemporalQueryParameters),
    /// A Retrieve Temporal Evolution of an Entity request (clause 5.7.3),
    /// carrying the identifier of the target entity so a per-block narrowing
    /// can tell which registration blocks cover it (clause 4.3.6.3).
    TemporalRetrieve {
        /// The identifier of the entity whose history is retrieved.
        id: &'a Identifier,
        /// The retrieval parameters, in the temporal query model; the members
        /// Table 6.19.3.1-1 does not define are never forwarded.
        params: &'a TemporalQueryParameters,
        /// Who walks the Linked Entities of the retrieved entity.
        traversal: TemporalTraversal,
    },
}

/// Who walks the Linked Entities a temporal retrieval reaches (clause 4.5.23
/// under `temporal_linked_retrieval`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TemporalTraversal {
    /// This broker walks them once every holder has answered, so the holder is
    /// asked for the entity alone.
    WalkedHere,
    /// The holder walks the levels below the entity: the continuation of a
    /// walk this broker started, sent only to a holder advertising the
    /// extension.
    Continued,
}

impl ClientQuery<'_> {
    /// The entity selectors whose union the client asked for (clause 5.2.33).
    ///
    /// A query contributes its structured `entities[]` or the one implicit
    /// selector of its flat parameters. A retrieval is the one selector naming
    /// the entity of its path and the client's type, if any.
    #[must_use]
    pub fn selectors(&self) -> Cow<'_, [EntitySelectorParams]> {
        match self {
            ClientQuery::EntityQuery(params) => params.selectors(),
            ClientQuery::TemporalQuery(params) => params.selectors(),
            ClientQuery::EntityRetrieve { id, params } => Cow::Owned(vec![EntitySelectorParams {
                id: Some(vec![(*id).clone()]),
                type_: params.entity_type.clone(),
                id_pattern: None,
            }]),
            ClientQuery::TemporalRetrieve { id, .. } => Cow::Owned(vec![EntitySelectorParams {
                id: Some(vec![(*id).clone()]),
                ..EntitySelectorParams::default()
            }]),
        }
    }

    /// The entity identifiers the client selected, if the operation has any.
    #[must_use]
    pub fn entity_ids(&self) -> Option<&[Identifier]> {
        match self {
            ClientQuery::EntityQuery(params) => params.id.as_deref(),
            ClientQuery::TemporalQuery(params) => params.id.as_deref(),
            ClientQuery::EntityRetrieve { .. } | ClientQuery::TemporalRetrieve { .. } => None,
        }
    }

    /// The Temporal Property the client's temporal operation runs on, which
    /// orders the instances of an entity's history; the operations that are
    /// not temporal have none, which reads as the default.
    #[must_use]
    pub fn timeproperty(&self) -> TimeProp {
        match self {
            ClientQuery::TemporalQuery(params) | ClientQuery::TemporalRetrieve { params, .. } => params.timeproperty_or_default(),
            ClientQuery::EntityQuery(_) | ClientQuery::EntityRetrieve { .. } => TimeProp::Observed,
        }
    }

    /// The `q` filter the client wrote, as written, if the operation forwards
    /// one: a retrieval carries none (Tables 6.5.3.1-1 and 6.19.3.1-1).
    #[must_use]
    pub const fn q(&self) -> Option<&Expr> {
        match self {
            ClientQuery::EntityQuery(params) => params.q.as_ref(),
            ClientQuery::TemporalQuery(params) => params.q.as_ref(),
            ClientQuery::EntityRetrieve { .. } | ClientQuery::TemporalRetrieve { .. } => None,
        }
    }

    /// The Scope query the client wrote, if the operation takes one: a
    /// retrieval takes none (Tables 6.5.3.1-1 and 6.19.3.1-1).
    #[must_use]
    pub const fn scope_q(&self) -> Option<&ScopeQuery> {
        match self {
            ClientQuery::EntityQuery(params) => params.scope_q.as_ref(),
            ClientQuery::TemporalQuery(params) => params.scope_q.as_ref(),
            ClientQuery::EntityRetrieve { .. } | ClientQuery::TemporalRetrieve { .. } => None,
        }
    }

    /// The `splitEntities` the client wrote on a query, `None` when it left
    /// the deployment default in force or the operation is a retrieval, which
    /// has no Scope query to defer.
    #[must_use]
    pub const fn split_entities(&self) -> Option<bool> {
        match self {
            ClientQuery::EntityQuery(params) => params.split_entities,
            ClientQuery::TemporalQuery(params) => params.split_entities,
            ClientQuery::EntityRetrieve { .. } | ClientQuery::TemporalRetrieve { .. } => None,
        }
    }
}

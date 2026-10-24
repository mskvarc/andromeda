//! Which registrations take part in a read (clauses 5.5.13 and 5.12).
//!
//! Two rules ask the same question of the Context Registry before a read
//! runs: the clause 4.23.1 ordering rule, which refuses an ordering a
//! registered source takes part in, and the `q_target_languages`
//! negotiation, which admits and reads a target language as every
//! participating source does. Both select the registrations here, so they cannot disagree on
//! which sources a read reaches.

use crate::{
    FederationService,
    error::FederationError,
    forwarded_query::client_query::ClientQuery,
    operation::ForwardingOperation,
    read_fan_out::{ReadSelection, ReadServing},
    request::{CsrModeFilter, EntityMapInUse},
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{entity_selection::ConsideredEntities, geo::query::GeoQuery};
use andromeda_ngsi_ld_request::query_narrowing::ExecutionScope;

/// One read, in the terms its participating registrations are selected by.
pub struct ParticipatingRead<'a> {
    /// The execution scope the request itself asked for (clause 5.5.13).
    pub requested_scope: ExecutionScope,
    /// The query, which decides which registrations could take part.
    pub client_query: ClientQuery<'a>,
    /// The query's geoquery, compiled, which a registration's declared
    /// geometry has to admit for it to take part (clause 5.10.2.4).
    pub geo: Option<&'a GeoQuery>,
    /// The operation a registration has to advertise to take part (clause 4.20).
    pub operation: ForwardingOperation,
    /// The `EntityMap` the read runs under, which fixes the registrations that
    /// may take part at all (clause 5.5.14).
    pub entity_map: EntityMapInUse<'a>,
}

impl FederationService {
    /// The registrations `read` reaches: none for a branch that cannot
    /// federate or a read limited to the local scope (clause 5.5.13), and none
    /// for a read left considering no entity (clause 5.5.14).
    ///
    /// The selection deliberately omits the context source filter: clause
    /// 5.5.13 marks a local execution by the Context Registry not being used at
    /// all, and a read carrying a `csf` uses it. It admits every forwarding
    /// mode, since an auxiliary source (clause 4.3.6.2) contributes to the
    /// answer exactly as a primary one does.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when the query's entity type cannot
    /// be expanded against the request's context, and
    /// [`FederationError::Registration`] when the matching registrations
    /// cannot be read.
    pub(crate) async fn participating_registrations(
        &self,
        target: &ContextTarget,
        context: &ContextSpec,
        read: &ParticipatingRead<'_>,
    ) -> Result<Vec<ContextSourceRegistration>, FederationError> {
        if !target.permits_federation() || read.requested_scope == ExecutionScope::Local {
            return Ok(Vec::new());
        }
        // Clause 5.5.14: an `EntityMap` "fixes the Entities to be considered",
        // so a read left considering none has no entity for a source to hold
        // and none is contacted.
        if matches!(ConsideredEntities::of_request(read.client_query.entity_ids()), ConsideredEntities::NoEntity) {
            return Ok(Vec::new());
        }
        self.select_read_registrations(
            context,
            ReadSelection {
                entity_type: read.client_query.entity_type(),
                entity_ids: read.client_query.entity_ids(),
                id_pattern: read.client_query.id_pattern(),
                exclude_registration: None,
                csf: None,
                scope_q: read.client_query.scope_q(),
                geo: read.geo,
                serving: ReadServing::Operation(read.operation),
                mode_filter: CsrModeFilter::All,
                entity_map: read.entity_map,
            },
        )
        .await
    }
}

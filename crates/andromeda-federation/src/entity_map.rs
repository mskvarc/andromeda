//! Forwarding the creation of an EntityMap to registered context sources
//! (clauses 5.14.4.4 and 5.14.5.4).
//!
//! A registration that advertises `createEntityMapQueryEntity` (or the
//! temporal twin) answers the same query with an EntityMap of its own. The
//! query is narrowed to each registration as the query itself is forwarded
//! (clause 4.3.6.1): one creation per information block that covers part of
//! it, naming only the types, identifiers and Attributes the block covers. The
//! caller folds each returned map into the local one: every entity the remote
//! map lists is attributed to the registration in `entityMap`, and the remote
//! map's identifier joins `linkedMaps` when it is the only map the registration
//! was asked for.

use crate::{
    FederationService,
    cache::Cacheability,
    entity_read::mode_admitted,
    error::FederationError,
    expanded_registration_filters::ExpandedRegistrationFilters,
    forwarded_query::{client_query::ClientQuery, entity_map_creation::CreationMembers},
    forwarding_context::BodyPresence,
    forwarding_url::{build_entity_map_url, build_temporal_entity_map_url},
    operation::ForwardingOperation,
    outcome::{ForwardGetOutcome, ForwardReadOutcome},
    read_fan_out::{ForwardedGetConditions, ForwardedRead, SourceAnswer, expand_registration_targets, read_outcome},
    registration_narrowing::{Narrowing, get_reads},
    registration_selection::{RegistrationSelection, RegistrationServing},
    request::{CsrModeFilter, EntityMapInUse, TriggeringRequest},
    single_source_failure::single_source_read_failure,
    source_reach::SourceReach,
    term_resolver::TermResolver,
};
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{
    entity_map::EntityMap,
    registration::{ContextSourceRegistration, forwarding_mode::ForwardingMode},
};
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use thiserror::Error;
use tracing::warn;
use url::Url;

/// A source answered the creation of an `EntityMap` with a body that is none.
#[derive(Debug, Error)]
#[error("context source {registration} returned an invalid EntityMap")]
struct InvalidEntityMap<'a> {
    /// The registration that answered.
    registration: &'a Identifier,
    /// Why the body does not decode.
    #[source]
    error: serde_json::Error,
}

/// The query an `EntityMap` is created for, which also decides the creation
/// resource a forwarded request targets.
#[derive(Debug, Clone, Copy)]
pub enum EntityMapQuery<'a> {
    /// A query over current entities (clause 5.14.4).
    Entities(&'a EntityQueryParameters),
    /// A query over temporal entities (clause 5.14.5).
    TemporalEntities(&'a TemporalQueryParameters),
}

impl<'a> EntityMapQuery<'a> {
    /// The operation a registration has to advertise for this query.
    const fn operation(self) -> ForwardingOperation {
        match self {
            EntityMapQuery::Entities(_) => ForwardingOperation::CreateEntityMapQueryEntity,
            EntityMapQuery::TemporalEntities(_) => ForwardingOperation::CreateEntityMapQueryTemporal,
        }
    }

    /// How the creation resource is addressed below a registration's endpoint
    /// (clauses 6.34.3.1 and 6.35.3.1).
    const fn creation_url(self) -> fn(&Url, &str) -> Result<Url, FederationError> {
        match self {
            EntityMapQuery::Entities(_) => build_entity_map_url,
            EntityMapQuery::TemporalEntities(_) => build_temporal_entity_map_url,
        }
    }

    /// The query as the client sent it, in the terms registrations are
    /// selected by.
    const fn client_query(self) -> ClientQuery<'a> {
        match self {
            EntityMapQuery::Entities(params) => ClientQuery::EntityQuery(params),
            EntityMapQuery::TemporalEntities(params) => ClientQuery::TemporalQuery(params),
        }
    }

    /// What every creation forwarded for this query adds to its narrowed
    /// query: the client's Linked Entity retrieval and the lifetime it
    /// suggested for the map (clauses 5.14.4.3 and 5.14.5.3).
    fn creation_members(self) -> CreationMembers<'a> {
        match self {
            EntityMapQuery::Entities(params) => CreationMembers::builder()
                .join(params.join)
                .join_level(params.join_level)
                .contained_by(params.contained_by.as_deref())
                .lifetime(params.entity_map_lifetime.as_ref())
                .build(),
            // Clause 5.14.5.4 refuses Linked Entity retrieval in a temporal
            // creation, so a source is never asked for one.
            EntityMapQuery::TemporalEntities(params) => CreationMembers::builder().lifetime(params.entity_map_lifetime.as_ref()).build(),
        }
    }
}

/// Parameters for forwarding an `EntityMap` creation.
pub struct ForwardEntityMapRequest<'a> {
    /// The query the map is created for: its entity selectors are matched by
    /// clause 5.12 and, unless its entities may be split, its Scope query
    /// against each registration's `scope`. Each source is sent the part of it
    /// its registration covers, without the filters clauses 5.14.4.4 and
    /// 5.14.5.4 remove when its entities may be split.
    pub query: EntityMapQuery<'a>,
    /// The query's context source filter and geoquery, expanded under
    /// `context`, which a registration has to pass to be contacted.
    pub filters: &'a ExpandedRegistrationFilters,
    /// The JSON-LD context the triggering request resolved to.
    pub context: &'a ContextSpec,
    /// The request that triggered this forwarding.
    pub triggering: TriggeringRequest<'a>,
}

/// What one creation forwarded to a registration's context source contributed.
pub struct ForwardedEntityMap {
    /// The registration whose context source was asked.
    pub registration_id: Identifier,
    /// The entities the map the source created lists, in its order; none when
    /// the attempt failed or the source declined to create a map.
    pub entity_ids: Vec<Identifier>,
    /// The map the source created, when it is the only creation the
    /// registration was sent. Table 5.2.39-2 links one map to a registration,
    /// and a map pins the query that created it (clause 5.5.14), so a
    /// registration sent one creation per covering block or per run of
    /// identifiers is linked to none of the maps they created.
    pub linked_map: Option<Identifier>,
    /// Protocol warnings generated by the attempt.
    pub warnings: Vec<NgsiLdWarning>,
}

/// What one forwarded creation answered, before the creation is decided.
struct CreationAnswer {
    /// What the creation contributed.
    contribution: ForwardedEntityMap,
    /// The failure of an exclusive or redirect source, which is the whole
    /// creation's when that source alone holds the data (clause 6.3.17).
    failure: Option<FederationError>,
}

impl FederationService {
    /// Ask every matching context source that advertises `EntityMap` creation
    /// for its map of the part of the query its registration covers.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::Query`] when a selector's type cannot be
    /// expanded against the request's context,
    /// [`FederationError::JsonLd`] when a name the narrowing writes cannot,
    /// and [`FederationError::Registration`] when the matching registrations
    /// cannot be read. When a single creation reaches an exclusive or redirect
    /// registration and no other registration holding data is asked, its
    /// failure is the creation's (clause 6.3.17):
    /// [`FederationError::LoopDetected`], [`FederationError::GatewayTimeout`]
    /// or [`FederationError::BadGateway`], except for the 501 Table 6.34.3.1-1
    /// relays, which the returned [`ForwardReadOutcome`] carries on its
    /// `sources`. Any other source that fails to answer does not fail the call:
    /// its attempt is reported as a [`ForwardedEntityMap`] listing no entity.
    pub async fn forward_entity_map_query(&self, request: ForwardEntityMapRequest<'_>) -> Result<ForwardReadOutcome<ForwardedEntityMap>, FederationError> {
        let ForwardEntityMapRequest {
            query,
            filters,
            context,
            triggering,
        } = request;
        if self.is_loop_detected(triggering.via) {
            return Ok(ForwardReadOutcome::empty());
        }
        let client_query = query.client_query();
        let registrations = self
            .select_registrations(
                context,
                RegistrationSelection {
                    selectors: &client_query.selectors(),
                    reach: SourceReach::Every,
                    csf: filters.csf(),
                    scope_q: self.registration_scope_query(&client_query),
                    geo: filters.geo(),
                    dataset: client_query.dataset_ids(),
                    serving: RegistrationServing::Operation(query.operation()),
                    // Clause 4.5.25: the map lists every registration that
                    // returned data, and an auxiliary source can return an
                    // entity nobody else holds (clause 5.7.2.4).
                    mode_filter: CsrModeFilter::All,
                    // Clauses 5.14.4.3 and 5.14.5.3 ignore the location of an
                    // EntityMap on a creation request.
                    entity_map: EntityMapInUse::None,
                },
            )
            .await?;
        let targets = self.creation_targets(query, registrations, context).await?;
        if targets.is_empty() {
            return Ok(ForwardReadOutcome::empty());
        }
        // Clause 6.3.17 counts the requests to registrations that hold the
        // data; an auxiliary one only supplements it.
        let matched = targets
            .iter()
            .filter(|(registration, _)| mode_admitted(registration, CsrModeFilter::ExcludeAuxiliary))
            .count();
        let answers = self
            .fan_out_read(
                targets,
                |registration, outcome| Some(creation_answer(registration, outcome)),
                ForwardedGetConditions {
                    context,
                    triggering,
                    // Clause 6.3.17 allows caching for `/entities` only.
                    cacheability: Cacheability::Uncacheable,
                    entity_map: EntityMapInUse::None,
                },
            )
            .await;
        let mut failures = Vec::new();
        let answers: Vec<SourceAnswer<ForwardedEntityMap>> = answers
            .into_iter()
            .map(|SourceAnswer { mode, error_status, entry }| SourceAnswer {
                mode,
                error_status,
                entry: entry.map(|CreationAnswer { contribution, failure }| {
                    failures.extend(failure);
                    contribution
                }),
            })
            .collect();
        if matched == 1
            && let Some(failure) = failures.into_iter().next()
        {
            return Err(failure);
        }
        let mut outcome = read_outcome(answers, matched);
        link_sole_creations(&mut outcome.results);
        Ok(outcome)
    }

    /// The creations forwarded for `query` to `registrations`: one for each
    /// information block that covers part of the query, and for each run of
    /// its identifiers that fits a URL (clauses 4.3.6.1 and 4.3.6.7), each
    /// naming only what the block covers, in the names of the registration's
    /// forwarding context (clause 4.3.6.6).
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type or attribute
    /// cannot be expanded against the request context, and
    /// [`FederationError::Query`] when a name in the client's query cannot.
    async fn creation_targets(
        &self,
        query: EntityMapQuery<'_>,
        registrations: Vec<ContextSourceRegistration>,
        context: &ContextSpec,
    ) -> Result<Vec<(ContextSourceRegistration, ForwardedRead)>, FederationError> {
        let client = query.client_query();
        let resolver = TermResolver::new(&self.engine, context);
        let narrowing = Narrowing::builder()
            .client(client)
            .inbound(context)
            .resolver(&resolver)
            .split(self.split_entities(client.split_entities()))
            .presence(BodyPresence::Absent)
            .build();
        let mut targets = Vec::with_capacity(registrations.len());
        for registration in registrations {
            let reads = self
                .registration_queries(&registration, &narrowing)
                .await?
                .into_reads(|narrowed| get_reads(&registration, narrowed.into_entity_map_creation(query.creation_members()), query.creation_url()));
            expand_registration_targets(registration, reads, &mut targets);
        }
        Ok(targets)
    }
}

/// Read what `registration`'s source answered one creation with.
fn creation_answer(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> CreationAnswer {
    let failure = match registration.access.mode.unwrap_or_default() {
        ForwardingMode::Exclusive | ForwardingMode::Redirect => single_source_read_failure(&registration.id, &outcome),
        ForwardingMode::Inclusive | ForwardingMode::Auxiliary => None,
    };
    let (map, warnings) = match outcome {
        ForwardGetOutcome::Success(response) => match serde_json::from_slice::<EntityMap>(&response.body) {
            Ok(map) => (Some(map), response.warnings),
            Err(error) => {
                warn!("Context source {} returned an invalid EntityMap: {}", registration.id, error);
                let mut warnings = response.warnings;
                warnings.push(NgsiLdWarning::new(
                    WarnCode::RevalidationFailed,
                    InvalidEntityMap {
                        registration: &registration.id,
                        error,
                    },
                ));
                (None, warnings)
            }
        },
        // A source that declines to create a map is not abnormal (clause 5.5.14).
        ForwardGetOutcome::NotFound(_) => (None, Vec::new()),
        other @ (ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected) => {
            (None, other.warning(&registration.id).into_iter().collect())
        }
    };
    let (entity_ids, linked_map) = match map {
        Some(EntityMap { id, entity_map, .. }) => (entity_map.into_keys().collect(), Some(id)),
        None => (Vec::new(), None),
    };
    CreationAnswer {
        contribution: ForwardedEntityMap {
            registration_id: registration.id.clone(), // clone: the contribution owns the identifier of the registration it is attributed to
            entity_ids,
            linked_map,
            warnings,
        },
        failure,
    }
}

/// Keep linked only the maps of registrations sent a single creation, as
/// [`ForwardedEntityMap::linked_map`] describes.
fn link_sole_creations(contributions: &mut [ForwardedEntityMap]) {
    let shared: Vec<bool> = contributions
        .iter()
        .map(|contribution| {
            contributions
                .iter()
                .filter(|other| other.registration_id == contribution.registration_id)
                .nth(1)
                .is_some()
        })
        .collect();
    for (contribution, shared) in contributions.iter_mut().zip(shared) {
        if shared {
            contribution.linked_map = None;
        }
    }
}

//! The forwarded queries one registration receives, spelled in its forwarding
//! context (clauses 4.3.6.1, 4.3.6.3 and 4.3.6.6).
//!
//! A read is narrowed per registration block ([`crate::forwarded_query`]), and
//! every name it carries is spelled in the registration's forwarding context
//! ([`crate::forwarding_context`]). Both depend on the registration, so each
//! forwarded read builds its queries here, registration by registration, from
//! the same decision the transport later sends them under.

use crate::{
    FederationService,
    error::FederationError,
    forwarded_names::ForwardedNames,
    forwarded_query::{ForwardedQuery, client_query::ClientQuery},
    forwarding_context::BodyPresence,
    read_fan_out::ForwardedRead,
    registration_payload::forwarding_decision,
    request::{SourcePage, TemporalAnswer},
    split_entities::SplitEntities,
    term_resolver::TermResolver,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use typed_builder::TypedBuilder;
use url::Url;

/// What one forwarded read narrows for every registration it contacts.
#[derive(TypedBuilder)]
pub(crate) struct Narrowing<'a> {
    /// The client's query.
    pub(crate) client: ClientQuery<'a>,
    /// The context the triggering request resolved to.
    pub(crate) inbound: &'a ContextSpec,
    /// The inbound-context expansion the operation shares.
    pub(crate) resolver: &'a TermResolver<'a>,
    /// Whether entities may be split across sources (clause 5.7.2.4).
    pub(crate) split: SplitEntities,
    /// The explicit source page a durable walk asks for; none for a read that
    /// walks every page itself.
    #[builder(default = None)]
    pub(crate) source_page: Option<SourcePage>,
    /// The representation a temporal source is asked to answer in.
    #[builder(default = TemporalAnswer::AsRequested)]
    pub(crate) temporal_answer: TemporalAnswer,
    /// Whether the request carrying the query has a body, which decides how
    /// its context travels (clause 6.3.5).
    pub(crate) presence: BodyPresence,
}

/// The forwarded queries one registration receives.
pub(crate) enum RegistrationQueries {
    /// One query per information block that covers part of the request
    /// (clause 4.3.6.3), each spelled in the registration's forwarding context.
    Narrowed(Vec<ForwardedQuery>),
    /// No request can be prepared for the registration, because its admitted
    /// `contextSourceInfo` or `jsonldContext` no longer reads as admitted. The
    /// failure is reported as the registration's own clause 6.3.17 warning, as
    /// every failure to prepare a forwarded request is, rather than failing
    /// the whole read.
    Unprepared(FederationError),
}

impl RegistrationQueries {
    /// The forwarded reads these queries become, each query turned into its
    /// reads by `read`, or the one read that carries the failure to prepare
    /// them.
    pub(crate) fn into_reads(self, read: impl Fn(ForwardedQuery) -> Vec<ForwardedRead>) -> Vec<ForwardedRead> {
        match self {
            RegistrationQueries::Narrowed(queries) => queries.into_iter().flat_map(read).collect(),
            RegistrationQueries::Unprepared(error) => vec![ForwardedRead::Get(Err::<Url, FederationError>(error))],
        }
    }
}

/// The GET reads that carry `query` to the resource `build_url` addresses on
/// `registration`, one per run of identifiers that fits the URL budget (clauses
/// 4.3.6.7 and 5.5.14). A query that fits no split is the registration's
/// failure to prepare, which a read reports as its own.
pub(crate) fn get_reads(
    registration: &ContextSourceRegistration,
    query: ForwardedQuery,
    build_url: fn(&Url, &str) -> Result<Url, FederationError>,
) -> Vec<ForwardedRead> {
    let endpoint = registration.access.endpoint.url();
    let prepared = build_url(endpoint, "").and_then(|resource| query.within_url_budget(&resource, &registration.id));
    match prepared {
        Ok(runs) => runs.iter().map(|run| ForwardedRead::Get(build_url(endpoint, &run.to_query_string()))).collect(),
        Err(error) => vec![ForwardedRead::Get(Err(error))],
    }
}

impl FederationService {
    /// The forwarded queries `registration` receives for `narrowing`.
    ///
    /// # Errors
    ///
    /// Returns [`FederationError::JsonLd`] when a client type or attribute
    /// cannot be expanded against the request context, and
    /// [`FederationError::Query`] when a name in the client's query cannot.
    pub(crate) async fn registration_queries(
        &self,
        registration: &ContextSourceRegistration,
        narrowing: &Narrowing<'_>,
    ) -> Result<RegistrationQueries, FederationError> {
        let decision = match forwarding_decision(registration, narrowing.inbound, narrowing.presence) {
            Ok(decision) => decision,
            Err(error) => return Ok(RegistrationQueries::Unprepared(error)),
        };
        let naming = decision.context.naming(narrowing.inbound);
        let names = match ForwardedNames::prepare(&self.engine, narrowing.resolver, &naming, decision.context.rewording()).await {
            Ok(names) => names,
            Err(error) => return Ok(RegistrationQueries::Unprepared(error)),
        };
        let queries = ForwardedQuery::for_registration(
            narrowing.client,
            registration,
            narrowing.split,
            narrowing.source_page,
            narrowing.temporal_answer,
            &names,
        )
        .await?;
        Ok(RegistrationQueries::Narrowed(queries))
    }

    /// A query string the client wrote, with every name-bearing parameter
    /// spelled in the forwarding context of a request to `registration` that
    /// has `presence` of a body (clause 4.3.6.6).
    ///
    /// # Errors
    ///
    /// Returns the failure to decide the registration's forwarding context, and
    /// the failures of [`ForwardedNames::query_string`].
    pub(crate) async fn spelled_query_string(
        &self,
        registration: &ContextSourceRegistration,
        inbound: &ContextSpec,
        resolver: &TermResolver<'_>,
        raw: &str,
        presence: BodyPresence,
    ) -> Result<String, FederationError> {
        let decision = forwarding_decision(registration, inbound, presence)?;
        let naming = decision.context.naming(inbound);
        ForwardedNames::prepare(&self.engine, resolver, &naming, decision.context.rewording())
            .await?
            .query_string(raw)
            .await
    }
}

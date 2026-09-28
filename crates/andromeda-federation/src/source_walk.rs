//! Walking every page of a context source's answer to a forwarded query
//! (clauses 4.3.6.7, 5.5.9.1, 5.7.2.4 and 6.3.10).
//!
//! Clause 5.7.2.4 makes a federated query's result the merge of the local
//! entities with every matching source's Entity Array, paginated afterwards
//! (clause 5.5.9), and clause 4.3.6.7 asks for "a broad request to each
//! registered Context Source" so that the pagination is consistent. A source
//! answers in pages of its own, so one forwarded query is a sequence of
//! exchanges: the first request, then each page its `Link rel="next"` points
//! at, until a page carries no such pointer. Clause 5.5.9.1 is explicit that
//! the pointer, not the size of the page, is what says the answer ended: "the
//! number of Elements retrieved per page can be lower than L".
//!
//! Each page is a forwarded request of its own, so the registration's clause
//! 5.2.34 `timeout` applies per page and every page is recorded on its health
//! members. A page that fails, and a pointer that cannot be followed, end the
//! walk with what was gathered so far and a clause 6.3.17 warning, exactly as a
//! source failing its first page contributes only its warning. What bounds the
//! walk is the request's [`EntityBudget`]: once every source together has
//! gathered more than it allows, the query is refused with `TooManyResults`.
//!
//! The POST binding of clause 6.23 carries its query in the body, so a source
//! can hand back no pointer a client could dereference to the next page; that
//! walk asks for successive pages by `offset` (clause 6.3.10) and ends on the
//! first empty one.

use crate::{
    FederationService,
    entity_budget::{EntityBudget, EntityBudgetExceeded},
    error::FederationError,
    forwarding_url::with_offset,
    outcome::{ForwardGetOutcome, ForwardedEntities},
    read_fan_out::{ForwardedGetConditions, ForwardedRead, SourceAnswer},
    source_continuation::SourceContinuation,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_model::{entity::Entity, registration::ContextSourceRegistration};
use futures::{StreamExt, stream};
use serde_json::Value;
use std::{collections::HashSet, num::NonZeroU32};
use url::Url;

/// The page size each source is asked for.
///
/// Only a hint: clause 5.5.9.1 lets a source answer with fewer entities than
/// asked, which is why a short page never ends the walk. One thousand is the
/// largest page the core API guarantees a source accepts.
pub(crate) const SOURCE_PAGE_SIZE: NonZeroU32 = NonZeroU32::MIN.saturating_add(999);

/// The request that fetches the next page of a source's answer.
enum NextPage {
    /// A GET to the page the source's `next` pointer names.
    Pointer(Url),
    /// The POST batch query again, starting where the answer so far ends.
    Offset {
        /// The URL of the first page, whose `offset` is replaced.
        url: Url,
        /// The serialized `Query` body every page repeats.
        body: Vec<u8>,
    },
}

/// What one fetched page leaves the walk with.
enum PageEnd {
    /// The walk goes on with this request.
    Continue(NextPage),
    /// The walk is over; the warning says why, when the answer is incomplete.
    Stop(Option<NgsiLdWarning>),
}

/// One exchange of a walk.
enum Fetch {
    /// A GET of a query page.
    Get(Result<Url, FederationError>),
    /// A POST batch query page.
    Post(Result<Url, FederationError>, Vec<u8>),
}

/// How far into a source's answer the walk has read, counted in the elements
/// the source sent -- including any the broker then discards, such as an
/// expired entity (clause 5.7.2.4) -- because that is what the source's own
/// `offset` counts.
#[derive(Default)]
struct AnswerPosition {
    /// Elements the source has sent so far.
    elements: u64,
    /// The pages already requested, so a source pointing back at one of them
    /// cannot keep the walk going forever.
    requested: HashSet<String>,
    /// The identifiers gathered so far, so an offset walk over a source that
    /// ignores `offset` -- answering the same entities again -- ends instead of
    /// gathering them forever.
    gathered: HashSet<String>,
}

impl AnswerPosition {
    /// Advance past a page of `elements` elements.
    fn advance(&mut self, elements: usize) {
        self.elements = self.elements.saturating_add(u64::try_from(elements).unwrap_or(u64::MAX));
    }

    /// Keep the entities of one page the walk has not gathered before: a
    /// source whose order shifted between two pages can answer one entity on
    /// both, and it is one entity of the source's answer.
    fn fresh(&mut self, entities: Vec<Entity>) -> Vec<Entity> {
        entities
            .into_iter()
            .filter(|entity| self.gathered.insert(entity.id.as_str().to_owned()))
            .collect()
    }

    /// Record `request`, answering `false` when it names a page already
    /// requested.
    fn first_request_of(&mut self, request: &NextPage) -> bool {
        let key = match request {
            NextPage::Pointer(url) => url.as_str().to_owned(),
            NextPage::Offset { .. } => format!("offset={}", self.elements),
        };
        self.requested.insert(key)
    }
}

impl FederationService {
    /// Walk every target's answer to its end, all targets concurrently, and
    /// read what each gathered (clause 5.7.2.4).
    ///
    /// # Errors
    ///
    /// Returns [`EntityBudgetExceeded`] once the targets together have
    /// gathered more entities than `budget` allows.
    pub(crate) async fn fan_out_walk(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedRead)>,
        conditions: ForwardedGetConditions<'_>,
        budget: &EntityBudget,
    ) -> Result<Vec<SourceAnswer<ForwardedEntities>>, EntityBudgetExceeded> {
        stream::iter(targets)
            .map(|(registration, first)| async move { self.walk_source(&registration, first, conditions, budget).await })
            .buffer_unordered(self.csr_concurrency)
            .collect::<Vec<_>>()
            .await
            .into_iter()
            .collect()
    }

    /// Walk one source's answer from `first` to its last page.
    async fn walk_source(
        &self,
        registration: &ContextSourceRegistration,
        first: ForwardedRead,
        conditions: ForwardedGetConditions<'_>,
        budget: &EntityBudget,
    ) -> Result<SourceAnswer<ForwardedEntities>, EntityBudgetExceeded> {
        let mode = registration.access.mode.unwrap_or_default();
        let (outcome, mut next) = match first {
            ForwardedRead::Get(url) => {
                let outcome = self.fetch_page(registration, Fetch::Get(url), conditions).await;
                let next = pointer_continuation(registration, &outcome);
                (outcome, next)
            }
            ForwardedRead::PostQuery { url, body } => {
                let first_url = url.as_ref().ok().cloned(); // clone: the later pages repeat the first page's URL
                let outcome = self.fetch_page(registration, Fetch::Post(url, body.clone()), conditions).await; // clone: every page repeats the body
                let next = first_url.map_or(PageEnd::Stop(None), |url| PageEnd::Continue(NextPage::Offset { url, body }));
                (outcome, next)
            }
        };
        let error_status = outcome.error_status();
        let mut position = AnswerPosition::default();
        let first_sent = elements_sent(&outcome);
        position.advance(first_sent);
        let Some(mut gathered) = FederationService::entities_of(registration, outcome) else {
            return Ok(SourceAnswer {
                mode,
                error_status,
                entry: None,
            });
        };
        gathered.entities = position.fresh(gathered.entities);
        budget.charge(gathered.entities.len())?;
        // The offset walk of the POST binding ends on the first empty page.
        if first_sent == 0 && matches!(next, PageEnd::Continue(NextPage::Offset { .. })) {
            next = PageEnd::Stop(None);
        }
        loop {
            let request = match next {
                PageEnd::Stop(warning) => {
                    gathered.warnings.extend(warning);
                    break;
                }
                PageEnd::Continue(request) => request,
            };
            if !position.first_request_of(&request) {
                gathered
                    .warnings
                    .push(incomplete_answer(registration, "pointed back at a page it already answered"));
                break;
            }
            let (outcome, following) = match request {
                NextPage::Pointer(url) => {
                    let outcome = self.fetch_page(registration, Fetch::Get(Ok(url)), conditions).await;
                    let following = pointer_continuation(registration, &outcome);
                    (outcome, following)
                }
                NextPage::Offset { url, body } => {
                    let page = with_offset(&url, position.elements);
                    let outcome = self.fetch_page(registration, Fetch::Post(Ok(page), body.clone()), conditions).await; // clone: every page repeats the body
                    (outcome, PageEnd::Continue(NextPage::Offset { url, body }))
                }
            };
            let sent = elements_sent(&outcome);
            position.advance(sent);
            let page = match outcome {
                success @ ForwardGetOutcome::Success(_) => FederationService::entities_of(registration, success),
                ForwardGetOutcome::NotFound(warnings) => {
                    gathered.warnings.extend(warnings);
                    gathered
                        .warnings
                        .push(incomplete_answer(registration, "no longer found the next page of its answer"));
                    break;
                }
                failure @ (ForwardGetOutcome::InvalidPayload(_)
                | ForwardGetOutcome::ErrorStatus(_)
                | ForwardGetOutcome::Transport(_)
                | ForwardGetOutcome::LoopDetected) => {
                    // Clause 6.3.17: the entities already gathered stand, and
                    // the failure says the answer is incomplete.
                    gathered.warnings.extend(failure.warning(&registration.id));
                    break;
                }
            };
            let Some(page) = page else {
                break;
            };
            let fresh = position.fresh(page.entities);
            budget.charge(fresh.len())?;
            let progressed = !fresh.is_empty();
            gathered.entities.extend(fresh);
            gathered.warnings.extend(page.warnings);
            next = match following {
                // The offset walk ends on the first empty page, and on the first
                // page adding nothing new: a source that ignores `offset`
                // answers its whole result again rather than an empty page.
                PageEnd::Continue(NextPage::Offset { .. }) if sent == 0 || !progressed => PageEnd::Stop(None),
                // An empty page that still points onward makes no progress a
                // bounded walk could rely on.
                PageEnd::Continue(NextPage::Pointer(_)) if sent == 0 => {
                    PageEnd::Stop(Some(incomplete_answer(registration, "answered an empty page that still points at a next one")))
                }
                continuing @ (PageEnd::Continue(_) | PageEnd::Stop(_)) => continuing,
            };
        }
        Ok(SourceAnswer {
            mode,
            error_status,
            entry: Some(gathered),
        })
    }

    /// Fetch one page, telling the source the location of the map in use
    /// (clause 5.5.14).
    async fn fetch_page(&self, registration: &ContextSourceRegistration, fetch: Fetch, conditions: ForwardedGetConditions<'_>) -> ForwardGetOutcome {
        let linked_map = conditions.entity_map.location_for(&registration.id);
        match fetch {
            Fetch::Get(url) => {
                self.forward_get(
                    registration,
                    url,
                    conditions.context,
                    conditions.triggering,
                    conditions.cacheability,
                    linked_map,
                )
                .await
            }
            Fetch::Post(url, body) => {
                self.forward_post_query(registration, url, body, conditions.context, conditions.triggering, linked_map)
                    .await
            }
        }
    }
}

/// How many elements a page carried, as the source counts them: the members
/// of an Entity Array, one for a single entity, none for anything else.
const fn elements_sent(outcome: &ForwardGetOutcome) -> usize {
    match outcome {
        ForwardGetOutcome::Success(response) => match &response.body {
            Value::Array(items) => items.len(),
            Value::Object(_) => 1,
            Value::Null | Value::Bool(_) | Value::Number(_) | Value::String(_) => 0,
        },
        ForwardGetOutcome::NotFound(_)
        | ForwardGetOutcome::InvalidPayload(_)
        | ForwardGetOutcome::ErrorStatus(_)
        | ForwardGetOutcome::Transport(_)
        | ForwardGetOutcome::LoopDetected => 0,
    }
}

/// Where a GET page's `next` pointer leaves the walk.
fn pointer_continuation(registration: &ContextSourceRegistration, outcome: &ForwardGetOutcome) -> PageEnd {
    match outcome {
        ForwardGetOutcome::Success(response) => match &response.continuation {
            SourceContinuation::Complete => PageEnd::Stop(None),
            // clone: the walk owns the URL of the page it fetches next
            SourceContinuation::Next(url) => PageEnd::Continue(NextPage::Pointer(Url::clone(url))),
            SourceContinuation::Unfollowable(reason) => PageEnd::Stop(Some(incomplete_answer(registration, &reason.to_string()))),
        },
        ForwardGetOutcome::NotFound(_)
        | ForwardGetOutcome::InvalidPayload(_)
        | ForwardGetOutcome::ErrorStatus(_)
        | ForwardGetOutcome::Transport(_)
        | ForwardGetOutcome::LoopDetected => PageEnd::Stop(None),
    }
}

/// The clause 6.3.17 warning of a source whose answer the walk could not read
/// to its end, saying `what` the source did.
fn incomplete_answer(registration: &ContextSourceRegistration, what: &str) -> NgsiLdWarning {
    NgsiLdWarning::MiscellaneousWarning(format!("Context source {} {what}; its answer is incomplete", registration.id))
}

#[cfg(test)]
mod tests {
    use crate::{
        FederationService,
        config::{FederationConfig, SplitEntities},
        entity_budget::{EntityBudget, EntityBudgetExceeded},
        fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
        forwarded_query::client_query::ClientQuery,
        outcome::{ForwardReadOutcome, ForwardedEntities},
        query_forwarding_error::QueryForwardingError,
        request::{CsrModeFilter, EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    };
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
    use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
    use andromeda_test_mocks::{
        callback_path::CallbackPath,
        callback_server::MockResponse,
        context_source_server::ContextSourceServer,
        server_config::MockServerConfig,
    };
    use bytes::Bytes;
    use http::{HeaderMap, HeaderValue, Method, StatusCode};
    use serde_json::json;
    use std::{
        net::{Ipv4Addr, SocketAddr},
        num::NonZeroU32,
        ops::Range,
        sync::Arc,
    };
    use url::Host;

    /// The query resource every GET page of these walks lands on.
    const QUERY_PATH: &str = "/ngsi-ld/v1/entities";
    /// The batch query resource every POST page lands on.
    const BATCH_PATH: &str = "/ngsi-ld/v1/entityOperations/query";

    /// A mock Context Source listening on an ephemeral local port.
    async fn source() -> ContextSourceServer {
        let config = MockServerConfig::new(SocketAddr::from((Ipv4Addr::LOCALHOST, 0)), Host::Ipv4(Ipv4Addr::LOCALHOST));
        ContextSourceServer::start(config).await.expect("the mock source starts")
    }

    fn path(raw: &str) -> CallbackPath {
        CallbackPath::parse(raw).expect("an absolute path")
    }

    /// An inclusive registration of `Vehicle` pointing at `source`, advertising
    /// `operations`.
    fn registration(source: &ContextSourceServer, operations: &[&str]) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": "urn:ngsi-ld:ContextSourceRegistration:walk",
            "type": "ContextSourceRegistration",
            "endpoint": source.endpoint(&path("/")).expect("the endpoint resolves").as_str(),
            "mode": "inclusive",
            "operations": operations,
            "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("a valid registration")
    }

    fn service(registration: ContextSourceRegistration) -> FederationService {
        FederationService::new(
            Arc::new(FixedRegistrationStore::new(vec![registration])),
            Arc::new(NoLocalEntities),
            JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
            ContextSourcePseudonym::new("walking-broker").expect("a valid alias"),
            &FederationConfig::default(),
        )
        .expect("the service builds")
    }

    /// One page of `count` Vehicles numbered from `first`, pointing at `next`
    /// when given.
    fn page(first: usize, count: usize, next: Option<&str>) -> MockResponse {
        let entities: Vec<serde_json::Value> = (first..first + count)
            .map(|number| json!({"id": format!("urn:ngsi-ld:Vehicle:{number:04}"), "type": "Vehicle"}))
            .collect();
        let mut headers = HeaderMap::new();
        headers.insert("content-type", HeaderValue::from_static("application/json"));
        if let Some(next) = next {
            headers.insert("link", HeaderValue::from_str(&format!("<{next}>; rel=\"next\"")).expect("a valid Link header"));
        }
        MockResponse::builder()
            .status(StatusCode::OK)
            .headers(headers)
            .body(Bytes::from(serde_json::to_vec(&entities).expect("the page serializes")))
            .build()
    }

    fn failure(status: StatusCode) -> MockResponse {
        MockResponse::builder().status(status).build()
    }

    /// Forward `type=Vehicle` through `delivery` under a budget of `maximum`.
    async fn walk(service: &FederationService, delivery: QueryDelivery, maximum: u32) -> Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError> {
        let query: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle").expect("a valid query");
        let budget = EntityBudget::new(NonZeroU32::new(maximum).expect("a positive budget"));
        let context = ContextSpec::none();
        service
            .forward_query(
                &ContextTarget::Live,
                ForwardQueryRequest {
                    client_query: ClientQuery::EntityQuery(&query),
                    delivery,
                    split: SplitEntities::Complete,
                    context: &context,
                    triggering: TriggeringRequest::default(),
                    csf_expr: None,
                    mode_filter: CsrModeFilter::All,
                    entity_map: EntityMapInUse::None,
                    budget: &budget,
                },
            )
            .await
    }

    /// The identifiers the walk gathered, in the order the source sent them.
    fn gathered(outcome: &ForwardReadOutcome<ForwardedEntities>) -> Vec<String> {
        outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|entity| entity.id.as_str().to_owned()))
            .collect()
    }

    fn warnings(outcome: &ForwardReadOutcome<ForwardedEntities>) -> Vec<&NgsiLdWarning> {
        outcome.results.iter().flat_map(|answer| answer.warnings.iter()).collect()
    }

    fn numbered(range: Range<usize>) -> Vec<String> {
        range.map(|number| format!("urn:ngsi-ld:Vehicle:{number:04}")).collect()
    }

    /// Clauses 5.5.9.1 and 6.3.10: the walk follows every `next` pointer, and a
    /// page shorter than the limit asked for does not end it -- only a page
    /// without a pointer does.
    #[tokio::test]
    async fn the_walk_follows_every_next_pointer_past_short_pages_to_the_last_page() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(2, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=4")))
            .await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(4, 1, None)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..5));
        assert!(warnings(&outcome).is_empty(), "{:?}", warnings(&outcome));
        let requests = source.drain_requests().await;
        let queries: Vec<Option<&str>> = requests.iter().map(|request| request.uri.query()).collect();
        pretty_assertions::assert_eq!(queries.len(), 3);
        assert!(queries[0].is_some_and(|query| query.contains("limit=1000")), "{queries:?}");
        pretty_assertions::assert_eq!(queries[1], Some("type=Vehicle&offset=2"));
        pretty_assertions::assert_eq!(queries[2], Some("type=Vehicle&offset=4"));
        source.shutdown().await.expect("the mock source stops");
    }

    /// Clause 5.5.9.1: an answer without a `next` pointer is complete after its
    /// first page.
    #[tokio::test]
    async fn an_answer_without_a_next_pointer_is_read_once() {
        let source = source().await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(0, 3, None)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..3));
        pretty_assertions::assert_eq!(source.total_request_count(), 1);
        source.shutdown().await.expect("the mock source stops");
    }

    /// A source pointing back at a page it already answered cannot keep the
    /// walk going: what was gathered stands, and a warning says the answer is
    /// incomplete (clause 6.3.17).
    #[tokio::test]
    async fn a_repeated_next_pointer_ends_the_walk_with_a_warning() {
        let source = source().await;
        let looping = "/ngsi-ld/v1/entities?type=Vehicle&offset=2";
        source.enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some(looping))).await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(2, 2, Some(looping))).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..4));
        pretty_assertions::assert_eq!(source.total_request_count(), 2);
        assert!(
            warnings(&outcome).iter().any(|warning| warning.to_string().contains("already answered")),
            "{:?}",
            warnings(&outcome)
        );
        source.shutdown().await.expect("the mock source stops");
    }

    /// An empty page that still points onward makes no progress, so the walk
    /// ends there with a warning rather than following it.
    #[tokio::test]
    async fn an_empty_page_that_still_points_onward_ends_the_walk_with_a_warning() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(2, 0, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=3")))
            .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(source.total_request_count(), 2);
        assert!(
            warnings(&outcome).iter().any(|warning| warning.to_string().contains("empty page")),
            "{:?}",
            warnings(&outcome)
        );
        source.shutdown().await.expect("the mock source stops");
    }

    /// Clause 6.3.17: a page that fails keeps what the earlier pages gathered
    /// and reports the failure as a warning, never as the query's failure.
    #[tokio::test]
    async fn a_failing_page_keeps_what_was_gathered_and_warns() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source.enqueue(Method::GET, path(QUERY_PATH), failure(StatusCode::INTERNAL_SERVER_ERROR)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        assert!(
            warnings(&outcome).iter().any(|warning| warning.to_string().contains("HTTP 500")),
            "{:?}",
            warnings(&outcome)
        );
        source.shutdown().await.expect("the mock source stops");
    }

    /// Clause 4.12: once the sources together gather more than the request's
    /// budget, the query is refused with `TooManyResults`.
    #[tokio::test]
    async fn a_walk_exceeding_the_budget_is_refused_with_too_many_results() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(2, 2, None)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let refused = walk(&service, QueryDelivery::Query, 3).await;

        match refused {
            Err(QueryForwardingError::TooManyResults(EntityBudgetExceeded { maximum })) => pretty_assertions::assert_eq!(maximum.get(), 3),
            Err(QueryForwardingError::Federation(error)) => panic!("the walk must be refused for its budget, not fail: {error}"),
            Ok(outcome) => panic!("the walk must be refused, it gathered {:?}", gathered(&outcome)),
        }
        source.shutdown().await.expect("the mock source stops");
    }

    /// A budget the answer fits into exactly is not exceeded.
    #[tokio::test]
    async fn a_walk_gathering_exactly_the_budget_completes() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(2, 2, None)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 4).await.expect("the walk fits its budget");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..4));
        source.shutdown().await.expect("the mock source stops");
    }

    /// Clause 4.3.6.1: the broker contacts the registration it matched, so a
    /// pointer to another origin is not followed.
    #[tokio::test]
    async fn a_next_pointer_to_another_origin_is_not_followed() {
        let source = source().await;
        source
            .enqueue(
                Method::GET,
                path(QUERY_PATH),
                page(0, 2, Some("http://elsewhere.invalid/ngsi-ld/v1/entities?offset=2")),
            )
            .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(source.total_request_count(), 1);
        assert!(
            warnings(&outcome).iter().any(|warning| warning.to_string().contains("origin")),
            "{:?}",
            warnings(&outcome)
        );
        source.shutdown().await.expect("the mock source stops");
    }

    /// A source that ignores `offset` answers its whole result again; the
    /// offset walk ends on the first page that adds nothing new, gathering each
    /// entity once rather than until the budget refuses the query.
    #[tokio::test]
    async fn an_offset_walk_over_a_source_ignoring_offset_ends_on_the_first_repeat() {
        let source = source().await;
        source.enqueue(Method::POST, path(BATCH_PATH), page(0, 2, None)).await;
        let service = service(registration(&source, &["queryBatch"]));

        let outcome = walk(&service, QueryDelivery::Batch, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(source.total_request_count(), 2);
        source.shutdown().await.expect("the mock source stops");
    }

    /// An entity a source answers on two pages is one entity of its answer.
    #[tokio::test]
    async fn an_entity_answered_on_two_pages_is_gathered_once() {
        let source = source().await;
        source
            .enqueue(Method::GET, path(QUERY_PATH), page(0, 3, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")))
            .await;
        source.enqueue(Method::GET, path(QUERY_PATH), page(2, 3, None)).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..5));
        source.shutdown().await.expect("the mock source stops");
    }

    /// Clause 6.23 carries the query in the body, so the POST binding walks by
    /// `offset`, counted in the elements the source sent, and ends on the first
    /// empty page.
    #[tokio::test]
    async fn the_post_binding_walks_by_offset_until_an_empty_page() {
        let source = source().await;
        source.enqueue(Method::POST, path(BATCH_PATH), page(0, 2, None)).await;
        source.enqueue(Method::POST, path(BATCH_PATH), page(2, 1, None)).await;
        source.enqueue(Method::POST, path(BATCH_PATH), page(3, 0, None)).await;
        let service = service(registration(&source, &["queryBatch"]));

        let outcome = walk(&service, QueryDelivery::Batch, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..3));
        let requests = source.drain_requests().await;
        let queries: Vec<Option<&str>> = requests.iter().map(|request| request.uri.query()).collect();
        pretty_assertions::assert_eq!(queries, vec![Some("limit=1000"), Some("limit=1000&offset=2"), Some("limit=1000&offset=3")]);
        source.shutdown().await.expect("the mock source stops");
    }
}

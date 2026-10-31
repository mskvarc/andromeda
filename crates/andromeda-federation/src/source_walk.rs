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
//! walk is the request's [`ResultBudget`], which every walk of the request
//! shares: once the sources together have answered more distinct entities, or
//! sent more elements, than it allows, the query is refused with
//! `TooManyResults`. The entities a walk keeps per source only end that walk;
//! the budget tells one entity several sources answer from several entities.
//!
//! The POST binding of clause 6.23 carries its query in the body, so a source
//! can hand back no pointer a client could dereference to the next page; that
//! walk asks for successive pages by `offset` (clause 6.3.10) and ends on the
//! first empty one.
//!
//! A Query Entities answer and a Query Temporal Evolution of Entities answer
//! are both an Entity Array a source pages the same way (clauses 5.7.2.4 and
//! 5.7.4.4), so one walk serves both; each reads its own pages through
//! [`WalkedAnswer`].

use crate::{
    FederationService,
    error::FederationError,
    forwarding_url::with_offset,
    incomplete_answer::{IncompleteReason, incomplete_answer},
    outcome::ForwardGetOutcome,
    read_fan_out::{ForwardedGetConditions, ForwardedRead, SourceAnswer},
    result_budget::{ResultBudget, ResultBudgetExceeded},
    source_continuation::SourceContinuation,
    source_read::SourceRead,
};
use andromeda_http_headers::ngsild_warning::NgsiLdWarning;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::{query::body::Query, registration::ContextSourceRegistration};
use futures::{StreamExt, stream};
use std::{collections::HashSet, mem, num::NonZeroU32};
use url::Url;

/// One source's answer as a walk gathers it, page by page.
///
/// Each kind of forwarded query reads a fetched page into its own answer; the
/// walk only needs to take a page's elements and warnings over and to tell two
/// elements for the same entity apart.
pub(crate) trait WalkedAnswer: Sized {
    /// One element of the Entity Array the source answers with.
    type Element;

    /// Read one fetched page: `None` for a 404, which contributes nothing
    /// (clause 6.3.17), beside how many elements the page carried.
    fn read(registration: &ContextSourceRegistration, outcome: ForwardGetOutcome) -> ReadPage<Self>;

    /// The elements gathered into this answer.
    fn elements(&mut self) -> &mut Vec<Self::Element>;

    /// The clause 6.3.17 warnings gathered into this answer.
    fn warnings(&mut self) -> &mut Vec<NgsiLdWarning>;

    /// The identifier of the entity an element stands for, when it names one.
    fn identifier(element: &Self::Element) -> Option<Identifier>;

    /// Take over what a later page of the same answer reported beside its
    /// elements, once the walk has taken the elements themselves.
    fn absorb(&mut self, page: Self);
}

/// One page of a source's answer, read.
pub(crate) struct ReadPage<A> {
    /// What the page was read into; `None` for a 404.
    pub(crate) answer: Option<A>,
    /// How many elements the page carried, as the source counts them: the
    /// members of an Entity Array, one for a single entity, none for anything
    /// else -- including any the broker then discards, such as an expired
    /// entity (clause 5.7.2.4), because that is what the source's own `offset`
    /// counts.
    pub(crate) sent: usize,
}

impl<A> ReadPage<A> {
    /// A page that carried no element.
    pub(crate) const fn empty(answer: Option<A>) -> ReadPage<A> {
        ReadPage { answer, sent: 0 }
    }
}

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
        /// The `Query` body every page repeats.
        body: Box<Query>,
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
    Post(Result<Url, FederationError>, Box<Query>),
}

/// One page a walk requested: the page a `next` pointer named, or the page of
/// a batch query starting at an offset.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum RequestedPage {
    /// The page a source's `next` pointer names.
    Pointer(Url),
    /// The page of a batch query starting after this many elements.
    Offset(u64),
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
    requested: HashSet<RequestedPage>,
    /// The identifiers gathered so far, so an offset walk over a source that
    /// ignores `offset` -- answering the same entities again -- ends instead of
    /// gathering them forever.
    gathered: HashSet<Identifier>,
}

impl AnswerPosition {
    /// Advance past a page of `elements` elements.
    fn advance(&mut self, elements: usize) {
        self.elements = self.elements.saturating_add(u64::try_from(elements).unwrap_or(u64::MAX));
    }

    /// Keep the elements of one page the walk has not gathered before: a
    /// source whose order shifted between two pages can answer one entity on
    /// both, and it is one entity of the source's answer. An element naming no
    /// entity cannot be matched against another and is kept.
    fn fresh<A: WalkedAnswer>(&mut self, elements: Vec<A::Element>) -> Vec<A::Element> {
        elements
            .into_iter()
            .filter(|element| A::identifier(element).is_none_or(|id| self.gathered.insert(id)))
            .collect()
    }

    /// Record `request`, answering `false` when it names a page already
    /// requested.
    fn first_request_of(&mut self, request: &NextPage) -> bool {
        let page = match request {
            NextPage::Pointer(url) => RequestedPage::Pointer(url.clone()), // clone: the set remembers the page the request still sends
            NextPage::Offset { .. } => RequestedPage::Offset(self.elements),
        };
        self.requested.insert(page)
    }
}

impl FederationService {
    /// Walk every target's answer to its end, concurrently up to the
    /// forwarding concurrency, and read what each gathered (clause 5.7.2.4).
    ///
    /// The first walk to cross `budget` ends them all: the walks still in
    /// flight are dropped and no target still waiting is contacted.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded`] once the targets together have
    /// answered more distinct entities, or sent more elements, than `budget`
    /// allows.
    pub(crate) async fn fan_out_walk<A: WalkedAnswer>(
        &self,
        targets: Vec<(ContextSourceRegistration, ForwardedRead)>,
        conditions: ForwardedGetConditions<'_>,
        budget: &ResultBudget,
    ) -> Result<Vec<SourceAnswer<A>>, ResultBudgetExceeded> {
        let mut walks = stream::iter(targets)
            .map(|(registration, first)| async move { self.walk_source::<A>(&registration, first, conditions, budget).await })
            .buffer_unordered(self.csr_concurrency);
        let mut answers = Vec::new();
        while let Some(walk) = walks.next().await {
            answers.push(walk?);
        }
        Ok(answers)
    }

    /// Walk one source's answer from `first` to its last page, charging each
    /// page to `budget`: every element the source sent as a row, and the
    /// entities the walk kept as entities unless the source is auxiliary.
    ///
    /// # Errors
    ///
    /// Returns [`ResultBudgetExceeded`] once everything charged to `budget`,
    /// by this walk and every other one sharing it, exceeds it. The walk asks
    /// before each request, so it sends none once another walk has crossed the
    /// limit.
    pub(crate) async fn walk_source<A: WalkedAnswer>(
        &self,
        registration: &ContextSourceRegistration,
        first: ForwardedRead,
        conditions: ForwardedGetConditions<'_>,
        budget: &ResultBudget,
    ) -> Result<SourceAnswer<A>, ResultBudgetExceeded> {
        budget.check()?;
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
        let ReadPage { answer, sent: first_sent } = A::read(registration, outcome);
        position.advance(first_sent);
        let Some(mut gathered) = answer else {
            return Ok(SourceAnswer {
                mode,
                error_status,
                entry: None,
            });
        };
        let first_page = mem::take(gathered.elements());
        *gathered.elements() = position.fresh::<A>(first_page);
        budget.charge_page(first_sent, gathered.elements().iter().map(A::identifier))?;
        // The offset walk of the POST binding ends on the first empty page.
        if first_sent == 0 && matches!(next, PageEnd::Continue(NextPage::Offset { .. })) {
            next = PageEnd::Stop(None);
        }
        loop {
            let request = match next {
                PageEnd::Stop(warning) => {
                    gathered.warnings().extend(warning);
                    break;
                }
                PageEnd::Continue(request) => request,
            };
            budget.check()?;
            if !position.first_request_of(&request) {
                gathered.warnings().push(incomplete_answer(&registration.id, IncompleteReason::PointedBack));
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
            let ReadPage { answer: page, sent } = match outcome {
                success @ ForwardGetOutcome::Success(_) => A::read(registration, success),
                ForwardGetOutcome::NotFound(warnings) => {
                    gathered.warnings().extend(warnings);
                    gathered.warnings().push(incomplete_answer(&registration.id, IncompleteReason::NextPageGone));
                    break;
                }
                failure @ (ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected) => {
                    // Clause 6.3.17: the entities already gathered stand, and
                    // the failure says the answer is incomplete.
                    gathered.warnings().extend(failure.warning(&registration.id));
                    break;
                }
            };
            position.advance(sent);
            let Some(mut page) = page else {
                break;
            };
            let fresh = position.fresh::<A>(mem::take(page.elements()));
            budget.charge_page(sent, fresh.iter().map(A::identifier))?;
            let progressed = !fresh.is_empty();
            gathered.elements().extend(fresh);
            gathered.absorb(page);
            next = match following {
                // The offset walk ends on the first empty page, and on the first
                // page adding nothing new: a source that ignores `offset`
                // answers its whole result again rather than an empty page.
                PageEnd::Continue(NextPage::Offset { .. }) if sent == 0 || !progressed => PageEnd::Stop(None),
                // An empty page that still points onward makes no progress a
                // bounded walk could rely on.
                PageEnd::Continue(NextPage::Pointer(_)) if sent == 0 => {
                    PageEnd::Stop(Some(incomplete_answer(&registration.id, IncompleteReason::EmptyPageWithNext)))
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
        let read = SourceRead::builder()
            .registration(registration)
            .inbound(conditions.context)
            .triggering(conditions.triggering)
            .linked_map(conditions.entity_map.location_for(&registration.id))
            .build();
        match fetch {
            Fetch::Get(url) => self.forward_get(read, url, conditions.cacheability).await,
            Fetch::Post(url, body) => self.forward_post_query(read, url, &body).await,
        }
    }
}

/// Where a GET page's `next` pointer leaves the walk.
fn pointer_continuation(registration: &ContextSourceRegistration, outcome: &ForwardGetOutcome) -> PageEnd {
    match outcome {
        ForwardGetOutcome::Success(response) => match &response.continuation {
            SourceContinuation::Complete => PageEnd::Stop(None),
            // clone: the walk owns the URL of the page it fetches next
            SourceContinuation::Next(url) => PageEnd::Continue(NextPage::Pointer(Url::clone(url))),
            SourceContinuation::Unfollowable(reason) => PageEnd::Stop(Some(incomplete_answer(&registration.id, IncompleteReason::Unfollowable(reason)))),
        },
        ForwardGetOutcome::NotFound(_) | ForwardGetOutcome::ErrorStatus(_) | ForwardGetOutcome::Transport(_) | ForwardGetOutcome::LoopDetected => {
            PageEnd::Stop(None)
        }
    }
}

#[cfg(test)]
#[expect(
    clippy::expect_used,
    clippy::panic,
    reason = "test code unwraps, expects and panics on purpose; migrating it to typed assertions is pending"
)]
mod tests {
    use crate::{
        FederationService,
        fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
        forwarded_query::client_query::ClientQuery,
        instance_budget::{InstanceBudget, InstanceBudgetExceeded},
        local_source::LocalSource,
        mock_source::{answer_in_turn, endpoint, received},
        operation::ForwardingOperation,
        outcome::{ForwardReadOutcome, ForwardedEntities, ForwardedTemporalEntities},
        query_forwarding_error::QueryForwardingError,
        request::{
            CsrModeFilter,
            EntityMapInUse,
            ForwardQueryRequest,
            ForwardTemporalQueryRequest,
            HistoryBudget,
            QueryDelivery,
            SourcePaging,
            TriggeringRequest,
        },
        result_budget::{ResultBudget, ResultBudgetExceeded, ResultLimits},
        source_reach::SourceReach,
        split_entities::SplitEntities,
    };
    use andromeda_configuration::schema::federation::FederationConfig;
    use andromeda_context_space::context_target::ContextTarget;
    use andromeda_http_headers::{
        ngsild_warning::{NgsiLdWarning, WarnCode},
        warn_text::WARN_TEXT_BYTES,
    };
    use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
    use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
    use andromeda_ngsi_ld_model::{
        registration::ContextSourceRegistration,
        temporal::{temporal_document::TemporalDocument, temporal_history::TemporalHistory},
    };
    use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
    use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
    use http::StatusCode;
    use serde_json::json;
    use std::{
        num::{NonZeroU32, NonZeroUsize},
        ops::Range,
        sync::Arc,
    };
    use wiremock::{MockServer, ResponseTemplate};

    /// The query resource every GET page of these walks lands on.
    const QUERY_PATH: &str = "/ngsi-ld/v1/entities";
    /// The temporal query resource every GET page of a temporal walk lands on.
    const TEMPORAL_PATH: &str = "/ngsi-ld/v1/temporal/entities";
    /// The batch query resource every POST page lands on.
    const BATCH_PATH: &str = "/ngsi-ld/v1/entityOperations/query";

    /// A mock Context Source listening on an ephemeral local port.
    async fn source() -> MockServer {
        MockServer::start().await
    }

    /// An inclusive registration of `Vehicle` pointing at `source`, advertising
    /// `operations`.
    fn registration(source: &MockServer, operations: &[&str]) -> ContextSourceRegistration {
        registration_of(source, "urn:ngsi-ld:ContextSourceRegistration:walk", "inclusive", operations)
    }

    /// The registration `id` of `Vehicle` under `mode` pointing at `source`,
    /// advertising `operations`.
    fn registration_of(source: &MockServer, id: &str, mode: &str, operations: &[&str]) -> ContextSourceRegistration {
        serde_json::from_value(json!({
            "id": id,
            "type": "ContextSourceRegistration",
            "endpoint": endpoint(source),
            "mode": mode,
            "operations": operations,
            "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
            "status": "ok",
            "createdAt": "2024-01-01T00:00:00Z",
            "modifiedAt": "2024-01-01T00:00:00Z"
        }))
        .expect("a valid registration")
    }

    fn service(registration: ContextSourceRegistration) -> FederationService {
        service_over(Arc::new(FixedRegistrationStore::new(vec![registration])))
    }

    /// A walking service choosing among what `store` holds.
    fn service_over(store: Arc<FixedRegistrationStore>) -> FederationService {
        service_configured(store, &FederationConfig::default())
    }

    /// A walking service choosing among what `store` holds, under `config`.
    fn service_configured(store: Arc<FixedRegistrationStore>, config: &FederationConfig) -> FederationService {
        FederationService::new(
            store,
            Arc::new(NoLocalEntities),
            JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
            LocalSource::new(
                ContextSourceAliases::new(ContextSourcePseudonym::new("walking-broker").expect("a valid alias")),
                TenantRef::UnnamedDefault,
            ),
            config,
        )
        .expect("the service builds")
    }

    /// One page of `count` Vehicles numbered from `first`, pointing at `next`
    /// when given.
    fn page(first: usize, count: usize, next: Option<&str>) -> ResponseTemplate {
        let entities: Vec<serde_json::Value> = (first..first + count)
            .map(|number| json!({"id": format!("urn:ngsi-ld:Vehicle:{number:04}"), "type": "Vehicle"}))
            .collect();
        let page = ResponseTemplate::new(StatusCode::OK).set_body_raw(serde_json::to_vec(&entities).expect("the page serializes"), "application/json");
        match next {
            Some(next) => page.insert_header("link", format!("<{next}>; rel=\"next\"")),
            None => page,
        }
    }

    /// One page holding the Vehicles `numbers` names, in that order.
    fn listed_page(numbers: &[usize]) -> ResponseTemplate {
        let entities: Vec<serde_json::Value> = numbers
            .iter()
            .map(|number| json!({"id": format!("urn:ngsi-ld:Vehicle:{number:04}"), "type": "Vehicle"}))
            .collect();
        ResponseTemplate::new(StatusCode::OK).set_body_raw(serde_json::to_vec(&entities).expect("the page serializes"), "application/json")
    }

    fn failure(status: StatusCode) -> ResponseTemplate {
        ResponseTemplate::new(status)
    }

    /// Limits of `entities` distinct entities and `rows` source rows.
    fn limits(entities: u32, rows: u32) -> ResultLimits {
        ResultLimits {
            distinct_entities: NonZeroU32::new(entities).expect("a positive entity limit"),
            source_rows: NonZeroU32::new(rows).expect("a positive row limit"),
        }
    }

    /// Forward `type=Vehicle` through `delivery` under an entity limit of
    /// `maximum` and no row ceiling it could reach.
    async fn walk(service: &FederationService, delivery: QueryDelivery, maximum: u32) -> Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError> {
        walk_under(service, delivery, limits(maximum, u32::MAX)).await
    }

    /// Forward `type=Vehicle` through `delivery` under `limits`.
    async fn walk_under(
        service: &FederationService,
        delivery: QueryDelivery,
        limits: ResultLimits,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError> {
        walk_against(service, delivery, &ResultBudget::new(limits)).await
    }

    /// Forward `type=Vehicle` through `delivery`, charging `budget`.
    async fn walk_against(
        service: &FederationService,
        delivery: QueryDelivery,
        budget: &ResultBudget,
    ) -> Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError> {
        let query: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle").expect("a valid query");
        let context = ContextSpec::none();
        let request = ForwardQueryRequest {
            client_query: ClientQuery::EntityQuery(&query),
            delivery,
            split: SplitEntities::Complete,
            context: &context,
            triggering: TriggeringRequest::default(),
            csf_expr: None,
            geo: None,
            entity_map: EntityMapInUse::None,
            budget,
        };
        let matched = service.match_query(&ContextTarget::Live, &request).await?;
        service.forward_query(&ContextTarget::Live, request, matched.into_registrations()).await
    }

    /// One temporal page of `count` Vehicles numbered from `first`, answered
    /// with `status`, pointing at `next` and carrying `content_range` when given.
    fn temporal_page(first: usize, count: usize, status: StatusCode, next: Option<&str>, content_range: Option<&str>) -> ResponseTemplate {
        let entities: Vec<serde_json::Value> = (first..first + count)
            .map(|number| {
                json!({
                    "id": format!("urn:ngsi-ld:Vehicle:{number:04}"),
                    "type": "Vehicle",
                    "speed": [{"type": "Property", "value": number, "observedAt": "2020-01-01T00:00:00Z"}]
                })
            })
            .collect();
        let page = ResponseTemplate::new(status).set_body_raw(serde_json::to_vec(&entities).expect("the page serializes"), "application/json");
        let page = match next {
            Some(next) => page.insert_header("link", format!("<{next}>; rel=\"next\"")),
            None => page,
        };
        match content_range {
            Some(content_range) => page.insert_header("content-range", content_range),
            None => page,
        }
    }

    /// Forward a temporal `type=Vehicle` query walking every source page under
    /// an entity limit of `maximum` and no row ceiling it could reach.
    async fn temporal_walk(service: &FederationService, maximum: u32) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        temporal_walk_under(service, limits(maximum, u32::MAX)).await
    }

    /// Forward a temporal `type=Vehicle` query walking every source page under
    /// `limits`.
    async fn temporal_walk_under(
        service: &FederationService,
        limits: ResultLimits,
    ) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        let query: TemporalQueryParameters =
            serde_urlencoded::from_str("type=Vehicle&timerel=after&timeAt=2019-01-01T00:00:00Z").expect("a valid temporal query");
        let budget = ResultBudget::new(limits);
        let context = ContextSpec::none();
        service
            .forward_temporal_query(
                &ContextTarget::Live,
                ForwardTemporalQueryRequest {
                    client_query: &query,
                    split: SplitEntities::Complete,
                    context: &context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    triggering: TriggeringRequest::default(),
                    csf_expr: None,
                    geo: None,
                    mode_filter: CsrModeFilter::All,
                    reach: SourceReach::Every,
                    entity_map: EntityMapInUse::None,
                    paging: SourcePaging::Walk(&budget),
                },
            )
            .await
    }

    /// The identifiers a temporal walk gathered, in the order the source sent them.
    fn temporal_gathered(outcome: &ForwardReadOutcome<ForwardedTemporalEntities>) -> Vec<String> {
        outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)))
            .collect()
    }

    /// The identifiers the walk gathered, in the order the source sent them.
    fn gathered(outcome: &ForwardReadOutcome<ForwardedEntities>) -> Vec<String> {
        outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|received| received.state.id().as_str().to_owned()))
            .collect()
    }

    fn warnings(outcome: &ForwardReadOutcome<ForwardedEntities>) -> Vec<&NgsiLdWarning> {
        outcome.results.iter().flat_map(|answer| answer.warnings.iter()).collect()
    }

    /// Whether `outcome` carries a warning with `code` whose text holds `text`.
    fn warned(outcome: &ForwardReadOutcome<ForwardedEntities>, code: WarnCode, text: &str) -> bool {
        warnings(outcome)
            .iter()
            .any(|warning| warning.code() == code && warning.text().as_str().contains(text))
    }

    fn positive(value: u32) -> NonZeroU32 {
        NonZeroU32::new(value).expect("a positive limit")
    }

    /// The bound a forwarded query was refused for.
    fn refusal<T>(outcome: Result<ForwardReadOutcome<T>, QueryForwardingError>) -> ResultBudgetExceeded {
        match outcome {
            Err(QueryForwardingError::TooManyResults(exceeded)) => exceeded,
            Err(QueryForwardingError::Federation(error)) => panic!("the walk must be refused for its budget, not fail: {error}"),
            Err(QueryForwardingError::TooManyInstances(exceeded)) => panic!("the walk must be refused for its entities: {exceeded}"),
            Ok(outcome) => panic!("the walk must be refused, it gathered {} answers", outcome.results.len()),
        }
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
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [
                page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")),
                page(2, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=4")),
                page(4, 1, None),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..5));
        assert!(warnings(&outcome).is_empty(), "{:?}", warnings(&outcome));
        let requests = received(&source).await;
        let queries: Vec<Option<&str>> = requests.iter().map(|request| request.url.query()).collect();
        pretty_assertions::assert_eq!(queries.len(), 3);
        assert!(queries[0].is_some_and(|query| query.contains("limit=1000")), "{queries:?}");
        pretty_assertions::assert_eq!(queries[1], Some("type=Vehicle&offset=2"));
        pretty_assertions::assert_eq!(queries[2], Some("type=Vehicle&offset=4"));
    }

    /// Clause 5.5.9.1: an answer without a `next` pointer is complete after its
    /// first page.
    #[tokio::test]
    async fn an_answer_without_a_next_pointer_is_read_once() {
        let source = source().await;
        answer_in_turn(&source, "GET", QUERY_PATH, [page(0, 3, None)]).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..3));
        pretty_assertions::assert_eq!(received(&source).await.len(), 1);
    }

    /// A source pointing back at a page it already answered cannot keep the
    /// walk going: what was gathered stands, and a warning says the answer is
    /// incomplete (clause 6.3.17).
    #[tokio::test]
    async fn a_repeated_next_pointer_ends_the_walk_with_a_warning() {
        let source = source().await;
        let looping = "/ngsi-ld/v1/entities?type=Vehicle&offset=2";
        answer_in_turn(&source, "GET", QUERY_PATH, [page(0, 2, Some(looping)), page(2, 2, Some(looping))]).await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..4));
        pretty_assertions::assert_eq!(received(&source).await.len(), 2);
        assert!(warned(&outcome, WarnCode::RevalidationFailed, "already answered"), "{:?}", warnings(&outcome));
    }

    /// An empty page that still points onward makes no progress, so the walk
    /// ends there with a warning rather than following it.
    #[tokio::test]
    async fn an_empty_page_that_still_points_onward_ends_the_walk_with_a_warning() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [
                page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")),
                page(2, 0, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=3")),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(received(&source).await.len(), 2);
        assert!(warned(&outcome, WarnCode::RevalidationFailed, "empty page"), "{:?}", warnings(&outcome));
    }

    /// Clause 6.3.17: a page that fails keeps what the earlier pages gathered
    /// and reports the failure as a warning, never as the query's failure.
    #[tokio::test]
    async fn a_failing_page_keeps_what_was_gathered_and_warns() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [
                page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")),
                failure(StatusCode::INTERNAL_SERVER_ERROR),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        assert!(
            warned(&outcome, WarnCode::MiscellaneousPersistentWarning, "HTTP 500"),
            "{:?}",
            warnings(&outcome)
        );
    }

    /// Table 6.3.17-1: a later page answered 404 is an error response from a
    /// source that pointed at it, so the walk ends with a 299 and keeps what
    /// the earlier pages gathered. Only a 404 for the whole answer is the
    /// normal empty answer of clause 6.3.17.
    #[tokio::test]
    async fn a_next_page_the_source_no_longer_finds_ends_the_walk_with_a_persistent_warning() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")), failure(StatusCode::NOT_FOUND)],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        let [warning] = warnings(&outcome).try_into().expect("one warning");
        pretty_assertions::assert_eq!(warning.code(), WarnCode::MiscellaneousPersistentWarning);
        assert!(warning.text().as_str().contains("no longer found the next page"), "{warning}");
    }

    /// A request this broker cannot build never reaches the source: its
    /// warning says so without blaming the source, and nothing is recorded on
    /// the registration, whose Table 5.2.9-2 counters count requests sent.
    #[tokio::test]
    async fn a_request_that_cannot_be_prepared_is_reported_without_blaming_the_source_and_not_recorded() {
        let source = source().await;
        answer_in_turn(&source, "GET", QUERY_PATH, [page(0, 2, None)]).await;
        let mut unresolvable = registration(&source, &["queryEntity"]);
        unresolvable.access.context_source_info = serde_json::from_value(json!([
            {"key": "jsonldContext", "value": "http://contexts.invalid/never-fetched.jsonld"}
        ]))
        .expect("valid context source info");
        let store = Arc::new(FixedRegistrationStore::new(vec![unresolvable]));
        let service = service_over(Arc::clone(&store));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), Vec::<String>::new());
        pretty_assertions::assert_eq!(received(&source).await.len(), 0);
        pretty_assertions::assert_eq!(store.recorded_outcomes(), 0);
        let [warning] = warnings(&outcome).try_into().expect("one warning");
        pretty_assertions::assert_eq!(warning.code(), WarnCode::MiscellaneousWarning);
        assert!(warning.text().as_str().contains("could not be prepared and was not sent"), "{warning}");
        assert!(!warning.text().as_str().contains("did not respond"), "{warning}");
        assert!(!warning.text().as_str().contains("contexts.invalid"), "{warning}");
    }

    /// Clause 6.3.17 relays what a source reports about the sources behind it;
    /// a relayed text past the bound arrives cut, so one source cannot grow the
    /// response header without limit.
    #[tokio::test]
    async fn a_relayed_warning_is_cut_to_the_text_bound() {
        let source = source().await;
        let relayed = format!("299 - \"{}\"", "x".repeat(147_000));
        answer_in_turn(&source, "GET", QUERY_PATH, [page(0, 1, None).insert_header("ngsild-warning", relayed.as_str())]).await;
        let store = Arc::new(FixedRegistrationStore::new(vec![registration(&source, &["queryEntity"])]));
        let service = service_over(Arc::clone(&store));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..1));
        pretty_assertions::assert_eq!(store.recorded_outcomes(), 1);
        let [warning] = warnings(&outcome).try_into().expect("one warning");
        pretty_assertions::assert_eq!(warning.code(), WarnCode::MiscellaneousPersistentWarning);
        pretty_assertions::assert_eq!(warning.text().quoted().to_string().len(), WARN_TEXT_BYTES);
        assert!(warning.text().as_str().ends_with("..."), "{warning}");
    }

    /// Clause 4.12: once the sources together gather more than the request's
    /// budget, the query is refused with `TooManyResults`.
    #[tokio::test]
    async fn a_walk_exceeding_the_budget_is_refused_with_too_many_results() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")), page(2, 2, None)],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let refused = refusal(walk(&service, QueryDelivery::Query, 3).await);

        pretty_assertions::assert_eq!(refused, ResultBudgetExceeded::DistinctEntities { maximum: positive(3) });
    }

    /// Clause 4.12: the first walk to cross the limit ends the query. Ten
    /// sources each answer four entities of their own over two pages under an
    /// entity limit of 3 and a concurrency of 2, so only the sources the walk
    /// had started are ever contacted.
    #[tokio::test]
    async fn a_walk_crossing_the_limit_leaves_the_waiting_sources_uncontacted() {
        let mut sources = Vec::new();
        let mut registrations = Vec::new();
        for index in 0..10_usize {
            let source = source().await;
            let next = format!("/ngsi-ld/v1/entities?type=Vehicle&offset=2&walk={index}");
            answer_in_turn(
                &source,
                "GET",
                QUERY_PATH,
                [page(index * 10, 2, Some(next.as_str())), page(index * 10 + 2, 2, None)],
            )
            .await;
            registrations.push(registration_of(
                &source,
                &format!("urn:ngsi-ld:ContextSourceRegistration:crossing-{index}"),
                "inclusive",
                &["queryEntity"],
            ));
            sources.push(source);
        }
        let mut config = FederationConfig::default();
        config.forwarding.max_concurrent = NonZeroUsize::MIN.saturating_add(1);
        let service = service_configured(Arc::new(FixedRegistrationStore::new(registrations)), &config);

        let refused = refusal(walk(&service, QueryDelivery::Query, 3).await);

        pretty_assertions::assert_eq!(refused, ResultBudgetExceeded::DistinctEntities { maximum: positive(3) });
        let mut contacted = 0;
        for source in &sources {
            if !received(source).await.is_empty() {
                contacted += 1;
            }
        }
        assert!(contacted <= 2, "{contacted} of the ten sources were contacted");
    }

    /// A budget another walk has already crossed sends no request at all, so a
    /// source never starts an answer the query is already refused for.
    #[tokio::test]
    async fn a_budget_already_crossed_sends_no_request() {
        let source = source().await;
        answer_in_turn(&source, "GET", QUERY_PATH, [page(0, 2, None)]).await;
        let service = service(registration(&source, &["queryEntity"]));
        let budget = ResultBudget::new(limits(1, u32::MAX));
        let crossing = (0..2_u32).map(|number| Some(format!("urn:ngsi-ld:Vehicle:{number:04}").parse().expect("a valid identifier")));
        assert!(budget.charge_page(2, crossing).is_err());

        let refused = refusal(walk_against(&service, QueryDelivery::Query, &budget).await);

        pretty_assertions::assert_eq!(refused, ResultBudgetExceeded::DistinctEntities { maximum: positive(1) });
        pretty_assertions::assert_eq!(received(&source).await.len(), 0);
    }

    /// A budget the answer fits into exactly is not exceeded.
    #[tokio::test]
    async fn a_walk_gathering_exactly_the_budget_completes() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [page(0, 2, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")), page(2, 2, None)],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 4).await.expect("the walk fits its budget");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..4));
    }

    /// Clause 4.3.6.1: the broker contacts the registration it matched, so a
    /// pointer to another origin is not followed.
    #[tokio::test]
    async fn a_next_pointer_to_another_origin_is_not_followed() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [page(0, 2, Some("http://elsewhere.invalid/ngsi-ld/v1/entities?offset=2"))],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(received(&source).await.len(), 1);
        assert!(warned(&outcome, WarnCode::RevalidationFailed, "origin"), "{:?}", warnings(&outcome));
        assert!(
            !warnings(&outcome).iter().any(|warning| warning.text().as_str().contains("elsewhere.invalid")),
            "{:?}",
            warnings(&outcome)
        );
    }

    /// A source that ignores `offset` answers its whole result again; the
    /// offset walk ends on the first page that adds nothing new, gathering each
    /// entity once rather than until the budget refuses the query.
    #[tokio::test]
    async fn an_offset_walk_over_a_source_ignoring_offset_ends_on_the_first_repeat() {
        let source = source().await;
        answer_in_turn(&source, "POST", BATCH_PATH, [page(0, 2, None)]).await;
        let service = service(registration(&source, &["queryBatch"]));

        let outcome = walk(&service, QueryDelivery::Batch, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..2));
        pretty_assertions::assert_eq!(received(&source).await.len(), 2);
    }

    /// An entity a source answers on two pages is one entity of its answer.
    #[tokio::test]
    async fn an_entity_answered_on_two_pages_is_gathered_once() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            QUERY_PATH,
            [page(0, 3, Some("/ngsi-ld/v1/entities?type=Vehicle&offset=2")), page(2, 3, None)],
        )
        .await;
        let service = service(registration(&source, &["queryEntity"]));

        let outcome = walk(&service, QueryDelivery::Query, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..5));
    }

    /// Clause 5.7.4.4: "The result of each remote query is an Entity Array"
    /// merged into the result, so a temporal query follows every `next`
    /// pointer of the source's answer, as an entity query does.
    #[tokio::test]
    async fn a_temporal_query_follows_every_next_pointer_of_the_source() {
        let source = source().await;
        let next = "/ngsi-ld/v1/temporal/entities?type=Vehicle&offset=2";
        answer_in_turn(
            &source,
            "GET",
            TEMPORAL_PATH,
            [
                temporal_page(0, 2, StatusCode::OK, Some(next), None),
                temporal_page(2, 1, StatusCode::OK, None, None),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryTemporal"]));

        let outcome = temporal_walk(&service, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(temporal_gathered(&outcome), numbered(0..3));
        pretty_assertions::assert_eq!(received(&source).await.len(), 2);
        assert!(outcome.results.iter().all(|answer| answer.partial_ranges.is_empty()));
    }

    /// Clause 6.3.10: every page the source cut short with a 206 says where,
    /// and the answer keeps each of those `Content-Range`s for the merge.
    #[tokio::test]
    async fn every_partial_page_of_a_temporal_answer_keeps_its_content_range() {
        let source = source().await;
        let next = "/ngsi-ld/v1/temporal/entities?type=Vehicle&offset=1";
        let first_range = "DateTime 2019-01-01T00:00:00Z-2020-01-01T00:00:00Z/*";
        let second_range = "DateTime 2019-01-01T00:00:00Z-2020-02-01T00:00:00Z/*";
        answer_in_turn(
            &source,
            "GET",
            TEMPORAL_PATH,
            [
                temporal_page(0, 1, StatusCode::PARTIAL_CONTENT, Some(next), Some(first_range)),
                temporal_page(1, 1, StatusCode::PARTIAL_CONTENT, None, Some(second_range)),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryTemporal"]));

        let outcome = temporal_walk(&service, 100).await.expect("the walk completes");

        let ranges: Vec<String> = outcome
            .results
            .iter()
            .flat_map(|answer| answer.partial_ranges.iter().map(ToString::to_string))
            .collect();
        pretty_assertions::assert_eq!(ranges, vec![first_range.to_owned(), second_range.to_owned()]);
        pretty_assertions::assert_eq!(temporal_gathered(&outcome), numbered(0..2));
    }

    /// A 206 that does not say where it stopped cannot be combined with the
    /// rest of the history, so the client is warned its part may be incomplete.
    #[tokio::test]
    async fn a_partial_temporal_page_without_a_content_range_is_reported() {
        let source = source().await;
        answer_in_turn(&source, "GET", TEMPORAL_PATH, [temporal_page(0, 1, StatusCode::PARTIAL_CONTENT, None, None)]).await;
        let service = service(registration(&source, &["queryTemporal"]));

        let outcome = temporal_walk(&service, 100).await.expect("the walk completes");

        let warnings: Vec<&NgsiLdWarning> = outcome.results.iter().flat_map(|answer| answer.warnings.iter()).collect();
        assert!(
            warnings
                .iter()
                .any(|warning| warning.code() == WarnCode::RevalidationFailed && warning.text().as_str().contains("without a readable Content-Range")),
            "{warnings:?}"
        );
    }

    /// Clause 4.12: a temporal walk gathering more entities than the budget is
    /// refused with `TooManyResults`, exactly as an entity query is.
    #[tokio::test]
    async fn a_temporal_walk_exceeding_the_budget_is_refused_with_too_many_results() {
        let source = source().await;
        let next = "/ngsi-ld/v1/temporal/entities?type=Vehicle&offset=2";
        answer_in_turn(
            &source,
            "GET",
            TEMPORAL_PATH,
            [
                temporal_page(0, 2, StatusCode::OK, Some(next), None),
                temporal_page(2, 2, StatusCode::OK, None, None),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryTemporal"]));

        let refused = refusal(temporal_walk(&service, 3).await);

        pretty_assertions::assert_eq!(refused, ResultBudgetExceeded::DistinctEntities { maximum: positive(3) });
    }

    /// Three sources answering the same Vehicles `0000` to `0004`, each under
    /// an inclusive registration advertising `operation`, answering `answer`
    /// on `path`.
    async fn three_overlapping_sources(path: &str, operation: &str, answer: impl Fn() -> ResponseTemplate) -> (Vec<MockServer>, FederationService) {
        let mut sources = Vec::new();
        let mut registrations = Vec::new();
        for index in 0..3 {
            let source = source().await;
            answer_in_turn(&source, "GET", path, [answer()]).await;
            registrations.push(registration_of(
                &source,
                &format!("urn:ngsi-ld:ContextSourceRegistration:overlap-{index}"),
                "inclusive",
                &[operation],
            ));
            sources.push(source);
        }
        (sources, service_over(Arc::new(FixedRegistrationStore::new(registrations))))
    }

    /// Clause 5.7.2.4 merges the sources' answers into one entity each, so
    /// three sources holding the same five entities answer five entities: the
    /// entity limit counts them once, and only the row ceiling sees all
    /// fifteen elements.
    #[tokio::test]
    async fn three_sources_holding_the_same_entities_are_gathered_under_the_entity_limit() {
        let (_sources, service) = three_overlapping_sources(QUERY_PATH, "queryEntity", || page(0, 5, None)).await;

        let outcome = walk_under(&service, QueryDelivery::Query, limits(5, 15))
            .await
            .expect("the walk fits its limits");

        pretty_assertions::assert_eq!(outcome.results.len(), 3);
        for answer in &outcome.results {
            let ids: Vec<String> = answer.entities.iter().map(|received| received.state.id().as_str().to_owned()).collect();
            pretty_assertions::assert_eq!(ids, numbered(0..5));
        }
    }

    /// Clause 4.12: the elements every source sends, duplicates included, are
    /// bounded too, and the refusal names that bound.
    #[tokio::test]
    async fn sources_fetching_past_the_row_ceiling_are_refused_naming_source_rows() {
        let (_sources, service) = three_overlapping_sources(QUERY_PATH, "queryEntity", || page(0, 5, None)).await;

        let refused = refusal(walk_under(&service, QueryDelivery::Query, limits(5, 14)).await);

        pretty_assertions::assert_eq!(refused, ResultBudgetExceeded::SourceRows { maximum: positive(14) });
    }

    /// Walk two inclusive sources answering entities 0 to 4 and an auxiliary
    /// one answering 0, 1, 2, 100 and 101 under `distinct_entities`, returning
    /// the walk and the auxiliary source.
    async fn walk_beside_an_auxiliary_source(distinct_entities: u32) -> (Result<ForwardReadOutcome<ForwardedEntities>, QueryForwardingError>, MockServer) {
        let mut registrations = Vec::new();
        let mut sources = Vec::new();
        for index in 0..2 {
            let inclusive = source().await;
            answer_in_turn(&inclusive, "GET", QUERY_PATH, [page(0, 5, None)]).await;
            registrations.push(registration_of(
                &inclusive,
                &format!("urn:ngsi-ld:ContextSourceRegistration:inclusive-{index}"),
                "inclusive",
                &["queryEntity"],
            ));
            sources.push(inclusive);
        }
        let auxiliary = source().await;
        answer_in_turn(&auxiliary, "GET", QUERY_PATH, [listed_page(&[0, 1, 2, 100, 101])]).await;
        registrations.push(registration_of(
            &auxiliary,
            "urn:ngsi-ld:ContextSourceRegistration:auxiliary",
            "auxiliary",
            &["queryEntity"],
        ));
        let service = service_over(Arc::new(FixedRegistrationStore::new(registrations)));
        (walk_under(&service, QueryDelivery::Query, limits(distinct_entities, 15)).await, auxiliary)
    }

    /// Clause 5.7.2.4: an entity only an auxiliary source answers is part of
    /// the result, so it is charged as an entity, while one it shares with
    /// another source counts once: seven distinct entities, fifteen rows.
    #[tokio::test]
    async fn an_auxiliary_source_charges_the_entities_only_it_answers() {
        let (outcome, auxiliary) = walk_beside_an_auxiliary_source(7).await;
        let outcome = outcome.expect("the walk fits its limits");
        pretty_assertions::assert_eq!(gathered(&outcome).len(), 15);
        pretty_assertions::assert_eq!(received(&auxiliary).await.len(), 1);

        let (refused, _auxiliary) = walk_beside_an_auxiliary_source(6).await;
        assert!(refused.is_err(), "the two entities only the auxiliary source answers cross a limit of six");
    }

    /// The temporal walk shares the request's budget the same way: three
    /// sources answering the same three entities charge each once.
    #[tokio::test]
    async fn a_temporal_walk_over_overlapping_sources_charges_each_entity_once() {
        let (_sources, service) = three_overlapping_sources(TEMPORAL_PATH, "queryTemporal", || temporal_page(0, 3, StatusCode::OK, None, None)).await;

        let outcome = temporal_walk_under(&service, limits(3, 9)).await.expect("the walk fits its limits");

        pretty_assertions::assert_eq!(temporal_gathered(&outcome).len(), 9);
    }

    /// Clause 6.23 carries the query in the body, so the POST binding walks by
    /// `offset`, counted in the elements the source sent, and ends on the first
    /// empty page.
    #[tokio::test]
    async fn the_post_binding_walks_by_offset_until_an_empty_page() {
        let source = source().await;
        answer_in_turn(&source, "POST", BATCH_PATH, [page(0, 2, None), page(2, 1, None), page(3, 0, None)]).await;
        let service = service(registration(&source, &["queryBatch"]));

        let outcome = walk(&service, QueryDelivery::Batch, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(gathered(&outcome), numbered(0..3));
        let requests = received(&source).await;
        let queries: Vec<Option<&str>> = requests.iter().map(|request| request.url.query()).collect();
        pretty_assertions::assert_eq!(queries, vec![Some("limit=1000"), Some("limit=1000&offset=2"), Some("limit=1000&offset=3")]);
    }

    /// One temporal page holding `urn:ngsi-ld:Vehicle:0000` with one `speed`
    /// instance per `(value, observedAt)`, each identified by its value,
    /// answered with `status` and carrying `content_range` when given.
    fn history_page(instances: &[(i64, &str)], status: StatusCode, content_range: Option<&str>) -> ResponseTemplate {
        let speed: Vec<serde_json::Value> = instances
            .iter()
            .map(|(value, at)| json!({"type": "Property", "value": value, "observedAt": at, "instanceId": format!("urn:ngsi-ld:instance:{value}")}))
            .collect();
        let entities = json!([{"id": "urn:ngsi-ld:Vehicle:0000", "type": "Vehicle", "speed": speed}]);
        let page = ResponseTemplate::new(status).set_body_raw(serde_json::to_vec(&entities).expect("the page serializes"), "application/json");
        match content_range {
            Some(content_range) => page.insert_header("content-range", content_range),
            None => page,
        }
    }

    /// Forward an aggregated temporal `type=Vehicle` query reading the sources'
    /// history to the end, under an instance budget of `instances`.
    async fn history_walk(service: &FederationService, instances: u32) -> Result<ForwardReadOutcome<ForwardedTemporalEntities>, QueryForwardingError> {
        let query: TemporalQueryParameters =
            serde_urlencoded::from_str("type=Vehicle&timerel=after&timeAt=2019-01-01T00:00:00Z&format=aggregatedValues&aggrMethods=avg")
                .expect("a valid temporal query");
        let entities = ResultBudget::new(limits(100, u32::MAX));
        let instances = InstanceBudget::new(NonZeroU32::new(instances).expect("a positive budget"));
        let context = ContextSpec::none();
        service
            .forward_temporal_query(
                &ContextTarget::Live,
                ForwardTemporalQueryRequest {
                    client_query: &query,
                    split: SplitEntities::Split,
                    context: &context,
                    operation: ForwardingOperation::QueryTemporalEntity,
                    triggering: TriggeringRequest::default(),
                    csf_expr: None,
                    geo: None,
                    mode_filter: CsrModeFilter::All,
                    reach: SourceReach::Every,
                    entity_map: EntityMapInUse::None,
                    paging: SourcePaging::WalkHistory(HistoryBudget {
                        entities: &entities,
                        instances: &instances,
                    }),
                },
            )
            .await
    }

    /// The values a history walk gathered for the one entity, each instance
    /// identified by its value.
    fn speed_values(outcome: &ForwardReadOutcome<ForwardedTemporalEntities>) -> Vec<i64> {
        outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter())
            .filter_map(TemporalHistory::instances)
            .flat_map(TemporalDocument::instances)
            .filter_map(|(_, instance)| instance.instance_id()?.as_str().rsplit(':').next()?.parse().ok())
            .collect()
    }

    /// Clause 6.3.10: a 206 says up to which instant the source answered, so
    /// the walk asks again from there and folds the rest of the history into
    /// the entity, each instance once; the source is asked for instances, not
    /// for its own aggregate.
    #[tokio::test]
    async fn a_partial_answer_is_resumed_at_its_range_end() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            TEMPORAL_PATH,
            [
                history_page(
                    &[(1, "2020-01-01T00:00:00Z"), (2, "2020-01-02T00:00:00Z")],
                    StatusCode::PARTIAL_CONTENT,
                    Some("DateTime 2020-01-01T00:00:00Z-2020-01-02T00:00:00Z/4"),
                ),
                history_page(
                    &[(2, "2020-01-02T00:00:00Z"), (3, "2020-01-03T00:00:00Z"), (4, "2020-01-04T00:00:00Z")],
                    StatusCode::OK,
                    None,
                ),
            ],
        )
        .await;
        let service = service(registration(&source, &["queryTemporal"]));

        let outcome = history_walk(&service, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(speed_values(&outcome), vec![1, 2, 3, 4]);
        assert!(outcome.results.iter().all(|answer| answer.partial_ranges.is_empty()));
        let requests = received(&source).await;
        let queries: Vec<&str> = requests.iter().filter_map(|request| request.url.query()).collect();
        pretty_assertions::assert_eq!(queries.len(), 2, "{queries:?}");
        for query in &queries {
            assert!(!query.contains("aggrMethods") && !query.contains("format"), "{query}");
        }
        assert!(queries[1].contains("timerel=after&timeAt=2020-01-02T00%3A00%3A00Z"), "{queries:?}");
    }

    /// A source cut short at the instant it was resumed at makes no progress,
    /// so the walk stops with a warning rather than asking forever.
    #[tokio::test]
    async fn a_range_that_does_not_advance_stops_with_a_warning() {
        let source = source().await;
        let stuck = "DateTime 2020-01-01T00:00:00Z-2020-01-02T00:00:00Z/*";
        for _ in 0..3 {
            answer_in_turn(
                &source,
                "GET",
                TEMPORAL_PATH,
                [history_page(&[(2, "2020-01-02T00:00:00Z")], StatusCode::PARTIAL_CONTENT, Some(stuck))],
            )
            .await;
        }
        let service = service(registration(&source, &["queryTemporal"]));

        let outcome = history_walk(&service, 100).await.expect("the walk completes");

        pretty_assertions::assert_eq!(received(&source).await.len(), 2);
        let warnings: Vec<String> = outcome
            .results
            .iter()
            .flat_map(|answer| answer.warnings.iter().map(ToString::to_string))
            .collect();
        assert!(warnings.iter().any(|warning| warning.contains("cannot be resumed past")), "{warnings:?}");
        assert!(
            outcome.results.iter().all(|answer| answer.partial_ranges.len() == 1),
            "the history is complete only up to the stop it could not be resumed past"
        );
    }

    /// Clause 4.12: once the sources' history holds more instances than the
    /// instance budget, the query is refused with `TooManyResults`.
    #[tokio::test]
    async fn instances_beyond_the_budget_are_too_many_results() {
        let source = source().await;
        answer_in_turn(
            &source,
            "GET",
            TEMPORAL_PATH,
            [history_page(
                &[(1, "2020-01-01T00:00:00Z"), (2, "2020-01-02T00:00:00Z"), (3, "2020-01-03T00:00:00Z")],
                StatusCode::OK,
                None,
            )],
        )
        .await;
        let service = service(registration(&source, &["queryTemporal"]));

        match history_walk(&service, 2).await {
            Err(QueryForwardingError::TooManyInstances(InstanceBudgetExceeded { maximum })) => pretty_assertions::assert_eq!(maximum.get(), 2),
            Err(QueryForwardingError::TooManyResults(exceeded)) => panic!("the entities fit their budget: {exceeded}"),
            Err(QueryForwardingError::Federation(error)) => panic!("the walk must be refused for its instances, not fail: {error}"),
            Ok(outcome) => panic!("the walk must be refused, it gathered {:?}", speed_values(&outcome)),
        }
    }
}

//! A read reaches a source declaring no `scope` whatever its Scope query:
//! clause 5.12 matching compares no scope, and Table 5.2.9-1 leaves such a
//! source free to hold an Entity in any Scope. Under split entities the filters
//! are removed before forwarding and applied to the merged entities (clauses
//! 5.7.2.4 and 5.7.4.4); otherwise they travel and the source applies them
//! (clause 4.3.6.7).

use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_query::client_query::ClientQuery,
    local_source::LocalSource,
    mock_source::{answer_in_turn, endpoint, received},
    operation::ForwardingOperation,
    request::{CsrModeFilter, EntityMapInUse, ForwardQueryRequest, ForwardTemporalQueryRequest, QueryDelivery, SourcePaging, TriggeringRequest},
    result_budget::{ResultBudget, ResultLimits},
    source_reach::SourceReach,
    split_entities::SplitEntities,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use http::StatusCode;
use serde_json::json;
use std::{num::NonZeroU32, sync::Arc};
use wiremock::{MockServer, ResponseTemplate};

const QUERY_PATH: &str = "/ngsi-ld/v1/entities";
const TEMPORAL_PATH: &str = "/ngsi-ld/v1/temporal/entities";

/// The client filters a split read holds back from every source.
const DEFERRED: [&str; 6] = ["scopeQ", "q", "attrs", "georel", "geometry", "coordinates"];

/// The client filters of the temporal query.
const TEMPORAL_FILTERS: [&str; 3] = ["scopeQ", "q", "attrs"];

const SPLIT_QUERY: &str =
    "type=Vehicle&scopeQ=%2FBarcelona&q=speed%3E10&attrs=speed&georel=near%3BmaxDistance%3D%3D1000&geometry=Point&coordinates=%5B2.17%2C41.38%5D";

const SPLIT_TEMPORAL_QUERY: &str = "type=Vehicle&timerel=after&timeAt=2019-01-01T00:00:00Z&scopeQ=%2FBarcelona&q=speed%3E10&attrs=speed";

/// An inclusive registration of `Vehicle` declaring no `scope`, pointing at
/// `source` and serving `operation`.
fn unscoped(source: &MockServer, operation: &str) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:unscoped",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": [operation],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

fn service(registration: ContextSourceRegistration) -> FederationService {
    let store = Arc::new(FixedRegistrationStore::new(vec![registration]));
    let census = store.census();
    FederationService::new(
        store,
        census,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("splitting-broker").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

fn budget() -> ResultBudget {
    ResultBudget::new(ResultLimits {
        distinct_entities: NonZeroU32::new(100).expect("a positive limit"),
        source_rows: NonZeroU32::new(100).expect("a positive limit"),
    })
}

/// One Barcelona Vehicle as the source at `path` answers it: its history on
/// the temporal resource, its current state otherwise.
fn barcelona_vehicle(path: &str) -> ResponseTemplate {
    let speed = if path == TEMPORAL_PATH {
        json!([{"type": "Property", "value": 12, "observedAt": "2020-01-01T00:00:00Z"}])
    } else {
        json!({"type": "Property", "value": 12})
    };
    let entities = json!([{"id": "urn:ngsi-ld:Vehicle:barcelona", "type": "Vehicle", "scope": "/Barcelona", "speed": speed}]);
    ResponseTemplate::new(StatusCode::OK).set_body_raw(serde_json::to_vec(&entities).expect("the answer serializes"), "application/json")
}

/// The names of the query parameters each request `source` received carried.
async fn forwarded_parameters(source: &MockServer) -> Vec<Vec<String>> {
    received(source)
        .await
        .iter()
        .map(|request| request.url.query_pairs().map(|(name, _)| name.into_owned()).collect())
        .collect()
}

fn assert_filters_travel(forwarded: &[Vec<String>], filters: &[&str]) {
    pretty_assertions::assert_eq!(forwarded.len(), 1, "one request reaches the source: {forwarded:?}");
    let names = &forwarded[0];
    for filter in filters {
        assert!(names.iter().any(|name| name == filter), "{filter} travels: {names:?}");
    }
}

fn assert_selectors_alone(forwarded: &[Vec<String>]) {
    pretty_assertions::assert_eq!(forwarded.len(), 1, "one request reaches the source: {forwarded:?}");
    let names = &forwarded[0];
    assert!(names.iter().any(|name| name == "type"), "the type travels: {names:?}");
    assert!(
        !names.iter().any(|name| DEFERRED.contains(&name.as_str())),
        "no deferred filter travels: {names:?}"
    );
}

/// Run an entity query under `split` against a source declaring no scope and
/// answer the query parameters of each request it received.
async fn entity_query(split: SplitEntities) -> Vec<Vec<String>> {
    let source = MockServer::builder().start().await;
    answer_in_turn(&source, "GET", QUERY_PATH, [barcelona_vehicle(QUERY_PATH)]).await;
    let service = service(unscoped(&source, "queryEntity"));
    let query: EntityQueryParameters = serde_urlencoded::from_str(SPLIT_QUERY).expect("a valid query");
    let context = ContextSpec::none();
    let budget = budget();
    let request = || ForwardQueryRequest {
        client_query: ClientQuery::EntityQuery(&query),
        delivery: QueryDelivery::Query,
        split,
        context: &context,
        triggering: TriggeringRequest::default(),
        csf_expr: None,
        geo: None,
        entity_map: EntityMapInUse::None,
        budget: &budget,
    };
    let phased = service
        .match_query(&ContextTarget::Live, &request())
        .await
        .expect("the registrations match")
        .into_phases();
    if !phased.primary.is_empty() {
        let outcome = service
            .forward_query(&ContextTarget::Live, request(), phased.primary)
            .await
            .expect("the query is forwarded");
        let answered: Vec<&str> = outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter().map(|received| received.state.id().as_str()))
            .collect();
        pretty_assertions::assert_eq!(answered, vec!["urn:ngsi-ld:Vehicle:barcelona"]);
    }
    forwarded_parameters(&source).await
}

/// Run a temporal query under `split` against a source declaring no scope and
/// answer the query parameters of each request it received.
async fn temporal_query(split: SplitEntities) -> Vec<Vec<String>> {
    let source = MockServer::builder().start().await;
    answer_in_turn(&source, "GET", TEMPORAL_PATH, [barcelona_vehicle(TEMPORAL_PATH)]).await;
    let service = service(unscoped(&source, "queryTemporal"));
    let query: TemporalQueryParameters = serde_urlencoded::from_str(SPLIT_TEMPORAL_QUERY).expect("a valid temporal query");
    let context = ContextSpec::none();
    let budget = budget();
    let outcome = service
        .forward_temporal_query(
            &ContextTarget::Live,
            ForwardTemporalQueryRequest {
                client_query: &query,
                split,
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
        .expect("the temporal query is forwarded");
    let forwarded = forwarded_parameters(&source).await;
    if !forwarded.is_empty() {
        let answered: Vec<String> = outcome
            .results
            .iter()
            .flat_map(|answer| answer.entities.iter().filter_map(|entity| entity.id().map(ToString::to_string)))
            .collect();
        pretty_assertions::assert_eq!(answered, vec!["urn:ngsi-ld:Vehicle:barcelona".to_owned()]);
    }
    forwarded
}

#[tokio::test]
async fn a_split_entity_query_reaches_an_unscoped_source_with_the_selectors_alone() {
    assert_selectors_alone(&entity_query(SplitEntities::Split).await);
}

#[tokio::test]
async fn a_complete_entity_query_reaches_an_unscoped_source_with_its_filters() {
    assert_filters_travel(&entity_query(SplitEntities::Complete).await, &DEFERRED);
}

#[tokio::test]
async fn a_split_temporal_query_reaches_an_unscoped_source_with_the_selectors_alone() {
    assert_selectors_alone(&temporal_query(SplitEntities::Split).await);
}

#[tokio::test]
async fn a_complete_temporal_query_reaches_an_unscoped_source_with_its_filters() {
    assert_filters_travel(&temporal_query(SplitEntities::Complete).await, &TEMPORAL_FILTERS);
}

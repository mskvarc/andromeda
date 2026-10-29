use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_query::{client_query::ClientQuery, id_chunks::FORWARDED_URL_BUDGET},
    mock_source::{answer_in_turn, endpoint, received},
    request::{EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultLimits},
    split_entities::SplitEntities,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use http::StatusCode;
use serde_json::json;
use std::{num::NonZeroU32, sync::Arc};
use wiremock::{MockServer, ResponseTemplate};

const QUERY_PATH: &str = "/ngsi-ld/v1/entities";

const IDENTIFIERS: usize = 3_000;

fn query_entity_source(source: &MockServer) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:peer",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

/// Clauses 4.3.6.7 and 5.5.14: the identifiers an `EntityMap` fixes reach a
/// source that advertises only `queryEntity` as several GET requests, each
/// within the URL budget, and none of them is lost or sent twice.
#[tokio::test]
async fn three_thousand_identifiers_reach_a_query_entity_source_in_urls_within_the_budget() {
    let source = MockServer::start().await;
    answer_in_turn(
        &source,
        "GET",
        QUERY_PATH,
        [ResponseTemplate::new(StatusCode::OK).set_body_raw(b"[]".to_vec(), "application/json")],
    )
    .await;
    let service = FederationService::new(
        Arc::new(FixedRegistrationStore::new(vec![query_entity_source(&source)])),
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        ContextSourcePseudonym::new("chunking-broker").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds");
    let ids = (0..IDENTIFIERS).map(|number| format!("urn:ngsi-ld:Vehicle:{number:05}")).collect::<Vec<_>>();
    let query: EntityQueryParameters = serde_urlencoded::from_str(&format!("type=Vehicle&id={}", ids.join(","))).expect("a valid query");
    let context = ContextSpec::none();
    let budget = ResultBudget::new(ResultLimits {
        distinct_entities: NonZeroU32::new(10_000).expect("a positive limit"),
        source_rows: NonZeroU32::new(10_000).expect("a positive limit"),
    });
    let request = || ForwardQueryRequest {
        client_query: ClientQuery::EntityQuery(&query),
        delivery: QueryDelivery::Query,
        split: SplitEntities::Complete,
        context: &context,
        triggering: TriggeringRequest::default(),
        csf_expr: None,
        geo: None,
        entity_map: EntityMapInUse::None,
        budget: &budget,
    };

    let matched = service
        .match_query(&ContextTarget::Live, &request())
        .await
        .expect("the registrations match")
        .into_phases();
    let outcome = service
        .forward_query(&ContextTarget::Live, request(), matched.primary)
        .await
        .expect("the query is forwarded");

    let requests = received(&source).await;
    assert!(requests.len() > 1, "{} requests", requests.len());
    for request in &requests {
        assert!(request.url.as_str().len() <= FORWARDED_URL_BUDGET, "{} bytes", request.url.as_str().len());
    }
    let sent = requests
        .iter()
        .flat_map(|request| {
            request
                .url
                .query_pairs()
                .filter(|(name, _)| name == "id")
                .flat_map(|(_, value)| value.split(',').map(str::to_owned).collect::<Vec<_>>())
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    pretty_assertions::assert_eq!(sent, ids);
    assert!(
        outcome.results.iter().all(|result| result.warnings.is_empty()),
        "the source answered every request, so no warning is raised"
    );
}

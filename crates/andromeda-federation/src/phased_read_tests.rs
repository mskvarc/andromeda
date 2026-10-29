use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_query::client_query::ClientQuery,
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

fn registration(source: &MockServer, id: &str, mode: &str) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": mode,
        "operations": ["queryEntity"],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

fn answer(number: usize) -> ResponseTemplate {
    let entities = json!([{"id": format!("urn:ngsi-ld:Vehicle:{number:04}"), "type": "Vehicle"}]);
    ResponseTemplate::new(StatusCode::OK).set_body_raw(serde_json::to_vec(&entities).expect("the answer serializes"), "application/json")
}

/// Clause 4.3.6.2: a query reaching an inclusive and an auxiliary registration
/// reads the registration store once, and each source is contacted in its own
/// phase.
#[tokio::test]
async fn a_query_reads_its_registrations_once_and_contacts_each_source_in_its_phase() {
    let inclusive = MockServer::start().await;
    answer_in_turn(&inclusive, "GET", QUERY_PATH, [answer(1)]).await;
    let auxiliary = MockServer::start().await;
    answer_in_turn(&auxiliary, "GET", QUERY_PATH, [answer(1)]).await;
    let store = Arc::new(FixedRegistrationStore::new(vec![
        registration(&inclusive, "urn:ngsi-ld:ContextSourceRegistration:inclusive", "inclusive"),
        registration(&auxiliary, "urn:ngsi-ld:ContextSourceRegistration:auxiliary", "auxiliary"),
    ]));
    let shared: Arc<FixedRegistrationStore> = Arc::clone(&store);
    let service = FederationService::new(
        shared,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        ContextSourcePseudonym::new("phased-broker").expect("a valid alias"),
        &FederationConfig::default(),
    )
    .expect("the service builds");
    let query: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle").expect("a valid query");
    let context = ContextSpec::none();
    let budget = ResultBudget::new(ResultLimits {
        distinct_entities: NonZeroU32::new(100).expect("a positive limit"),
        source_rows: NonZeroU32::new(100).expect("a positive limit"),
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

    let phased = service
        .match_query(&ContextTarget::Live, &request())
        .await
        .expect("the registrations match")
        .into_phases();
    pretty_assertions::assert_eq!((phased.primary.len(), phased.auxiliary.len()), (1, 1));
    service
        .forward_query(&ContextTarget::Live, request(), phased.primary)
        .await
        .expect("the primary phase completes");
    pretty_assertions::assert_eq!((received(&inclusive).await.len(), received(&auxiliary).await.len()), (1, 0));
    service
        .forward_query(&ContextTarget::Live, request(), phased.auxiliary)
        .await
        .expect("the auxiliary phase completes");

    pretty_assertions::assert_eq!((received(&inclusive).await.len(), received(&auxiliary).await.len()), (1, 1));
    pretty_assertions::assert_eq!(store.asked_specifications().len(), 1);
}

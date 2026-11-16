//! A forwarded query links the map a source used only when it is the one read
//! its registration was sent (clause 5.5.14 and Table 5.2.39-2).

use crate::{
    fixed_registration_store::offline_service,
    forwarded_query::client_query::ClientQuery,
    mock_source::{answer_in_turn, endpoint, received},
    outcome::{ForwardReadOutcome, ForwardedEntities},
    request::{EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultLimits},
    split_entities::SplitEntities,
};
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use serde_json::{Value, json};
use std::num::NonZeroU32;
use wiremock::{MockServer, ResponseTemplate};

const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
const BUILDING: &str = "https://uri.etsi.org/ngsi-ld/default-context/Building";
const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
const FUEL: &str = "https://uri.etsi.org/ngsi-ld/default-context/fuel";

/// A page holding the entity `id`, answered as read under the map `map`.
fn page_under(id: &str, map: &str) -> ResponseTemplate {
    let entities = json!([{"id": id, "type": "Vehicle"}]);
    ResponseTemplate::new(200)
        .set_body_raw(serde_json::to_vec(&entities).expect("the page serializes"), "application/json")
        .insert_header("ngsild-entitymap", map)
}

/// The inclusive registration `id` of `source` serving Query Entities for
/// `information`.
fn registration(id: &str, source: &MockServer, information: &Value) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "information": information,
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

/// Each answer's registration and linked map, as the identifiers' text, sorted
/// since the reads are sent concurrently.
fn linked(outcome: &ForwardReadOutcome<ForwardedEntities>) -> Vec<(Option<&str>, Option<&str>)> {
    let mut linked: Vec<(Option<&str>, Option<&str>)> = outcome
        .results
        .iter()
        .map(|answer| {
            (
                answer.registration_id.as_ref().map(Identifier::as_str),
                answer.linked_map.as_ref().map(Identifier::as_str),
            )
        })
        .collect();
    linked.sort_unstable();
    linked
}

/// Clause 4.3.6.3 sends a registration one read per covering block, and each
/// answer names the map its source used for that block's request; Table
/// 5.2.39-2 links one map to a registration and clause 5.5.14 pins a map to
/// the request that created it, so the registration of two blocks keeps
/// neither map while the registration of one keeps its own.
#[tokio::test]
async fn a_registration_read_once_per_block_keeps_none_of_the_maps_its_source_used() {
    // Unpooled servers: a pooled one can still hold the requests another test
    // sent it, and this test counts every request each source received.
    let blocks = MockServer::builder().start().await;
    answer_in_turn(
        &blocks,
        "GET",
        "/ngsi-ld/v1/entities",
        [
            page_under("urn:ngsi-ld:Vehicle:blocks-1", "urn:ngsi-ld:EntityMap:blocks-1"),
            page_under("urn:ngsi-ld:Vehicle:blocks-2", "urn:ngsi-ld:EntityMap:blocks-2"),
        ],
    )
    .await;
    let sole = MockServer::builder().start().await;
    answer_in_turn(
        &sole,
        "GET",
        "/ngsi-ld/v1/entities",
        [page_under("urn:ngsi-ld:Vehicle:sole", "urn:ngsi-ld:EntityMap:sole")],
    )
    .await;
    let service = offline_service(vec![
        registration(
            "urn:ngsi-ld:ContextSourceRegistration:blocks",
            &blocks,
            &json!([
                {"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]},
                {"entities": [{"type": BUILDING}], "propertyNames": [FUEL]}
            ]),
        ),
        registration(
            "urn:ngsi-ld:ContextSourceRegistration:sole",
            &sole,
            &json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]),
        ),
    ]);
    let query: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle,Building&attrs=speed,fuel").expect("a valid query");
    let context = ContextSpec::none();
    let limit = NonZeroU32::new(100).expect("a positive limit");
    let budget = ResultBudget::new(ResultLimits {
        distinct_entities: limit,
        source_rows: limit,
    });
    let request = ForwardQueryRequest {
        client_query: ClientQuery::EntityQuery(&query),
        delivery: QueryDelivery::Query,
        split: SplitEntities::Complete,
        context: &context,
        triggering: TriggeringRequest::default(),
        csf: None,
        geo: None,
        entity_map: EntityMapInUse::None,
        budget: &budget,
    };
    let matched = service.match_query(&ContextTarget::Live, &request).await.expect("the registrations match");
    let outcome = service
        .forward_query(&ContextTarget::Live, request, matched.into_registrations())
        .await
        .expect("the query is forwarded");

    pretty_assertions::assert_eq!(received(&blocks).await.len(), 2);
    pretty_assertions::assert_eq!(
        linked(&outcome),
        vec![
            (Some("urn:ngsi-ld:ContextSourceRegistration:blocks"), None),
            (Some("urn:ngsi-ld:ContextSourceRegistration:blocks"), None),
            (Some("urn:ngsi-ld:ContextSourceRegistration:sole"), Some("urn:ngsi-ld:EntityMap:sole")),
        ]
    );
}

//! A forwarded `EntityMap` creation asks each source only for what its
//! registration covers, one creation per covering block, and links the map a
//! source created only when it is the one creation its registration was sent
//! (clauses 4.3.6.1, 5.14.4.4, 5.14.5.4 and Table 5.2.39-2).

use crate::{
    FederationService,
    entity_map::{EntityMapQuery, ForwardEntityMapRequest, ForwardedEntityMap},
    expanded_registration_filters::ExpandedRegistrationFilters,
    fixed_registration_store::offline_service,
    mock_source::{answer_in_turn, endpoint, received},
    outcome::ForwardReadOutcome,
    request::TriggeringRequest,
};
use andromeda_jsonld::context_spec::ContextSpec;
use andromeda_ngsi_ld_lexicon::identifier::Identifier;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use serde_json::{Value, json};
use wiremock::{MockServer, ResponseTemplate};

const VEHICLE: &str = "https://uri.etsi.org/ngsi-ld/default-context/Vehicle";
const BUILDING: &str = "https://uri.etsi.org/ngsi-ld/default-context/Building";
const SPEED: &str = "https://uri.etsi.org/ngsi-ld/default-context/speed";
const FUEL: &str = "https://uri.etsi.org/ngsi-ld/default-context/fuel";
const REMOTE_MAP: &str = "urn:ngsi-ld:EntityMap:remote";
const REMOTE_VEHICLE: &str = "urn:ngsi-ld:Vehicle:2";

/// A source answering every creation on `served` with a map listing
/// [`REMOTE_VEHICLE`].
async fn source_creating_maps(served: &str) -> MockServer {
    // An unpooled server: a pooled one can still hold the requests another
    // test sent it, and these tests read every request a source received.
    let source = MockServer::builder().start().await;
    let map = json!({
        "id": REMOTE_MAP,
        "type": "EntityMap",
        "expiresAt": "2099-01-01T00:00:00Z",
        "entityMap": {REMOTE_VEHICLE: ["@none"]}
    });
    answer_in_turn(&source, "GET", served, [ResponseTemplate::new(201).set_body_json(map)]).await;
    source
}

/// A registration of `source` advertising `operation` for `information`.
fn registration(source: &MockServer, operation: &str, information: &Value) -> ContextSourceRegistration {
    serde_json::from_value(json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:maps",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "operations": [operation],
        "information": information,
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    }))
    .expect("a valid registration")
}

/// Forward the creation of a map for `query` under the core context.
async fn create(service: &FederationService, query: EntityMapQuery<'_>) -> ForwardReadOutcome<ForwardedEntityMap> {
    service
        .forward_entity_map_query(ForwardEntityMapRequest {
            query,
            filters: &ExpandedRegistrationFilters::default(),
            context: &ContextSpec::none(),
            triggering: TriggeringRequest::default(),
        })
        .await
        .expect("the creation is forwarded")
}

/// The query strings `source` received, sorted, since the creations of one
/// registration are sent concurrently.
async fn asked(source: &MockServer) -> Vec<String> {
    let mut asked: Vec<String> = received(source)
        .await
        .iter()
        .map(|request| request.url.query().unwrap_or_default().to_owned())
        .collect();
    asked.sort();
    asked
}

/// What each contribution attributes and links, as the identifiers' text.
fn contributed(outcome: &ForwardReadOutcome<ForwardedEntityMap>) -> Vec<(Vec<&str>, Option<&str>)> {
    outcome
        .results
        .iter()
        .map(|contribution| {
            (
                contribution.entity_ids.iter().map(Identifier::as_str).collect(),
                contribution.linked_map.as_ref().map(Identifier::as_str),
            )
        })
        .collect()
}

fn entity_query(raw: &str) -> EntityQueryParameters {
    serde_urlencoded::from_str(raw).expect("a valid entity query")
}

/// Clause 4.3.6.1: the source is asked for the one type and the one Attribute
/// its registration covers, with the filters, without the page clause 5.14.4.3
/// ignores, and with the suggested lifetime; the map it created is linked.
#[tokio::test]
async fn a_creation_names_only_the_type_and_attributes_the_registration_covers() {
    let source = source_creating_maps("/ngsi-ld/v1/entityMaps").await;
    let service = offline_service(vec![registration(
        &source,
        "createEntityMapQueryEntity",
        &json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]),
    )]);
    let query = entity_query("type=Vehicle,Building&attrs=speed,fuel&q=speed%3E10&limit=5&splitEntities=false&entityMapLifetime=PT1H");
    let outcome = create(&service, EntityMapQuery::Entities(&query)).await;
    pretty_assertions::assert_eq!(
        asked(&source).await,
        vec!["type=Vehicle&attrs=speed&q=speed%3E10&entityMapLifetime=PT1H".to_owned()]
    );
    pretty_assertions::assert_eq!(contributed(&outcome), vec![(vec![REMOTE_VEHICLE], Some(REMOTE_MAP))]);
}

/// Clause 4.3.6.1 forbids asking a block for what another block covers, so
/// each block is asked on its own, and Table 5.2.39-2 links one map to a
/// registration, so neither map is linked while both maps' entities are
/// attributed to it.
#[tokio::test]
async fn a_registration_asked_once_for_each_covering_block_links_none_of_the_maps() {
    let source = source_creating_maps("/ngsi-ld/v1/entityMaps").await;
    let service = offline_service(vec![registration(
        &source,
        "createEntityMapQueryEntity",
        &json!([
            {"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]},
            {"entities": [{"type": BUILDING}], "propertyNames": [FUEL]}
        ]),
    )]);
    let query = entity_query("type=Vehicle,Building&attrs=speed,fuel&splitEntities=false");
    let outcome = create(&service, EntityMapQuery::Entities(&query)).await;
    pretty_assertions::assert_eq!(
        asked(&source).await,
        vec!["type=Building&attrs=fuel".to_owned(), "type=Vehicle&attrs=speed".to_owned()]
    );
    pretty_assertions::assert_eq!(contributed(&outcome), vec![(vec![REMOTE_VEHICLE], None), (vec![REMOTE_VEHICLE], None)]);
}

/// Clause 5.14.4.4: under split entities the query, the Scope query and the
/// client's Attributes stay behind, while the context source filter, the
/// language filter and the suggested lifetime travel.
#[tokio::test]
async fn a_split_creation_leaves_the_filters_behind() {
    let source = source_creating_maps("/ngsi-ld/v1/entityMaps").await;
    let service = offline_service(vec![registration(
        &source,
        "createEntityMapQueryEntity",
        &json!([{"entities": [{"type": VEHICLE}]}]),
    )]);
    let query =
        entity_query("type=Vehicle&q=speed%3E10&attrs=speed&scopeQ=%2FMadrid&csf=tier%3D%3D%22gold%22&lang=fr&splitEntities=true&entityMapLifetime=PT1H");
    create(&service, EntityMapQuery::Entities(&query)).await;
    pretty_assertions::assert_eq!(
        asked(&source).await,
        vec!["type=Vehicle&csf=tier%3D%3D%22gold%22&lang=fr&entityMapLifetime=PT1H".to_owned()]
    );
}

/// Clause 5.14.4.4 refuses a `q` over Linked Entity Attributes without
/// `join` as too deep a query, so the creation carries the client's Linked
/// Entity retrieval beside that `q`.
#[tokio::test]
async fn an_entity_creation_carries_the_linked_entity_retrieval_of_the_client() {
    let source = source_creating_maps("/ngsi-ld/v1/entityMaps").await;
    let service = offline_service(vec![registration(
        &source,
        "createEntityMapQueryEntity",
        &json!([{"entities": [{"type": VEHICLE}]}]),
    )]);
    let query = entity_query(
        "type=Vehicle&q=isParkedIn%7Bcategory%7D%3D%3D%22commercial%22&join=flat&joinLevel=2&containedBy=urn:ngsi-ld:Building:1&splitEntities=false",
    );
    create(&service, EntityMapQuery::Entities(&query)).await;
    pretty_assertions::assert_eq!(
        asked(&source).await,
        vec!["type=Vehicle&q=isParkedIn%7Bcategory%7D%3D%3D%22commercial%22&join=flat&joinLevel=2&containedBy=urn%3Angsi-ld%3ABuilding%3A1".to_owned()]
    );
}

/// Clause 5.14.5.4 narrows a temporal creation as clause 5.14.4.4 does an
/// entity one; the temporal query and `lastN` travel, and no representation
/// or aggregation does (clause 5.14.5.3). It refuses Linked Entity retrieval,
/// so a `join` admitted by an extension stays behind.
#[tokio::test]
async fn a_temporal_creation_keeps_its_window_and_names_only_what_the_registration_covers() {
    let source = source_creating_maps("/ngsi-ld/v1/temporal/entityMaps").await;
    let service = offline_service(vec![registration(
        &source,
        "createEntityMapQueryTemporal",
        &json!([{"entities": [{"type": VEHICLE}], "propertyNames": [SPEED]}]),
    )]);
    let query: TemporalQueryParameters = serde_urlencoded::from_str(
        "type=Vehicle,Building&attrs=speed,fuel&timerel=before&timeAt=2022-01-01T00:00:00Z&lastN=2&format=aggregatedValues&aggrMethods=avg&aggrPeriodDuration=PT1H&join=flat&joinLevel=2&splitEntities=false",
    )
    .expect("a valid temporal query");
    let outcome = create(&service, EntityMapQuery::TemporalEntities(&query)).await;
    pretty_assertions::assert_eq!(
        asked(&source).await,
        vec!["type=Vehicle&attrs=speed&timerel=before&timeAt=2022-01-01T00%3A00%3A00Z&lastN=2".to_owned()]
    );
    pretty_assertions::assert_eq!(contributed(&outcome), vec![(vec![REMOTE_VEHICLE], Some(REMOTE_MAP))]);
}

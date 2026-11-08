//! Purge Entities and EntityMap creation select their registrations as a read
//! does: every entity selector whole by clause 5.12, then the context source
//! filter, the Scope query and the geoquery (clauses 4.9, 4.19, 5.6.21.4,
//! 5.10.2.4, 5.14.4.4 and 5.14.5.4).

use crate::{
    FederationService,
    entity_map::{EntityMapQuery, ForwardEntityMapRequest},
    expanded_registration_filters::ExpandedRegistrationFilters,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities, generous_geo_admission},
    local_source::LocalSource,
    mock_source::{endpoint, received},
    provision_purge::ForwardPurgeRequest,
    request::{EntityMapInUse, TriggeringRequest},
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_json_literal::JsonLiteral;
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_query::{filter::expression::Expr, geo::input::GeoQueryInput};
use andromeda_ngsi_ld_request::query::{entity::parameters::EntityQueryParameters, temporal::TemporalQueryParameters};
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use serde_json::{Value, json};
use std::sync::Arc;
use wiremock::{Mock, MockServer, ResponseTemplate, matchers::any};

/// The operation whose registration selection a test reads.
#[derive(Debug, Clone, Copy)]
enum Selecting {
    /// Purge Entities (clause 5.6.21).
    Purge,
    /// Create `EntityMap` for Query Entities (clause 5.14.4).
    EntityMap,
    /// Create `EntityMap` for Query Temporal Evolution of Entities (clause
    /// 5.14.5).
    TemporalEntityMap,
}

/// One registered source: its registration identifier, the members merged
/// into its registration, and the mock it answers from.
struct Source {
    /// The registration identifier.
    id: &'static str,
    /// The mock the source answers from.
    source: MockServer,
}

/// A source answering every request with 204, registered as `id` with
/// `extra` merged into a registration of `Vehicle` entities serving every
/// operation the tests select by.
async fn registered(id: &'static str, extra: &Value) -> (Source, ContextSourceRegistration) {
    // An unpooled server: a pooled one can still hold the requests another
    // test sent it, and these tests read every request a source received.
    let source = MockServer::builder().start().await;
    Mock::given(any()).respond_with(ResponseTemplate::new(204)).mount(&source).await;
    let mut document = json!({
        "id": id,
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(&source),
        "mode": "inclusive",
        "operations": ["purgeEntity", "createEntityMapQueryEntity", "createEntityMapQueryTemporal"],
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    if let (Some(document), Some(extra)) = (document.as_object_mut(), extra.as_object()) {
        document.extend(extra.clone());
    }
    (Source { id, source }, serde_json::from_value(document).expect("a valid registration"))
}

fn service(store: Arc<FixedRegistrationStore>) -> FederationService {
    FederationService::new(
        store,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("selecting-broker").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// The request context, naming the Context Source Property the filters read.
fn context() -> ContextSpec {
    ContextSpec::from_body(JsonLiteral::admit(&json!({"sourceGroup": "https://example.org/sourceGroup"}).to_string()).expect("an admissible literal"))
}

/// What one selection did.
struct Contacted {
    /// The identifiers of the sources contacted, in registration order.
    reached: Vec<&'static str>,
    /// For each source in `reached`, the `scopeQ` values its requests carried.
    scope_queries: Vec<Vec<String>>,
    /// The specifications the store was asked with.
    asked: Vec<Vec<String>>,
}

/// Run `selecting` with `query` against `registrations`.
async fn contacted(selecting: Selecting, query: &str, registrations: Vec<(Source, ContextSourceRegistration)>) -> Contacted {
    let (sources, registrations): (Vec<Source>, Vec<ContextSourceRegistration>) = registrations.into_iter().unzip();
    let store = Arc::new(FixedRegistrationStore::new(registrations));
    let service = service(Arc::clone(&store));
    let context = context();
    match selecting {
        Selecting::Purge => {
            let params: EntityQueryParameters = serde_urlencoded::from_str(query).expect("a valid entity query");
            let filters = expanded(&service, &context, params.csf.as_ref(), params.geo.0.as_ref()).await;
            service
                .forward_purge(
                    &ContextTarget::Live,
                    ForwardPurgeRequest {
                        query: &params,
                        filters: &filters,
                        local_ids: &[],
                        query_string: query,
                        entity_map: EntityMapInUse::None,
                        context: &context,
                        triggering: TriggeringRequest::default(),
                    },
                )
                .await
                .expect("the purge is forwarded");
        }
        Selecting::EntityMap => {
            let params: EntityQueryParameters = serde_urlencoded::from_str(query).expect("a valid entity query");
            let filters = expanded(&service, &context, params.csf.as_ref(), params.geo.0.as_ref()).await;
            create_map(&service, EntityMapQuery::Entities(&params), &filters, query, &context).await;
        }
        Selecting::TemporalEntityMap => {
            let params: TemporalQueryParameters = serde_urlencoded::from_str(query).expect("a valid temporal query");
            let filters = expanded(&service, &context, params.csf.as_ref(), params.geo.0.as_ref()).await;
            create_map(&service, EntityMapQuery::TemporalEntities(&params), &filters, query, &context).await;
        }
    }
    let mut reached = Vec::new();
    let mut scope_queries = Vec::new();
    for source in &sources {
        let requests = received(&source.source).await;
        if !requests.is_empty() {
            reached.push(source.id);
            scope_queries.push(
                requests
                    .iter()
                    .flat_map(|request| {
                        request
                            .url
                            .query_pairs()
                            .filter(|(name, _)| name == "scopeQ")
                            .map(|(_, value)| value.into_owned())
                    })
                    .collect(),
            );
        }
    }
    Contacted {
        reached,
        scope_queries,
        asked: store.asked_specifications(),
    }
}

async fn expanded(service: &FederationService, context: &ContextSpec, csf: Option<&Expr>, geo: Option<&GeoQueryInput>) -> ExpandedRegistrationFilters {
    ExpandedRegistrationFilters::expand(service.engine(), context, csf, geo, generous_geo_admission())
        .await
        .expect("the filters expand")
}

async fn create_map(service: &FederationService, query: EntityMapQuery<'_>, filters: &ExpandedRegistrationFilters, query_string: &str, context: &ContextSpec) {
    service
        .forward_entity_map_query(ForwardEntityMapRequest {
            query,
            filters,
            query_string,
            context,
            triggering: TriggeringRequest::default(),
        })
        .await
        .expect("the creation is forwarded");
}

/// Two sources told apart by the `sourceGroup` Context Source Property.
async fn grouped() -> Vec<(Source, ContextSourceRegistration)> {
    vec![
        registered("urn:ngsi-ld:csr:alpha", &json!({"https://example.org/sourceGroup": "alpha"})).await,
        registered("urn:ngsi-ld:csr:beta", &json!({"https://example.org/sourceGroup": "beta"})).await,
    ]
}

/// Two sources scoped to different cities and one declaring no scope.
async fn scoped() -> Vec<(Source, ContextSourceRegistration)> {
    vec![
        registered("urn:ngsi-ld:csr:madrid", &json!({"scope": ["/Madrid"]})).await,
        registered("urn:ngsi-ld:csr:berlin", &json!({"scope": ["/Berlin"]})).await,
        registered("urn:ngsi-ld:csr:unscoped", &json!({})).await,
    ]
}

/// Two sources located in different cities and one declaring no location.
async fn located() -> Vec<(Source, ContextSourceRegistration)> {
    vec![
        registered("urn:ngsi-ld:csr:madrid", &json!({"location": {"type": "Point", "coordinates": [-3.7, 40.4]}})).await,
        registered("urn:ngsi-ld:csr:berlin", &json!({"location": {"type": "Point", "coordinates": [13.4, 52.5]}})).await,
        registered("urn:ngsi-ld:csr:anywhere", &json!({})).await,
    ]
}

/// One source that every selection matches.
async fn single() -> Vec<(Source, ContextSourceRegistration)> {
    vec![registered("urn:ngsi-ld:csr:only", &json!({})).await]
}

const NEAR_MADRID: &str = "georel=near%3BmaxDistance%3D%3D100000&geometry=Point&coordinates=%5B-3.7%2C40.4%5D";

/// Clause 4.9: only the source whose Context Source Properties satisfy the
/// context source filter is contacted.
#[tokio::test]
async fn a_selection_contacts_only_the_sources_its_context_source_filter_admits() {
    for selecting in [Selecting::Purge, Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { reached, .. } = contacted(selecting, "type=Vehicle&csf=sourceGroup%3D%3D%22alpha%22", grouped().await).await;
        pretty_assertions::assert_eq!(reached, vec!["urn:ngsi-ld:csr:alpha"], "{selecting:?}");
    }
}

/// Table 5.2.9-1 and clause 5.12: a source whose declared Scopes the Scope
/// query matches none of is not contacted; one declaring no `scope` may hold a
/// matching entity and is.
#[tokio::test]
async fn a_selection_contacts_the_sources_whose_scope_may_hold_a_scope_query_match() {
    for selecting in [Selecting::Purge, Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { reached, scope_queries, .. } = contacted(selecting, "type=Vehicle&scopeQ=%2FMadrid", scoped().await).await;
        pretty_assertions::assert_eq!(reached, vec!["urn:ngsi-ld:csr:madrid", "urn:ngsi-ld:csr:unscoped"], "{selecting:?}");
        // Clause 4.3.6.7: without split entities the source applies the Scope
        // query, so a purge removes only what it matches there.
        pretty_assertions::assert_eq!(scope_queries, vec![vec!["/Madrid".to_owned()], vec!["/Madrid".to_owned()]], "{selecting:?}");
    }
}

/// Clauses 5.14.4.4 and 5.14.5.4: under split entities the Scope query is
/// applied to the merged entities and clause 5.12 compares no scope, so every
/// source matching the selectors is asked for its map, one declaring no
/// `scope` included.
#[tokio::test]
async fn a_split_entity_map_contacts_every_source_whatever_its_scope() {
    for selecting in [Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { reached, .. } = contacted(selecting, "type=Vehicle&scopeQ=%2FBarcelona&splitEntities=true", scoped().await).await;
        pretty_assertions::assert_eq!(
            reached,
            vec!["urn:ngsi-ld:csr:madrid", "urn:ngsi-ld:csr:berlin", "urn:ngsi-ld:csr:unscoped"],
            "{selecting:?}"
        );
    }
}

/// Clauses 4.3.6.1 and 5.10.2.4: a source located where no entity the
/// geoquery matches can be is not contacted; one declaring no location may
/// serve any area.
#[tokio::test]
async fn a_selection_contacts_only_the_sources_whose_location_may_hold_a_geoquery_match() {
    for selecting in [Selecting::Purge, Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { reached, .. } = contacted(selecting, &format!("type=Vehicle&{NEAR_MADRID}"), located().await).await;
        pretty_assertions::assert_eq!(reached, vec!["urn:ngsi-ld:csr:madrid", "urn:ngsi-ld:csr:anywhere"], "{selecting:?}");
    }
}

/// Clauses 4.17 and 5.12: a disjunction of names reaches the registration
/// store whole, every name expanded, rather than leaving the type unread.
#[tokio::test]
async fn a_selection_asks_the_store_with_every_name_of_a_type_disjunction() {
    for selecting in [Selecting::Purge, Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { asked, .. } = contacted(selecting, "type=Vehicle,Boat", single().await).await;
        pretty_assertions::assert_eq!(asked.len(), 1, "{selecting:?} asked the store {asked:?}");
        pretty_assertions::assert_eq!(asked[0].len(), 1, "{selecting:?} asked the store {asked:?}");
        let specification = &asked[0][0];
        assert!(specification.contains("Or("), "{selecting:?}: {specification}");
        assert!(
            specification.contains("https://uri.etsi.org/ngsi-ld/default-context/Vehicle")
                && specification.contains("https://uri.etsi.org/ngsi-ld/default-context/Boat"),
            "{selecting:?}: {specification}"
        );
    }
}

/// Clauses 4.17 and 5.12: a conjunction reaches the registration store as a
/// conjunction, which an `EntityInfo` meets only by listing every named type.
#[tokio::test]
async fn a_selection_asks_the_store_with_a_type_conjunction_whole() {
    for selecting in [Selecting::Purge, Selecting::EntityMap, Selecting::TemporalEntityMap] {
        let Contacted { asked, .. } = contacted(selecting, "type=%28Home%3BVehicle%29", single().await).await;
        let specification = asked.concat().concat();
        assert!(specification.contains("And("), "{selecting:?}: {specification}");
        assert!(
            specification.contains("https://uri.etsi.org/ngsi-ld/default-context/Home")
                && specification.contains("https://uri.etsi.org/ngsi-ld/default-context/Vehicle"),
            "{selecting:?}: {specification}"
        );
    }
}

/// Clause 5.2.33: the entity selectors of a query body are a union, so an
/// `EntityMap` created from one asks the store with each selector on its own
/// rather than with the cross product of their flattened members.
#[tokio::test]
async fn an_entity_map_from_a_query_body_asks_the_store_with_each_selector() {
    let mut params: EntityQueryParameters = serde_urlencoded::from_str("type=Vehicle,Boat&id=urn:ngsi-ld:A,urn:ngsi-ld:B").expect("a valid entity query");
    params.entity_selectors = vec![
        serde_urlencoded::from_str::<EntityQueryParameters>("type=Vehicle&id=urn:ngsi-ld:A")
            .expect("a valid selector")
            .selectors()
            .into_owned()
            .remove(0),
        serde_urlencoded::from_str::<EntityQueryParameters>("type=Boat&id=urn:ngsi-ld:B")
            .expect("a valid selector")
            .selectors()
            .into_owned()
            .remove(0),
    ];
    let (_, registration) = registered("urn:ngsi-ld:csr:only", &json!({})).await;
    let store = Arc::new(FixedRegistrationStore::new(vec![registration]));
    let service = service(Arc::clone(&store));
    let context = context();
    create_map(
        &service,
        EntityMapQuery::Entities(&params),
        &ExpandedRegistrationFilters::default(),
        "",
        &context,
    )
    .await;
    let asked = store.asked_specifications();
    pretty_assertions::assert_eq!(asked.len(), 1, "the store was asked {asked:?}");
    pretty_assertions::assert_eq!(asked[0].len(), 2, "the store was asked {asked:?}");
    let (vehicle, boat) = (&asked[0][0], &asked[0][1]);
    assert!(
        vehicle.contains("default-context/Vehicle") && vehicle.contains("urn:ngsi-ld:A") && !vehicle.contains("urn:ngsi-ld:B"),
        "{vehicle}"
    );
    assert!(
        boat.contains("default-context/Boat") && boat.contains("urn:ngsi-ld:B") && !boat.contains("urn:ngsi-ld:A"),
        "{boat}"
    );
}

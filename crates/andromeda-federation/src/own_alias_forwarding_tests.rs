use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    forwarded_query::client_query::ClientQuery,
    local_source::LocalSource,
    mock_source::{answer_in_turn, endpoint, received},
    request::{EntityMapInUse, ForwardQueryRequest, QueryDelivery, TriggeringRequest},
    result_budget::{ResultBudget, ResultLimits},
    split_entities::SplitEntities,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_http_headers::ngsild_warning::{NgsiLdWarning, WarnCode};
use andromeda_jsonld::{context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::{context_source_pseudonym::ContextSourcePseudonym, tenant::TenantName};
use andromeda_ngsi_ld_model::registration::ContextSourceRegistration;
use andromeda_ngsi_ld_request::query::entity::parameters::EntityQueryParameters;
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use http::StatusCode;
use serde_json::json;
use std::{num::NonZeroU32, sync::Arc};
use wiremock::{MockServer, ResponseTemplate};

const QUERY_PATH: &str = "/ngsi-ld/v1/entities";

/// The alias this broker is configured with.
const CONFIGURED_ALIAS: &str = "own-broker";

/// The named tenant the query runs in.
const TENANT: &str = "fleet";

/// An inclusive registration of `Vehicle` at `source`, aliased as this
/// broker's configured alias and targeting `tenant` when one is given.
fn registration_naming_this_broker(source: &MockServer, tenant: Option<&str>) -> ContextSourceRegistration {
    let mut document = json!({
        "id": "urn:ngsi-ld:ContextSourceRegistration:own",
        "type": "ContextSourceRegistration",
        "endpoint": endpoint(source),
        "mode": "inclusive",
        "operations": ["queryEntity"],
        "contextSourceAlias": CONFIGURED_ALIAS,
        "information": [{"entities": [{"type": "https://uri.etsi.org/ngsi-ld/default-context/Vehicle"}]}],
        "status": "ok",
        "createdAt": "2024-01-01T00:00:00Z",
        "modifiedAt": "2024-01-01T00:00:00Z"
    });
    if let Some(tenant) = tenant {
        document["tenant"] = json!(tenant);
    }
    serde_json::from_value(document).expect("a valid registration")
}

/// The requests `source` received, and the warnings of the forwarded query
/// `tenant` runs over `registration`.
async fn query_over(source: &MockServer, registration: ContextSourceRegistration, tenant: TenantRef) -> (usize, Vec<NgsiLdWarning>) {
    let store = Arc::new(FixedRegistrationStore::new(vec![registration]));
    let census = store.census();
    let service = FederationService::new(
        store,
        census,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new(CONFIGURED_ALIAS).expect("a valid alias")),
            tenant,
        ),
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
    let matched = service.match_query(&ContextTarget::Live, &request()).await.expect("the registrations match");
    let outcome = service
        .forward_query(&ContextTarget::Live, request(), matched.into_registrations())
        .await
        .expect("the query completes");
    let warnings = outcome.results.into_iter().flat_map(|result| result.warnings).collect();
    (received(source).await.len(), warnings)
}

async fn answering_source() -> MockServer {
    let source = MockServer::start().await;
    answer_in_turn(
        &source,
        "GET",
        QUERY_PATH,
        [ResponseTemplate::new(StatusCode::OK).set_body_raw("[]", "application/json")],
    )
    .await;
    source
}

fn codes(warnings: &[NgsiLdWarning]) -> Vec<WarnCode> {
    warnings.iter().map(NgsiLdWarning::code).collect()
}

/// Clauses 4.3.6.4 and 6.3.17: the broker is the first of the Context Sources
/// a request has encountered, so a registration naming it and leading back
/// into the tenant the query runs in is never contacted, and the loop is
/// named with one 199 warning.
#[tokio::test]
async fn a_query_covered_only_by_a_registration_naming_this_broker_sends_no_forwarded_request() {
    for (registration_tenant, tenant) in [
        (None, TenantRef::UnnamedDefault),
        (Some(TENANT), TenantRef::Named(TENANT.parse::<TenantName>().expect("a valid tenant name"))),
    ] {
        let source = answering_source().await;
        let (requests, warnings) = query_over(&source, registration_naming_this_broker(&source, registration_tenant), tenant).await;
        pretty_assertions::assert_eq!(requests, 0, "{registration_tenant:?}");
        pretty_assertions::assert_eq!(codes(&warnings), vec![WarnCode::MiscellaneousWarning], "{registration_tenant:?}");
    }
}

/// Clause 4.3.6.4 considers each tenant separately: a registration naming this
/// broker but targeting the default tenant leads out of a named tenant, so the
/// source is asked.
#[tokio::test]
async fn a_registration_naming_this_broker_in_another_tenant_is_contacted() {
    let source = answering_source().await;
    let tenant = TenantRef::Named(TENANT.parse::<TenantName>().expect("a valid tenant name"));
    let (requests, warnings) = query_over(&source, registration_naming_this_broker(&source, None), tenant).await;
    pretty_assertions::assert_eq!(requests, 1);
    pretty_assertions::assert_eq!(codes(&warnings), Vec::<WarnCode>::new());
}

//! How a forwarded batch reads its candidate registrations. It reads the store
//! once, narrowed to the batch's identifiers and types, and not at all in a
//! tenant holding no registration.

use crate::{
    FederationService,
    fixed_registration_store::{FixedRegistrationStore, NoLocalEntities},
    local_source::LocalSource,
    provision_batch::{
        BatchEntity,
        BatchProvisionOperation,
        ForwardBatchRequest,
        registration_fixture::{document, registration, uri},
    },
    request::TriggeringRequest,
};
use andromeda_configuration::schema::federation::FederationConfig;
use andromeda_context_space::context_target::ContextTarget;
use andromeda_jsonld::{batch_contexts::BatchContext, context_spec::ContextSpec, engine::processor::JsonLdEngine};
use andromeda_ngsi_ld_lexicon::context_source_pseudonym::ContextSourcePseudonym;
use andromeda_registration::registration_census::RegistrationCensus;
use andromeda_tenancy::{context_source_alias::ContextSourceAliases, tenant_ref::TenantRef};
use pretty_assertions::assert_eq;
use std::sync::Arc;

const VEHICLE: &str = "https://example.org/Vehicle";
const BOAT: &str = "https://example.org/Boat";
const SPEED: &str = "https://example.org/speed";

/// A store holding one registration of Vehicles that no forward reaches, its
/// endpoint unused because the registration advertises no write.
fn store() -> Arc<FixedRegistrationStore> {
    Arc::new(FixedRegistrationStore::new(vec![registration(
        "urn:ngsi-ld:ContextSourceRegistration:vehicles",
        "inclusive",
        Some(VEHICLE),
        None,
        &[SPEED],
        &["retrieveEntity"],
    )]))
}

/// A forwarding service reading `store`, its census counting `census`.
fn service(store: &Arc<FixedRegistrationStore>, census: RegistrationCensus) -> FederationService {
    let shared: Arc<FixedRegistrationStore> = Arc::clone(store);
    FederationService::new(
        shared,
        census,
        Arc::new(NoLocalEntities),
        JsonLdEngine::builder().allow_remote(false).build().expect("the engine builds"),
        LocalSource::new(
            ContextSourceAliases::new(ContextSourcePseudonym::new("andromeda-test").expect("a valid alias")),
            TenantRef::UnnamedDefault,
        ),
        &FederationConfig::default(),
    )
    .expect("the service builds")
}

/// Forward a batch creation of a Vehicle and a Boat through `service`.
async fn forward_vehicle_and_boat(service: &FederationService) {
    let context = BatchContext::shared(ContextSpec::none());
    let ids = [uri("urn:ngsi-ld:Vehicle:1"), uri("urn:ngsi-ld:Boat:1")];
    let vehicle = document("urn:ngsi-ld:Vehicle:1", VEHICLE, &[SPEED]);
    let boat = document("urn:ngsi-ld:Boat:1", BOAT, &[SPEED]);
    let entities = [
        BatchEntity {
            id: &ids[0],
            document: Some(&vehicle),
            context: &context,
        },
        BatchEntity {
            id: &ids[1],
            document: Some(&boat),
            context: &context,
        },
    ];
    service
        .forward_batch(
            &ContextTarget::Live,
            ForwardBatchRequest {
                operation: BatchProvisionOperation::Create,
                entities: &entities,
                query_string: None,
                triggering: TriggeringRequest::default(),
            },
        )
        .await
        .expect("the registrations are read");
}

#[tokio::test]
async fn a_batch_reads_its_candidates_once_narrowed_to_its_identifiers_and_types() {
    let store = store();
    let census = store.census();
    forward_vehicle_and_boat(&service(&store, census)).await;
    let asked = store.asked_specifications();
    assert_eq!(asked.len(), 1, "{asked:?}");
    let specifications = &asked[0];
    assert_eq!(specifications.len(), 1, "one specification covers the whole array: {specifications:?}");
    for named in ["urn:ngsi-ld:Vehicle:1", "urn:ngsi-ld:Boat:1", VEHICLE, BOAT] {
        assert!(specifications[0].contains(named), "{named}: {specifications:?}");
    }
}

#[tokio::test]
async fn a_batch_in_a_tenant_holding_no_registration_reads_none() {
    let store = store();
    forward_vehicle_and_boat(&service(&store, RegistrationCensus::new(0))).await;
    assert_eq!(store.asked_specifications(), Vec::<Vec<String>>::new());
}

#[tokio::test]
async fn an_empty_batch_reads_no_registration() {
    let store = store();
    let census = store.census();
    let results = service(&store, census)
        .forward_batch(
            &ContextTarget::Live,
            ForwardBatchRequest {
                operation: BatchProvisionOperation::Delete,
                entities: &[],
                query_string: None,
                triggering: TriggeringRequest::default(),
            },
        )
        .await
        .expect("nothing is read");
    assert!(results.is_empty());
    assert_eq!(store.asked_specifications(), Vec::<Vec<String>>::new());
}
